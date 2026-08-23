/**
 * Obsidian Export Service
 *
 * Handles all Obsidian markdown export Tauri backend calls.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke calls.
 *
 * The folder picker lives on the Rust side because `@tauri-apps/plugin-dialog`
 * is not a frontend dependency of this project.
 */

import { invoke } from '@tauri-apps/api/core';

export interface ObsidianSettings {
  vaultPath: string | null;
  /** Optional override of `vaultPath` for transcript notes. */
  transcriptPath: string | null;
  /** Optional override of `vaultPath` for summary notes. */
  summaryPath: string | null;
  autoExport: boolean;
}

/**
 * Opens the native folder picker.
 * Resolves to `null` when the user dismisses the dialog.
 */
export async function pickObsidianVault(): Promise<string | null> {
  return invoke<string | null>('api_pick_obsidian_vault');
}

/** Reads the stored Obsidian export configuration. */
export async function getObsidianSettings(): Promise<ObsidianSettings> {
  return invoke<ObsidianSettings>('api_get_obsidian_settings');
}

/**
 * Persists the Obsidian export configuration.
 *
 * The backend rejects relative paths, and rejects a configuration that leaves
 * no usable destination (no vault and not both overrides set).
 */
export async function saveObsidianSettings(
  settings: ObsidianSettings
): Promise<void> {
  return invoke<void>('api_save_obsidian_settings', {
    vaultPath: settings.vaultPath,
    transcriptPath: settings.transcriptPath,
    summaryPath: settings.summaryPath,
    autoExport: settings.autoExport,
  });
}

/**
 * Exports a single meeting on demand, independently of the auto-export switch.
 * Returns the absolute paths actually written (transcript first, summary second).
 */
export async function exportMeeting(meetingId: string): Promise<string[]> {
  return invoke<string[]>('api_export_meeting', { meetingId });
}
