CREATE TABLE IF NOT EXISTS tags
(
    name                TEXT PRIMARY KEY,
    content             BIGINT NOT NULL,
    creator_user_id     BIGINT NOT NULL,
    last_editor_user_id TEXT,
    creation_date       TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    last_edit_date      TIMESTAMPTZ,
    times_used          INT         DEFAULT 0,
    restricted          BOOLEAN     DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS tag_aliases
(
    alias    TEXT PRIMARY KEY,
    tag_name TEXT NOT NULL,
    FOREIGN KEY (tag_name) REFERENCES tags (name) ON DELETE CASCADE
);
