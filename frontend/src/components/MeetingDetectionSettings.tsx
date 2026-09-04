import React, { useEffect, useState } from 'react';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { toast } from 'sonner';
import { usePlatform } from '@/hooks/usePlatform';
import {
  getDetectionSettings,
  saveDetectionSettings,
  type DetectionSettings,
} from '@/services/detectionService';

const FALLBACK: DetectionSettings = {
  enabled: false,
  ignoredBundleIds: [],
  alwaysMeetingBundleIds: [],
  minDurationSeconds: 15,
  showNotifications: true,
  autoStopEnabled: true,
  silenceDurationSeconds: 120,
  confirmationTimeoutSeconds: 120,
  maxRecordingMinutes: 240,
};

/** Renders a bundle-id list as one identifier per line. */
function IdListEditor({
  label,
  hint,
  value,
  disabled,
  onCommit,
}: {
  label: string;
  hint: string;
  value: string[];
  disabled: boolean;
  onCommit: (next: string[]) => void;
}) {
  const [draft, setDraft] = useState(value.join('\n'));

  useEffect(() => {
    setDraft(value.join('\n'));
  }, [value]);

  const commit = () => {
    const next = draft
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
    if (next.join('\n') !== value.join('\n')) onCommit(next);
  };

  return (
    <div className="flex flex-col gap-2">
      <div>
        <span className="text-sm font-medium">{label}</span>
        <p className="text-sm text-muted-foreground">{hint}</p>
      </div>
      <textarea
        className="min-h-24 rounded border px-3 py-2 font-mono text-xs"
        value={draft}
        disabled={disabled}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
      />
    </div>
  );
}

/**
 * Automatic meeting detection settings.
 *
 * A meeting is recognised either as a known meeting application, or as any
 * application rendering and capturing audio at the same time — that pairing is
 * what separates a call from music playback. Screen recorders do both as well,
 * which is why they are excluded by identifier.
 */
