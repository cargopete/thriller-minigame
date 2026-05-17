CREATE TABLE IF NOT EXISTS save (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    seed       BLOB    NOT NULL,
    day        INTEGER NOT NULL DEFAULT 1,
    phase      TEXT    NOT NULL DEFAULT 'dawn'
                       CHECK (phase IN ('dawn','day','dusk','night')),
    created_at TEXT    NOT NULL,
    updated_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS player (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    name           TEXT    NOT NULL,
    gender         TEXT,
    age            INTEGER,
    interests      TEXT,
    backstory      TEXT,
    sanity         INTEGER NOT NULL DEFAULT 80,
    location       TEXT    NOT NULL DEFAULT 'town',
    alive          INTEGER NOT NULL DEFAULT 1,
    inventory_json TEXT    NOT NULL DEFAULT '[]'
);
