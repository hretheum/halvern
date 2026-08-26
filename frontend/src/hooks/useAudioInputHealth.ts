import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

/**
 * Whether the recording is actually hearing anything, and if not, which of the
 * two ways it is failing.
 *
 * On 26 August a recording ran for three minutes and forty-nine seconds under
 * the words "Listening for speech…", which were true and useless: the stream
 * had not delivered a single sample. Two later recordings did receive samples,
 * every one of them zero, and looked identical on screen. Both were only
 * discoverable by reading a log afterwards.
 *
 * The two states are kept apart because they send a person to different
 * places. Nothing arriving is the audio device or its permission; silence
 * arriving is the microphone itself — a muted headset, or a laptop closed in
 * clamshell with its built-in microphone selected, which is what it was.
 */

interface SourceActivity {
  samples: number;
  signalSamples: number;
  peak: number;
}

interface InputActivity {
  microphone: SourceActivity;
  system: SourceActivity;
  microphoneDevice: string | null;
  systemDevice: string | null;
}

export type AudioInputHealth =
  | { state: 'ok' }
  /** The stream is open and has never delivered a sample. */
  | { state: 'waiting'; device: string | null; seconds: number }
  /** Samples are arriving and all of them are silence. */
  | { state: 'silent'; device: string | null; seconds: number };

/** How often to ask. The counters are cumulative, so a missed poll costs nothing. */
const POLL_MS = 2000;

/**
 * Grace before saying nothing has arrived.
 *
 * Opening a device is not instant — a Bluetooth headset has to be put into its
 * hands-free profile first — so a few seconds of nothing is ordinary. Ten is
 * long enough not to cry wolf and far short of the three and a half minutes it
 * took to notice by hand.
 */
const WAITING_THRESHOLD_MS = 10_000;

/**
 * Grace before calling arriving samples silence.
 *
 * The signal floor is just above zero, and a real microphone in a quiet room
 * still crosses it constantly, so this is not measuring whether anyone is
 * talking. Fifteen seconds of exact digital zero is a broken input, not a pause
 * in the conversation.
 */
const SILENT_THRESHOLD_MS = 15_000;

export function useAudioInputHealth(enabled: boolean): AudioInputHealth {
  const [health, setHealth] = useState<AudioInputHealth>({ state: 'ok' });

  // When this recording's counters started being watched, and when the
  // microphone last produced something above the floor.
  const startedAtRef = useRef<number | null>(null);
  const lastSignalAtRef = useRef<number | null>(null);
  const lastSignalSamplesRef = useRef(0);

  useEffect(() => {
    if (!enabled) {
      startedAtRef.current = null;
      lastSignalAtRef.current = null;
      lastSignalSamplesRef.current = 0;
      setHealth({ state: 'ok' });
      return;
    }

    let cancelled = false;

    const poll = async () => {
      let activity: InputActivity;
      try {
        activity = await invoke<InputActivity>('audio_input_activity');
      } catch {
        // Outside Tauri, or the command is unavailable. Saying nothing is
        // better than accusing a working microphone.
        return;
      }
      if (cancelled) return;

      const now = Date.now();
      if (startedAtRef.current === null) startedAtRef.current = now;

      const { microphone, microphoneDevice } = activity;

      if (microphone.signalSamples > lastSignalSamplesRef.current) {
        lastSignalSamplesRef.current = microphone.signalSamples;
        lastSignalAtRef.current = now;
        setHealth({ state: 'ok' });
        return;
      }

      if (microphone.samples === 0) {
        const waitedFor = now - startedAtRef.current;
        setHealth(
          waitedFor >= WAITING_THRESHOLD_MS
            ? { state: 'waiting', device: microphoneDevice, seconds: Math.round(waitedFor / 1000) }
            : { state: 'ok' }
        );
        return;
      }

      // Samples are arriving. Silence is measured from the last time any of
      // them carried signal, or from the start if none ever did.
      const silentSince = lastSignalAtRef.current ?? startedAtRef.current;
      const silentFor = now - silentSince;
      setHealth(
        silentFor >= SILENT_THRESHOLD_MS
          ? { state: 'silent', device: microphoneDevice, seconds: Math.round(silentFor / 1000) }
          : { state: 'ok' }
      );
    };

    void poll();
    const timer = setInterval(() => void poll(), POLL_MS);

    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [enabled]);

  return health;
}
