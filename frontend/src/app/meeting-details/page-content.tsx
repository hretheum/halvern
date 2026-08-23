"use client";
import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { motion } from 'framer-motion';
import { Summary, SummaryResponse } from '@/types';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { WorkshopHeader, type WorkshopMeta } from '@/components/MeetingDetails/WorkshopHeader';
import { WorkshopTranscriptPanel } from '@/components/MeetingDetails/WorkshopTranscriptPanel';
import { SummaryPanel } from '@/components/MeetingDetails/SummaryPanel';
import { RetranscribeDialog } from '@/components/MeetingDetails/RetranscribeDialog';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { speakerLabel } from '@/lib/speaker-labels';
import { labelForCode } from '@/lib/summary-languages';

// Custom hooks
import { useMeetingData } from '@/hooks/meeting-details/useMeetingData';
import { useSummaryGeneration } from '@/hooks/meeting-details/useSummaryGeneration';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { useCopyOperations } from '@/hooks/meeting-details/useCopyOperations';
import { useMeetingOperations } from '@/hooks/meeting-details/useMeetingOperations';
import { useConfig } from '@/contexts/ConfigContext';

/** The workshop's transcript pane collapses into a drawer below this width. */
const NARROW_BREAKPOINT_PX = 900;

function useIsNarrow(): boolean {
  const [isNarrow, setIsNarrow] = useState(false);
  useEffect(() => {
    const mq = window.matchMedia(`(max-width: ${NARROW_BREAKPOINT_PX}px)`);
    const update = () => setIsNarrow(mq.matches);
    update();
    mq.addEventListener('change', update);
    return () => mq.removeEventListener('change', update);
  }, []);
  return isNarrow;
}

function formatMetaDate(createdAt: string): string {
  const date = new Date(createdAt);
  if (Number.isNaN(date.getTime())) return createdAt;
  const day = date.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
  const time = date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return `${day} · ${time}`;
}

function formatMetaDuration(seconds?: number): string | null {
  if (seconds === undefined || !Number.isFinite(seconds)) return null;
  const minutes = Math.max(1, Math.round(seconds / 60));
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `${h}h ${m}m` : `${h}h`;
}

