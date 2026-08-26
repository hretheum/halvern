'use client';

import { useCallback, useRef, useReducer, startTransition, useEffect, useState, memo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useAutoScroll } from "@/hooks/useAutoScroll";
import { useTranscriptStreaming } from "@/hooks/useTranscriptStreaming";
import { ConfidenceIndicator } from "./ConfidenceIndicator";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";
import { RecordingStatusBar } from "./RecordingStatusBar";
import { motion, AnimatePresence } from "framer-motion";
import { TranscriptSegmentData } from "@/types";
import { speakerLabel } from "@/lib/speaker-labels";
import { LAYOUT } from "@/lib/layout";
import { useAudioInputHealth } from "@/hooks/useAudioInputHealth";

/** A device name when there is one, and something honest when there is not. */
function describeSource(device: string | null): string {
    return device ? `“${device}”` : 'the microphone';
}

export interface VirtualizedTranscriptViewProps {
    /** Transcript segments to display */
    segments: TranscriptSegmentData[];
    /** Whether recording is in progress */
    isRecording?: boolean;
    /** Whether recording is paused */
    isPaused?: boolean;
    /** Whether processing/finalizing transcription */
    isProcessing?: boolean;
    /** Whether stopping */
    isStopping?: boolean;
    /** Enable streaming effect for latest segment */
    enableStreaming?: boolean;
    /** Show confidence indicators */
    showConfidence?: boolean;
    /** Completely disable auto-scroll behavior (for meeting details page) */
    disableAutoScroll?: boolean;

    /** Term to visually mark inside segment text (workshop search). */
    highlightQuery?: string;
    /** Index of the segment the current search match sits in; the view
     *  scrolls it into the middle of the viewport when it changes. */
    activeSegmentIndex?: number | null;

    // Pagination props (infinite scroll)
    hasMore?: boolean;
    isLoadingMore?: boolean;
    totalCount?: number;
    loadedCount?: number;
    onLoadMore?: () => void;
}

// Threshold for enabling virtualization (below this, use simple rendering)
const VIRTUALIZATION_THRESHOLD = 10;

