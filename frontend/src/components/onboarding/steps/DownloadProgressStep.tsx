import React, { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Mic, Sparkles, Check, Loader2, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { toast } from 'sonner';
import { motion, AnimatePresence } from 'framer-motion';
import { getSummaryModelSizeLabel, getSummaryModelSizeMb } from '@/lib/onboarding-summary-model';

type DownloadStatus = 'waiting' | 'downloading' | 'completed' | 'error';

interface DownloadState {
  status: DownloadStatus;
  progress: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
  error?: string;
}

export function DownloadProgressStep() {
  const {
    goNext,
    selectedSummaryModel,
    recommendedSummaryModel,
    transcriptionModelDownloaded,
    setTranscriptionModelDownloaded,
    summaryModelDownloaded,
    setSummaryModelDownloaded,
    startBackgroundDownloads,
    retryTranscriptionDownload,
    completeOnboarding,
    plan,
  } = useOnboarding();

  const [isMac, setIsMac] = useState(false);

  const [transcriptionState, setTranscriptionState] = useState<DownloadState>({
    status: transcriptionModelDownloaded ? 'completed' : 'waiting',
    progress: transcriptionModelDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: plan?.transcription_size_mb ?? 0,
    speedMbps: 0,
  });

  // The size is only known once the language answer has been resolved into a
  // plan, which can land after this step mounts.
  useEffect(() => {
    if (plan?.transcription_size_mb) {
      setTranscriptionState(previous => ({
        ...previous,
        totalMb: previous.totalMb || plan.transcription_size_mb,
      }));
    }
  }, [plan?.transcription_size_mb]);

  const [summaryState, setSummaryState] = useState<DownloadState>({
    status: summaryModelDownloaded ? 'completed' : 'waiting',
    progress: summaryModelDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 0,
    speedMbps: 0,
  });

  const [isCompleting, setIsCompleting] = useState(false);
  const transcriptionDownloadStartedRef = useRef(false);
  const summaryDownloadStartedRef = useRef(false);
  const retryingRef = useRef(false);
  const retryingSummaryRef = useRef(false);

  // Retry download handler
  const handleRetryDownload = async () => {
    // Prevent multiple simultaneous retries
    if (retryingRef.current) {
      console.log('[DownloadProgressStep] Retry already in progress, ignoring');
      return;
    }

    console.log('[DownloadProgressStep] Retrying transcription download');
    retryingRef.current = true;

    // Reset error state
    setTranscriptionState((prev) => ({
      ...prev,
      status: 'waiting',
      error: undefined,
      progress: 0,
      downloadedMb: 0,
      speedMbps: 0,
    }));

    try {
      await retryTranscriptionDownload();
      // Progress events will update state
    } catch (error) {
      console.error('[DownloadProgressStep] Retry failed:', error);
      setTranscriptionState((prev) => ({
        ...prev,
        status: 'error',
        error: error instanceof Error ? error.message : 'Retry failed',
      }));

      toast.error('Download retry failed', {
        description: 'Please check your connection and try again.',
      });
    } finally {
      // Allow retry again after 2 seconds
      setTimeout(() => {
        retryingRef.current = false;
      }, 2000);
    }
  };

  // Retry summary download handler
  const handleRetrySummaryDownload = async () => {
    // Prevent multiple simultaneous retries
    if (retryingSummaryRef.current) {
      console.log('[DownloadProgressStep] Summary retry already in progress, ignoring');
      return;
    }

    console.log('[DownloadProgressStep] Retrying summary model download');
    retryingSummaryRef.current = true;

    // Reset error state
    setSummaryState((prev) => ({
      ...prev,
      status: 'downloading',
      error: undefined,
      progress: 0,
      downloadedMb: 0,
      totalMb: getSummaryModelSizeMb(selectedSummaryModel || recommendedSummaryModel),
      speedMbps: 0,
    }));

    try {
      // Call download command directly (no retry command exists for built-in AI)
      const modelName = selectedSummaryModel;
      if (!modelName) {
        throw new Error('Summary model recommendation is not ready yet');
      }
      await invoke('builtin_ai_download_model', { modelName });
    } catch (error) {
      console.error('[DownloadProgressStep] Summary retry failed:', error);
      setSummaryState((prev) => ({
        ...prev,
        status: 'error',
        error: error instanceof Error ? error.message : 'Retry failed',
      }));

      toast.error('Summary model download retry failed', {
        description: 'Please check your connection and try again.',
      });
    } finally {
      // Allow retry again after 2 seconds
      setTimeout(() => {
        retryingSummaryRef.current = false;
      }, 2000);
    }
  };

  // Detect platform on mount
  useEffect(() => {
    const checkPlatform = async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };

    checkPlatform();
  }, []);

  // Start the required transcription model as soon as we know which one it is;
  // summary readiness must not block it. The plan is what names the model, so
  // this waits for it rather than guessing an engine and downloading the wrong
  // one.
  useEffect(() => {
    if (transcriptionDownloadStartedRef.current) return;
    if (!plan) return;
    transcriptionDownloadStartedRef.current = true;

    if (!transcriptionModelDownloaded) {
      setTranscriptionState((prev) => ({ ...prev, status: 'downloading' }));
    }

    startBackgroundDownloads({
      includeTranscription: true,
      includeSummary: false,
    }).catch((error) => {
      console.error('Failed to start transcription download:', error);
      if (!transcriptionModelDownloaded) {
        setTranscriptionState((prev) => ({ ...prev, status: 'error', error: String(error) }));
      }
    });
  }, [plan]);

  // Start the selected summary model only after the backend recommendation is known.
  useEffect(() => {
    if (summaryDownloadStartedRef.current) return;
    if (!selectedSummaryModel) return;
    summaryDownloadStartedRef.current = true;

    startSummaryDownload();
  }, [selectedSummaryModel]);

  // Listen to transcription model download progress. Parakeet and Whisper emit
  // different event families, so both are wired and only one ever fires.
  useEffect(() => {
    const expectedModel = plan?.transcription_model;
    if (!expectedModel) return;

    const unlistenProgress = listen<{
      modelName: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status?: string;
    }>('parakeet-model-download-progress', (event) => {
      const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
      if (modelName === expectedModel) {
        setTranscriptionState((prev) => ({
          ...prev,
          status: status === 'completed' ? 'completed' : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: total_mb ?? prev.totalMb,
          speedMbps: speed_mbps ?? prev.speedMbps,
        }));

        if (status === 'completed' || progress >= 100) {
          setTranscriptionModelDownloaded(true);
        }
      }
    });

    // Whisper reports a percentage only, so the byte counters are derived from
    // the planned size rather than from the event.
    const unlistenWhisperProgress = listen<{ modelName: string; progress: number }>(
      'model-download-progress',
      (event) => {
        const { modelName, progress } = event.payload;
        if (modelName === expectedModel) {
          setTranscriptionState((prev) => ({
            ...prev,
            status: 'downloading',
            progress,
            downloadedMb: (prev.totalMb * progress) / 100,
          }));

          if (progress >= 100) {
            setTranscriptionModelDownloaded(true);
          }
        }
      }
    );

    const markComplete = (event: { payload: { modelName: string } }) => {
      if (event.payload.modelName === expectedModel) {
        setTranscriptionState((prev) => ({
          ...prev,
          status: 'completed',
          progress: 100,
          downloadedMb: prev.totalMb,
        }));
        setTranscriptionModelDownloaded(true);
      }
    };

    const markError = (event: { payload: { modelName: string; error: string } }) => {
      if (event.payload.modelName === expectedModel) {
        setTranscriptionState((prev) => ({
          ...prev,
          status: 'error',
          error: event.payload.error,
        }));
      }
    };

    const unlistenComplete = listen<{ modelName: string }>(
      'parakeet-model-download-complete',
      markComplete
    );
    const unlistenWhisperComplete = listen<{ modelName: string }>(
      'model-download-complete',
      markComplete
    );
    const unlistenError = listen<{ modelName: string; error: string }>(
      'parakeet-model-download-error',
      markError
    );
    const unlistenWhisperError = listen<{ modelName: string; error: string }>(
      'model-download-error',
      markError
    );

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenWhisperProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenWhisperComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
      unlistenWhisperError.then((fn) => fn());
    };
  }, [plan?.transcription_model]);

  // Listen to Summary Model download progress (always downloading for builtin-ai)
  useEffect(() => {
    const unlisten = listen<{
      model: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status: string;
      error?: string;
    }>('builtin-ai-download-progress', (event) => {
      const { model, progress, downloaded_mb, total_mb, speed_mbps, status, error } = event.payload;
      if (selectedSummaryModel && model === selectedSummaryModel) {
        setSummaryState((prev) => ({
          ...prev,
          status: status === 'completed'
            ? 'completed'
            : status === 'error'
            ? 'error'
            : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: (total_mb ?? prev.totalMb) || getSummaryModelSizeMb(model),
          speedMbps: speed_mbps ?? prev.speedMbps,
          error: status === 'error' ? error : undefined,
        }));

        if (status === 'completed' || progress >= 100) {
          setSummaryModelDownloaded(true);
        }
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [selectedSummaryModel]);

  useEffect(() => {
    const modelForSize = selectedSummaryModel || recommendedSummaryModel;
    if (!modelForSize) return;

    setSummaryState((prev) => ({
      ...prev,
      status: summaryModelDownloaded
        ? 'completed'
        : prev.status === 'completed'
        ? 'waiting'
        : prev.status,
      progress: summaryModelDownloaded
        ? 100
        : prev.status === 'completed'
        ? 0
        : prev.progress,
      totalMb: prev.totalMb || getSummaryModelSizeMb(modelForSize),
    }));
  }, [selectedSummaryModel, recommendedSummaryModel, summaryModelDownloaded]);

  const startSummaryDownload = async () => {
    if (!summaryModelDownloaded && selectedSummaryModel) {
      try {
        setSummaryState((prev) => ({
          ...prev,
          status: 'downloading',
          totalMb: getSummaryModelSizeMb(selectedSummaryModel),
        }));
        await startBackgroundDownloads({
          includeTranscription: false,
          includeSummary: true,
          summaryModel: selectedSummaryModel,
        });
      } catch (error) {
        console.error('Failed to start summary model download:', error);
        setSummaryState((prev) => ({ ...prev, status: 'error', error: String(error) }));
      }
    }
  };

  const handleContinue = async () => {
    // Verify actual model availability (catches state drift)
    try {
      // Explicit provider for the same reason as in OnboardingContext: the
      // transcript config is only written once onboarding completes.
      const actuallyAvailable = await invoke<boolean>('transcription_model_available', {
        provider: plan?.engine === 'whisper' ? 'localWhisper' : 'parakeet',
      });

      if (actuallyAvailable && !transcriptionModelDownloaded) {
        console.log('[DownloadProgressStep] Model available but state not updated');
        setTranscriptionModelDownloaded(true);
        setTranscriptionState((prev) => ({
          ...prev,
          status: 'completed',
          progress: 100,
        }));
      } else if (!actuallyAvailable && transcriptionState.status === 'error') {
        toast.error('Transcription engine required', {
          description: 'Please retry the download before continuing.',
        });
        return;
      }
    } catch (error) {
      console.warn('[DownloadProgressStep] Failed to verify model:', error);
    }

    // Check if downloads are complete for toast notification
    const downloadsComplete = transcriptionState.status === 'completed' &&
      summaryState.status === 'completed';

    // Show toast if downloads still in progress
    if (!downloadsComplete) {
      toast.info('Downloads will continue in the background', {
        description: 'You can start using the app. Recording will be available once speech recognition is ready.',
        duration: 5000,
      });
    }

    if (isMac) {
      // macOS: Go to Permissions step (will complete after permissions granted)
      goNext();
    } else {
      // Non-macOS: Complete onboarding immediately (downloads continue in background)
      setIsCompleting(true);
      try {
        await completeOnboarding();

        // Small delay to ensure state is saved before reload
        await new Promise(resolve => setTimeout(resolve, 100));

        window.location.reload();
      } catch (error) {
        console.error('Failed to complete onboarding:', error);
        toast.error('Failed to complete setup', {
          description: 'Please try again.',
        });
        setIsCompleting(false);
      }
    }
  };

  const renderDownloadCard = (
    title: string,
    icon: React.ReactNode,
    state: DownloadState,
    modelSize: string,
    sizeUnit = 'MB'
  ) => (
    <div className="bg-card rounded-xl border border-border p-5">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-full bg-muted flex items-center justify-center">
            {icon}
          </div>
          <div>
            <h3 className="font-medium text-foreground">{title}</h3>
            <p className="text-sm text-muted-foreground">{modelSize}</p>
          </div>
        </div>
        <div role="status" aria-live="polite">
          {state.status === 'waiting' && (
            <span className="text-sm text-muted-foreground">Waiting...</span>
          )}
          {state.status === 'downloading' && (
            <>
              <Loader2 className="w-5 h-5 text-foreground animate-spin" aria-hidden="true" />
              <span className="sr-only">Downloading {title}</span>
            </>
          )}
          {state.status === 'completed' && (
            <div className="w-6 h-6 rounded-full bg-green-100 dark:bg-green-900/40 flex items-center justify-center">
              <Check className="w-4 h-4 text-green-600 dark:text-green-400" aria-hidden="true" />
              <span className="sr-only">{title} downloaded</span>
            </div>
          )}
          {state.status === 'error' && (
            <span className="text-sm text-danger-text">Failed</span>
          )}
        </div>
      </div>

      {/* Progress Bar */}
      {(state.status === 'downloading' || state.status === 'completed') && (
        <div className="space-y-2">
          <div
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(state.progress)}
            aria-valuetext={`${Math.round(state.progress)} percent, ${state.downloadedMb.toFixed(0)} of ${state.totalMb.toFixed(0)} ${sizeUnit}`}
            aria-label={`Downloading ${title}`}
            className="w-full h-2 bg-muted rounded-full overflow-hidden"
          >
            <div
              className="h-full bg-linear-to-r from-primary/80 to-primary rounded-full transition-all duration-300"
              style={{ width: `${state.progress}%` }}
            />
          </div>
          <div className="flex items-center justify-between text-sm">
            <span className="text-muted-foreground">
              {state.downloadedMb.toFixed(1)} {sizeUnit} / {state.totalMb.toFixed(1)} {sizeUnit}
            </span>
            <div className="flex items-center gap-2">
              {state.speedMbps > 0 && (
                <span className="text-muted-foreground">
                  {state.speedMbps.toFixed(1)} {sizeUnit}/s
                </span>
              )}
              <span className="font-semibold text-foreground">
                {Math.round(state.progress)}%
              </span>
            </div>
          </div>
        </div>
      )}

      {state.status === 'error' && state.error && (
        <div className="mt-2 p-3 bg-red-50 dark:bg-red-950/40 border border-red-200 dark:border-red-900 rounded-md">
          <p className="text-sm text-red-600 dark:text-red-400 font-medium">Download Error</p>
          <p className="mt-1 text-sm text-danger-text">{state.error}</p>
          {(title === 'Transcription Engine' || title === 'Summary Engine') && (
            <button
              onClick={title === 'Transcription Engine' ? handleRetryDownload : handleRetrySummaryDownload}
              className="mt-3 w-full h-9 px-4 bg-primary hover:bg-primary/90 text-primary-foreground text-sm font-medium rounded-md transition-colors flex items-center justify-center gap-2"
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                      d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              Try Again
            </button>
          )}
        </div>
      )}
    </div>
  );

  return (
    <OnboardingContainer
      title="Getting things ready"
      description="You can start using Halvern after downloading the Transcription Engine."
      step={5}
      totalSteps={6}
    >
      <div className="flex flex-col items-center space-y-6">
        {/* Download Cards */}
        <div className="w-full max-w-lg space-y-4">
          {renderDownloadCard(
            'Transcription Engine',
            <Mic className="w-5 h-5 text-muted-foreground" />,
            transcriptionState,
            plan ? `~${plan.transcription_size_mb} MB` : 'Size pending'
          )}

          {renderDownloadCard(
            'Summary Engine',
            <Sparkles className="w-5 h-5 text-muted-foreground" />,
            summaryState,
            getSummaryModelSizeLabel(selectedSummaryModel || recommendedSummaryModel),
            'MiB'
          )}
        </div>

        {/* Info Message - Only show when Parakeet is downloaded */}
        <AnimatePresence>
          {transcriptionModelDownloaded && !summaryModelDownloaded && (
            <motion.div
              initial={{ opacity: 0, y: -10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.3, ease: 'easeOut' }}
              className="w-full max-w-lg bg-muted rounded-lg p-4 text-sm text-foreground"
            >
              <div className="flex items-start gap-3">
                <Download className="w-5 h-5 text-muted-foreground shrink-0 mt-0.5" />
                <div>
                  <p className="font-medium">You can continue while this finishes</p>
                  <p className="text-foreground mt-1">
                    Download will continue in the background.
                  </p>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Continue Button */}
        <div className="w-full max-w-xs">
          <Button
            onClick={handleContinue}
            disabled={!transcriptionModelDownloaded || isCompleting}
            className="w-full h-11 bg-primary hover:bg-primary/90 text-primary-foreground disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {(isCompleting || !transcriptionModelDownloaded) ? (
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
            ) : (
              'Continue'
            )}
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
