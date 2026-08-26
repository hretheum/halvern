'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscriptRecovery } from '@/hooks/useTranscriptRecovery';
import { TranscriptRecovery } from '@/components/TranscriptRecovery';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { indexedDBService } from '@/services/indexedDBService';

/**
 * Startup recovery of transcripts that never reached the database (e.g. the
 * app died mid-recording). Mounted globally: this used to live on the home
 * page back when home was the recording screen, but recovery is a startup
 * concern, not a screen concern - it must run no matter where the user goes
 * first. The dialog offers itself at most once per session.
 */
export function RecoveryPrompt() {
  const [showDialog, setShowDialog] = useState(false);
  const router = useRouter();
  const recordingState = useRecordingState();
  const { status } = recordingState;
  const { refetchMeetings } = useSidebar();

  const {
    recoverableMeetings,
    checkForRecoverableTranscripts,
    recoverMeeting,
    loadMeetingTranscripts,
    deleteRecoverableMeeting,
  } = useTranscriptRecovery();

  useEffect(() => {
    const performStartupChecks = async () => {
      try {
        // A recovery prompt over a live recording would be nonsense.
        if (
          recordingState.isRecording ||
          status === RecordingStatus.STOPPING ||
          status === RecordingStatus.PROCESSING_TRANSCRIPTS ||
          status === RecordingStatus.SAVING
        ) {
          return;
        }

        try {
          await indexedDBService.deleteOldMeetings(7);
        } catch (error) {
          console.warn('⚠️ Failed to clean up old meetings:', error);
        }
        try {
          await indexedDBService.deleteSavedMeetings(24);
        } catch (error) {
          console.warn('⚠️ Failed to clean up saved meetings:', error);
        }
        await checkForRecoverableTranscripts();
      } catch (error) {
        console.error('Failed to perform startup checks:', error);
      }
    };

    performStartupChecks();
  }, [checkForRecoverableTranscripts, recordingState.isRecording, status]);

  // Offer the dialog once per session when something recoverable shows up.
  useEffect(() => {
    if (recoverableMeetings.length > 0) {
      const shownThisSession = sessionStorage.getItem('recovery_dialog_shown');
      if (!shownThisSession) {
        setShowDialog(true);
        sessionStorage.setItem('recovery_dialog_shown', 'true');
      }
    }
  }, [recoverableMeetings]);

  // Nothing left to decide about: close, and hand the screen back to whatever
  // the dialog was covering. Recovering or deleting the last entry used to
  // leave an empty dialog on screen whose only remaining function was a close
  // button — a question with no question left in it.
  //
  // Safe against the moment before the startup check has answered, because the
  // dialog is only ever opened once the list is non-empty.
  useEffect(() => {
    if (showDialog && recoverableMeetings.length === 0) {
      setShowDialog(false);
      sessionStorage.removeItem('recovery_dialog_shown');
    }
  }, [showDialog, recoverableMeetings.length]);

  const handleRecovery = async (meetingId: string) => {
    try {
      const result = await recoverMeeting(meetingId);

      if (result.success) {
        toast.success('Meeting recovered successfully!', {
          description:
            result.audioRecoveryStatus?.status === 'success'
              ? 'Transcripts and audio recovered'
              : 'Transcripts recovered (no audio available)',
          action: result.meetingId
            ? {
                label: 'View Meeting',
                onClick: () => {
                  router.push(`/meeting-details?id=${result.meetingId}`);
                },
              }
            : undefined,
          duration: 10000,
        });

        await refetchMeetings();

        if (recoverableMeetings.length === 0) {
          sessionStorage.removeItem('recovery_dialog_shown');
        }

        if (result.meetingId) {
          setTimeout(() => {
            router.push(`/meeting-details?id=${result.meetingId}`);
          }, 2000);
        }
      }
    } catch (error) {
      toast.error('Failed to recover meeting', {
        description: error instanceof Error ? error.message : 'Unknown error occurred',
      });
      throw error;
    }
  };

  const handleDialogClose = () => {
    setShowDialog(false);
    if (recoverableMeetings.length === 0) {
      sessionStorage.removeItem('recovery_dialog_shown');
    }
  };

  return (
    <TranscriptRecovery
      isOpen={showDialog}
      onClose={handleDialogClose}
      recoverableMeetings={recoverableMeetings}
      onRecover={handleRecovery}
      onDelete={deleteRecoverableMeeting}
      onLoadPreview={loadMeetingTranscripts}
    />
  );
}
