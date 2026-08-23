'use client';

import React, { createContext, useContext, useState, useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  PermissionStatus,
  OnboardingPermissions,
  OnboardingPlan,
} from '@/types/onboarding';
import { resolveOnboardingSummaryModelStatus } from '@/lib/onboarding-summary-model';

const PARAKEET_MODEL = 'parakeet-tdt-0.6b-v3-int8';

interface OnboardingStatus {
  version: string;
  completed: boolean;
  current_step: number;
  model_status: {
    // Still keyed "parakeet" on disk for installed copies; the engine behind it
    // is whatever the meeting language calls for.
    parakeet: string;
    summary: string;
    selected_summary_model?: string;
  };
  meeting_languages?: string[];
  last_updated: string;
}

const LAST_STEP = 6;

interface SummaryModelProgressInfo {
  percent: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
}

interface TranscriptionProgressInfo {
  percent: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
}

interface OnboardingContextType {
  currentStep: number;
  transcriptionModelDownloaded: boolean;
  transcriptionProgress: number;
  transcriptionProgressInfo: TranscriptionProgressInfo;
  summaryModelDownloaded: boolean;
  summaryModelProgress: number;
  summaryModelProgressInfo: SummaryModelProgressInfo;
  selectedSummaryModel: string;
  recommendedSummaryModel: string;
  databaseExists: boolean;
  isBackgroundDownloading: boolean;
  // The meeting-language answer, and what it implies. `plan` is null until the
  // backend has been asked; every consumer treats that as "not known yet"
  // rather than substituting a default.
  meetingLanguages: string[];
  plan: OnboardingPlan | null;
  // Permissions
  permissions: OnboardingPermissions;
  permissionsSkipped: boolean;
  // Navigation
  goToStep: (step: number) => void;
  goNext: () => void;
  goPrevious: () => void;
  // Setters
  setTranscriptionModelDownloaded: (value: boolean) => void;
  setSummaryModelDownloaded: (value: boolean) => void;
  setSelectedSummaryModel: (value: string) => void;
  setDatabaseExists: (value: boolean) => void;
  setMeetingLanguages: (codes: string[]) => void;
  setPermissionStatus: (permission: keyof OnboardingPermissions, status: PermissionStatus) => void;
  setPermissionsSkipped: (skipped: boolean) => void;
  completeOnboarding: () => Promise<void>;
  startBackgroundDownloads: (options: StartBackgroundDownloadsOptions) => Promise<void>;
  retryTranscriptionDownload: () => Promise<void>;
}

interface StartBackgroundDownloadsOptions {
  includeTranscription: boolean;
  includeSummary: boolean;
  summaryModel?: string;
}

const OnboardingContext = createContext<OnboardingContextType | undefined>(undefined);

// True once any provider mount has kicked off database initialization.
let databaseInitStarted = false;

