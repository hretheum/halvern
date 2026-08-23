import React, { useEffect, useState } from 'react';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { FolderOpen, X } from 'lucide-react';
import { toast } from 'sonner';
import {
  getObsidianSettings,
  pickObsidianVault,
  saveObsidianSettings,
  type ObsidianSettings,
} from '@/services/exportService';

const EMPTY: ObsidianSettings = {
  vaultPath: null,
  transcriptPath: null,
  summaryPath: null,
  autoExport: false,
};

interface FolderRowProps {
  label: string;
  hint: string;
  value: string | null;
  placeholder: string;
  disabled: boolean;
  onPick: () => void;
  onClear: () => void;
}

function FolderRow({ label, hint, value, placeholder, disabled, onPick, onClear }: FolderRowProps) {
  return (
    <div className="flex flex-col gap-2">
      <div>
        <span className="text-sm font-medium">{label}</span>
        <p className="text-sm text-muted-foreground">{hint}</p>
      </div>
      <div className="flex items-center gap-2">
        <code className="flex-1 truncate rounded border px-3 py-2 text-xs" title={value ?? ''}>
          {value ?? placeholder}
        </code>
        <Button variant="outline" size="sm" onClick={onPick} disabled={disabled}>
          <FolderOpen className="mr-2 h-4 w-4" />
          Choose…
        </Button>
        {value && (
          <Button
            variant="ghost"
            size="sm"
            onClick={onClear}
            disabled={disabled}
            aria-label={`Clear ${label}`}
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>
    </div>
  );
}

/**
 * Obsidian export settings.
 *
 * Two notes per meeting, `YYYY-MM-DD-<slug>-transcript.md` and `-summary.md`.
 * The two kinds can go to separate folders: a transcript carries raw client
 * speech while a summary is filtered, so summaries can live in a synced vault
 * while transcripts stay on a local-only disk.
 *
 * Existing notes are never overwritten; a numbered variant is written instead,
 * and both notes of a meeting always share the same number.
 */
export function ObsidianExportSettings() {
  const [settings, setSettings] = useState<ObsidianSettings>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;

    getObsidianSettings()
      .then((loaded) => {
        if (!cancelled) setSettings(loaded);
      })
      .catch((error) => {
        console.error('Failed to load Obsidian settings:', error);
        toast.error('Could not load Obsidian export settings');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const persist = async (next: ObsidianSettings) => {
    setBusy(true);
    try {
      await saveObsidianSettings(next);
      setSettings(next);
      toast.success('Obsidian export settings saved');
    } catch (error) {
      // Backend rejects relative paths and configurations with no usable
      // destination. Surface its message rather than a generic one.
      console.error('Failed to save Obsidian settings:', error);
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const pickInto = async (key: keyof Omit<ObsidianSettings, 'autoExport'>) => {
    try {
      const picked = await pickObsidianVault();
      if (!picked) return; // user dismissed the dialog
      await persist({ ...settings, [key]: picked });
    } catch (error) {
      console.error('Folder picker failed:', error);
      toast.error('Could not open the folder picker');
    }
  };

  const clear = async (key: keyof Omit<ObsidianSettings, 'autoExport'>) => {
    const next = { ...settings, [key]: null };
    // Dropping the vault while no overrides cover both kinds leaves nowhere to
    // write, so the automatic export has to go off with it.
    const stillUsable = next.vaultPath || (next.transcriptPath && next.summaryPath);
    if (!stillUsable) next.autoExport = false;
    await persist(next);
  };

  const hasDestination = Boolean(
    settings.vaultPath || (settings.transcriptPath && settings.summaryPath)
  );

  const handleToggleAuto = async (checked: boolean) => {
    if (checked && !hasDestination) {
      toast.error('Choose a folder first');
      return;
    }
    await persist({ ...settings, autoExport: checked });
  };

  if (loading) {
    return <div className="p-4 text-sm text-muted-foreground">Loading…</div>;
  }

  const fallbackHint = settings.vaultPath ?? 'the vault folder';

  return (
    <div className="flex flex-col gap-6 p-4">
      <div>
        <h3 className="text-base font-medium">Obsidian Export</h3>
        <p className="text-sm text-muted-foreground">
          Writes the transcript and the summary as Markdown notes.
        </p>
      </div>

      <FolderRow
        label="Vault folder"
        hint="Default destination for both notes."
        value={settings.vaultPath}
        placeholder="Not set"
        disabled={busy}
        onPick={() => pickInto('vaultPath')}
        onClear={() => clear('vaultPath')}
      />

      <FolderRow
        label="Transcripts folder"
        hint={`Optional. Transcripts hold raw speech, so you may want them outside a synced vault. Empty means ${fallbackHint}.`}
        value={settings.transcriptPath}
        placeholder="Same as vault folder"
        disabled={busy}
        onPick={() => pickInto('transcriptPath')}
        onClear={() => clear('transcriptPath')}
      />

      <FolderRow
        label="Summaries folder"
        hint={`Optional. Empty means ${fallbackHint}.`}
        value={settings.summaryPath}
        placeholder="Same as vault folder"
        disabled={busy}
        onPick={() => pickInto('summaryPath')}
        onClear={() => clear('summaryPath')}
      />

      <div className="flex items-start justify-between gap-4">
        <div>
          <span className="text-sm font-medium">Export automatically</span>
          <p className="text-sm text-muted-foreground">
            Write both notes as soon as a summary finishes generating.
          </p>
        </div>
        <Switch
          checked={settings.autoExport}
          onCheckedChange={handleToggleAuto}
          disabled={busy || !hasDestination}
        />
      </div>

      <p className="text-xs text-muted-foreground">
        Existing notes are never overwritten. When a file of the same name already exists, a
        numbered variant is written instead, and both notes of one meeting always share the same
        number — even when they live in different folders.
      </p>
    </div>
  );
}