// Helper function to format seconds as recording-relative time [MM:SS]
function formatRecordingTime(seconds: number | undefined): string {
    if (seconds === undefined) return '[--:--]';

    const totalSeconds = Math.floor(seconds);
    const minutes = Math.floor(totalSeconds / 60);
    const secs = totalSeconds % 60;

    return `[${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

// Helper function to remove filler words and repetitions. Exported so the
// workshop search can match against exactly the text the reader sees.
export function cleanStopWords(text: string): string {
    const stopWords = ['uh', 'um', 'er', 'ah', 'hmm', 'hm', 'eh', 'oh'];

    let cleanedText = text;
    stopWords.forEach(word => {
        const pattern = new RegExp(`\\b${word}\\b[,\\s]*`, 'gi');
        cleanedText = cleanedText.replace(pattern, ' ');
    });

    return cleanedText.replace(/\s+/g, ' ').trim();
}

// Memoized transcript segment component
function HighlightedText({ text, query }: { text: string; query: string }) {
    const q = query.trim().toLowerCase();
    if (!q) return <>{text}</>;
    const lower = text.toLowerCase();
    const parts: React.ReactNode[] = [];
    let i = 0;
    let key = 0;
    while (i < text.length) {
        const idx = lower.indexOf(q, i);
        if (idx === -1) {
            parts.push(text.slice(i));
            break;
        }
        if (idx > i) parts.push(text.slice(i, idx));
        parts.push(
            <mark key={key++} className="bg-accent text-accent-foreground font-semibold rounded-[3px]">
                {text.slice(idx, idx + q.length)}
            </mark>
        );
        i = idx + q.length;
    }
    return <>{parts}</>;
}

const TranscriptSegment = memo(function TranscriptSegment({
    id,
    timestamp,
    text,
    confidence,
    speaker,
    isStreaming,
    showConfidence,
    highlightQuery = '',
    isActiveMatch = false,
}: {
    id: string;
    timestamp: number;
    text: string;
    confidence?: number;
    speaker?: string | null;
    isStreaming: boolean;
    showConfidence: boolean;
    highlightQuery?: string;
    isActiveMatch?: boolean;
}) {
    const displayText = cleanStopWords(text) || (text.trim() === '' ? '[Silence]' : text);
    const displaySpeaker = speakerLabel(speaker);

    return (
        <div id={`segment-${id}`} className={`mb-3 ${isActiveMatch ? 'bg-accent rounded-md -mx-1.5 px-1.5 py-1' : ''}`}>
            <div className={`flex items-start ${LAYOUT.GAP}`}>
                <Tooltip>
                    {/* asChild, so the span itself is the flex child. Wrapped,
                        TooltipTrigger renders its own element and the span
                        becomes inline inside it — where `width` is ignored and
                        the column collapses to the text (34px instead of 48),
                        dragging every paragraph 14px left of the rule. */}
                    <TooltipTrigger asChild>
                        <span className={`block text-xs text-muted-foreground mt-1 shrink-0 tabular-nums ${LAYOUT.GUTTER}`}>
                            {formatRecordingTime(timestamp)}
                        </span>
                    </TooltipTrigger>
                    <TooltipContent>
                        {confidence !== undefined && showConfidence && (
                            <ConfidenceIndicator confidence={confidence} showIndicator={showConfidence} />
                        )}
                    </TooltipContent>
                </Tooltip>
                <div className="flex-1">
                    {displaySpeaker && (
                        <span className="text-xs font-medium text-muted-foreground block mb-0.5">{displaySpeaker}</span>
                    )}
                    {isStreaming ? (
                        <div className="bg-muted border border-border rounded-lg px-3 py-2">
                            <p className="text-base text-foreground leading-relaxed">{displayText}</p>
                        </div>
                    ) : (
                        <p className="text-base text-foreground leading-relaxed">
                            <HighlightedText text={displayText} query={highlightQuery} />
                        </p>
                    )}
                </div>
            </div>
        </div>
    );
});

export const VirtualizedTranscriptView: React.FC<VirtualizedTranscriptViewProps> = ({
    segments,
    isRecording = false,
    isPaused = false,
    isProcessing = false,
    isStopping = false,
    enableStreaming = false,
    showConfidence = true,
    disableAutoScroll = false,
    highlightQuery = '',
    activeSegmentIndex = null,
    hasMore = false,
    isLoadingMore = false,
    totalCount = 0,
    loadedCount = 0,
    onLoadMore,
}) => {
    // Create scroll ref first - shared between virtualizer and auto-scroll hook
    const scrollRef = useRef<HTMLDivElement>(null);
    // Ref for infinite scroll trigger element
    const loadMoreTriggerRef = useRef<HTMLDivElement>(null);

    // Force re-render without flushSync (avoids React warning)
    const [, rerender] = useReducer((x: number) => x + 1, 0);

    // Setup virtualizer for efficient rendering of large lists
    const virtualizer = useVirtualizer({
        count: segments.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => 60, // Estimated height per segment
        overscan: 10, // Render extra items above/below viewport
        onChange: () => {
            startTransition(() => {
                rerender();
            });
        },
    });

    // Is the recording actually hearing anything? Only asked while running and
    // not paused; the hook is inert otherwise.
    const inputHealth = useAudioInputHealth(isRecording && !isPaused);

    // Custom hook for auto-scrolling (supports both virtualized and non-virtualized)
    useAutoScroll({
        scrollRef,
        segments,
        isRecording,
        isPaused,
        virtualizer,
        virtualizationThreshold: VIRTUALIZATION_THRESHOLD,
        disableAutoScroll,
    });

    // Streaming text effect hook (typewriter animation for new transcripts)
    const { streamingSegmentId, getDisplayText } = useTranscriptStreaming(
        segments,
        isRecording,
        enableStreaming
    );

    // Infinite scroll: IntersectionObserver to trigger loading more
    useEffect(() => {
        if (!onLoadMore || !hasMore || isLoadingMore || isRecording || segments.length === 0) {
            return;
        }

        const triggerElement = loadMoreTriggerRef.current;
        if (!triggerElement) return;

        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting && hasMore && !isLoadingMore) {
                    onLoadMore();
                }
            },
            {
                root: null,
                rootMargin: '100px',
                threshold: 0,
            }
        );

        observer.observe(triggerElement);

        return () => observer.disconnect();
    }, [hasMore, isLoadingMore, onLoadMore, isRecording, segments.length]);

    // Scroll-based fallback for fast scrolling
    useEffect(() => {
        if (!onLoadMore || !hasMore || isLoadingMore || isRecording) return;

        const scrollElement = scrollRef.current;
        if (!scrollElement) return;

        let ticking = false;

        const handleScroll = () => {
            if (ticking || isLoadingMore || !hasMore) return;

            ticking = true;
            requestAnimationFrame(() => {
                const { scrollTop, scrollHeight, clientHeight } = scrollElement;
                const scrollBottom = scrollHeight - scrollTop - clientHeight;

                // Trigger load when within 200px of bottom
                if (scrollBottom < 200 && hasMore && !isLoadingMore) {
                    onLoadMore();
                }
                ticking = false;
            });
        };

        scrollElement.addEventListener('scroll', handleScroll, { passive: true });
        return () => scrollElement.removeEventListener('scroll', handleScroll);
    }, [onLoadMore, hasMore, isLoadingMore, isRecording]);

    // Use simple rendering for small lists, virtualization for large lists
    const useVirtualization = segments.length >= VIRTUALIZATION_THRESHOLD;

    // Bring the segment holding the current search match into view.
    useEffect(() => {
        if (activeSegmentIndex === null || activeSegmentIndex === undefined) return;
        if (activeSegmentIndex < 0 || activeSegmentIndex >= segments.length) return;
        if (useVirtualization) {
            virtualizer.scrollToIndex(activeSegmentIndex, { align: 'center' });
        } else {
            const el = document.getElementById(`segment-${segments[activeSegmentIndex].id}`);
            el?.scrollIntoView({ block: 'center' });
        }
        // segments.length, not segments: a new page appending must not yank
        // the view back to the match the user already moved away from.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [activeSegmentIndex, useVirtualization, segments.length]);

    return (
        <div ref={scrollRef} className={`flex flex-col h-full overflow-y-auto ${LAYOUT.INSET} py-2`}>
            {/* Recording Status Bar - Sticky at top, always visible when recording */}
            <AnimatePresence>
                {isRecording && (
                    <div className="sticky top-0 z-10 bg-card pb-2">
                        <RecordingStatusBar isPaused={isPaused} />
                    </div>
                )}
            </AnimatePresence>

            {/* Content - add padding when recording to prevent overlap */}
            <div className={isRecording ? 'pt-2' : ''}>
            {segments.length === 0 ? (
                // Empty state
                <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    className="text-center text-muted-foreground mt-8"
                >
                    {isRecording ? (
                        <>
                            <div className="flex items-center justify-center mb-3">
                                <div className={`w-3 h-3 rounded-full ${
                                    isPaused
                                        ? 'bg-orange-500'
                                        : inputHealth.state === 'ok'
                                            ? 'bg-blue-500 animate-pulse'
                                            : 'bg-warning-text'
                                }`}></div>
                            </div>
                            <p className={`text-sm ${inputHealth.state === 'ok' ? 'text-muted-foreground' : 'text-warning-text'}`}>
                                {isPaused
                                    ? 'Recording paused'
                                    : inputHealth.state === 'waiting'
                                        ? `No audio from ${describeSource(inputHealth.device)} yet`
                                        : inputHealth.state === 'silent'
                                            ? `${describeSource(inputHealth.device)} is delivering silence`
                                            : 'Listening for speech...'}
                            </p>
                            <p className="text-sm mt-1 text-muted-foreground">
                                {isPaused
                                    ? 'Click resume to continue recording'
                                    : inputHealth.state === 'waiting'
                                        ? `The stream opened ${inputHealth.seconds}s ago and has not delivered a single sample. The recording is running and will keep whatever arrives.`
                                        : inputHealth.state === 'silent'
                                            ? `Sound is arriving and every sample is silent, for ${inputHealth.seconds}s. Check that the microphone is not muted and that it is the one you meant.`
                                            : 'Speak to see live transcription'}
                            </p>
                        </>
                    ) : (
                        <>
                            <p className="text-lg font-semibold">Welcome to Halvern!</p>
                            <p className="text-xs mt-1">Start recording to see live transcription</p>
                        </>
                    )}
                </motion.div>
            ) : useVirtualization ? (
                // Virtualized rendering for large lists
                <>
                    <div
                        style={{
                            height: virtualizer.getTotalSize(),
                            width: "100%",
                            position: "relative",
                        }}
                    >
                        {virtualizer.getVirtualItems().map((virtualRow) => {
                            const segment = segments[virtualRow.index];
                            const isStreaming = streamingSegmentId === segment.id;

                            return (
                                <div
                                    key={segment.id}
                                    data-index={virtualRow.index}
                                    ref={virtualizer.measureElement}
                                    style={{
                                        position: "absolute",
                                        top: 0,
                                        left: 0,
                                        width: "100%",
                                        transform: `translateY(${virtualRow.start}px)`,
                                    }}
                                >
                                    <TranscriptSegment
                                        id={segment.id}
                                        timestamp={segment.timestamp}
                                        text={getDisplayText(segment)}
                                        confidence={segment.confidence}
                                        speaker={segment.speaker}
                                        isStreaming={isStreaming}
                                        showConfidence={showConfidence}
                                        highlightQuery={highlightQuery}
                                        isActiveMatch={virtualRow.index === activeSegmentIndex}
                                    />
                                </div>
                            );
                        })}
                    </div>

                    {/* Infinite scroll trigger and loading indicator */}
                    {(hasMore || isLoadingMore) && !isRecording && segments.length > 0 && (
                        <div ref={loadMoreTriggerRef} className="flex justify-center items-center py-4 mt-2">
                            {isLoadingMore ? (
                                <div className="flex items-center gap-2 text-muted-foreground">
                                    <div className="w-4 h-4 border-2 border-border border-t-gray-600 rounded-full animate-spin" />
                                    <span className="text-sm">Loading more...</span>
                                </div>
                            ) : hasMore && totalCount > 0 ? (
                                <span className="text-sm text-muted-foreground">
                                    Showing {loadedCount} of {totalCount} segments
                                </span>
                            ) : null}
                        </div>
                    )}

                    {/* Listening indicator when recording */}
                    {!isStopping && isRecording && !isPaused && !isProcessing && segments.length > 0 && (
                        <motion.div
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            className="flex items-center gap-2 mt-4 text-muted-foreground"
                        >
                            <div className="w-2 h-2 bg-blue-500 rounded-full animate-pulse"></div>
                            <span className="text-sm">Listening...</span>
                        </motion.div>
                    )}
                </>
            ) : (
                // Simple rendering for small lists (better animations)
                <>
                    <div className="space-y-1">
                        {segments.map((segment, index) => {
                            const isStreaming = streamingSegmentId === segment.id;

                            return (
                                <motion.div
                                    key={segment.id}
                                    initial={{ opacity: 0, y: 5 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    transition={{ duration: 0.15 }}
                                >
                                    <TranscriptSegment
                                        id={segment.id}
                                        timestamp={segment.timestamp}
                                        text={getDisplayText(segment)}
                                        confidence={segment.confidence}
                                        speaker={segment.speaker}
                                        isStreaming={isStreaming}
                                        showConfidence={showConfidence}
                                        highlightQuery={highlightQuery}
                                        isActiveMatch={index === activeSegmentIndex}
                                    />
                                </motion.div>
                            );
                        })}
                    </div>

                    {/* Infinite scroll trigger (for small lists that grow) */}
                    {(hasMore || isLoadingMore) && !isRecording && segments.length > 0 && (
                        <div ref={loadMoreTriggerRef} className="flex justify-center items-center py-4 mt-2">
                            {isLoadingMore ? (
                                <div className="flex items-center gap-2 text-muted-foreground">
                                    <div className="w-4 h-4 border-2 border-border border-t-gray-600 rounded-full animate-spin" />
                                    <span className="text-sm">Loading more...</span>
                                </div>
                            ) : hasMore && totalCount > 0 ? (
                                <span className="text-sm text-muted-foreground">
                                    Showing {loadedCount} of {totalCount} segments
                                </span>
                            ) : null}
                        </div>
                    )}

                    {/* Listening indicator when recording */}
                    {!isStopping && isRecording && !isPaused && !isProcessing && segments.length > 0 && (
                        <motion.div
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            className="flex items-center gap-2 mt-4 text-muted-foreground"
                        >
                            <div className="w-2 h-2 bg-blue-500 rounded-full animate-pulse"></div>
                            <span className="text-sm">Listening...</span>
                        </motion.div>
                    )}
                </>
            )}
            </div>
        </div>
    );
};
