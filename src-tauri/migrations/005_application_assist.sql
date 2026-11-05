-- Phase 4: application assistance. Your standard answers, and the answers proposed for each job.

CREATE TABLE answer_bank (
    key        TEXT PRIMARY KEY,
    answer     TEXT NOT NULL DEFAULT '',
    updated_at TEXT
);

CREATE TABLE job_answers (
    id         INTEGER PRIMARY KEY,
    job_id     INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    key        TEXT NOT NULL DEFAULT '',        -- standard question key, or '' for a question you added
    question   TEXT NOT NULL,
    answer     TEXT NOT NULL DEFAULT '',
    source     TEXT NOT NULL DEFAULT '',        -- where the proposal came from, shown to you
    status     TEXT NOT NULL DEFAULT 'proposed',-- proposed | needs_input | confirmed
    sort_order INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT,
    UNIQUE (job_id, question)
);
CREATE INDEX idx_job_answers_job ON job_answers(job_id);

INSERT OR IGNORE INTO settings (key, value) VALUES ('follow_up_days', '7');
