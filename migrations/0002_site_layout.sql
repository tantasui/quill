-- The blog's customized layout, as JSON validated by the quill-design crate.
-- No row means the active preset's own layout is in use, which is how every
-- blog upgraded from 0.1 starts out. A layout belongs to the preset it was
-- customized from; articles never depend on it.
CREATE TABLE site_layout (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    preset     TEXT NOT NULL,
    layout     TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
