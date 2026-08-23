import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, MemoryStick, TriangleAlert } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';

/**
 * Choosing the summarisation model, rather than having it chosen silently.
 *
 * Before this step, onboarding asked `builtin_ai_get_recommended_model` and
 * downloaded the answer without ever showing the list. That made a wrong
 * default unrecoverable in practice: the user could not know a choice had
 * existed, so there was nothing for them to undo.
 *
 * Two things make the list worth showing rather than merely honest. Every model
 * says whether it fits the machine's memory, with both numbers, so "not
 * available" is never a mystery. And the ones that are a bad idea here are
 * listed below a line with the reason attached, instead of being hidden —
 * hiding them would be the same paternalism in a nicer coat.
 *
 * The notes come from a measured comparison
 * (`docs/experiments/summary-model-bakeoff/results/REPORT.md`), not from the
 * shipped display names, which are inherited marketing: "High Quality",
 * "Balanced", "Fast" decide nothing for anybody.
 */

interface SummaryModelOption {
  name: string;
  display_name: string;
  size_mb: number;
  min_ram_gb: number;
  fits_ram: boolean;
  is_default: boolean;
  recommended: boolean;
  note: string;
  blocker: string;
}

interface SummaryModelChoices {
  system_ram_gb: number;
  options: SummaryModelOption[];
}

function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

function ModelTile({
  option,
  selected,
  onSelect,
}: {
  option: SummaryModelOption;
  selected: boolean;
  onSelect: (name: string) => void;
}) {
  const unavailable = !option.fits_ram;

  // `aria-disabled` rather than `disabled`. A disabled button is removed from
  // the tab order, which would hide the one thing this tile exists to say —
  // why the model cannot be used here. Screen-reader users would meet a gap in
  // the list and no explanation. The click is refused in the handler instead.
  return (
    <button
      type="button"
      aria-disabled={unavailable || undefined}
      onClick={() => !unavailable && onSelect(option.name)}
      aria-pressed={unavailable ? undefined : selected}
      className={[
        'w-full rounded-lg border p-4 text-left transition-colors',
        'focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:border-ring',
        unavailable
          ? 'cursor-not-allowed border-border bg-muted/40'
          : selected
            ? 'border-primary bg-accent'
            : 'border-border hover:bg-muted',
      ].join(' ')}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className={unavailable ? 'font-medium text-disabled' : 'font-medium'}>
              {option.display_name}
            </span>
            {/* A badge, so 12px is fine — but it still has to clear 4.5:1 at
                that size, and accent-foreground on accent measures 4.37/3.75.
                foreground on the same ground is 12.79/10.70. The bronze reads
                as branding, so the border carries it instead of the text. */}
            {option.is_default && (
              <span className="rounded border border-primary bg-accent px-1.5 py-0.5 text-xs font-medium text-foreground">
                Suggested
              </span>
            )}
          </div>
          <div className="mt-1 text-sm text-muted-foreground tabular-nums">
            {formatSize(option.size_mb)} download
            {option.min_ram_gb > 0 && ` · needs ${option.min_ram_gb} GB memory`}
          </div>
        </div>
        {selected && <Check className="mt-0.5 h-4 w-4 shrink-0 text-accent-foreground" />}
      </div>

      {/* The blocker replaces the note rather than joining it: when a model is
          not usable here, why not is the only thing worth the reader's time. */}
      {option.blocker ? (
        <p className="mt-2 flex items-start gap-1.5 text-sm text-warning-text">
          <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{option.blocker}</span>
        </p>
      ) : (
        option.note && <p className="mt-2 text-sm text-muted-foreground">{option.note}</p>
      )}
    </button>
  );
}

export function SummaryModelStep() {
  const { meetingLanguages, selectedSummaryModel, setSelectedSummaryModel, goNext } =
    useOnboarding();

  const [choices, setChoices] = useState<SummaryModelChoices | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<SummaryModelChoices>('summary_model_choices', { languages: meetingLanguages ?? [] })
      .then(result => {
        if (cancelled) return;
        setChoices(result);
        // Preselect the suggestion, so somebody who reads nothing and presses
        // Continue still lands on the sensible option.
        if (!selectedSummaryModel) {
          const suggested = result.options.find(o => o.is_default);
          if (suggested) setSelectedSummaryModel(suggested.name);
        }
      })
      .catch(e => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
    // meetingLanguages is the only input that changes the answer.
  }, [meetingLanguages, selectedSummaryModel, setSelectedSummaryModel]);

  const handleSelect = useCallback(
    (name: string) => setSelectedSummaryModel(name),
    [setSelectedSummaryModel]
  );

  const recommended = choices?.options.filter(o => o.recommended) ?? [];
  const rest = choices?.options.filter(o => !o.recommended) ?? [];

  return (
    <OnboardingContainer
      title="Which model should write your summaries?"
      description="It runs on this Mac, so the choice is a trade between how good the summaries are and how long they take. You can change it later in Settings."
      step={3}
      totalSteps={6}
    >
      <div className="mx-auto w-full max-w-md">
      {error && <p className="text-sm text-danger-text">Could not read the model list: {error}</p>}

      {choices && (
        <>
          <p className="mb-4 flex items-center gap-2 text-sm text-muted-foreground">
            <MemoryStick className="h-4 w-4 shrink-0" />
            <span>
              This Mac has <span className="tabular-nums">{choices.system_ram_gb} GB</span> of
              memory
              {recommended.length > 0 && (
                <>
                  {' '}
                  — enough for{' '}
                  {recommended.length === choices.options.length
                    ? 'every model below'
                    : `${recommended.length} of ${choices.options.length} models`}
                </>
              )}
              .
            </span>
          </p>

          <div className="space-y-2">
            {recommended.map(option => (
              <ModelTile
                key={option.name}
                option={option}
                selected={option.name === selectedSummaryModel}
                onSelect={handleSelect}
              />
            ))}
          </div>

          {rest.length > 0 && (
            <>
              <h3 className="mb-2 mt-6 text-sm font-medium uppercase tracking-wide text-tertiary">
                Not recommended for this setup
              </h3>
              <div className="space-y-2">
                {rest.map(option => (
                  <ModelTile
                    key={option.name}
                    option={option}
                    selected={option.name === selectedSummaryModel}
                    onSelect={handleSelect}
                  />
                ))}
              </div>
            </>
          )}
        </>
      )}

      </div>

      <div className="mx-auto mt-8 w-full max-w-xs">
        <Button
          onClick={goNext}
          disabled={!selectedSummaryModel}
          className="h-11 w-full bg-primary text-primary-foreground hover:bg-primary/90"
        >
          Continue
        </Button>
      </div>
    </OnboardingContainer>
  );
}
