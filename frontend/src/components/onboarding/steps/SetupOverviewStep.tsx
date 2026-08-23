import React, { useEffect, useState } from 'react';
import { Info } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

export function SetupOverviewStep() {
  const { goNext, plan } = useOnboarding();
  const [isMac, setIsMac] = useState(false);

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

  // Sizes come from the plan the language answer produced, so the step tells
  // the truth about this particular first run rather than a generic figure.
  const steps = [
    {
      number: 1,
      type: 'transcription',
      title: plan
        ? `Download Transcription Engine (${formatSize(plan.transcription_size_mb)})`
        : 'Download Transcription Engine',
    },
    {
      number: 2,
      type: 'summarization',
      title: plan
        ? `Download Summarization Engine (${formatSize(plan.summary_size_mb)})`
        : 'Download Summarization Engine',
    },
  ];

  const totalMb = plan ? plan.transcription_size_mb + plan.summary_size_mb : 0;

  const handleContinue = () => {
    goNext();
  };

  return (
    <OnboardingContainer
      title="Setup Overview"
      description="Halvern requires that you download the Transcription & Summarization AI models for the software to work."
      step={4}
      totalSteps={6}
    >
      <div className="flex flex-col items-center space-y-10">
        {/* Steps Card */}
        <div className="w-full max-w-md bg-card rounded-lg border border-border p-4">
          <div className="space-y-4">
            {steps.map((step, idx) => {
              return (
                <div
                  key={step.number}
                  className={`flex items-start gap-4 p-1`}
                >
                  <div className="flex-1 ml-1">
                    <h3 className="font-medium text-foreground flex items-center gap-2">
                        Step {step.number} :  {step.title}

                        {step.type === "summarization" && (
                            <TooltipProvider>
                            <Tooltip>
                                <TooltipTrigger asChild>
                                <button className="text-muted-foreground hover:text-foreground">
                                    <Info className="w-4 h-4" />
                                </button>
                                </TooltipTrigger>
                                <TooltipContent className="max-w-xs text-sm">
                                You can also select external AI providers like OpenAI, Claude, or
                                Ollama for summary generation in settings.
                                </TooltipContent>
                            </Tooltip>
                            </TooltipProvider>
                        )}
                        </h3>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {totalMb > 0 && (
          <p className="text-sm text-muted-foreground text-center max-w-md -mt-6">
            About {formatSize(totalMb)} in total, downloaded once. Everything then runs on this
            machine.
          </p>
        )}

        {/* CTA Section */}
        <div className="w-full max-w-xs space-y-4">
          <Button
            onClick={handleContinue}
            className="w-full h-11 bg-primary hover:bg-primary/90 text-primary-foreground"
          >
            Let's Go
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
