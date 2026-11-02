//! Quill's design model: typed design tokens, layouts and presets.
//!
//! This crate does no I/O and knows nothing about the web server, so the server
//! and the command line share one set of validation rules.

mod color;
pub mod compiler;
mod error;
mod font;
mod layout;
mod length;
pub mod presets;
mod shadow;
mod tokens;

pub use color::Color;
pub use compiler::diagnostic::{Code, Diagnostic, Level, Span};
pub use compiler::{
    compile, compile_bytes, Compilation, CompiledDesign, Scheme, TokenOrigin, MAX_SOURCE_BYTES,
};
pub use error::TokenError;
pub use font::{FontStack, FontWeight};
pub use layout::{
    ArticleLayout, ArticleMeasure, CoverStyle, FeedVariant, Footer, FooterVariant, Header,
    HeaderVariant, HomeSection, Labels, Layout, LayoutDraft, LayoutError, PageWidth, TagRail,
};
pub use length::{Length, Offset};
pub use shadow::Shadow;
pub use tokens::{DesignTokens, Palette};
