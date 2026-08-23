import React, { useEffect } from 'react';
import { useOnboarding } from '@/contexts/OnboardingContext';
import {
  WelcomeStep,
  MeetingLanguageStep,
  SummaryModelStep,
  PermissionsStep,
  DownloadProgressStep,
  SetupOverviewStep,
} from './steps';

interface OnboardingFlowProps {
  onComplete: () => void;
}

export function OnboardingFlow({ onComplete }: OnboardingFlowProps) {
  const { currentStep } = useOnboarding();
  const [isMac, setIsMac] = React.useState(false);

  useEffect(() => {
    // Check if running on macOS
    const checkPlatform = async () => {
      try {
        // Dynamic import to avoid SSR issues if any
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        console.error('Failed to detect platform:', e);
        // Fallback
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    checkPlatform();
  }, []);

  // 5-Step Onboarding Flow:
  // Step 1: Welcome - Introduce Halvern features
  // Step 2: Meeting Language - decides the speech engine and ranks the summary model
  // Step 3: Setup Overview - Database initialization + show what the answer will download
  // Step 4: Download Progress - Download the chosen speech engine + Summary Model
  // Step 5: Permissions - Request mic + system audio (macOS only)
  //
  // The language question comes before the overview because the overview
  // announces what is about to be downloaded and how large it is, and both of
  // those now depend on the answer.

  return (
    <div className="onboarding-flow">
      {currentStep === 1 && <WelcomeStep />}
      {currentStep === 2 && <MeetingLanguageStep />}
      {currentStep === 3 && <SummaryModelStep />}
      {currentStep === 4 && <SetupOverviewStep />}
      {currentStep === 5 && <DownloadProgressStep />}
      {currentStep === 6 && isMac && <PermissionsStep />}
    </div>
  );
}
