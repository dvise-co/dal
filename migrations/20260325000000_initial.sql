CREATE TABLE IF NOT EXISTS queue (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    url         TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    progress    TEXT,
    error       TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_id    INTEGER REFERENCES queue(id),
    filename    TEXT NOT NULL,
    original    TEXT NOT NULL,
    path        TEXT NOT NULL,
    rating      INTEGER,
    size_bytes  INTEGER,
    duration    TEXT,
    thumbnail   TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    rated_at    DATETIME
);
