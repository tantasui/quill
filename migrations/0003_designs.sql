-- Imported DESIGN.md files. The source is kept so a design can be recompiled by
-- a later version, and `compiled` is JSON validated by quill-design when read.
-- A row with no `applied_at` is a draft being reviewed; the rest are history.
CREATE TABLE designs (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    source      TEXT NOT NULL,
    compiled    TEXT NOT NULL,
    diagnostics TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    applied_at  TEXT,
    is_active   INTEGER NOT NULL DEFAULT 0
);

-- At most one design is live, and the database enforces it.
CREATE UNIQUE INDEX designs_one_active ON designs (is_active) WHERE is_active = 1;
