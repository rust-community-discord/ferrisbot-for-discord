-- Create tags table
CREATE TABLE IF NOT EXISTS tags
(
	name                TEXT PRIMARY KEY NOT NULL,
	content             TEXT NOT NULL,
	creator_user_id     INTEGER NOT NULL,
	last_editor_user_id INTEGER,
	creation_date       TEXT DEFAULT CURRENT_TIMESTAMP NOT NULL,
	last_edit_date      TEXT,
	times_used          INTEGER DEFAULT 0 NOT NULL,
	restricted          INTEGER DEFAULT 0 NOT NULL
);

-- Create tag_aliases table
CREATE TABLE IF NOT EXISTS tag_aliases
(
	alias    TEXT PRIMARY KEY NOT NULL,
	tag_name TEXT NOT NULL,
	FOREIGN KEY (tag_name) REFERENCES tags (name) ON DELETE CASCADE
);
