'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Mic, Square } from 'lucide-react';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

interface StopProposalPayload {
  /** Identifies this proposal; echoed back verbatim with the user's answer. */
  proposalId: number;
  /** Seconds the user has to answer before the recording stops anyway. */
  timeoutSeconds: number;
}

/**
 * Asks whether to stop the recording when the meeting appears to be over.
 *
 * Mounted globally: the proposal can arrive while the user is anywhere in the
 * app, or nowhere near the machine at all. In the latter case the backend
 * stops the recording on its own once the countdown runs out — this dialog is
 * an offer to decide sooner, not the mechanism that enforces the stop.
 *
 * The dialog closes on `recording-stop-proposal-resolved` no matter which side
 * resolved it (answer, timeout, or the meeting resuming), and additionally on
 * `recording-stopped`, so a manual stop from elsewhere in the UI does not
 * leave a stale question on screen.
 */
export function RecordingStopPrompt() {
  const [open, setOpen] = useState(false);
  const [secondsLeft, setSecondsLeft] = useState<number | null>(null);
  const [answering, setAnswering] = useState(false);
  const [proposalId, setProposalId] = useState<number | null>(null);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const clearCountdown = () => {
    if (countdownRef.current) {
      clearInterval(countdownRef.current);
      countdownRef.current = null;
    }
  };

  const close = () => {
    clearCountdown();
    setOpen(false);
    setSecondsLeft(null);
    setAnswering(false);
    setProposalId(null);
  };

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    // Guards the gap between cleanup and listen() resolving; a late listener would leak.
    const cleanedUpRef = { current: false };

    const setup = async () => {
      const unlistenProposed = await listen<StopProposalPayload>('recording-stop-proposed', (event) => {
        clearCountdown();
        setOpen(true);
        setProposalId(event.payload.proposalId);
        setSecondsLeft(event.payload.timeoutSeconds);
        countdownRef.current = setInterval(() => {
          setSecondsLeft((s) => (s === null || s <= 1 ? 0 : s - 1));
        }, 1000);
      });
      if (cleanedUpRef.current) {
        unlistenProposed();
        return;
      }
      unlisteners.push(unlistenProposed);

      const unlistenResolved = await listen('recording-stop-proposal-resolved', () => close());
      if (cleanedUpRef.current) {
        unlistenResolved();
        return;
      }
      unlisteners.push(unlistenResolved);

      // A manual stop from anywhere else must not leave this question open.
      const unlistenStopped = await listen('recording-stopped', () => close());
      if (cleanedUpRef.current) {
        unlistenStopped();
        return;
      }
      unlisteners.push(unlistenStopped);
    };

    setup();
    return () => {
      cleanedUpRef.current = true;
      clearCountdown();
      unlisteners.forEach((unlisten) => unlisten());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const respond = async (accept: boolean) => {
    if (proposalId === null) {
      // Nothing to answer: the dialog is being dismissed before the proposed
      // event ever arrived, which onOpenChange's `!next` path can trigger.
      return;
    }
    // The buttons disable immediately; the dialog itself closes on the
    // resolved event, keeping one source of truth for its visibility.
    setAnswering(true);
    try {
      await invoke('api_respond_to_stop_proposal', { proposalId, accept });
    } catch (error) {
      console.error('Failed to answer the stop proposal:', error);
      close();
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) respond(false); }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Meeting over? Recording is still running</DialogTitle>
          <DialogDescription>
            No meeting audio has been detected for a while.
            {secondsLeft !== null && (
              <>
                {' '}
                Without an answer, the recording stops on its own in{' '}
                <span className="font-medium tabular-nums">{secondsLeft}s</span>.
              </>
            )}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="gap-2 sm:gap-2">
          <Button
            variant="outline"
            disabled={answering}
            onClick={() => respond(false)}
            className="flex items-center gap-2"
          >
            <Mic className="w-4 h-4" />
            Keep recording
          </Button>
          <Button
            variant="destructive"
            disabled={answering}
            onClick={() => respond(true)}
            className="flex items-center gap-2"
          >
            <Square className="w-4 h-4" />
            Stop recording
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
