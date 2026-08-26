'use client';

import React, { useState, useEffect } from 'react';
import { ChevronLeft } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { takeSettingsOrigin } from '@/lib/settingsOrigin';
import { invoke } from '@tauri-apps/api/core';
import { TranscriptSettings } from '@/components/TranscriptSettings';
import { RecordingSettings } from '@/components/RecordingSettings';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { SummaryModelSettings } from '@/components/SummaryModelSettings';
import { BetaSettings } from '@/components/BetaSettings';
import { ObsidianExportSettings } from '@/components/ObsidianExportSettings';
import { MeetingDetectionSettings } from '@/components/MeetingDetectionSettings';
import { TemplateSettings } from '@/components/TemplateSettings';
import { DataLocationsSettings } from '@/components/DataLocationsSettings';
import Info from '@/components/Info';
import { useConfig } from '@/contexts/ConfigContext';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

/**
 * Six categories in a left column, per the redesign. Everything the old
 * eight flat tabs offered lives on: General (notifications, telemetry),
 * Recording (devices, saving, detection incl. its diagnostics), Transcription
 * (engines, models), Summarization (model providers, language, templates),
 * Export (Obsidian), Advanced (beta flags, data locations). Theme moved to
 * the top bar, visible on every screen.
 */
const CATEGORIES = [
  { value: 'general', label: 'General' },
  { value: 'recording', label: 'Recording' },
  { value: 'transcription', label: 'Transcription' },
  { value: 'summarization', label: 'Summarization' },
  { value: 'export', label: 'Export' },
  { value: 'advanced', label: 'Advanced' },
] as const;

type CategoryValue = (typeof CATEGORIES)[number]['value'];

export default function SettingsPage() {
  const router = useRouter();
  const { transcriptModelConfig, setTranscriptModelConfig } = useConfig();
  const { setActiveSettingsTab } = useSidebar();

  const [category, setCategory] = useState<CategoryValue>('general');

  // Tell globally-mounted components (e.g. the download progress toast)
  // which category is open, so they can skip duplicating a status this page
  // already shows inline. Cleared on unmount/category change so nothing
  // stays suppressed once the user leaves.
  useEffect(() => {
    setActiveSettingsTab(category);
    return () => setActiveSettingsTab(null);
  }, [category, setActiveSettingsTab]);

  // Escape leaves Settings, the way it closes anything else layered over the
  // work. The back button in the corner stays: this is the shortcut for people
  // who expect it, not a replacement for the visible way out.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return;

      // A dialog, a select or a popover open on top of Settings owns Escape —
      // closing the whole screen out from under one would be the wrong answer
      // to "get me out of this". Radix marks its open layers with data-state,
      // so their presence is the signal.
      if (document.querySelector('[role="dialog"], [data-state="open"][role="listbox"]')) {
        return;
      }

      // Typing in a field: Escape belongs to the field first — reverting an
      // edit is what people expect there, not navigating away from it.
      const target = event.target as HTMLElement | null;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target?.isContentEditable
      ) {
        return;
      }

      router.push(takeSettingsOrigin());
    };

    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [router]);

  // Load saved transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const config = await invoke('api_get_transcript_config') as any;
        if (config) {
          setTranscriptModelConfig({
            provider: config.provider || 'localWhisper',
            model: config.model || 'large-v3',
            apiKey: config.apiKey || null
          });
        }
      } catch (error) {
        console.error('Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, [setTranscriptModelConfig]);

  return (
    <div className="flex h-full bg-background">
      {/* Category column */}
      <div className="w-[190px] shrink-0 border-r border-border bg-card px-2 py-3 flex flex-col gap-0.5 overflow-y-auto">
        <button
          onClick={() => router.push(takeSettingsOrigin())}
          className="flex items-center gap-1.5 px-2 py-1.5 mb-1.5 rounded-md text-xs text-muted-foreground hover:text-foreground text-left"
        >
          <ChevronLeft className="w-3.5 h-3.5" />
          Done
        </button>
        <div className="text-[11px] font-bold uppercase tracking-wider text-muted-foreground px-2 pb-1">
          Settings
        </div>
        {CATEGORIES.map(({ value, label }) => (
          <button
            key={value}
            onClick={() => setCategory(value)}
            className={`px-2.5 py-2 rounded-lg text-[13px] text-left transition-colors ${
              category === value
                ? 'bg-muted font-semibold'
                : 'font-normal hover:bg-muted/60'
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {/* Category content */}
      <div className="flex-1 min-w-0 overflow-y-auto px-8 py-6 pb-16">
        {category === 'general' && (
          <div className="max-w-xl">
            <h2 className="text-lg font-bold mb-4">General</h2>
            <PreferenceSettings />
            {/* About + version, rehomed from the removed sidebar footer */}
            <div className="mt-6 max-w-[200px]">
              <Info isCollapsed={false} />
            </div>
          </div>
        )}

        {category === 'recording' && (
          <div className="max-w-2xl space-y-8">
            <h2 className="text-lg font-bold mb-4">Recording</h2>
            <RecordingSettings />
            <MeetingDetectionSettings />
          </div>
        )}

        {category === 'transcription' && (
          <div className="max-w-2xl">
            <h2 className="text-lg font-bold mb-4">Transcription</h2>
            <TranscriptSettings
              transcriptModelConfig={transcriptModelConfig}
              setTranscriptModelConfig={setTranscriptModelConfig}
            />
          </div>
        )}

        {category === 'summarization' && (
          <div className="max-w-2xl space-y-8">
            <h2 className="text-lg font-bold mb-4">Summarization</h2>
            <SummaryModelSettings />
            <TemplateSettings />
          </div>
        )}

        {category === 'export' && (
          <div className="max-w-xl">
            <h2 className="text-lg font-bold mb-4">Export</h2>
            <ObsidianExportSettings />
          </div>
        )}

        {category === 'advanced' && (
          <div className="max-w-2xl space-y-8">
            <h2 className="text-lg font-bold mb-4">Advanced</h2>
            <BetaSettings />
            <DataLocationsSettings />
          </div>
        )}
      </div>
    </div>
  );
}
