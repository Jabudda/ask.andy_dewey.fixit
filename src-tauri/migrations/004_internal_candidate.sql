-- Phase 3.1: per-job application context (internal candidate, lateral move, etc.).

ALTER TABLE jobs ADD COLUMN internal_candidate INTEGER NOT NULL DEFAULT 0;
ALTER TABLE jobs ADD COLUMN candidate_context TEXT NOT NULL DEFAULT '';  -- the candidate's own words, sent to Claude when AI writing is on
