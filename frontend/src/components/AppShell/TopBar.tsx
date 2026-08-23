'use client';

import { useEffect } from 'react';
import { useRouter, usePathname } from 'next/navigation';
import { Settings } from 'lucide-react';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { HalvernMark } from '@/components/brand/HalvernMark';
import { LAYOUT } from '@/lib/layout';

/**
 * The application toolbar shown on every screen: the logo on the left and
 * settings on the right. When a recording is running it also shows a live
 * indicator that jumps to the recording screen, so a detector-started
 * recording stays reachable from anywhere. Theme selection moved to
 * Settings > General, where it can carry labels.
 *
 * The logo navigates home. Since the toolbar is the one element mounted on
 * every screen, it is the only place a way back to the library can live
 * without each screen having to provide its own.
 */
export function TopBar() {
  const router = useRouter();
  const pathname = usePathname();
  const { isRecording, isPaused } = useRecordingState();

  // The tray's "Settings" item calls window.openSettings. The sidebar used
  // to define it against a modal it never rendered; the top bar is mounted
  // on every screen, so this is where the hook now lives.
  useEffect(() => {
    (window as unknown as { openSettings?: () => void }).openSettings = () =>
      router.push('/settings');
    return () => {
      delete (window as unknown as { openSettings?: () => void }).openSettings;
    };
  }, [router]);

  return (
    <div className={`h-10 shrink-0 flex items-center gap-2.5 ${LAYOUT.INSET} bg-card border-b border-border`}>
      {/* Mark and wordmark are one control, the way a site logo is: from a
          recording or a transcript this is the way back to the library, which
          is the app's home. Disabled rather than hidden on the library itself,
          so the toolbar does not reflow between screens. */}
      <button
        type="button"
        onClick={() => router.push('/')}
        disabled={pathname === '/'}
        aria-label="Go to Library"
        aria-current={pathname === '/' ? 'page' : undefined}
        className="group -ml-2 flex h-8 items-center gap-2 rounded-md pl-2 pr-3 transition-colors enabled:hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-default"
      >
        <HalvernMark size={17} className="text-primary shrink-0" />
        <span className="text-[13px] font-semibold text-muted-foreground transition-colors group-enabled:group-hover:text-foreground">
          Halvern
        </span>
      </button>
      <div className="flex-1" />

      {isRecording && pathname !== '/record' && (
        <button
          onClick={() => router.push('/record')}
          className="flex items-center gap-2 px-3 h-7 rounded-full bg-muted hover:bg-accent transition-colors"
          aria-label="Go to the active recording"
        >
          <span
            className={`w-2 h-2 rounded-full ${
              isPaused ? 'bg-orange-500' : 'bg-recording animate-pulse'
            }`}
          />
          <span className="text-xs font-semibold">{isPaused ? 'Paused' : 'Recording'}</span>
        </button>
      )}

      <button
        title="Settings"
        aria-label="Settings"
        onClick={() => router.push('/settings')}
        className={`flex items-center justify-center ${LAYOUT.ICON_BUTTON} rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground transition-colors`}
      >
        <Settings className="w-4 h-4" />
      </button>
    </div>
  );
}