export function OnboardingProvider({ children }: { children: React.ReactNode }) {
  const [currentStep, setCurrentStep] = useState(1);
  const [completed, setCompleted] = useState(false);
  const [transcriptionModelDownloaded, setTranscriptionModelDownloaded] = useState(false);
  const [transcriptionProgress, setTranscriptionProgress] = useState(0);
  const [transcriptionProgressInfo, setTranscriptionProgressInfo] =
    useState<TranscriptionProgressInfo>({
      percent: 0,
      downloadedMb: 0,
      totalMb: 0,
      speedMbps: 0,
    });
  const [meetingLanguages, setMeetingLanguages] = useState<string[]>([]);
  const [plan, setPlan] = useState<OnboardingPlan | null>(null);
  const [summaryModelDownloaded, setSummaryModelDownloaded] = useState(false);
  const [summaryModelProgress, setSummaryModelProgress] = useState(0);
  const [summaryModelProgressInfo, setSummaryModelProgressInfo] = useState<SummaryModelProgressInfo>({
    percent: 0,
    downloadedMb: 0,
    totalMb: 0,
    speedMbps: 0,
  });
  const [selectedSummaryModel, setSelectedSummaryModel] = useState<string>('');
  const [recommendedSummaryModel, setRecommendedSummaryModel] = useState<string>('');
  const [databaseExists, setDatabaseExists] = useState(false);
  const [isBackgroundDownloading, setIsBackgroundDownloading] = useState(false);

  // Permissions state
  const [permissions, setPermissions] = useState<OnboardingPermissions>({
    microphone: 'not_determined',
    systemAudio: 'not_determined',
    screenRecording: 'not_determined',
  });
  const [permissionsSkipped, setPermissionsSkipped] = useState(false);

  const saveTimeoutRef = useRef<NodeJS.Timeout | undefined>(undefined);

  const initializeSummaryModelSelection = async (preferredModel = selectedSummaryModel) => {
    try {
      const recommendedModel = await invoke<string>('builtin_ai_get_recommended_model');
      setRecommendedSummaryModel(recommendedModel);
      const modelToCheck = preferredModel || recommendedModel;
      setSelectedSummaryModel(modelToCheck);

      const selectedModelReady = await invoke<boolean>('builtin_ai_is_model_ready', {
        modelName: modelToCheck,
        refresh: true,
      });
      const resolved = resolveOnboardingSummaryModelStatus({
        selectedModel: preferredModel,
        recommendedModel,
        selectedModelReady,
      });

      setSelectedSummaryModel(resolved.selectedSummaryModel);
      setSummaryModelDownloaded(resolved.summaryModelDownloaded);
      console.log('[OnboardingContext] Set recommended model:', resolved.selectedSummaryModel);

      return resolved;
    } catch (error) {
      console.error('[OnboardingContext] Failed to initialize summary model:', error);
      return null;
    }
  };

  const requestSummaryModelDownload = (modelName: string) => {
    console.log('[OnboardingContext] Starting Summary Model download');
    invoke('builtin_ai_download_model', { modelName })
      .catch(err => {
        if (String(err).includes('Download already in progress')) {
          return;
        }
        console.error('[OnboardingContext] Summary Model download failed:', err);
      });
  };

  // Database initialization must run once per app, not once per mount: on a
  // true first launch two concurrent runs would both see check_first_launch as
  // true and both start import_and_initialize. A module-level flag rather than
  // a ref, because strict mode double-mounts the provider itself and each mount
  // gets fresh refs. The status loads are read-only and may repeat freely.
  useEffect(() => {
    loadOnboardingStatus();
    checkDatabaseStatus();
    if (!databaseInitStarted) {
      databaseInitStarted = true;
      initializeDatabaseInBackground();
    }
  }, []);

  // Initialize database silently in background (moved from SetupOverviewStep)
  const initializeDatabaseInBackground = async () => {
    try {
      console.log('[OnboardingContext] Starting background database initialization');
      const isFirstLaunch = await invoke<boolean>('check_first_launch');

      if (!isFirstLaunch) {
        console.log('[OnboardingContext] Database exists, skipping initialization');
        setDatabaseExists(true);
        return;
      }

      // First launch - attempt auto-detection and import
      await performAutoDetection();
    } catch (error) {
      console.error('[OnboardingContext] Database initialization failed:', error);
      // Don't throw - database init failure shouldn't block onboarding
    }
  };

  const performAutoDetection = async () => {
    // Check Homebrew (macOS only)
    if (typeof navigator !== 'undefined' && navigator.platform?.toLowerCase().includes('mac')) {
      const homebrewDbPath = '/usr/local/var/meetily/meeting_minutes.db';
      try {
        const homebrewCheck = await invoke<{ exists: boolean; size: number } | null>(
          'check_homebrew_database',
          { path: homebrewDbPath }
        );

        if (homebrewCheck?.exists) {
          console.log('[OnboardingContext] Found Homebrew database, importing');
          await invoke('import_and_initialize_database', { legacyDbPath: homebrewDbPath });
          setDatabaseExists(true);
          return;
        }
      } catch (e) {
        console.log('[OnboardingContext] Homebrew check failed, continuing:', e);
      }
    }

    // Check default legacy database location
    try {
      const legacyPath = await invoke<string | null>('check_default_legacy_database');
      if (legacyPath) {
        console.log('[OnboardingContext] Found legacy database, importing');
        await invoke('import_and_initialize_database', { legacyDbPath: legacyPath });
        setDatabaseExists(true);
        return;
      }
    } catch (e) {
      console.log('[OnboardingContext] Legacy check failed, continuing:', e);
    }

    // No legacy database found - initialize fresh
    console.log('[OnboardingContext] No legacy database found, initializing fresh');
    await invoke('initialize_fresh_database');
    setDatabaseExists(true);
  };

  const isCompletingRef = useRef(false);

  // Ask the backend what this answer installs. The decision lives in Rust
  // (src-tauri/src/language.rs) so that it is testable and so that the engine's
  // language coverage exists in exactly one place; this side only renders it.
  useEffect(() => {
    let cancelled = false;

    invoke<OnboardingPlan>('onboarding_plan_for_languages', { languages: meetingLanguages })
      .then(result => {
        if (!cancelled) setPlan(result);
      })
      .catch(error => {
        console.error('[OnboardingContext] Failed to resolve onboarding plan:', error);
      });

    return () => {
      cancelled = true;
    };
  }, [meetingLanguages]);

  // Keep the summary recommendation in step with the language answer, since the
  // group the models are ranked against comes from it.
  //
  // The selection only fills a blank rather than overriding, so that a model
  // already chosen and possibly downloading is never swapped underneath the
  // download step. Today that distinction is invisible, because language does
  // not yet move the ranking; when the bake-off lands and it does, this is the
  // line to revisit.
  useEffect(() => {
    if (plan?.summary_model) {
      setRecommendedSummaryModel(plan.summary_model);
      setSelectedSummaryModel(previous => previous || plan.summary_model);
    }
  }, [plan?.summary_model]);

  // Auto-save on state change (debounced)
  useEffect(() => {
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);

    // Don't auto-save if completed (to avoid overwriting completion status)
    // Also don't auto-save if we are currently in the process of completing
    if (completed || isCompletingRef.current) return;

    saveTimeoutRef.current = setTimeout(() => {
      saveOnboardingStatus();
    }, 1000);

    return () => {
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    };
  }, [currentStep, transcriptionModelDownloaded, summaryModelDownloaded, completed, meetingLanguages]);

  // Listen to transcription model download progress.
  //
  // The two engines emit different event families — Parakeet reports rich
  // progress, Whisper reports a bare percentage — so both are wired up and the
  // one that is not downloading simply never fires.
  useEffect(() => {
    const expectedModel = plan?.transcription_model ?? PARAKEET_MODEL;

    const unlisten = listen<{
      modelName: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status?: string;
    }>(
      'parakeet-model-download-progress',
      (event) => {
        const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
        if (modelName === expectedModel) {
          setTranscriptionProgress(progress);
          setTranscriptionProgressInfo({
            percent: progress,
            downloadedMb: downloaded_mb ?? 0,
            totalMb: total_mb ?? 0,
            speedMbps: speed_mbps ?? 0,
          });
          if (status === 'completed' || progress >= 100) {
            setTranscriptionModelDownloaded(true);
          }
        }
      }
    );

    // Whisper's downloader emits only a percentage, so the byte counters stay
    // at whatever the size lookup provided rather than being zeroed here.
    const unlistenWhisper = listen<{ modelName: string; progress: number }>(
      'model-download-progress',
      (event) => {
        const { modelName, progress } = event.payload;
        if (modelName === expectedModel) {
          setTranscriptionProgress(progress);
          setTranscriptionProgressInfo(previous => ({ ...previous, percent: progress }));
          if (progress >= 100) {
            setTranscriptionModelDownloaded(true);
          }
        }
      }
    );

    const unlistenComplete = listen<{ modelName: string }>(
      'parakeet-model-download-complete',
      (event) => {
        if (event.payload.modelName === expectedModel) {
          setTranscriptionModelDownloaded(true);
          setTranscriptionProgress(100);
        }
      }
    );

    const unlistenWhisperComplete = listen<{ modelName: string }>(
      'model-download-complete',
      (event) => {
        if (event.payload.modelName === expectedModel) {
          setTranscriptionModelDownloaded(true);
          setTranscriptionProgress(100);
        }
      }
    );

    const unlistenError = listen<{ modelName: string; error: string }>(
      'parakeet-model-download-error',
      (event) => {
        if (event.payload.modelName === expectedModel) {
          console.error('Transcription model download error:', event.payload.error);
        }
      }
    );

    const unlistenWhisperError = listen<{ modelName: string; error: string }>(
      'model-download-error',
      (event) => {
        if (event.payload.modelName === expectedModel) {
          console.error('Transcription model download error:', event.payload.error);
        }
      }
    );

    return () => {
      unlisten.then(fn => fn());
      unlistenWhisper.then(fn => fn());
      unlistenComplete.then(fn => fn());
      unlistenWhisperComplete.then(fn => fn());
      unlistenError.then(fn => fn());
      unlistenWhisperError.then(fn => fn());
    };
  }, [plan?.transcription_model]);

  // Listen to summary model (Built-in AI) download progress
  useEffect(() => {
    const unlisten = listen<{
      model: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status: string;
    }>(
      'builtin-ai-download-progress',
      (event) => {
        const { model, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
        if (selectedSummaryModel && model === selectedSummaryModel) {
          setSummaryModelProgress(progress);
          setSummaryModelProgressInfo({
            percent: progress,
            downloadedMb: downloaded_mb ?? 0,
            totalMb: total_mb ?? 0,
            speedMbps: speed_mbps ?? 0,
          });
          if (status === 'completed' || progress >= 100) {
            setSummaryModelDownloaded(true);
          }
        }
      }
    );

    return () => {
      unlisten.then(fn => fn());
    };
  }, [selectedSummaryModel]);

  const checkDatabaseStatus = async () => {
    try {
      const isFirstLaunch = await invoke<boolean>('check_first_launch');
      setDatabaseExists(!isFirstLaunch);
      console.log('[OnboardingContext] Database exists:', !isFirstLaunch);
    } catch (error) {
      console.error('[OnboardingContext] Failed to check database status:', error);
      setDatabaseExists(false);
    }
  };

  const loadOnboardingStatus = async () => {
    try {
      const status = await invoke<OnboardingStatus | null>('get_onboarding_status');
      if (status) {
        console.log('[OnboardingContext] Loaded saved status:', status);

        if (status.meeting_languages?.length) {
          setMeetingLanguages(status.meeting_languages);
        }

        if (status.completed) {
          setCurrentStep(status.current_step);
          setCompleted(true);
          setTranscriptionModelDownloaded(status.model_status.parakeet === 'downloaded');
          setSummaryModelDownloaded(status.model_status.summary === 'downloaded');
          if (status.model_status.selected_summary_model) {
            setSelectedSummaryModel(status.model_status.selected_summary_model);
          }
          console.log('[OnboardingContext] Restored completed onboarding status without model verification');
          return;
        }

        // Don't trust saved status - verify actual model status on disk
        const verifiedStatus = await verifyModelStatus(status);

        setCurrentStep(verifiedStatus.currentStep);
        setCompleted(verifiedStatus.completed);
        setTranscriptionModelDownloaded(verifiedStatus.transcriptionModelDownloaded);
        setSummaryModelDownloaded(verifiedStatus.summaryModelDownloaded);
        if (verifiedStatus.selectedSummaryModel) {
          setSelectedSummaryModel(verifiedStatus.selectedSummaryModel);
        }

        console.log('[OnboardingContext] Verified status:', verifiedStatus);

        // Check if any downloads are active to restore isBackgroundDownloading state
        await checkActiveDownloads();
      } else {
        await initializeSummaryModelSelection();
      }
    } catch (error) {
      console.error('[OnboardingContext] Failed to load onboarding status:', error);
    }
  };

  // Verify that models actually exist on disk, not just trust saved JSON
  const verifyModelStatus = async (savedStatus: OnboardingStatus) => {
    let transcriptionModelDownloaded = false;
    let summaryModelDownloaded = false;
    let selectedSummaryModel = '';

    // Verify the transcription model exists on disk. Which engine to ask
    // depends on the stored language answer; with no answer this falls to
    // Parakeet, matching what a pre-existing install already has.
    const savedLanguages = savedStatus.meeting_languages ?? [];
    try {
      const savedPlan = await invoke<OnboardingPlan>('onboarding_plan_for_languages', {
        languages: savedLanguages,
      });

      // The provider is passed explicitly because complete_onboarding has not
      // written the transcript config yet, so the stored one would answer for
      // whichever engine this machine used last rather than the one this
      // answer calls for.
      transcriptionModelDownloaded = await invoke<boolean>('transcription_model_available', {
        provider: savedPlan.engine === 'whisper' ? 'localWhisper' : 'parakeet',
      });
      console.log(
        '[OnboardingContext] Transcription model verified on disk:',
        transcriptionModelDownloaded,
        'engine:',
        savedPlan.engine
      );
    } catch (error) {
      console.warn('[OnboardingContext] Failed to verify transcription model:', error);
      transcriptionModelDownloaded = false;
    }

    // Verify the selected/recommended Summary model exists on disk.
    try {
      const recommendedModel = await invoke<string>('builtin_ai_get_recommended_model');
      setRecommendedSummaryModel(recommendedModel);
      const savedSelectedModel = savedStatus.model_status.selected_summary_model || '';
      const modelToCheck = savedSelectedModel || recommendedModel;
      const selectedModelReady = await invoke<boolean>('builtin_ai_is_model_ready', {
        modelName: modelToCheck,
        refresh: true,
      });
      const resolved = resolveOnboardingSummaryModelStatus({
        selectedModel: savedSelectedModel,
        recommendedModel,
        selectedModelReady,
      });
      selectedSummaryModel = resolved.selectedSummaryModel;
      summaryModelDownloaded = resolved.summaryModelDownloaded;
      console.log('[OnboardingContext] Summary model verified on disk:', summaryModelDownloaded, 'model:', selectedSummaryModel);
    } catch (error) {
      console.warn('[OnboardingContext] Failed to verify Summary model:', error);
      summaryModelDownloaded = false;
    }

    // Determine the correct step based on verified status.
    // Flow: 1 Welcome, 2 Meeting Language, 3 Setup Overview, 4 Download
    // Progress, 5 Permissions (macOS).
    let currentStep = savedStatus.current_step;
    let completed = savedStatus.completed;

    // Clamp step to the current max.
    if (currentStep > LAST_STEP) {
      currentStep = LAST_STEP - 1; // Go to download progress step
    }

    // Someone who stopped part-way through the old four-step flow has a step
    // number from before the language question existed. Sending them back to
    // the question is the only way they get asked it at all, and re-answering
    // costs one click.
    if (!savedStatus.meeting_languages && !completed) {
      currentStep = Math.min(currentStep, 2);
    }

    // Trust the completed status - don't revert based on model downloads
    // Downloads continue in background; user stays in main app regardless
    return {
      currentStep,
      completed,
      transcriptionModelDownloaded,
      summaryModelDownloaded,
      selectedSummaryModel,
    };
  };

  const saveOnboardingStatus = async () => {
    // Safety check: if we are in the process of completing, DO NOT save
    // This prevents a race condition where a download completion event triggers a save
    // that overwrites the "completed" status set by completeOnboarding
    if (isCompletingRef.current) {
      console.log('[OnboardingContext] Skipping saveOnboardingStatus because completion is in progress');
      return;
    }

    try {
      await invoke('save_onboarding_status_cmd', {
        status: {
          version: '1.0',
          completed: completed,
          current_step: currentStep,
          model_status: {
            parakeet: transcriptionModelDownloaded ? 'downloaded' : 'not_downloaded',
            summary: summaryModelDownloaded ? 'downloaded' : 'not_downloaded',
            selected_summary_model: selectedSummaryModel || undefined,
          },
          meeting_languages: meetingLanguages,
          last_updated: new Date().toISOString(),
        },
      });
    } catch (error) {
      console.error('[OnboardingContext] Failed to save onboarding status:', error);
    }
  };

  const completeOnboarding = async () => {
    try {
      // Set completion flag to prevent race conditions with auto-save
      isCompletingRef.current = true;

      // Clear any pending auto-saves
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
        saveTimeoutRef.current = undefined;
      }

      let modelToSave = selectedSummaryModel;
      if (!modelToSave) {
        modelToSave = await invoke<string>('builtin_ai_get_recommended_model');
        setSelectedSummaryModel(modelToSave);
      }

      const selectedModelReady = await invoke<boolean>('builtin_ai_is_model_ready', {
        modelName: modelToSave,
        refresh: true,
      });
      setSummaryModelDownloaded(selectedModelReady);
      if (!selectedModelReady) {
        requestSummaryModelDownload(modelToSave);
      }

      // Onboarding always uses builtin-ai with selected model. The languages go
      // with it: they decide which transcription engine gets written to the
      // transcript config, and they are stored so Settings can show the answer.
      await invoke('complete_onboarding', {
        model: modelToSave,
        languages: meetingLanguages,
      });
      setCompleted(true);
      console.log('[OnboardingContext] Onboarding completed with model:', modelToSave);

      // Reset the flag so subsequent state updates can be saved
      isCompletingRef.current = false;
    } catch (error) {
      console.error('[OnboardingContext] Failed to complete onboarding:', error);
      isCompletingRef.current = false; // Reset flag on error
      throw error; // Re-throw so PermissionsStep can handle it
    }
  };

  // Start background downloads for models.
  const startBackgroundDownloads = async ({
    includeTranscription,
    includeSummary,
    summaryModel,
  }: StartBackgroundDownloadsOptions) => {
    console.log('[OnboardingContext] Starting background downloads:', {
      includeTranscription,
      includeSummary,
      summaryModel,
    });

    try {
      const shouldStartTranscription =
        includeTranscription && !transcriptionModelDownloaded && !!plan;
      const shouldStartSummary = includeSummary && !summaryModelDownloaded && !!summaryModel;

      if (!shouldStartTranscription && !shouldStartSummary) {
        if (includeSummary && !summaryModelDownloaded && !summaryModel) {
          console.warn('[OnboardingContext] Summary Model download skipped until recommendation is loaded');
        }
        if (includeTranscription && !transcriptionModelDownloaded && !plan) {
          console.warn('[OnboardingContext] Transcription download skipped until the language answer is resolved');
        }
        return;
      }

      setIsBackgroundDownloading(true);

      // Start the transcription model first; it is what gates recording.
      if (shouldStartTranscription && plan) {
        console.log('[OnboardingContext] Starting transcription download:', plan.transcription_model);
        const command =
          plan.engine === 'whisper' ? 'whisper_download_model' : 'parakeet_download_model';
        invoke(command, { modelName: plan.transcription_model })
          .catch(err => console.error('[OnboardingContext] Transcription download failed:', err));
      }

      // Start selected Summary Model download immediately so completion cannot race the request.
      if (shouldStartSummary && summaryModel) {
        requestSummaryModelDownload(summaryModel);
      }
    } catch (error) {
      console.error('[OnboardingContext] Failed to start background downloads:', error);
      setIsBackgroundDownloading(false);
      throw error;
    }
  };

  // Check if any models are currently downloading (for re-entry)
  const checkActiveDownloads = async () => {
    try {
      const models = await invoke<any[]>('parakeet_get_available_models');
      const isDownloading = models.some(m => m.status && (typeof m.status === 'object' ? 'Downloading' in m.status : m.status === 'Downloading'));
      
      if (isDownloading) {
        console.log('[OnboardingContext] Detected active background downloads on mount');
        setIsBackgroundDownloading(true);
      }
      
      // Also check for Built-in AI downloads if possible (though less critical as Parakeet is the main blocker)
      
    } catch (error) {
      console.warn('[OnboardingContext] Failed to check active downloads:', error);
    }
  };

  const retryTranscriptionDownload = async () => {
    const model = plan?.transcription_model ?? PARAKEET_MODEL;
    console.log('[OnboardingContext] Retrying transcription download:', model);
    try {
      // Whisper has no dedicated retry command; re-issuing the download is the
      // retry, and its downloader clears the previous cancellation flag itself.
      const command =
        plan?.engine === 'whisper' ? 'whisper_download_model' : 'parakeet_retry_download';
      await invoke(command, { modelName: model });
    } catch (error) {
      console.error('[OnboardingContext] Retry failed:', error);
      throw error;
    }
  };

  const setPermissionStatus = useCallback((permission: keyof OnboardingPermissions, status: PermissionStatus) => {
    setPermissions((prev: OnboardingPermissions) => ({
      ...prev,
      [permission]: status,
    }));
  }, []);

  const goToStep = useCallback((step: number) => {
    setCurrentStep(Math.max(1, Math.min(step, LAST_STEP)));
  }, []);

  const goNext = useCallback(() => {
    setCurrentStep((prev: number) => {
      const next = prev + 1;
      return Math.min(next, LAST_STEP);
    });
  }, []);

  const goPrevious = useCallback(() => {
    setCurrentStep((prev: number) => {
      const previous = prev - 1;
      // Don't go below step 1
      return Math.max(previous, 1);
    });
  }, []);

  return (
    <OnboardingContext.Provider
      value={{
        currentStep,
        transcriptionModelDownloaded,
        transcriptionProgress,
        transcriptionProgressInfo,
        summaryModelDownloaded,
        summaryModelProgress,
        summaryModelProgressInfo,
        selectedSummaryModel,
        recommendedSummaryModel,
        databaseExists,
        isBackgroundDownloading,
        meetingLanguages,
        plan,
        permissions,
        permissionsSkipped,
        goToStep,
        goNext,
        goPrevious,
        setTranscriptionModelDownloaded,
        setSummaryModelDownloaded,
        setSelectedSummaryModel,
        setDatabaseExists,
        setMeetingLanguages,
        setPermissionStatus,
        setPermissionsSkipped,
        completeOnboarding,
        startBackgroundDownloads,
        retryTranscriptionDownload,
      }}
    >
      {children}
    </OnboardingContext.Provider>
  );
}

export function useOnboarding() {
  const context = useContext(OnboardingContext);
  if (!context) {
    throw new Error('useOnboarding must be used within OnboardingProvider');
  }
  return context;
}
