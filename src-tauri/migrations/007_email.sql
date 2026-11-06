-- Phase 4: read-only Gmail monitoring. Messages are copied here so they can be matched to your
-- applications without asking Gmail again. Nothing here is ever sent anywhere.

CREATE TABLE email_messages (
    id               TEXT PRIMARY KEY,                  -- Gmail message id
    thread_id        TEXT NOT NULL DEFAULT '',
    direction        TEXT NOT NULL,                     -- in | out
    from_addr        TEXT NOT NULL DEFAULT '',
    from_name        TEXT NOT NULL DEFAULT '',
    to_addrs         TEXT NOT NULL DEFAULT '',          -- comma-separated addresses (To and Cc)
    subject          TEXT NOT NULL DEFAULT '',
    snippet          TEXT NOT NULL DEFAULT '',
    body_text        TEXT NOT NULL DEFAULT '',
    body_html        TEXT NOT NULL DEFAULT '',          -- kept for job-board emails only, so alerts can be re-read
    sent_at          TEXT NOT NULL,                     -- local time
    category         TEXT NOT NULL DEFAULT 'other',     -- alert | confirmation | rejection | interview | assessment | offer | recruiter | sent | other
    board            TEXT NOT NULL DEFAULT '',          -- Indeed, LinkedIn, … when a job board sent it
    job_id           INTEGER REFERENCES jobs (id) ON DELETE SET NULL,
    match_note       TEXT NOT NULL DEFAULT '',          -- why it was linked to that job
    match_manual     INTEGER NOT NULL DEFAULT 0,        -- you linked or unlinked it yourself
    suggestion       TEXT,                              -- a status for the linked job, waiting for your OK
    suggestion_state TEXT NOT NULL DEFAULT '',          -- '' | pending | accepted | dismissed
    alert_jobs       TEXT NOT NULL DEFAULT '[]',        -- JSON: jobs read from an alert and what happened to each
    fetched_at       TEXT NOT NULL
);
CREATE INDEX ix_email_job ON email_messages (job_id);
CREATE INDEX ix_email_sent ON email_messages (sent_at);
CREATE INDEX ix_email_thread ON email_messages (thread_id);
CREATE INDEX ix_email_pending ON email_messages (suggestion_state);

INSERT OR IGNORE INTO settings (key, value) VALUES ('gmail_interval_minutes', '30');
INSERT OR IGNORE INTO settings (key, value) VALUES ('gmail_first_sync_days', '30');
