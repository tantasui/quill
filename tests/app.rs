//! End-to-end tests: real router, real templates, in-memory database.

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use quill::{db, AppState};

const SETUP_CODE: &str = "TEST-CODE";
const PASSWORD: &str = "a long enough password";

struct TestApp {
    app: Router,
    state: AppState,
    cookie: Option<String>,
    csrf: String,
    _dir: tempfile::TempDir,
}

struct Res {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Res {
    fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn extract_csrf(html: &str) -> String {
    let marker = r#"name="_csrf" value=""#;
    let start = html.find(marker).expect("csrf field") + marker.len();
    html[start..start + 64].to_string()
}

impl TestApp {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::connect_memory().await.unwrap();
        db::migrate(&pool, None).await.unwrap();
        let state = AppState::new(
            pool,
            dir.path().to_path_buf(),
            "127.0.0.1:3000".into(),
            Some(SETUP_CODE.into()),
        )
        .await
        .unwrap();
        TestApp {
            app: quill::app(state.clone()),
            state,
            cookie: None,
            csrf: String::new(),
            _dir: dir,
        }
    }

    /// A blog that has completed setup at https://example.com/blog, signed in.
    async fn ready() -> Self {
        let mut t = Self::new().await;
        let res = t
            .post_form(
                "/admin/setup",
                &[
                    ("setup_code", "test-code"),
                    ("site_name", "Test Blog"),
                    ("base_url", "https://example.com/blog/"),
                    ("username", "admin"),
                    ("password", PASSWORD),
                    ("password_confirm", PASSWORD),
                    ("theme", "editorial"),
                ],
            )
            .await;
        assert_eq!(res.status, StatusCode::SEE_OTHER, "{}", res.body);
        assert_eq!(res.location(), "/blog/admin?notice=welcome");
        t.take_cookie(&res);
        t.refresh_csrf().await;
        t
    }

    fn take_cookie(&mut self, res: &Res) {
        let set = res
            .headers
            .get(header::SET_COOKIE)
            .expect("session cookie")
            .to_str()
            .unwrap();
        self.cookie = Some(set.split(';').next().unwrap().to_string());
    }

    async fn refresh_csrf(&mut self) {
        let res = self.get("/blog/admin").await;
        assert_eq!(res.status, StatusCode::OK);
        self.csrf = extract_csrf(&res.body);
    }

    async fn send(&self, req: Request<Body>) -> Res {
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        Res {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    fn builder(&self, method: Method, uri: &str) -> axum::http::request::Builder {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "example.com");
        if let Some(c) = &self.cookie {
            b = b.header(header::COOKIE, c);
        }
        b
    }

    async fn get(&self, uri: &str) -> Res {
        self.send(self.builder(Method::GET, uri).body(Body::empty()).unwrap())
            .await
    }

    async fn post_form(&self, uri: &str, pairs: &[(&str, &str)]) -> Res {
        let req = self
            .builder(Method::POST, uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form(pairs)))
            .unwrap();
        self.send(req).await
    }

