//! Tokens from the Markdown body: tables, short lists, font sections, the
//! `**Theme:**` line, fenced blocks, and prose that might state a rule.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::css;
use super::diagnostic::{LineIndex, Span};
use super::extract::{Extracted, FontDecl, FontRole, Origin, ProseLine, Scheme, Token};
use super::roles::words;
use super::yaml;
use crate::{Color, Length, Shadow};

struct Cell {
    text: String,
    offset: usize,
}

#[derive(Default)]
struct TableState {
    headers: Vec<String>,
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    in_head: bool,
}

struct Walker<'a> {
    source: &'a str,
    index: &'a LineIndex<'a>,
    out: &'a mut Extracted,
    headings: Vec<(HeadingLevel, String)>,
    /// Text being collected for the innermost open block.
    buffers: Vec<(String, usize)>,
    table: Option<TableState>,
    code: Option<(String, Option<(usize, usize)>)>,
    pending_family: Option<(String, usize)>,
    current_font: Option<usize>,
}

/// Reads the part of `source` from byte `start` onward.
pub(crate) fn extract(source: &str, start: usize, index: &LineIndex, out: &mut Extracted) {
    let mut walker = Walker {
        source,
        index,
        out,
        headings: Vec::new(),
        buffers: Vec::new(),
        table: None,
        code: None,
        pending_family: None,
        current_font: None,
    };
    let parser = Parser::new_ext(&source[start..], Options::ENABLE_TABLES).into_offset_iter();
    for (event, range) in parser {
        let offset = start + range.start;
        let end = start + range.end;
        walker.event(event, offset, end);
    }
}

