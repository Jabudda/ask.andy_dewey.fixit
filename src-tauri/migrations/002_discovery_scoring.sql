-- Phase 2: education (needed for degree checks), job sources, richer search-run reporting.

CREATE TABLE education (
    id          INTEGER PRIMARY KEY,
    institution TEXT NOT NULL,
    degree      TEXT NOT NULL DEFAULT '',   -- e.g. "Bachelor's degree"
    field       TEXT NOT NULL DEFAULT '',
    minor       TEXT NOT NULL DEFAULT '',
    start_year  TEXT NOT NULL DEFAULT '',
    end_year    TEXT NOT NULL DEFAULT '',
    notes       TEXT NOT NULL DEFAULT ''
);

CREATE TABLE job_sources (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,              -- greenhouse | lever | ashby | remotive
    identifier  TEXT NOT NULL,              -- board token / company slug ('' for keyword boards)
    label       TEXT NOT NULL DEFAULT '',
    enabled     INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    last_status TEXT,                       -- ok | error
    last_error  TEXT,
    created_at  TEXT NOT NULL,
    UNIQUE (kind, identifier)
);
INSERT INTO job_sources (kind, identifier, label, created_at) VALUES
    ('himalayas', '', 'Himalayas — remote jobs, US-eligible (keyword search)', datetime('now', 'localtime')),
    ('jobicy',    '', 'Jobicy — remote jobs, USA (keyword search; listings credited to Jobicy)', datetime('now', 'localtime')),
    ('remotive',  '', 'Remotive — remote jobs board (latest postings)', datetime('now', 'localtime'));

ALTER TABLE search_runs ADD COLUMN source_id INTEGER REFERENCES job_sources (id) ON DELETE SET NULL;
ALTER TABLE search_runs ADD COLUMN trigger TEXT NOT NULL DEFAULT 'manual';   -- manual | scheduled
ALTER TABLE search_runs ADD COLUMN filtered INTEGER NOT NULL DEFAULT 0;      -- not relevant / too old
ALTER TABLE search_runs ADD COLUMN excluded INTEGER NOT NULL DEFAULT 0;      -- failed a mandatory requirement
ALTER TABLE search_runs ADD COLUMN duplicates INTEGER NOT NULL DEFAULT 0;

INSERT OR IGNORE INTO settings (key, value) VALUES ('search_interval_hours', '12');
