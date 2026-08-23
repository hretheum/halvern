export type OnboardingStep = 1 | 2 | 3 | 4 | 5;

export type TranscriptionEngine = 'parakeet' | 'whisper';

/// What a given set of meeting languages will install, and what it weighs.
/// Mirrors `OnboardingPlan` in src-tauri/src/onboarding.rs; the decision itself
/// is made and tested there, and this side only renders the answer.
export interface OnboardingPlan {
  engine: TranscriptionEngine;
  transcription_model: string;
  transcription_size_mb: number;
  summary_model: string;
  summary_size_mb: number;
}

export type PermissionStatus = 'checking' | 'not_determined' | 'authorized' | 'denied';

export interface OnboardingPermissions {
  microphone: PermissionStatus;
  systemAudio: PermissionStatus;
  screenRecording: PermissionStatus;
}

export interface OnboardingContainerProps {
  title: string;
  description?: React.ReactNode;
  children: React.ReactNode;
  step?: number;
  totalSteps?: number;
  stepOffset?: number;
  hideProgress?: boolean;
  className?: string;
  showNavigation?: boolean;
  onNext?: () => void;
  onPrevious?: () => void;
  canGoNext?: boolean;
  canGoPrevious?: boolean;
}

export interface PermissionRowProps {
  icon: React.ReactNode;
  title: string;
  description: string;
  status: PermissionStatus;
  isPending?: boolean;
  onAction: () => void;
}

export interface StatusIndicatorProps {
  status: 'idle' | 'checking' | 'success' | 'error';
  size?: 'sm' | 'md' | 'lg';
}
