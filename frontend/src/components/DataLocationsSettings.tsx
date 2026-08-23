"use client";

import { useEffect } from 'react';
import { FolderOpen } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useConfig } from '@/contexts/ConfigContext';
import Analytics from '@/lib/analytics';

const OPEN_COMMANDS = {
  database: 'open_database_folder',
  models: 'open_models_folder',
  recordings: 'open_recordings_folder',
} as const;

type FolderKey = keyof typeof OPEN_COMMANDS;

/**
 * Where Halvern keeps its data, with a reveal button per location. The paths
 * come from the backend commands, not from constants - what this panel shows
 * is what the running app actually uses.
 */
export function DataLocationsSettings() {
  const { storageLocations, loadPreferences } = useConfig();

  useEffect(() => {
    loadPreferences();
  }, [loadPreferences]);

  const rows: Array<{ key: FolderKey; label: string; path?: string }> = [
    { key: 'database', label: 'Database', path: storageLocations?.database },
    { key: 'models', label: 'Models folder', path: storageLocations?.models },
    { key: 'recordings', label: 'Recordings folder', path: storageLocations?.recordings },
  ];

  const handleReveal = async (key: FolderKey) => {
    try {
      await invoke(OPEN_COMMANDS[key]);
      await Analytics.track('storage_folder_opened', { folder_type: key });
    } catch (error) {
      console.error(`Failed to open ${key} folder:`, error);
    }
  };

  return (
    <div>
      <h3 className="text-[13px] font-semibold mb-1">Data locations</h3>
      <div className="bg-muted border border-border rounded-lg px-3.5 py-1 mt-2">
        {rows.map(({ key, label, path }, i) => (
          <div
            key={key}
            className={`flex items-center justify-between gap-3 py-2.5 ${
              i < rows.length - 1 ? 'border-b border-border' : ''
            }`}
          >
            <div className="min-w-0">
              <div className="text-[11px] text-muted-foreground">{label}</div>
              <div className="text-xs font-mono whitespace-nowrap overflow-hidden text-ellipsis">
                {path || 'Loading…'}
              </div>
            </div>
            <button
              title="Reveal in Finder"
              aria-label={`Reveal ${label} in Finder`}
              onClick={() => handleReveal(key)}
              disabled={!path}
              className="flex p-1 text-muted-foreground hover:text-foreground disabled:opacity-40 shrink-0"
            >
              <FolderOpen className="w-3.5 h-3.5" />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