impl Walker<'_> {
    fn event(&mut self, event: Event<'_>, offset: usize, end: usize) {
        match event {
            // A paragraph inside a list item belongs to the item's text.
            Event::Start(Tag::Heading { .. } | Tag::Paragraph) if self.buffers.is_empty() => {
                self.buffers.push((String::new(), offset));
            }
            Event::End(TagEnd::Heading(level)) => {
                if let Some((text, offset)) = self.buffers.pop() {
                    self.heading(level, text.trim().to_string(), offset);
                }
            }
            Event::End(TagEnd::Paragraph) if self.buffers.len() == 1 && self.table.is_none() => {
                if let Some((text, offset)) = self.buffers.pop() {
                    self.lines(&text, offset);
                }
            }
            Event::Start(Tag::Item) => self.buffers.push((String::new(), offset)),
            Event::End(TagEnd::Item) => {
                if let Some((text, offset)) = self.buffers.pop() {
                    self.lines(&text, offset);
                }
            }
            Event::Start(Tag::Table(_)) => self.table = Some(TableState::default()),
            Event::Start(Tag::TableHead) => {
                if let Some(table) = &mut self.table {
                    table.in_head = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = &mut self.table {
                    table.in_head = false;
                    table.headers = table.row.drain(..).map(|c| c.text).collect();
                }
            }
            Event::Start(Tag::TableCell) => self.buffers.push((String::new(), offset)),
            Event::End(TagEnd::TableCell) => {
                if let (Some((text, offset)), Some(table)) = (self.buffers.pop(), &mut self.table) {
                    table.row.push(Cell {
                        text: text.trim().to_string(),
                        offset,
                    });
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    self.handle_table(&table);
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.trim().to_ascii_lowercase(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, None));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, Some((from, to)))) = self.code.take() {
                    self.handle_code(&lang, from, to);
                }
            }
            Event::Text(text) => {
                if let Some((_, range)) = &mut self.code {
                    *range = Some(range.map_or((offset, end), |(from, _)| (from, end)));
                } else if let Some((buffer, _)) = self.buffers.last_mut() {
                    buffer.push_str(&text);
                }
            }
            Event::Code(text) => {
                if let Some((buffer, _)) = self.buffers.last_mut() {
                    buffer.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((buffer, _)) = self.buffers.last_mut() {
                    buffer.push('\n');
                }
            }
            _ => {}
        }
    }

    fn context(&self) -> String {
        let parts: Vec<String> = self
            .headings
            .iter()
            .map(|(_, t)| t.to_lowercase())
            .collect();
        parts.join(" / ")
    }

    fn scheme(&self) -> Scheme {
        let context = words(&self.context());
        let dark = [
            " dark mode ",
            " dark theme ",
            " dark palette ",
            " dark colors ",
            " dark colours ",
            " dark scheme ",
        ];
        let exact = self
            .headings
            .iter()
            .any(|(_, t)| t.eq_ignore_ascii_case("dark"));
        if exact || dark.iter().any(|d| context.contains(d)) {
            Scheme::Dark
        } else {
            Scheme::Light
        }
    }

    fn in_section(&self, names: &[&str]) -> bool {
        let context = words(&self.context());
        names
            .iter()
            .any(|name| context.contains(&format!(" {name} ")))
    }

    fn in_typography(&self) -> bool {
        self.in_section(&[
            "typography",
            "type",
            "font",
            "fonts",
            "typeface",
            "typefaces",
        ])
    }

    /// The caret position for `needle` near a block's start, or the start itself.
    /// The window stops a missing needle from scanning the rest of the file.
    fn locate(&self, needle: &str, offset: usize) -> Span {
        let mut end = (offset + 4096).min(self.source.len());
        while !self.source.is_char_boundary(end) {
            end -= 1;
        }
        let found = self.source[offset..end].find(needle);
        self.index.span(offset + found.unwrap_or(0), needle.len())
    }

    fn heading(&mut self, level: HeadingLevel, text: String, offset: usize) {
        while self.headings.last().is_some_and(|(l, _)| *l >= level) {
            self.headings.pop();
        }
        if level == HeadingLevel::H1 && self.out.name.is_none() {
            self.out.name = Some(text.clone());
        }
        let is_font_section = self.in_typography();
        self.headings.push((level, text.clone()));
        self.current_font = None;
        self.pending_family = None;
        if is_font_section && level > HeadingLevel::H2 {
            // A heading such as "### Lausanne": a family name if what follows
            // describes one, which `lines` finds out.
            let family = text.split(['—', '–', ':', '(']).next().unwrap_or("").trim();
            let generic = words(family);
            let is_generic = [
                "scale",
                "hierarchy",
                "rule",
                "rules",
                "font",
                "fonts",
                "type",
                "usage",
                "weight",
                "weights",
                "pairing",
                "principles",
                "body",
                "headings",
                "display",
                "families",
                "family",
            ];
            let looks_like_name = !family.is_empty()
                && family.split_whitespace().count() <= 4
                && !is_generic
                    .iter()
                    .any(|w| generic.contains(&format!(" {w} ")));
            if looks_like_name {
                self.pending_family = Some((family.to_string(), offset));
            }
        }
    }

    fn lines(&mut self, text: &str, offset: usize) {
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() {
                self.line(line, offset);
            }
        }
    }

    fn line(&mut self, line: &str, offset: usize) {
        let (key, value) = split_key_value(line);
        let key_words = words(key.unwrap_or(""));
        let key_is = |names: &[&str]| names.iter().any(|n| key_words.contains(&format!(" {n} ")));

        if let (Some(_), Some(value)) = (key, value) {
            if key_is(&["theme", "mode", "color scheme"]) {
                let first = words(value);
                if first.starts_with(" dark ") {
                    self.out.theme = Some(Scheme::Dark);
                    return;
                }
                if first.starts_with(" light ") {
                    self.out.theme = Some(Scheme::Light);
                    return;
                }
            }
            // `Light/Dark: ✗ No / ✓ Full` says which schemes a design supports.
            if key_is(&["light dark"]) {
                if let Some((light, dark)) = value.split_once('/') {
                    let no = |side: &str| side.contains('✗') || words(side).contains(" no ");
                    match (no(light), no(dark)) {
                        (true, false) => self.out.theme = Some(Scheme::Dark),
                        (false, true) => self.out.theme = Some(Scheme::Light),
                        _ => {}
                    }
                    return;
                }
            }
            if self.in_typography() && self.font_line(&key_words, value, offset) {
                return;
            }
            if self.color_line(key.unwrap_or(""), value, offset) {
                return;
            }
            if self.measure_line(&key_words, value, offset) {
                return;
            }
        }
        if self.parenthesized_color(line, offset) {
            return;
        }
        let span = self.locate(line.get(..line.len().min(24)).map_or(line, |s| s), offset);
        let context = self.context();
        self.out.prose.push(ProseLine {
            text: line.to_string(),
            span,
            context,
        });
    }

    /// `Substitute: Inter`, `Family: Lausanne`, `Weights: 400`.
    fn font_line(&mut self, key: &str, value: &str, offset: usize) -> bool {
        let has = |names: &[&str]| names.iter().any(|n| key.contains(&format!(" {n} ")));
        if has(&[
            "substitute",
            "substitutes",
            "fallback",
            "alternative",
            "alternatives",
        ]) {
            let names = split_font_list(value);
            self.commit_pending_font();
            match self.current_font {
                Some(font) => self.out.fonts[font].substitutes = names,
                None => {
                    if let Some(last) = self.out.fonts.last_mut() {
                        last.substitutes = names;
                    }
                }
            }
            return true;
        }
        if has(&["family", "font", "typeface"]) {
            if has(&["size", "weight"]) {
                return false;
            }
            let family = split_font_list(value)
                .into_iter()
                .next()
                .unwrap_or_default();
            if family.is_empty() {
                return false;
            }
            let span = self.locate(&family, offset);
            let role = font_role_from_key(key);
            self.out.fonts.push(FontDecl {
                family,
                role,
                substitutes: Vec::new(),
                span,
                origin: Origin::List,
            });
            self.current_font = Some(self.out.fonts.len() - 1);
            self.pending_family = None;
            return true;
        }
        if has(&["weight", "weights"]) {
            self.commit_pending_font();
            let first = value
                .split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty());
            if let Some(weight) = first.filter(|_| !has(&["body"])) {
                let span = self.locate(weight, offset);
                self.out.heading_weights.push(Token {
                    name: "font weight".to_string(),
                    value: weight.to_string(),
                    span,
                    origin: Origin::List,
                    scheme: Scheme::Light,
                });
            }
            return true;
        }
        if has(&[
            "role",
            "use",
            "usage",
            "sizes",
            "line height",
            "letter spacing",
        ]) {
            self.commit_pending_font();
            return true;
        }
        false
    }

    fn commit_pending_font(&mut self) {
        if let Some((family, offset)) = self.pending_family.take() {
            let span = self.locate(&family, offset);
            let role = font_role_from_key(&words(&family));
            self.out.fonts.push(FontDecl {
                family,
                role,
                substitutes: Vec::new(),
                span,
                origin: Origin::List,
            });
            self.current_font = Some(self.out.fonts.len() - 1);
        }
    }

    /// `- text: #0c0a08` or `- primary action: #e4f222 (filled action)`. A value
    /// followed by more prose isn't a token: "Content card: #fff fill, 1px border".
    fn color_line(&mut self, name: &str, value: &str, offset: usize) -> bool {
        let Some(color) = leading_color(value) else {
            return false;
        };
        let rest = value[color.len()..].trim();
        let closed = rest.is_empty() || (rest.starts_with('(') && rest.ends_with(')'));
        if !closed {
            return false;
        }
        let span = self.locate(color, offset);
        let scheme = self.scheme();
        self.out.colors.push(Token {
            name: format!("{name} {rest}"),
            value: color.to_string(),
            span,
            origin: Origin::List,
            scheme,
        });
        true
    }

    /// `**Primary (#1A1C1E):** A deep ink used for core text.`
    fn parenthesized_color(&mut self, line: &str, offset: usize) -> bool {
        let Some(open) = line.find('(') else {
            return false;
        };
        let Some(close) = line[open..].find(')').map(|i| open + i) else {
            return false;
        };
        let (name, inside) = (line[..open].trim(), &line[open + 1..close]);
        let after = line[close + 1..].trim_start();
        let word_count = name.split_whitespace().count();
        if word_count == 0
            || word_count > 4
            || !after.starts_with(':')
            || Color::parse(inside).is_err()
        {
            return false;
        }
        let description = after[1..].trim().to_string();
        let span = self.locate(inside, offset);
        let scheme = self.scheme();
        self.out
            .descriptions
            .insert(words(name), description.clone());
        self.out.colors.push(Token {
            name: format!("{name} {description}"),
            value: inside.to_string(),
            span,
            origin: Origin::List,
            scheme,
        });
        true
    }

    fn measure_line(&mut self, key: &str, value: &str, offset: usize) -> bool {
        let has = |names: &[&str]| names.iter().any(|n| key.contains(&format!(" {n} ")));
        let page = has(&["max width", "page width", "container", "page max width"]);
        let measure = has(&["measure", "reading width", "content width", "prose width"]);
        if !page && !measure {
            return false;
        }
        let first = value
            .split_whitespace()
            .next()
            .unwrap_or(value)
            .trim_end_matches([',', '.', ';']);
        if Length::parse(first).is_err() {
            return false;
        }
        let token = Token {
            name: key.trim().to_string(),
            value: first.to_string(),
            span: self.locate(first, offset),
            origin: Origin::List,
            scheme: Scheme::Light,
        };
        if measure {
            self.out.measures.push(token);
        } else {
            self.out.page_widths.push(token);
        }
        true
    }

    fn handle_table(&mut self, table: &TableState) {
        let headers: Vec<String> = table.headers.iter().map(|h| words(h)).collect();
        let column = |names: &[&str]| {
            headers
                .iter()
                .position(|h| names.iter().any(|n| h.contains(&format!(" {n} "))))
        };
        let font_column = column(&["font", "family", "typeface"]);
        let weight_column = column(&["weight"]);
        if font_column.is_some() || (weight_column.is_some() && self.in_typography()) {
            self.type_table(table, font_column, weight_column);
        } else if self.in_section(&["radius", "radii", "rounded", "corners", "shape", "shapes"])
            || column(&["radius"]).is_some()
        {
            self.value_table(table, ValueKind::Radius);
        } else if self.in_section(&["shadow", "shadows", "elevation", "depth"])
            || column(&["shadow"]).is_some()
        {
            self.value_table(table, ValueKind::Shadow);
        } else if self.in_section(&["spacing", "space", "layout"]) {
            self.value_table(table, ValueKind::Spacing);
        } else {
            self.color_table(table);
        }
    }

    fn type_table(&mut self, table: &TableState, font: Option<usize>, weight: Option<usize>) {
        for row in &table.rows {
            let name = row.first().map_or("", |c| c.text.as_str());
            let role = font_role_from_key(&words(name));
            if let Some(cell) = font.and_then(|i| row.get(i)) {
                let family = split_font_list(&cell.text)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                if !family.is_empty() {
                    self.out.fonts.push(FontDecl {
                        family,
                        role,
                        substitutes: split_font_list(&cell.text).into_iter().skip(1).collect(),
                        span: self.index.span(cell.offset, cell.text.len()),
                        origin: Origin::Table,
                    });
                }
            }
            if role == FontRole::Heading {
                if let Some(cell) = weight.and_then(|i| row.get(i)) {
                    self.out.heading_weights.push(Token {
                        name: name.to_string(),
                        value: cell.text.clone(),
                        span: self.index.span(cell.offset, cell.text.len()),
                        origin: Origin::Table,
                        scheme: Scheme::Light,
                    });
                }
            }
        }
    }

    fn value_table(&mut self, table: &TableState, kind: ValueKind) {
        for row in &table.rows {
            let parses = |text: &str| match kind {
                ValueKind::Shadow => Shadow::parse(text).is_ok(),
                ValueKind::Radius | ValueKind::Spacing => Length::parse(text).is_ok(),
            };
            let value = row.iter().position(|c| parses(&c.text));
            let name: Vec<&str> = row
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != value)
                .map(|(_, c)| c.text.as_str())
                .collect();
            let name = name.join(" ");
            let Some(cell) = value.map(|i| &row[i]) else {
                // A row whose value looks like a size but doesn't parse.
                let bad = row
                    .iter()
                    .find(|c| c.text.starts_with(|c: char| c.is_ascii_digit()));
                if let Some(cell) = bad {
                    let span = self.index.span(cell.offset, cell.text.len());
                    let reason = "use a single number with px, rem, em or %, such as 16px";
                    self.out.invalid(&name, &cell.text, span, reason);
                }
                continue;
            };
            let span = self.index.span(cell.offset, cell.text.len());
            let token = Token {
                name: name.clone(),
                value: cell.text.clone(),
                span,
                origin: Origin::Table,
                scheme: Scheme::Light,
            };
            match kind {
                ValueKind::Radius => self.out.radii.push(token),
                ValueKind::Shadow => self.out.shadows.push(token),
                ValueKind::Spacing => self.out.unused.push((format!("spacing {name}"), span)),
            }
        }
    }

    fn color_table(&mut self, table: &TableState) {
        let base = self.scheme();
        for row in &table.rows {
            for (i, cell) in row.iter().enumerate() {
                let Some(color) = color_in_cell(&cell.text) else {
                    continue;
                };
                let header = table.headers.get(i).map(|h| words(h)).unwrap_or_default();
                let scheme = if header.contains(" dark ") {
                    Scheme::Dark
                } else if header.contains(" light ") {
                    Scheme::Light
                } else {
                    base
                };
                let others: Vec<&str> = row
                    .iter()
                    .enumerate()
                    .filter(|(j, c)| *j != i && color_in_cell(&c.text).is_none())
                    .map(|(_, c)| c.text.as_str())
                    .collect();
                let offset = cell.offset + self.source[cell.offset..].find(color).unwrap_or(0);
                self.out.colors.push(Token {
                    name: others.join(" "),
                    value: color.to_string(),
                    span: self.index.span(offset, color.len()),
                    origin: Origin::Table,
                    scheme,
                });
            }
        }
    }

    fn handle_code(&mut self, lang: &str, from: usize, to: usize) {
        let text = &self.source[from..to];
        match lang.split_whitespace().next().unwrap_or("") {
            "css" | "scss" => css::extract(text, from, self.index, self.out),
            "yaml" | "yml" => {
                let line = self.index.span(from, 1).line;
                yaml::extract(text, line - 1, self.out);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
enum ValueKind {
    Radius,
    Shadow,
    Spacing,
}

/// Splits `Key: value`, ignoring colons inside the value.
fn split_key_value(line: &str) -> (Option<&str>, Option<&str>) {
    match line.split_once(':') {
        Some((key, value)) if key.split_whitespace().count() <= 6 => {
            (Some(key.trim()), Some(value.trim()))
        }
        _ => (None, None),
    }
}

fn font_role_from_key(key: &str) -> FontRole {
    let has = |names: &[&str]| names.iter().any(|n| key.contains(&format!(" {n} ")));
    if has(&["mono", "monospace", "code"]) {
        FontRole::Mono
    } else if has(&[
        "heading",
        "headings",
        "headline",
        "headlines",
        "display",
        "title",
        "titles",
        "h1",
        "h2",
        "h3",
    ]) {
        FontRole::Heading
    } else if has(&["body", "text", "paragraph", "label", "labels", "caption"]) {
        FontRole::Body
    } else {
        FontRole::Any
    }
}

/// Splits `Inter, IBM Plex Sans, or Söhne` into names.
pub(crate) fn split_font_list(value: &str) -> Vec<String> {
    value
        .split([',', '/', ';'])
        .flat_map(|part| part.split(" or "))
        .flat_map(|part| part.split(" and "))
        .map(|part| {
            let part = part.split(['(', '—', '–']).next().unwrap_or(part);
            let part = part.trim().trim_matches(['"', '\'', '`', '.']).trim();
            part.strip_prefix("or ").unwrap_or(part).trim().to_string()
        })
        .filter(|part| !part.is_empty())
        .collect()
}

/// The color at the very start of `value`, if there is one.
fn leading_color(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let end = if value.starts_with('#') {
        value
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '#')
            .unwrap_or(value.len())
    } else if ["rgb", "hsl", "oklch"].iter().any(|f| value.starts_with(f)) {
        value.find(')')? + 1
    } else {
        return None;
    };
    Color::parse(&value[..end]).ok().map(|_| &value[..end])
}

/// A cell that is a color, or holds one among other text such as "`#fff` white".
fn color_in_cell(text: &str) -> Option<&str> {
    if let Some(color) = leading_color(text) {
        return Some(color);
    }
    let hash = text.find('#')?;
    leading_color(&text[hash..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Extracted {
        let index = LineIndex::new(source);
        let mut out = Extracted::default();
        extract(source, 0, &index, &mut out);
        out
    }

    fn colors(out: &Extracted) -> Vec<(&str, &str)> {
        out.colors
            .iter()
            .map(|c| (c.name.trim(), c.value.as_str()))
            .collect()
    }

    #[test]
    fn reads_a_quick_color_reference_list() {
        let out = run(r"
Quick Color Reference:
- text: #0c0a08
- background: #f4f2f0
- card surface: #ffffff
- primary action: #e4f222 (filled action)

Example Component Prompts:
1. Content card: #ffffff fill, 1px #e5e7eb border, 16px radius.
2. Hero headline: 64px lausanne weight 400, #0c0a08.
");
        assert_eq!(
            colors(&out),
            [
                ("text", "#0c0a08"),
                ("background", "#f4f2f0"),
                ("card surface", "#ffffff"),
                ("primary action (filled action)", "#e4f222"),
            ]
        );
        assert_eq!(
            out.colors[0].span,
            Span {
                line: 3,
                column: 9,
                len: 7
            }
        );
        assert_eq!(out.colors[0].origin, Origin::List);
    }

    #[test]
    fn reads_a_role_table() {
        let out = run(r"
## Colors

| Name | Value | Token | Role |
|---|---|---|---|
| Ink | `#0c0a08` | `--color-ink` | Primary text, headlines |
| Paper | #f4f2f0 | `--color-paper` | Page canvas |
| Broken | notacolor | `--color-x` | Nothing |
");
        assert_eq!(out.colors.len(), 2);
        assert_eq!(out.colors[0].value, "#0c0a08");
        assert_eq!(
            out.colors[0].name,
            "Ink --color-ink Primary text, headlines"
        );
        assert_eq!(out.colors[0].origin, Origin::Table);
        assert_eq!(out.colors[1].span.line, 7);
        assert_eq!(out.colors[1].span.column, 11);
    }

    #[test]
    fn reads_light_and_dark_columns_and_sections() {
        let out = run(r"
| Role | Light | Dark |
|---|---|---|
| Text | #111111 | #eeeeee |

## Dark mode

- background: #000000
");
        let found: Vec<_> = out
            .colors
            .iter()
            .map(|c| (c.value.as_str(), c.scheme))
            .collect();
        assert_eq!(
            found,
            [
                ("#111111", Scheme::Light),
                ("#eeeeee", Scheme::Dark),
                ("#000000", Scheme::Dark)
            ]
        );
    }

    #[test]
    fn reads_spec_style_prose_colors() {
        let out = run("## Colors\n\n- **Primary (#1A1C1E):** A deep ink used for core text.\n- **Tertiary (#B8422E):** The sole driver for interaction.\n");
        assert_eq!(out.colors[0].value, "#1A1C1E");
        assert!(out.colors[0].name.contains("deep ink"));
        assert_eq!(
            out.descriptions[&words("Primary")],
            "A deep ink used for core text."
        );
    }

    #[test]
    fn reads_font_sections_and_substitutes() {
        let out = run(r"
## Typography

### Lausanne — primary

**Substitute:** Inter, IBM Plex Sans, or Söhne
**Weights:** 400

### Type Scale

Some words about scale.

Mono font: Berkeley Mono
");
        assert_eq!(out.fonts[0].family, "Lausanne");
        assert_eq!(
            out.fonts[0].substitutes,
            ["Inter", "IBM Plex Sans", "Söhne"]
        );
        assert_eq!(out.heading_weights[0].value, "400");
        assert_eq!(out.fonts[1].family, "Berkeley Mono");
        assert_eq!(out.fonts[1].role, FontRole::Mono);
    }

    #[test]
    fn reads_type_radius_shadow_and_spacing_tables() {
        let out = run(r"
## Typography

| Role | Font | Size | Weight |
|---|---|---|---|
| Heading | Tiempos | 48px | 500 |
| Body | Inter | 16px | 400 |

## Shapes

| Name | Radius |
|---|---|
| sm | 4px |
| md | 8px |

## Elevation

| Level | Shadow |
|---|---|
| card | 0 1px 2px #00000020 |

## Layout

| Name | Value |
|---|---|
| section | 64-128px |
| md | 16px |
");
        assert_eq!(out.fonts[0].family, "Tiempos");
        assert_eq!(out.fonts[0].role, FontRole::Heading);
        assert_eq!(out.fonts[1].role, FontRole::Body);
        assert_eq!(out.heading_weights.len(), 1);
        assert_eq!(out.heading_weights[0].value, "500");
        let radii: Vec<_> = out.radii.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(radii, ["4px", "8px"]);
        assert_eq!(out.shadows[0].value, "0 1px 2px #00000020");
        assert_eq!(out.diagnostics.len(), 1);
        assert!(out.diagnostics[0].message.contains("64-128px"));
    }

    #[test]
    fn reads_theme_and_name() {
        let out = run("# Ramp\n\n**Theme:** dark\n\nWords.\n");
        assert_eq!(out.name.as_deref(), Some("Ramp"));
        assert_eq!(out.theme, Some(Scheme::Dark));
        assert_eq!(run("**Theme:** light mode\n").theme, Some(Scheme::Light));
    }

    #[test]
    fn reads_which_schemes_a_design_supports() {
        assert_eq!(
            run("**Light/Dark:** ✗ No / ✓ Full\n").theme,
            Some(Scheme::Dark)
        );
        assert_eq!(
            run("**Light/Dark:** ✓ Full / ✗ No\n").theme,
            Some(Scheme::Light)
        );
        assert_eq!(run("**Light/Dark:** ✓ Full / ✓ Full\n").theme, None);
    }

    #[test]
    fn reads_fenced_blocks_with_correct_positions() {
        let out = run("# X\n\n```css\n:root {\n  --ink: #111111;\n}\n```\n\n```yaml\ncolors:\n  neutral: \"#222222\"\n```\n\n```js\nconst a = '#333333';\n```\n");
        assert_eq!(colors(&out), [("--ink", "#111111"), ("neutral", "#222222")]);
        assert_eq!(out.colors[0].span.line, 5);
        assert_eq!(out.colors[0].span.column, 10);
        assert_eq!(out.colors[1].span.line, 11);
    }

    #[test]
    fn keeps_other_prose_for_rule_checking() {
        let out = run("## Do's and Don'ts\n\n- Never use bold.\n\nA paragraph\nwith two lines.\n");
        let lines: Vec<_> = out.prose.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(lines, ["Never use bold.", "A paragraph", "with two lines."]);
        assert_eq!(out.prose[0].context, "do's and don'ts");
        assert_eq!(out.prose[0].span.line, 3);
    }

    #[test]
    fn splits_font_lists() {
        assert_eq!(
            split_font_list("Inter, IBM Plex Sans, or Söhne"),
            ["Inter", "IBM Plex Sans", "Söhne"]
        );
        assert_eq!(split_font_list("Inter or Roboto"), ["Inter", "Roboto"]);
        assert_eq!(split_font_list("\"Inter\" (free)"), ["Inter"]);
    }

    #[test]
    fn survives_unusual_markdown() {
        for source in [
            "",
            "|",
            "| a |\n|---|\n",
            "| a |\n|---|\n| |\n",
            "- ",
            "- - - x",
            "# ",
            "```",
            "```css\n",
            "```yaml\n{{{",
            "Theme:",
            ":",
            "(#fff):",
            "é (é): é",
            "**(",
        ] {
            run(source);
        }
    }
}
