-- Migration: Add Obsidian markdown export settings
--
-- obsidianVaultPath  - absolute path to the vault directory, NULL when unset
-- obsidianAutoExport - 0/1 flag, writes both notes automatically once a summary completes

ALTER TABLE settings ADD COLUMN obsidianVaultPath TEXT;
ALTER TABLE settings ADD COLUMN obsidianAutoExport INTEGER NOT NULL DEFAULT 0;
