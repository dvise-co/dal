ALTER TABLE queue ADD COLUMN attempts INTEGER DEFAULT 0;

CREATE TABLE IF NOT EXISTS queue_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_id    INTEGER NOT NULL REFERENCES queue(id),
    status      TEXT NOT NULL,
    error       TEXT,
    output      TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);
