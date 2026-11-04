-- Phase 3: generated documents with structured content, claim checks and approval; AI usage ledger.

ALTER TABLE documents ADD COLUMN content_json TEXT;          -- structured resume / cover-letter content
ALTER TABLE documents ADD COLUMN claim_report TEXT;          -- JSON list of claim-check findings
ALTER TABLE documents ADD COLUMN status TEXT NOT NULL DEFAULT 'final';  -- draft | approved | final
ALTER TABLE documents ADD COLUMN approved_at TEXT;
ALTER TABLE documents ADD COLUMN pdf_path TEXT;

CREATE TABLE ai_usage (
    id            INTEGER PRIMARY KEY,
    ts            TEXT NOT NULL,
    purpose       TEXT NOT NULL,      -- resume | cover_letter
    job_id        INTEGER REFERENCES jobs (id) ON DELETE SET NULL,
    model         TEXT NOT NULL,
    input_tokens  INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd      REAL NOT NULL DEFAULT 0,
    outcome       TEXT NOT NULL       -- ok | refused | error
);
