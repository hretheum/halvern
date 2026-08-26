import { useEffect } from 'react';
import { toast } from 'sonner';

/**
 * Says when the recording has moved itself onto a different microphone.
 *
 * The switch happens in Rust, on purpose: the interface can be on another
 * screen or busy, and a recording that has stopped hearing anything is the
 * worst moment to depend on the webview answering. So this listens rather than
 * decides — by the time the event arrives the streams have already been
 * replaced.
 *
 * It is a notice and not a question because that was the call: connecting a
 * headset mid-meeting is a statement about where the sound should come from,
 * and anyone who disagrees can stop the recording. What it must never be is
 * silent — a recording that changes device without saying so is worse than one
 * that does not change at all.
 */

interface SwitchPayload {
  from: string | null;
  to: string;
}

interface SwitchFailedPayload {
  device: string;
  reason: string;
}

export function useMicrophoneSwitchNotice() {
  useEffect(() => {
    let unlisteners: Array<() => void> = [];
    let cancelled = false;

    const subscribe = async () => {
      const { listen } = await import('@tauri-apps/api/event');

      const switched = await listen<SwitchPayload>('microphone-switched', (event) => {
        const { from, to } = event.payload;
        toast.info(`Recording moved to “${to}”`, {
          description: from
            ? `“${from}” is no longer the system's default input. The recording continues on the new one.`
            : 'It became the system default input while the recording was running.',
          duration: 8000,
        });
      });

      const failed = await listen<SwitchFailedPayload>('microphone-switch-failed', (event) => {
        const { device, reason } = event.payload;
        toast.error(`Could not move the recording to “${device}”`, {
          description: `${reason}. The recording continues on the microphone it started with.`,
          duration: 10000,
        });
      });

      if (cancelled) {
        switched();
        failed();
        return;
      }
      unlisteners = [switched, failed];
    };

    void subscribe();

    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);
}