    async fn create_article(&self, title: &str, content: &str, extra: &[(&str, &str)]) -> Res {
        let mut pairs = vec![
            ("_csrf", self.csrf.as_str()),
            ("title", title),
            ("content", content),
            ("action", "publish"),
        ];
        pairs.extend_from_slice(extra);
        self.post_form("/blog/admin/articles", &pairs).await
    }
}

#[tokio::test]
async fn fresh_install_requires_setup_code() {
    let t = TestApp::new().await;
    let home = t.get("/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains("almost ready"));

    let admin = t.get("/admin").await;
    assert_eq!(admin.location(), "/admin/setup");

    let form = t.get("/admin/setup").await;
    assert_eq!(form.status, StatusCode::OK);
    assert!(
        form.body.contains("http://example.com"),
        "base URL guessed from Host header"
    );

    let wrong = t
        .post_form(
            "/admin/setup",
            &[
                ("setup_code", "WRONG-CODE"),
                ("site_name", "X"),
                ("base_url", "https://example.com"),
                ("username", "admin"),
                ("password", PASSWORD),
                ("password_confirm", PASSWORD),
            ],
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(wrong.body.contains("setup code doesn"));
    assert!(t.state.needs_setup());
}

#[tokio::test]
async fn publish_and_read_under_base_path() {
    let t = TestApp::ready().await;
    assert!(!t.state.needs_setup());
    assert_eq!(t.state.settings().base_path, "/blog");

    let res = t
        .create_article(
            "Hello, World!",
            "First paragraph.\n\n<script>alert('x')</script>\n\n![cat](/media/cat.png)",
            &[("tags", "Rust, Web Dev, rust")],
        )
        .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER, "{}", res.body);
    assert_eq!(
        res.location(),
        "/blog/admin/articles/1/edit?notice=published"
    );

    let home = t.get("/blog/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains(r#"href="/blog/posts/hello-world""#));
    assert!(home.body.contains("/blog/static/themes/editorial.css"));
    assert!(home
        .body
        .contains(r#"<link rel="canonical" href="https://example.com/blog/">"#));

    let article = t.get("/blog/posts/hello-world").await;
    assert_eq!(article.status, StatusCode::OK);
    assert!(article.body.contains("First paragraph."));
    assert!(
        !article.body.contains("<script>alert"),
        "markdown must be sanitized"
    );
    assert!(
        article.body.contains(r#"src="/blog/media/cat.png""#),
        "media paths get the base path"
    );
    assert!(article
        .body
        .contains("https://example.com/blog/posts/hello-world"));
    assert!(article.body.contains(r#"href="/blog/tags/rust""#));
    assert!(article.body.contains("Web Dev"));

    // A reverse proxy that strips the prefix works too.
    assert_eq!(t.get("/posts/hello-world").await.status, StatusCode::OK);
    // The bare base path serves the home page.
    assert_eq!(t.get("/blog").await.status, StatusCode::OK);

    let tag = t.get("/blog/tags/rust").await;
    assert_eq!(tag.status, StatusCode::OK);
    assert!(tag.body.contains("Hello, World!"));
    assert_eq!(
        t.get("/blog/tags/missing").await.status,
        StatusCode::NOT_FOUND
    );

    let feed = t.get("/blog/feed.xml").await;
    assert_eq!(feed.status, StatusCode::OK);
    assert!(feed.headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("application/atom+xml"));
    assert!(feed
        .body
        .contains("<id>https://example.com/blog/posts/hello-world</id>"));
    assert!(
        feed.body.contains("https://example.com/blog/media/cat.png"),
        "feeds use absolute media URLs"
    );

    let sitemap = t.get("/blog/sitemap.xml").await;
    assert!(sitemap
        .body
        .contains("<loc>https://example.com/blog/posts/hello-world</loc>"));
    let robots = t.get("/blog/robots.txt").await;
    assert!(robots
        .body
        .contains("Sitemap: https://example.com/blog/sitemap.xml"));

    let archive = t.get("/blog/archive").await;
    assert!(archive.body.contains("Hello, World!"));

    let missing = t.get("/blog/posts/nope").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(missing.body.contains("doesn't exist"));
}

#[tokio::test]
async fn drafts_and_scheduled_articles_stay_private() {
    let t = TestApp::ready().await;
    let res = t
        .post_form(
            "/blog/admin/articles",
            &[
                ("_csrf", &t.csrf),
                ("title", "Secret draft"),
                ("content", "x"),
                ("action", "draft"),
            ],
        )
        .await;
    assert_eq!(res.location(), "/blog/admin/articles/1/edit?notice=saved");
    assert_eq!(
        t.get("/blog/posts/secret-draft").await.status,
        StatusCode::NOT_FOUND
    );
    assert!(!t.get("/blog/").await.body.contains("Secret draft"));

    let res = t
        .create_article(
            "Future post",
            "soon",
            &[("published_at", "2999-01-01T09:00"), ("tz_offset", "0")],
        )
        .await;
    assert!(
        res.location().ends_with("notice=scheduled"),
        "{}",
        res.location()
    );
    assert_eq!(
        t.get("/blog/posts/future-post").await.status,
        StatusCode::NOT_FOUND
    );
    assert!(!t.get("/blog/feed.xml").await.body.contains("Future post"));

    let list = t.get("/blog/admin/articles?status=scheduled").await;
    assert!(list.body.contains("Future post"));
    assert!(!list.body.contains("Secret draft"));

    // Signed-out visitors can't reach the editor.
    let mut anon = t.get("/blog/admin/articles/1/edit").await;
    assert_eq!(anon.status, StatusCode::OK);
    let t2 = TestApp { cookie: None, ..t };
    anon = t2.get("/blog/admin/articles/1/edit").await;
    assert_eq!(anon.location(), "/blog/admin/login");
}

#[tokio::test]
async fn renamed_slugs_redirect() {
    let t = TestApp::ready().await;
    t.create_article("Original title", "body", &[]).await;
    let res = t
        .post_form(
            "/blog/admin/articles/1",
            &[
                ("_csrf", &t.csrf),
                ("title", "Original title"),
                ("slug", "Better Slug"),
                ("content", "body"),
                ("action", "publish"),
            ],
        )
        .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER, "{}", res.body);
    let old = t.get("/blog/posts/original-title").await;
    assert_eq!(old.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(old.location(), "/blog/posts/better-slug");
    assert_eq!(
        t.get("/blog/posts/better-slug").await.status,
        StatusCode::OK
    );

    // A new article may take over a retired slug; duplicates get a suffix.
    t.create_article("Original title", "again", &[]).await;
    assert_eq!(
        t.get("/blog/posts/original-title").await.status,
        StatusCode::OK
    );
    t.create_article("Original title", "third", &[]).await;
    assert_eq!(
        t.get("/blog/posts/original-title-2").await.status,
        StatusCode::OK
    );
    let dup = t
        .create_article("Another", "x", &[("slug", "better-slug")])
        .await;
    assert_eq!(dup.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(dup.body.contains("already used"));
}

#[tokio::test]
async fn forms_are_protected() {
    let t = TestApp::ready().await;
    let no_token = t
        .post_form(
            "/blog/admin/articles",
            &[("title", "x"), ("action", "publish")],
        )
        .await;
    assert_eq!(no_token.status, StatusCode::FORBIDDEN);

    let req = t
        .builder(Method::POST, "/blog/admin/articles")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form(&[("_csrf", &t.csrf), ("title", "x")])))
        .unwrap();
    assert_eq!(t.send(req).await.status, StatusCode::FORBIDDEN);

    let res = t.get("/blog/");
    let res = res.await;
    assert_eq!(res.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(res.headers.contains_key(header::CONTENT_SECURITY_POLICY));
    assert_eq!(
        t.get("/blog/admin").await.headers[header::CACHE_CONTROL],
        "no-store"
    );

    assert_eq!(
        t.get("/blog/media/..%2fquill.db").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn login_logout_and_throttling() {
    let mut t = TestApp::ready().await;
    let res = t
        .post_form("/blog/admin/logout", &[("_csrf", &t.csrf)])
        .await;
    assert_eq!(res.location(), "/blog/admin/login");
    assert_eq!(
        t.get("/blog/admin").await.location(),
        "/blog/admin/login",
        "session revoked"
    );

    t.cookie = None;
    let bad = t
        .post_form(
            "/blog/admin/login",
            &[("username", "admin"), ("password", "wrong")],
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    let good = t
        .post_form(
            "/blog/admin/login",
            &[("username", "ADMIN"), ("password", PASSWORD)],
        )
        .await;
    assert_eq!(good.status, StatusCode::SEE_OTHER);
    t.take_cookie(&good);
    assert_eq!(t.get("/blog/admin").await.status, StatusCode::OK);

    for _ in 0..5 {
        t.post_form(
            "/blog/admin/login",
            &[("username", "admin"), ("password", "nope")],
        )
        .await;
    }
    let blocked = t
        .post_form(
            "/blog/admin/login",
            &[("username", "admin"), ("password", PASSWORD)],
        )
        .await;
    assert_eq!(blocked.status, StatusCode::UNAUTHORIZED);
    assert!(blocked.body.contains("Too many failed attempts"));
}

#[tokio::test]
async fn every_theme_renders_every_page() {
    let t = TestApp::ready().await;
    t.create_article(
        "Covered",
        "Some *text*.\n\n```rust\nfn main() {}\n```",
        &[
            ("tags", "one"),
            ("cover_image", "/media/abc.png"),
            ("excerpt", "A dek."),
        ],
    )
    .await;
    t.create_article("Plain", "More text.", &[("tags", "two")])
        .await;
    for theme in quill::themes::ALL {
        let res = t
            .post_form(
                "/blog/admin/themes",
                &[("_csrf", &t.csrf), ("theme", theme.id)],
            )
            .await;
        assert_eq!(res.location(), "/blog/admin/themes?notice=theme");
        for path in [
            "/blog/",
            "/blog/posts/covered",
            "/blog/posts/plain",
            "/blog/tags/one",
            "/blog/archive",
        ] {
            let page = t.get(path).await;
            assert_eq!(page.status, StatusCode::OK, "{} {path}", theme.id);
            assert!(
                page.body
                    .contains(&format!("/static/themes/{}.css", theme.id)),
                "{} {path}",
                theme.id
            );
        }
        assert_eq!(t.get("/blog/nowhere").await.status, StatusCode::NOT_FOUND);
        let preview = t
            .get(&format!("/blog/admin/themes/{}/preview", theme.id))
            .await;
        assert_eq!(preview.status, StatusCode::OK);
        assert!(preview.body.contains("Previewing"));
    }
    let css = t.get("/blog/static/themes/gazette.css").await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(css.headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/css"));
}

#[tokio::test]
async fn dashboard_pages_render() {
    let t = TestApp::ready().await;
    t.create_article("Something", "text", &[]).await;
    for path in [
        "/blog/admin",
        "/blog/admin/articles",
        "/blog/admin/articles/new",
        "/blog/admin/articles/1/edit",
        "/blog/admin/themes",
        "/blog/admin/settings",
        "/blog/admin/integration",
        "/blog/admin/account",
        "/blog/admin/media",
    ] {
        let res = t.get(path).await;
        assert_eq!(res.status, StatusCode::OK, "{path}: {}", res.body);
    }
    let integration = t.get("/blog/admin/integration").await;
    assert!(integration.body.contains("location /blog/ {"));
    assert_eq!(
        t.get("/blog/admin/articles/99/edit").await.status,
        StatusCode::NOT_FOUND
    );

    let preview = t
        .post_form(
            "/blog/admin/preview",
            &[
                ("_csrf", &t.csrf),
                ("content", "**bold** <script>x</script>"),
            ],
        )
        .await;
    assert_eq!(preview.body.trim(), "<p><strong>bold</strong> </p>");

    let full = t
        .post_form(
            "/blog/admin/articles/preview",
            &[("_csrf", &t.csrf), ("title", "Unsaved"), ("content", "hi")],
        )
        .await;
    assert_eq!(full.status, StatusCode::OK);
    assert!(full.body.contains("been saved"));
    assert_eq!(
        t.get("/blog/posts/unsaved").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn settings_change_moves_the_blog() {
    let t = TestApp::ready().await;
    let bad = t
        .post_form(
            "/blog/admin/settings",
            &[
                ("_csrf", &t.csrf),
                ("site_name", "New"),
                ("base_url", "ftp://x"),
                ("posts_per_page", "10"),
                ("language", "en"),
            ],
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = t
        .post_form(
            "/blog/admin/settings",
            &[
                ("_csrf", &t.csrf),
                ("site_name", "Renamed"),
                ("base_url", "https://blog.example.com"),
                ("posts_per_page", "5"),
                ("language", "fr"),
            ],
        )
        .await;
    assert_eq!(ok.location(), "/admin/settings?notice=settings");
    let home = t.get("/").await;
    assert!(home.body.contains("Renamed"));
    assert!(home.body.contains(r#"<html lang="fr">"#));
}

#[tokio::test]
async fn upload_and_delete_media() {
    let t = TestApp::ready().await;
    let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\rIDATx\x9cc\xf8\x0f\0\0\x01\x01\0\x05\x18\xd8N\0\0\0\0IEND\xaeB`\x82";
    let boundary = "XBOUNDARYX";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{}\r\n",
            t.csrf
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"dot.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = t
        .builder(Method::POST, "/blog/admin/media")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::ACCEPT, "application/json")
        .body(Body::from(body))
        .unwrap();
    let res = t.send(req).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let json: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    let url = json["url"].as_str().unwrap();
    assert!(url.starts_with("/blog/media/") && url.ends_with(".png"));

    let file = t.get(url).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.headers[header::CONTENT_TYPE], "image/png");

    let res = t
        .post_form("/blog/admin/media/1/delete", &[("_csrf", &t.csrf)])
        .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(t.get(url).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn backup_restore_and_export_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(data.join("media")).unwrap();
    std::fs::write(data.join("media/abc.png"), b"img").unwrap();
    {
        let pool = db::connect(&data.join(db::DB_FILE)).await.unwrap();
        db::migrate(&pool, None).await.unwrap();
        let input = quill::models::ArticleInput {
            title: "Kept \"safe\"".into(),
            slug: "kept".into(),
            content: "Body".into(),
            status: "published".into(),
            published_at: Some(chrono::Utc::now()),
            tags: vec!["a".into()],
            ..Default::default()
        };
        sqlx::query("INSERT INTO users (username, password_hash, created_at) VALUES ('u', 'x', '2026-01-01')")
            .execute(&pool)
            .await
            .unwrap();
        quill::models::create(&pool, &input, 1).await.unwrap();
        pool.close().await;
    }

    let backup = quill::backup::backup(&data, None).await.unwrap();
    assert!(backup.join("quill.db").exists());
    assert!(backup.join("media/abc.png").exists());

    std::fs::remove_file(data.join("media/abc.png")).unwrap();
    let aside = quill::backup::restore(&data, &backup).await.unwrap();
    assert!(aside.join("quill.db").exists());
    assert!(data.join("media/abc.png").exists());
    let pool = db::connect(&data.join(db::DB_FILE)).await.unwrap();
    assert_eq!(quill::models::all_articles(&pool).await.unwrap().len(), 1);
    pool.close().await;

    let out = dir.path().join("export");
    assert_eq!(quill::backup::export(&data, &out).await.unwrap(), 1);
    let md = std::fs::read_to_string(out.join("kept.md")).unwrap();
    assert!(md.starts_with("---\ntitle: \"Kept \\\"safe\\\"\"\n"));
    assert!(md.contains("tags: [\"a\"]"));
    assert!(md.ends_with("Body\n"));
}

fn customized_layout() -> quill_design::Layout {
    let mut draft = quill::themes::find("editorial")
        .unwrap()
        .layout()
        .to_draft();
    draft.header.variant = quill_design::HeaderVariant::Masthead;
    draft.try_into().unwrap()
}

#[tokio::test]
async fn customized_layouts_survive_a_restart_and_are_dropped_by_changing_preset() {
    let t = TestApp::ready().await;
    t.state.save_layout(customized_layout()).await.unwrap();
    assert!(t.state.has_custom_layout());
    assert!(t.get("/blog/").await.body.contains("masthead-title"));

    let restarted = AppState::new(
        t.state.db.clone(),
        t.state.data_dir.clone(),
        t.state.listen.clone(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(restarted.look().layout, customized_layout());

    let res = t
        .post_form(
            "/blog/admin/themes",
            &[("_csrf", &t.csrf), ("theme", "devlog")],
        )
        .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert!(!t.state.has_custom_layout());
    assert!(t.get("/blog/").await.body.contains(r#"class="topbar""#));
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM site_layout")
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn a_corrupt_stored_layout_falls_back_to_the_preset() {
    let t = TestApp::ready().await;
    sqlx::query(
        "INSERT INTO site_layout (id, preset, layout, updated_at)
         VALUES (1, 'editorial', '{\"header\": \"not a layout\"}', '2026-01-01')",
    )
    .execute(&t.state.db)
    .await
    .unwrap();
    let restarted = AppState::new(
        t.state.db.clone(),
        t.state.data_dir.clone(),
        t.state.listen.clone(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        restarted.look().layout,
        *quill::themes::find("editorial").unwrap().layout()
    );
}

#[tokio::test]
async fn upgrading_a_0_1_database_keeps_content_and_takes_a_snapshot() {
    use std::borrow::Cow;

    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&dir.path().join(db::DB_FILE)).await.unwrap();
    // Build the database exactly as 0.1 left it: only its one migration applied.
    let first_only = sqlx::migrate::Migrator {
        migrations: Cow::Owned(vec![db::MIGRATOR.migrations[0].clone()]),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    first_only.run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO articles (title, slug, created_at, updated_at)
         VALUES ('Kept', 'kept', '2026-01-01', '2026-01-01')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let backups = dir.path().join("backups");
    db::migrate(&pool, Some(&backups)).await.unwrap();

    let title: String = sqlx::query_scalar("SELECT title FROM articles")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, "Kept");
    let snapshots: Vec<_> = std::fs::read_dir(&backups).unwrap().collect();
    assert_eq!(snapshots.len(), 1, "a snapshot is written before migrating");
    sqlx::query("SELECT COUNT(*) FROM site_layout")
        .fetch_one(&pool)
        .await
        .unwrap();
}

const HEADERS: [&str; 3] = ["centered", "left", "masthead"];
const WIDTHS: [&str; 3] = ["narrow", "regular", "wide"];
const RAILS: [&str; 3] = ["left", "right", "none"];
const FEEDS: [&str; 4] = ["list", "grid", "cards", "compact"];
const MEASURES: [&str; 2] = ["narrow", "wide"];
const COVERS: [&str; 3] = ["hero", "inline", "none"];
const FOOTERS: [&str; 2] = ["simple", "columns"];

/// A layout form with every required field, for the tests to adjust.
fn layout_form<'a>(csrf: &'a str, overrides: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut fields = vec![
        ("_csrf", csrf),
        ("page_width", "regular"),
        ("header_variant", "centered"),
        ("tag_rail", "none"),
        ("footer_variant", "simple"),
        ("feed", "on"),
        ("feed_order", "2"),
        ("feed_variant", "list"),
        ("article_measure", "narrow"),
        ("article_cover", "inline"),
        ("show_author", "on"),
        ("show_date", "on"),
        ("show_reading_time", "on"),
    ];
    for (name, value) in overrides {
        fields.retain(|(field, _)| field != name);
        fields.push((name, value));
    }
    fields
}

#[tokio::test]
async fn the_layout_form_saves_and_orders_sections() {
    let t = TestApp::ready().await;
    t.create_article("Lead", "Text.", &[("tags", "one")]).await;
    t.create_article("Second", "Text.", &[("tags", "one")])
        .await;

    let page = t.get("/blog/admin/appearance").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("layout-preview"));

    // The feed comes first and the lead story after it: the order is the user's.
    let fields = layout_form(
        &t.csrf,
        &[
            ("header_variant", "masthead"),
            ("tag_rail", "left"),
            ("feed_variant", "cards"),
            ("feed_order", "1"),
            ("featured", "on"),
            ("featured_order", "2"),
            ("archive_link", "on"),
            ("archive_link_order", "3"),
            ("show_date", ""),
        ],
    );
    let res = t.post_form("/blog/admin/appearance", &fields).await;
    assert_eq!(res.location(), "/blog/admin/appearance?notice=layout");

    let home = t.get("/blog/").await.body;
    assert!(home.contains("masthead-title"));
    assert!(home.contains("rail-left"));
    let (feed, lead, archive) = (
        home.find("post-cards").unwrap(),
        home.find("lead-story").unwrap(),
        home.find("archive-link").unwrap(),
    );
    assert!(
        feed < lead && lead < archive,
        "sections follow the saved order"
    );

    let article = t.get("/blog/posts/second").await.body;
    assert!(!article.contains("<time"), "the date can be hidden");
    assert!(article.contains("min read"));

    // The editor shows what was saved.
    let page = t.get("/blog/admin/appearance").await.body;
    assert!(page.contains("Active, customized"));
    assert!(page.find(r#"data-section="feed""#) < page.find(r#"data-section="featured""#));
}

#[tokio::test]
async fn the_layout_form_rejects_invalid_input_with_a_clear_error() {
    let t = TestApp::ready().await;
    let before = t.state.look().layout;
    let cases: [(&[(&str, &str)], &str); 3] = [
        (&[("header_variant", "zigzag")], "zigzag"),
        (&[("feed_variant", "mosaic")], "mosaic"),
        (&[("feed", "")], "needs a `feed` section"),
    ];
    for (overrides, expected) in cases {
        let res = t
            .post_form("/blog/admin/appearance", &layout_form(&t.csrf, overrides))
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{expected}");
        assert!(res.body.contains(expected), "{expected}");
    }
    assert_eq!(t.state.look().layout, before, "nothing was saved");

    let no_token = t
        .post_form("/blog/admin/appearance", &layout_form("", &[]))
        .await;
    assert_eq!(no_token.status, StatusCode::FORBIDDEN);
    let no_token = t
        .post_form("/blog/admin/appearance/preview", &layout_form("", &[]))
        .await;
    assert_eq!(no_token.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn previewing_a_layout_saves_nothing() {
    let t = TestApp::ready().await;
    let before = t.state.look().layout;

    // With nothing published, previews fall back to sample content.
    for page in ["home", "article"] {
        let res = t
            .post_form(
                "/blog/admin/appearance/preview",
                &layout_form(&t.csrf, &[("page", page), ("header_variant", "left")]),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{page}: {}", res.body);
        assert!(res.body.contains("Welcome to your new blog"), "{page}");
        assert!(res.body.contains(r#"class="topbar""#), "{page}");
        assert!(res.body.contains("been saved"), "{page}");
    }
    assert_eq!(t.state.look().layout, before);
    assert!(!t.get("/blog/").await.body.contains(r#"class="topbar""#));
}

/// A small deterministic generator, so a failing combination can be reproduced.
struct Sequence(u64);

impl Sequence {
    fn pick<'a>(&mut self, options: &[&'a str]) -> &'a str {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        options[((self.0 >> 33) as usize) % options.len()]
    }
}

#[tokio::test]
async fn many_layout_combinations_render_on_every_page() {
    let t = TestApp::ready().await;
    t.create_article(
        "Covered",
        "Some *text*.",
        &[("tags", "one"), ("cover_image", "/media/abc.png")],
    )
    .await;
    t.create_article("Plain", "More text.", &[("tags", "one")])
        .await;

    let mut sequence = Sequence(2026);
    for round in 0..250 {
        let header = sequence.pick(&HEADERS);
        let feed = sequence.pick(&FEEDS);
        let fields = layout_form(
            &t.csrf,
            &[
                ("header_variant", header),
                ("page_width", sequence.pick(&WIDTHS)),
                ("tag_rail", sequence.pick(&RAILS)),
                ("feed_variant", feed),
                ("article_measure", sequence.pick(&MEASURES)),
                ("article_cover", sequence.pick(&COVERS)),
                ("footer_variant", sequence.pick(&FOOTERS)),
                ("featured", sequence.pick(&["on", ""])),
                ("featured_order", sequence.pick(&["1", "3"])),
                ("archive_link", sequence.pick(&["on", ""])),
                ("archive_link_order", sequence.pick(&["1", "3"])),
                ("show_author", sequence.pick(&["on", ""])),
                ("show_date", sequence.pick(&["on", ""])),
                ("show_reading_time", sequence.pick(&["on", ""])),
            ],
        );
        let res = t.post_form("/blog/admin/appearance", &fields).await;
        assert_eq!(
            res.status,
            StatusCode::SEE_OTHER,
            "{fields:?}: {}",
            res.body
        );
        for path in [
            "/blog/",
            "/blog/posts/covered",
            "/blog/posts/plain",
            "/blog/tags/one",
            "/blog/archive",
        ] {
            let page = t.get(path).await;
            assert_eq!(
                page.status,
                StatusCode::OK,
                "round {round} {path} {fields:?}"
            );
            assert!(page.body.contains("</html>"), "round {round} {path}");
            assert!(page.body.contains(&format!("header-{header}")));
        }
        assert_eq!(t.get("/blog/nowhere").await.status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn every_variant_of_every_component_appears() {
    let t = TestApp::ready().await;
    t.create_article("One", "Text.", &[("tags", "tagged")])
        .await;
    t.create_article("Two", "Text.", &[("tags", "tagged")])
        .await;

    for (header, marker) in HEADERS.iter().zip(["site-header", "topbar", "masthead"]) {
        let fields = layout_form(&t.csrf, &[("header_variant", header)]);
        t.post_form("/blog/admin/appearance", &fields).await;
        assert!(t.get("/blog/").await.body.contains(marker), "{header}");
    }
    for (feed, marker) in FEEDS
        .iter()
        .zip(["post-list", "story-grid", "post-cards", "post-rows"])
    {
        let fields = layout_form(&t.csrf, &[("feed_variant", feed)]);
        t.post_form("/blog/admin/appearance", &fields).await;
        assert!(t.get("/blog/").await.body.contains(marker), "{feed}");
    }
    for (footer, marker) in FOOTERS.iter().zip(["<p>©", "footer-columns"]) {
        let fields = layout_form(&t.csrf, &[("footer_variant", footer)]);
        t.post_form("/blog/admin/appearance", &fields).await;
        assert!(t.get("/blog/").await.body.contains(marker), "{footer}");
    }
    for rail in RAILS {
        let fields = layout_form(&t.csrf, &[("tag_rail", rail)]);
        t.post_form("/blog/admin/appearance", &fields).await;
        let home = t.get("/blog/").await.body;
        assert!(home.contains(&format!("rail-{rail}")));
        assert_eq!(home.contains("side-block"), rail != "none", "{rail}");
    }
}

#[tokio::test]
async fn presets_serve_generated_tokens() {
    let t = TestApp::ready().await;
    for preset in quill::themes::ALL {
        let css = t
            .get(&format!("/blog/static/tokens/{}.css", preset.id))
            .await;
        assert_eq!(css.status, StatusCode::OK);
        assert!(css.headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/css"));
        assert_eq!(css.body, preset.tokens().to_css());
    }
    assert_eq!(
        t.get("/blog/static/tokens/nope.css").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.get("/blog/static/tokens/editorial").await.status,
        StatusCode::NOT_FOUND
    );
    assert!(t
        .get("/blog/")
        .await
        .body
        .contains("/blog/static/tokens/editorial.css"));
}

#[tokio::test]
async fn article_data_never_depends_on_the_look() {
    let t = TestApp::ready().await;
    t.create_article(
        "Stays",
        "Body text.",
        &[("tags", "kept"), ("excerpt", "Sum.")],
    )
    .await;
    async fn snapshot(t: &TestApp) -> Vec<(String, String, String, String, String)> {
        sqlx::query_as("SELECT title, slug, content, excerpt, updated_at FROM articles ORDER BY id")
            .fetch_all(&t.state.db)
            .await
            .unwrap()
    }
    let before = snapshot(&t).await;
    t.post_form(
        "/blog/admin/appearance",
        &layout_form(&t.csrf, &[("header_variant", "masthead")]),
    )
    .await;
    for preset in quill::themes::ALL {
        t.post_form(
            "/blog/admin/themes",
            &[("_csrf", &t.csrf), ("theme", preset.id)],
        )
        .await;
        assert_eq!(t.get("/blog/posts/stays").await.status, StatusCode::OK);
    }
    assert_eq!(snapshot(&t).await, before);
}
