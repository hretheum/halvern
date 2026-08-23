'use client';

import { useEffect, useState } from 'react';
import { useTheme } from 'next-themes';
import { Monitor, Sun, Moon } from 'lucide-react';

const THEME_MODES = [
  { value: 'system', label: 'System', hint: 'Follow the operating system', Icon: Monitor },
  { value: 'light', label: 'Light', hint: 'Always light', Icon: Sun },
  { value: 'dark', label: 'Dark', hint: 'Always dark', Icon: Moon },
] as const;

/**
 * Theme selection, as a labelled segmented control.
 *
 * This lived in the toolbar as three unlabelled icons, where it competed for
 * attention with the recording indicator on every screen while being a setting
 * people change once. Settings has room for the labels, which is what makes
 * "System" legible as a distinct choice rather than a third icon.
 */
export function AppearanceSetting() {
  const { theme, setTheme } = useTheme();

  // next-themes only knows the resolved theme after mount; rendering the
  // selection before that would mismatch the server markup.
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  return (
    <div className="bg-card rounded-lg border border-border p-6 shadow-xs">
      <div className="mb-4">
        <h3 className="text-lg font-semibold text-foreground mb-2">Appearance</h3>
        <p className="text-sm text-muted-foreground">
          Choose how Halvern looks, or let it follow your system setting.
        </p>
      </div>

      <div
        className="inline-flex gap-1 rounded-lg bg-muted p-1"
        role="radiogroup"
        aria-label="Theme"
      >
        {THEME_MODES.map(({ value, label, hint, Icon }) => {
          const selected = mounted && theme === value;
          return (
            <button
              key={value}
              type="button"
              role="radio"
              aria-checked={selected}
              title={hint}
              onClick={() => setTheme(value)}
              className={`flex items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                selected
                  ? 'bg-card text-foreground shadow-sm font-medium'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              <Icon className="w-4 h-4" />
              {label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
