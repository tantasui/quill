-- Initial schema. Migrations are embedded in the binary and applied on startup;
-- never edit a migration once it has shipped — add a new one instead.

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'admin',
    created_at    TEXT NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE articles (
    id               INTEGER PRIMARY KEY,
    title            TEXT NOT NULL,
    slug             TEXT NOT NULL UNIQUE,
    content          TEXT NOT NULL DEFAULT '',
    excerpt          TEXT NOT NULL DEFAULT '',
    status           TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published')),
    cover_image      TEXT NOT NULL DEFAULT '',
    meta_title       TEXT NOT NULL DEFAULT '',
    meta_description TEXT NOT NULL DEFAULT '',
    author_id        INTEGER REFERENCES users(id) ON DELETE SET NULL,
    published_at     TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE INDEX articles_public ON articles (status, published_at DESC);

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE article_tags (
    article_id INTEGER NOT NULL REFERENCES articles(id) ON DELETE CASCADE,
    tag_id     INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (article_id, tag_id)
);

-- Old slugs of published articles, so links keep working after a rename.
CREATE TABLE slug_redirects (
    old_slug   TEXT PRIMARY KEY,
    article_id INTEGER NOT NULL REFERENCES articles(id) ON DELETE CASCADE
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE media (
    id            INTEGER PRIMARY KEY,
    filename      TEXT NOT NULL UNIQUE,
    original_name TEXT NOT NULL,
    content_type  TEXT NOT NULL,
    size          INTEGER NOT NULL,
    created_at    TEXT NOT NULL
);
