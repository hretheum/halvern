/**
 * Meeting Detection Service
 *
 * Handles all automatic meeting detection Tauri backend calls.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke calls.
 */

import { invoke } from '@tauri-apps/api/core';

export interface DetectionSettings {
  enabled: boolean;
  /** Bundle identifiers never treated as a meeting, e.g. screen recorders. */
  ignoredBundleIds: string[];
  /** Bundle identifiers treated as a meeting on audio output alone. */
  alwaysMeetingBundleIds: string[];
  /** How long the activity must persist before recording starts. */
  minDurationSeconds: number;
  showNotifications: boolean;
  /** Master switch for automatic stopping; covers the hard length cap too. */
  autoStopEnabled: boolean;
  /** Seconds of meeting-audio silence before the app asks whether to stop. */
  silenceDurationSeconds: number;
  /** Seconds the question stays open before the recording stops on its own. */
  confirmationTimeoutSeconds: number;
  /** Hard cap in minutes for every recording, manual ones included. 0 = off. */
  maxRecordingMinutes: number;
}

export async function getDetectionSettings(): Promise<DetectionSettings> {
  return invoke<DetectionSettings>('api_get_detection_settings');
}

export async function saveDetectionSettings(settings: DetectionSettings): Promise<void> {
  return invoke<void>('api_save_detection_settings', { settings });
}
