-- Migration: Separate destinations for transcript and summary notes
--
-- Both are optional overrides of obsidianVaultPath. NULL or empty means
-- "write into the base vault path", which is the behaviour before this change.
--
-- Rationale: a transcript carries raw client speech, a summary is filtered.
-- Splitting the destinations lets summaries live in a synced vault while
-- transcripts stay on a local-only disk.

ALTER TABLE settings ADD COLUMN obsidianTranscriptPath TEXT;
ALTER TABLE settings ADD COLUMN obsidianSummaryPath TEXT;
