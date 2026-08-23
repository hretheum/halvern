'use client';

import { useEffect, useMemo, useState } from 'react';
import { Search, ChevronUp, ChevronDown, Copy } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  VirtualizedTranscriptView,
  cleanStopWords,
} from '@/components/VirtualizedTranscriptView';
import { TranscriptSegmentData } from '@/types';
import Analytics from '@/lib/analytics';
import { LAYOUT } from '@/lib/layout';

interface WorkshopTranscriptPanelProps {
  segments: TranscriptSegmentData[];
  onCopyTranscript: () => void;
  customPrompt: string;
  onPromptChange: (value: string) => void;

  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  /** Narrow-window drawer mode: absolutely positioned over the summary. */
  asDrawer: boolean;
}

/**
 * The workshop's transcript pane: search with match stepping that scrolls
 * the hit into view, and the free-text context box the summary prompt uses.
 * Matching runs on the cleaned text the reader actually sees.
 */
export function WorkshopTranscriptPanel({
  segments,
  onCopyTranscript,
  customPrompt,
  onPromptChange,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  asDrawer,
}: WorkshopTranscriptPanelProps) {
  const [search, setSearch] = useState('');
  const [matchStep, setMatchStep] = useState(0);

  const query = search.trim().toLowerCase();

  // Only loaded segments can match. While a search is active and pages
  // remain, keep pulling them in so the match count converges on the truth;
  // the "Showing X of Y segments" line reports the progress.
  const searchIncomplete = !!query && !!hasMore;
  useEffect(() => {
    if (searchIncomplete && !isLoadingMore) onLoadMore?.();
  }, [searchIncomplete, isLoadingMore, onLoadMore]);

  const matches = useMemo(() => {
    if (!query) return [];
    const found: number[] = [];
    segments.forEach((segment, index) => {
      if (cleanStopWords(segment.text).toLowerCase().includes(query)) {
        found.push(index);
      }
    });
    return found;
  }, [segments, query]);

  const matchCount = matches.length;
  const matchIndex = matchCount ? ((matchStep % matchCount) + matchCount) % matchCount : 0;
  const activeSegmentIndex = matchCount ? matches[matchIndex] : null;

  return (
    <div
      className={
        asDrawer
          ? 'absolute inset-y-0 left-0 w-[300px] z-10 flex flex-col bg-card shadow-[6px_0_24px_rgba(0,0,0,0.2)]'
          : 'relative w-[340px] shrink-0 flex flex-col border-r border-border bg-card'
      }
    >
      {/* Search header */}
      <div className={`flex items-center ${LAYOUT.GAP} ${LAYOUT.INSET} py-2.5 border-b border-border`}>
        {/* The icon sits in the marginalia column; the box is what is 48px
            wide, not the glyph. */}
        <span className={`${LAYOUT.GUTTER} shrink-0 flex items-center`}>
          <Search className="w-[13px] h-[13px] text-muted-foreground" />
        </span>
        <input
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setMatchStep(0);
          }}
          placeholder="Search transcript…"
          className="flex-1 min-w-0 bg-transparent outline-none text-xs text-foreground placeholder:text-muted-foreground"
        />
        {query && (
          <>
            <button
              onClick={() => setMatchStep((s) => s - 1)}
              disabled={!matchCount}
              aria-label="Previous match"
              className="text-muted-foreground hover:text-foreground disabled:opacity-40 flex"
            >
              <ChevronUp className="w-3 h-3" strokeWidth={2.5} />
            </button>
            <button
              onClick={() => setMatchStep((s) => s + 1)}
              disabled={!matchCount}
              aria-label="Next match"
              className="text-muted-foreground hover:text-foreground disabled:opacity-40 flex"
            >
              <ChevronDown className="w-3 h-3" strokeWidth={2.5} />
            </button>
            <span className="text-[11px] text-muted-foreground whitespace-nowrap tabular-nums">
              {matchCount ? `${matchIndex + 1}/${matchCount}` : '0/0'}
              {searchIncomplete ? '…' : ''}
            </span>
          </>
        )}
      </div>

      {/* Segments, with the copy action floating over their top-right corner.
          `pointer-events-none` on the wrapper so the button does not steal a
          drag or a text selection from the column it sits above. */}
      <div className="relative flex-1 min-h-0 overflow-hidden">
        <div className="pointer-events-none absolute top-2 right-3 z-10">
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => {
                  Analytics.trackButtonClick('copy_transcript', 'workshop');
                  onCopyTranscript();
                }}
                disabled={segments.length === 0}
                aria-label="Copy transcript"
                className="pointer-events-auto flex items-center justify-center h-7 w-7 rounded-full border border-border bg-elevated text-muted-foreground shadow-md transition-colors hover:bg-muted hover:text-foreground disabled:opacity-0 disabled:pointer-events-none"
              >
                <Copy className="w-3.5 h-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>Copy transcript</TooltipContent>
          </Tooltip>
        </div>
        <VirtualizedTranscriptView
          segments={segments}
          isRecording={false}
          enableStreaming={false}
          showConfidence={true}
          disableAutoScroll={true}
          highlightQuery={query}
          activeSegmentIndex={activeSegmentIndex}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
        />
      </div>

      {/* Context for the AI summary - existing feature, kept */}
      {segments.length > 0 && (
        <div className="p-1.5 border-t border-border">
          <textarea
            placeholder="Add context for AI summary. For example people involved, meeting overview, objective etc..."
            className="w-full px-3 py-2 border border-border rounded-md text-xs focus:outline-hidden focus:ring-1 focus:ring-ring bg-card min-h-[64px] resize-y"
            value={customPrompt}
            onChange={(e) => onPromptChange(e.target.value)}
          />
        </div>
      )}
    </div>
  );
}
