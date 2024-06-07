CREATE TABLE users (
	id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	resonite_id TEXT UNIQUE,
	resonite_name TEXT NOT NULL,
	created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
	UNIQUE(resonite_id, resonite_name)
);
