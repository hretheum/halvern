'use client';

import { useState, useEffect, useRef } from 'react';
import { motion } from 'framer-motion';
import { RecordingControls } from '@/components/RecordingControls';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import Analytics from '@/lib/analytics';
import { SettingsModals } from '@/app/_components/SettingsModal';
import { TranscriptPanel } from '@/app/_components/TranscriptPanel';
import { useModalState } from '@/hooks/useModalState';
import { useRecordingStateSync } from '@/hooks/useRecordingStateSync';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

function formatClock(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

/**
 * The recording screen: status pill, elapsed clock, what is being recorded,
 * the pause/stop controls - and the live transcript below them. The mockup
 * shows no transcript here, but the running transcription is an existing
 * feature this redesign deliberately keeps (decision of 2026-08-15).
 */
export default function RecordScreen() {
  const [isRecording, setIsRecordingState] = useState(false);

  const { meetingTitle } = useTranscripts();
  const { transcriptModelConfig, selectedDevices } = useConfig();
  const recordingState = useRecordingState();
  const { status, isStopping, isProcessing, activeDuration, isPaused } = recordingState;

  const { hasMicrophone } = usePermissionCheck();
  const { setIsMeetingActive } = useSidebar();
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  const { isRecordingDisabled, setIsRecordingDisabled } = useRecordingStateSync(isRecording, setIsRecordingState, setIsMeetingActive);
  const { handleRecordingStart } = useRecordingStart(isRecording, setIsRecordingState, showModal);
  const { handleRecordingStop, setIsStopping } = useRecordingStop(
    setIsRecordingState,
    setIsRecordingDisabled
  );

  // Once per mount: strict mode invokes effects twice and a page view is a
  // statement of fact, not of render count.
  const pageViewTrackedRef = useRef(false);
  useEffect(() => {
    if (pageViewTrackedRef.current) return;
    pageViewTrackedRef.current = true;
    Analytics.trackPageView('record');
  }, []);

  const isProcessingStop = status === RecordingStatus.PROCESSING_TRANSCRIPTS || isProcessing;
  const isActive = recordingState.isRecording;

  const statusLabel = isActive ? (isPaused ? 'Paused' : 'Recording') : 'Ready to record';
  // What is being captured. For detector-started recordings the meeting name
  // carries the source app; before any recording there is nothing to claim.
  const sourceLabel = isActive ? meetingTitle || 'Recording in progress' : null;

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col h-full bg-background"
    >
      <SettingsModals
        modals={modals}
        messages={messages}
        onClose={hideModal}
      />

      {/* Status block, per the mockup: pill, clock, source, controls */}
      <div className="shrink-0 flex flex-col items-center gap-3 pt-7 pb-5 border-b border-border">
        <div className="flex items-center gap-2 px-3.5 py-1.5 rounded-full bg-muted">
          <span
            className={`w-2 h-2 rounded-full ${
              isActive
                ? isPaused
                  ? 'bg-orange-500'
                  : 'bg-recording animate-pulse'
                : 'bg-border'
            }`}
          />
          <span className="text-[13px] font-semibold">{statusLabel}</span>
        </div>

        <div className="text-[56px] leading-none font-bold font-mono tabular-nums tracking-wide">
          {formatClock(isActive ? activeDuration ?? 0 : 0)}
        </div>

        {sourceLabel && (
          <div className="text-[13px] text-muted-foreground">{sourceLabel}</div>
        )}

        {(hasMicrophone || isRecording) &&
          status !== RecordingStatus.PROCESSING_TRANSCRIPTS &&
          status !== RecordingStatus.SAVING && (
            <RecordingControls
              isRecording={recordingState.isRecording}
              onRecordingStop={(callApi = true) => handleRecordingStop(callApi)}
              onRecordingStart={handleRecordingStart}
              onStopInitiated={() => setIsStopping(true)}
              onTranscriptionError={(message) => {
                showModal('errorAlert', message);
              }}
              isRecordingDisabled={isRecordingDisabled}
              isParentProcessing={isProcessingStop}
              selectedDevices={selectedDevices}
              meetingName={meetingTitle}
            />
          )}
      </div>

      {/* Live transcript - kept from the current app, absent from the mockup */}
      <div className="flex-1 min-h-0 flex">
        <TranscriptPanel
          isProcessingStop={isProcessingStop}
          isStopping={isStopping}
          showModal={showModal}
        />
      </div>

      <StatusOverlays
        isProcessing={status === RecordingStatus.PROCESSING_TRANSCRIPTS && !recordingState.isRecording}
        isSaving={status === RecordingStatus.SAVING}
      />
    </motion.div>
  );
}
