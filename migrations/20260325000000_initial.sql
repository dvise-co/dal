CREATE TABLE IF NOT EXISTS categories (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS queue (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    url         TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    progress    TEXT,
    error       TEXT,
    output      TEXT,
    category_id INTEGER REFERENCES categories(id),
    position    INTEGER DEFAULT 0,
    archived    BOOLEAN DEFAULT FALSE,
    attempts    INTEGER DEFAULT 0,
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
    category_id INTEGER REFERENCES categories(id),
    provisional BOOLEAN DEFAULT FALSE,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    rated_at    DATETIME
);

CREATE TABLE IF NOT EXISTS queue_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_id    INTEGER NOT NULL REFERENCES queue(id),
    status      TEXT NOT NULL,
    error       TEXT,
    output      TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);
