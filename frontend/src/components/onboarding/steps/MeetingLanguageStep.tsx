import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Languages, Info } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { useConfig } from '@/contexts/ConfigContext';
import { LANGUAGES } from '@/constants/languages';
import { normaliseLanguageCode } from '@/lib/summary-languages';
import { writePinnedSummaryLanguageDefault } from '@/lib/summary-language-preferences';
import type { OnboardingPlan } from '@/types/onboarding';

// "Auto detect" is not an answer to "what language are your meetings in" — it
// is a transcription mode, and offering it here would collect a non-answer we
// could not choose an engine from.
const CHOOSABLE_LANGUAGES = LANGUAGES.filter(
  language => language.code !== 'auto' && language.code !== 'auto-translate'
);

function languageName(code: string): string {
  return CHOOSABLE_LANGUAGES.find(language => language.code === code)?.name ?? code;
}

/// The OS locale is a good guess and a bad assumption: plenty of people run an
/// English system and hold meetings in something else, which is exactly the
/// case this question exists to catch. So it preselects and nothing more.
function guessFromLocale(): string {
  if (typeof navigator === 'undefined') return 'en';
  const base = (navigator.language || 'en').split(/[-_]/)[0].toLowerCase();
  return CHOOSABLE_LANGUAGES.some(language => language.code === base) ? base : 'en';
}

function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

export function MeetingLanguageStep() {
  const { goNext, meetingLanguages, setMeetingLanguages } = useOnboarding();
  const { setSelectedLanguage } = useConfig();

  const [primary, setPrimary] = useState<string>(() => meetingLanguages[0] ?? guessFromLocale());
  const [isMultilingual, setIsMultilingual] = useState(meetingLanguages.length > 1);
  const [extras, setExtras] = useState<string[]>(() => meetingLanguages.slice(1));
  const [plan, setPlan] = useState<OnboardingPlan | null>(null);

  const selected = useMemo(
    () => (isMultilingual ? [primary, ...extras] : [primary]),
    [isMultilingual, primary, extras]
  );

  useEffect(() => {
    let cancelled = false;

    invoke<OnboardingPlan>('onboarding_plan_for_languages', { languages: selected })
      .then(result => {
        if (!cancelled) setPlan(result);
      })
      .catch(error => {
        console.error('[MeetingLanguageStep] Failed to plan for languages:', error);
        if (!cancelled) setPlan(null);
      });

    return () => {
      cancelled = true;
    };
  }, [selected]);

  const toggleExtra = useCallback((code: string) => {
    setExtras(previous =>
      previous.includes(code) ? previous.filter(c => c !== code) : [...previous, code]
    );
  }, []);

  // One answer, three settings. Beyond choosing the engine, a single named
  // language is also a good default for the transcription hint and for the
  // language summaries are written in — both of which were otherwise left to
  // per-meeting detection. A multilingual answer seeds neither: "several" is
  // not a language, and detection is genuinely the right behaviour there
  // rather than a degraded one.
  const seedLanguageDefaults = (codes: string[]) => {
    if (codes.length !== 1) return;
    const [code] = codes;

    // Whisper takes this as its language hint; Parakeet ignores it and detects
    // for itself, so setting it is correct under either engine.
    setSelectedLanguage(code);

    const summaryCode = normaliseLanguageCode(code);
    if (summaryCode) {
      writePinnedSummaryLanguageDefault(summaryCode);
    }
  };

  const handleContinue = () => {
    setMeetingLanguages(selected);
    seedLanguageDefaults(selected);
    goNext();
  };

  return (
    <OnboardingContainer
      title="What language are your meetings in?"
      description="Not the language of the app — the language your meetings are held in. It decides which speech model transcribes them and which model writes the summaries. You can change it later in Settings."
      step={2}
      totalSteps={6}
    >
      <div className="flex flex-col items-center space-y-6">
        <div className="w-full max-w-md space-y-4">
          <div className="space-y-2">
            <label htmlFor="meeting-language" className="text-sm font-medium text-foreground">
              Most of my meetings are in
            </label>
            <Select value={primary} onValueChange={setPrimary}>
              <SelectTrigger id="meeting-language" className="w-full">
                <SelectValue placeholder="Select a language" />
              </SelectTrigger>
              <SelectContent className="max-h-72">
                {CHOOSABLE_LANGUAGES.map(language => (
                  <SelectItem key={language.code} value={language.code}>
                    {language.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-start gap-2">
            <Checkbox
              id="multilingual"
              checked={isMultilingual}
              onCheckedChange={checked => setIsMultilingual(checked === true)}
            />
            <label htmlFor="multilingual" className="text-sm text-muted-foreground leading-tight">
              I also hold meetings in other languages
            </label>
          </div>

          {isMultilingual && (
            <ScrollArea className="h-44 w-full rounded-md border border-border p-3">
              <div className="grid grid-cols-2 gap-2">
                {CHOOSABLE_LANGUAGES.filter(language => language.code !== primary).map(language => (
                  <div key={language.code} className="flex items-center gap-2">
                    <Checkbox
                      id={`extra-${language.code}`}
                      checked={extras.includes(language.code)}
                      onCheckedChange={() => toggleExtra(language.code)}
                    />
                    <label
                      htmlFor={`extra-${language.code}`}
                      className="text-sm text-foreground truncate"
                    >
                      {language.name}
                    </label>
                  </div>
                ))}
              </div>
            </ScrollArea>
          )}
        </div>

        {plan && (
          <div className="w-full max-w-md rounded-lg border border-border bg-card p-4 space-y-2">
            <div className="flex items-center gap-2 text-sm font-medium text-foreground">
              <Languages className="w-4 h-4" />
              {plan.engine === 'parakeet' ? 'Fast on-device recognition' : 'Wide language coverage'}
            </div>
            <p className="text-sm text-muted-foreground">
              {plan.engine === 'parakeet'
                ? `We will install the fast recognition model, which covers ${
                    selected.length > 1 ? 'all the languages you picked' : languageName(primary)
                  }, plus a summarization model.`
                : `Your selection needs the broader multilingual model, so we will install that one instead, plus a summarization model.`}
            </p>
            <p className="text-sm text-muted-foreground">
              Download: {formatSize(plan.transcription_size_mb)} for speech recognition and{' '}
              {formatSize(plan.summary_size_mb)} for summaries.
            </p>
          </div>
        )}

        <div className="w-full max-w-md flex items-start gap-2 text-sm text-muted-foreground">
          <Info className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span>
            Everything runs on this machine. Nothing about your meetings is sent anywhere.
          </span>
        </div>

        <div className="w-full max-w-xs">
          <Button
            onClick={handleContinue}
            className="w-full h-11 bg-primary hover:bg-primary/90 text-primary-foreground"
          >
            Continue
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