export function MeetingDetectionSettings() {
  const [settings, setSettings] = useState<DetectionSettings>(FALLBACK);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);

  // Which platforms have something that notices an application using audio.
  // macOS listens to a Core Audio property; Windows enumerates WASAPI audio
  // sessions. Linux has neither yet, and `snapshot_audio_apps()` returns an
  // empty list there, so the rules above it never see a candidate.
  //
  // `usePlatform` reads `navigator` and resolves after mount, so it is
  // 'unknown' during the static prerender. Treating unknown as "has a sensor"
  // is the right default: it keeps the control live for the platform this is
  // usually read on, and the worst case is a switch that works.
  const platform = usePlatform();
  const hasSensor = platform !== 'linux';

  useEffect(() => {
    let cancelled = false;
    getDetectionSettings()
      .then((loaded) => {
        if (!cancelled) setSettings(loaded);
      })
      .catch((error) => {
        console.error('Failed to load detection settings:', error);
        toast.error('Could not load meeting detection settings');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = async (next: DetectionSettings) => {
    setBusy(true);
    try {
      await saveDetectionSettings(next);
      setSettings(next);
      toast.success('Detection settings saved');
    } catch (error) {
      console.error('Failed to save detection settings:', error);
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  if (loading) {
    return <div className="p-4 text-sm text-muted-foreground">Loading…</div>;
  }

  return (
    <div className="flex flex-col gap-6 p-4">
      <div>
        <h3 className="text-base font-medium">Automatic Meeting Detection</h3>
        <p className="text-sm text-muted-foreground">
          Starts recording on its own when a call begins. A meeting is an application using the
          microphone and the speakers at the same time, or one on the known-meeting list.
        </p>
      </div>

      <div className="flex items-start justify-between gap-4">
        <div>
          <span className="text-sm font-medium">Detect meetings</span>
          <p id="detect-meetings-reason" className="text-sm text-muted-foreground">
            {hasSensor
              ? 'Takes effect on the next application start.'
              : 'Not available on this platform yet. The rules are ready; what is missing is the part that notices an application using audio, which exists for macOS and Windows and not for Linux.'}
          </p>
        </div>
        {/*
          `aria-disabled`, not `disabled`. A disabled switch leaves the tab
          order, so a screen-reader user meets a gap where the explanation is.
          The reason is on screen either way.

          This switch was offered on every platform until 4 September 2026,
          while the sensor behind it existed only on macOS. It saved happily
          and did nothing, which cost the first Windows tester an evening
          before anyone traced the call path.
        */}
        <Switch
          checked={hasSensor && settings.enabled}
          disabled={busy && hasSensor}
          aria-disabled={!hasSensor}
          aria-describedby="detect-meetings-reason"
          onCheckedChange={(checked) => {
            if (!hasSensor) return;
            persist({ ...settings, enabled: checked });
          }}
        />
      </div>

      <div className="flex items-start justify-between gap-4">
        <div>
          <span className="text-sm font-medium">Notify on automatic start</span>
        </div>
        <Switch
          checked={settings.showNotifications}
          disabled={busy}
          onCheckedChange={(checked) => persist({ ...settings, showNotifications: checked })}
        />
      </div>

      <div className="flex flex-col gap-2">
        <div>
          <span className="text-sm font-medium">Confirmation delay</span>
          <p className="text-sm text-muted-foreground">
            Seconds the activity must persist before recording starts, so a short preview does not
            create a meeting.
          </p>
        </div>
        <Input
          type="number"
          min={0}
          max={600}
          className="w-32"
          value={settings.minDurationSeconds}
          disabled={busy}
          onChange={(e) =>
            setSettings({ ...settings, minDurationSeconds: Number(e.target.value) })
          }
          onBlur={() => persist(settings)}
        />
      </div>

      <div className="flex flex-col gap-6 rounded border p-3">
        <div>
          <h4 className="text-sm font-medium">Automatic stop</h4>
          <p className="text-sm text-muted-foreground">
            Asks whether to stop once the meeting goes quiet, and caps how long any recording can
            run. Born of a recording that ran for seven hours after a meeting ended.
          </p>
        </div>

        <div className="flex items-start justify-between gap-4">
          <div>
            <span className="text-sm font-medium">Stop when the meeting ends</span>
            <p className="text-sm text-muted-foreground">
              Works for manually started recordings too, and applies from the next observation —
              no restart needed.
            </p>
          </div>
          <Switch
            checked={settings.autoStopEnabled}
            disabled={busy}
            onCheckedChange={(checked) => persist({ ...settings, autoStopEnabled: checked })}
          />
        </div>

        <div className="flex flex-col gap-2">
          <div>
            <span className="text-sm font-medium">Silence before asking</span>
            <p className="text-sm text-muted-foreground">
              Seconds without meeting audio before the question appears. Short blips do not count;
              the clock restarts whenever the meeting comes back.
            </p>
          </div>
          <Input
            type="number"
            min={30}
            max={3600}
            className="w-32"
            value={settings.silenceDurationSeconds}
            disabled={busy || !settings.autoStopEnabled}
            onChange={(e) =>
              setSettings({ ...settings, silenceDurationSeconds: Number(e.target.value) })
            }
            onBlur={() => persist(settings)}
          />
        </div>

        <div className="flex flex-col gap-2">
          <div>
            <span className="text-sm font-medium">Answer window</span>
            <p className="text-sm text-muted-foreground">
              Seconds the question stays open. With no answer the recording stops on its own —
              silence here means nobody is at the machine.
            </p>
          </div>
          <Input
            type="number"
            min={10}
            max={3600}
            className="w-32"
            value={settings.confirmationTimeoutSeconds}
            disabled={busy || !settings.autoStopEnabled}
            onChange={(e) =>
              setSettings({ ...settings, confirmationTimeoutSeconds: Number(e.target.value) })
            }
            onBlur={() => persist(settings)}
          />
        </div>

        <div className="flex flex-col gap-2">
          <div>
            <span className="text-sm font-medium">Maximum recording length</span>
            <p className="text-sm text-muted-foreground">
              Minutes after which any recording stops, no question asked — including recordings
              started by hand, which the detector never sees. 0 switches the cap off.
            </p>
          </div>
          <Input
            type="number"
            min={0}
            max={1440}
            className="w-32"
            value={settings.maxRecordingMinutes}
            disabled={busy || !settings.autoStopEnabled}
            onChange={(e) =>
              setSettings({ ...settings, maxRecordingMinutes: Number(e.target.value) })
            }
            onBlur={() => persist(settings)}
          />
        </div>
      </div>

      <IdListEditor
        label="Never a meeting"
        hint="Bundle identifiers, one per line. Screen recorders belong here: they use both directions but are not calls."
        value={settings.ignoredBundleIds}
        disabled={busy}
        onCommit={(ignoredBundleIds) => persist({ ...settings, ignoredBundleIds })}
      />

      <IdListEditor
        label="Always a meeting"
        hint="Recognised on audio output alone, because the microphone often joins a moment later."
        value={settings.alwaysMeetingBundleIds}
        disabled={busy}
        onCommit={(alwaysMeetingBundleIds) => persist({ ...settings, alwaysMeetingBundleIds })}
      />

    </div>
  );
}