export default function PageContent({
  meeting,
  summaryData,
  shouldAutoGenerate = false,
  onAutoGenerateComplete,
  onMeetingUpdated,
  onRefetchTranscripts,
  // Pagination props for efficient transcript loading
  segments,
  hasMore,
  isLoadingMore,
  isLoadingTranscripts,
  totalCount,
  loadedCount,
  onLoadMore,
}: {
  meeting: any;
  summaryData: Summary | null;
  shouldAutoGenerate?: boolean;
  onAutoGenerateComplete?: () => void;
  onMeetingUpdated?: () => Promise<void>;
  onRefetchTranscripts?: () => Promise<void>;
  // Pagination props
  segments?: any[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  isLoadingTranscripts?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
}) {
  // State
  const [customPrompt, setCustomPrompt] = useState<string>('');
  const [summaryResponse] = useState<SummaryResponse | null>(null);
  const [transcriptDrawerOpen, setTranscriptDrawerOpen] = useState(false);
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);

  const isNarrow = useIsNarrow();

  // Ref to store the modal open function from SummaryGeneratorButtonGroup
  const openModelSettingsRef = useRef<(() => void) | null>(null);

  // Get model config from ConfigContext
  const { modelConfig, setModelConfig } = useConfig();
  const { betaFeatures } = useConfig();

  // Custom hooks
  const meetingData = useMeetingData({ meeting, summaryData, onMeetingUpdated });
  const templates = useTemplates();

  // Callback to register the modal open function
  const handleRegisterModalOpen = (openFn: () => void) => {
    openModelSettingsRef.current = openFn;
  };

  // Callback to trigger modal open (called from error handler)
  const handleOpenModelSettings = () => {
    if (openModelSettingsRef.current) {
      openModelSettingsRef.current();
    } else {
      console.warn('⚠️ Modal open function not yet registered');
    }
  };

  // Save model config to backend database and sync via event
  const handleSaveModelConfig = async (config?: ModelConfig) => {
    if (!config) return;
    try {
      await invoke('api_save_model_config', {
        provider: config.provider,
        model: config.model,
        whisperModel: config.whisperModel,
        apiKey: config.apiKey ?? null,
        ollamaEndpoint: config.ollamaEndpoint ?? null,
      });

      // Emit event so ConfigContext and other listeners stay in sync
      const { emit } = await import('@tauri-apps/api/event');
      await emit('model-config-updated', config);

      toast.success('Model settings saved successfully');
    } catch (error) {
      console.error('Failed to save model config:', error);
      toast.error('Failed to save model settings');
    }
  };

  const summaryGeneration = useSummaryGeneration({
    meeting,
    transcripts: meetingData.transcripts,
    modelConfig: modelConfig,
    isModelConfigLoading: false, // ConfigContext loads on mount
    selectedTemplate: templates.selectedTemplate,
    onMeetingUpdated,
    updateMeetingTitle: meetingData.updateMeetingTitle,
    setAiSummary: meetingData.setAiSummary,
    onOpenModelSettings: handleOpenModelSettings,
  });

  const copyOperations = useCopyOperations({
    meeting,
    transcripts: meetingData.transcripts,
    meetingTitle: meetingData.meetingTitle,
    aiSummary: meetingData.aiSummary,
    blockNoteSummaryRef: meetingData.blockNoteSummaryRef,
  });

  const meetingOperations = useMeetingOperations({
    meeting,
  });

  // Title edits from the header commit immediately - the inline edit is its
  // own gesture, not part of the summary's dirty/save cycle.
  const handleTitleCommit = useCallback(
    async (title: string) => {
      meetingData.handleTitleChange(title);
      try {
        await invoke('api_save_meeting_title', { meetingId: meeting.id, title });
        await onMeetingUpdated?.();
      } catch (error) {
        toast.error('Failed to rename meeting', { description: String(error) });
      }
    },
    [meetingData, meeting.id, onMeetingUpdated],
  );

  const displaySegments = useMemo(() => {
    if (segments) return segments;
    return (meetingData.transcripts ?? []).map((t: any) => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: t.speaker,
    }));
  }, [segments, meetingData.transcripts]);

  const meta = useMemo<WorkshopMeta>(() => {
    const speakerSet = new Set<string>();
    for (const s of displaySegments) {
      const label = speakerLabel(s.speaker);
      if (label) speakerSet.add(label);
    }
    let sourceLabel: string | null = null;
    if (meeting.source === 'auto') sourceLabel = meeting.app_name || 'Detected';
    else if (meeting.source === 'manual') sourceLabel = 'Manual';
    else if (meeting.source === 'imported') sourceLabel = 'Imported';

    return {
      dateTimeLabel: formatMetaDate(meeting.created_at),
      durationLabel: formatMetaDuration(meeting.duration_seconds),
      sourceLabel,
      languageLabel: meeting.language ? labelForCode(meeting.language) : null,
      speakersLabel: speakerSet.size ? `Speakers: ${[...speakerSet].join(', ')}` : null,
    };
  }, [meeting, displaySegments]);

  // Re-transcription needs the beta flag and the recording folder. The
  // speaker-label warning inside the dialog needs at least the first page
  // of segments, so the trigger stays hidden until loading settles.
  const canRetranscribe =
    betaFeatures.importAndRetranscribe && !!meeting.folder_path && !isLoadingTranscripts;
  const hasSpeakerLabels = useMemo(
    () => displaySegments.some((s: any) => !!s.speaker),
    [displaySegments],
  );

  // Claimed by the auto-generate effect before its first await.
  const autoGenerateStartedRef = useRef(false);

  // Track page view once per mount; strict mode invokes effects twice and a
  // page view is a statement of fact, not of render count.
  const pageViewTrackedRef = useRef(false);
  useEffect(() => {
    if (pageViewTrackedRef.current) return;
    pageViewTrackedRef.current = true;
    Analytics.trackPageView('meeting_details');
  }, []);

  // Auto-generate summary when flag is set
  useEffect(() => {
    let cancelled = false;

    const autoGenerate = async () => {
      if (shouldAutoGenerate && meetingData.transcripts.length > 0 && !cancelled) {
        // The `cancelled` flag alone cannot stop a duplicate: the generation
        // starts before cleanup runs, so a double-invoked effect meant two LLM
        // calls for one meeting. The ref is claimed synchronously, before any
        // await, and released when the flag drops so a deliberate second
        // auto-generation still works.
        if (autoGenerateStartedRef.current) return;
        autoGenerateStartedRef.current = true;
        console.log(`🤖 Auto-generating summary with ${modelConfig.provider}/${modelConfig.model}...`);
        await summaryGeneration.handleGenerateSummary('');

        // Notify parent that auto-generation is complete (only if not cancelled)
        if (onAutoGenerateComplete && !cancelled) {
          onAutoGenerateComplete();
        }
      }
    };

    if (!shouldAutoGenerate) {
      autoGenerateStartedRef.current = false;
    }
    autoGenerate();

    // Cleanup: cancel if component unmounts or meeting changes
    return () => {
      cancelled = true;
    };
  }, [shouldAutoGenerate, meeting.id]); // Re-run if meeting changes

  const transcriptVisible = !isNarrow || transcriptDrawerOpen;

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col h-full bg-background"
    >
      <WorkshopHeader
        meetingId={meeting.id}
        title={meetingData.meetingTitle}
        onTitleCommit={handleTitleCommit}
        meta={meta}
        onOpenFolder={meetingOperations.handleOpenMeetingFolder}
        onRetranscribe={canRetranscribe ? () => setShowRetranscribeDialog(true) : undefined}
        isNarrow={isNarrow}
        onToggleTranscript={() => setTranscriptDrawerOpen((open) => !open)}
      />

      <div className="flex flex-1 min-h-0 relative">
        {transcriptVisible && (
          <WorkshopTranscriptPanel
            segments={displaySegments}
            onCopyTranscript={copyOperations.handleCopyTranscript}
            customPrompt={customPrompt}
            onPromptChange={setCustomPrompt}
            hasMore={hasMore}
            isLoadingMore={isLoadingMore}
            totalCount={totalCount}
            loadedCount={loadedCount}
            onLoadMore={onLoadMore}
            asDrawer={isNarrow}
          />
        )}
        {isNarrow && transcriptDrawerOpen && (
          <div
            onClick={() => setTranscriptDrawerOpen(false)}
            className="absolute inset-0 bg-black/30 z-[5]"
            aria-hidden
          />
        )}

        <SummaryPanel
          meeting={meeting}
          meetingTitle={meetingData.meetingTitle}
          onTitleChange={meetingData.handleTitleChange}
          isEditingTitle={meetingData.isEditingTitle}
          onStartEditTitle={() => meetingData.setIsEditingTitle(true)}
          onFinishEditTitle={() => meetingData.setIsEditingTitle(false)}
          isTitleDirty={meetingData.isTitleDirty}
          summaryRef={meetingData.blockNoteSummaryRef}
          isSaving={meetingData.isSaving}
          onSaveAll={meetingData.saveAllChanges}
          onCopySummary={copyOperations.handleCopySummary}
          onOpenFolder={meetingOperations.handleOpenMeetingFolder}
          aiSummary={meetingData.aiSummary}
          summaryStatus={summaryGeneration.summaryStatus}
          transcripts={meetingData.transcripts}
          modelConfig={modelConfig}
          setModelConfig={setModelConfig}
          onSaveModelConfig={handleSaveModelConfig}
          onGenerateSummary={summaryGeneration.handleGenerateSummary}
          onStopGeneration={summaryGeneration.handleStopGeneration}
          customPrompt={customPrompt}
          summaryResponse={summaryResponse}
          onSaveSummary={meetingData.handleSaveSummary}
          onSummaryChange={meetingData.handleSummaryChange}
          onDirtyChange={meetingData.setIsSummaryDirty}
          summaryError={summaryGeneration.summaryError}
          onRegenerateSummary={summaryGeneration.handleRegenerateSummary}
          getSummaryStatusMessage={summaryGeneration.getSummaryStatusMessage}
          availableTemplates={templates.availableTemplates}
          selectedTemplate={templates.selectedTemplate}
          onTemplateSelect={templates.handleTemplateSelection}
          isModelConfigLoading={false}
          onOpenModelSettings={handleRegisterModalOpen}
        />
      </div>

      {canRetranscribe && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meeting.id}
          meetingFolderPath={meeting.folder_path}
          onComplete={async () => {
            if (onRefetchTranscripts) await onRefetchTranscripts();
          }}
          hasSpeakerLabels={hasSpeakerLabels}
        />
      )}
    </motion.div>
  );
}
