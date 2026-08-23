'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Circle, Upload, Inbox, AlertCircle, SearchX, X } from 'lucide-react';
import { Skeleton } from '@/components/ui/skeleton';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { useMeetingLibrary, type MeetingListItem } from '@/hooks/useMeetingLibrary';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useConfig } from '@/contexts/ConfigContext';
import Analytics from '@/lib/analytics';
import { LibraryToolbar } from './LibraryToolbar';
import { MeetingRow } from './MeetingRow';
import { GROUP_ORDER, groupLabelFor, timeLabelFor, type GroupLabel } from './format';

interface Group {
  label: GroupLabel;
  items: Array<{ item: MeetingListItem; timeLabel: string }>;
}

function CenteredState({
  icon,
  title,
  description,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center text-center gap-2.5 py-24 px-5">
      {icon}
      <div className="text-[15px] font-semibold">{title}</div>
      <div className="text-[13px] text-muted-foreground max-w-[300px]">{description}</div>
      {action}
    </div>
  );
}

/**
 * The library: the app's home screen. Searching, filtering, sorting and bulk
 * actions over every meeting, replacing the old sidebar drawer's list.
 */
export function LibraryScreen() {
  const router = useRouter();
  const library = useMeetingLibrary();
  const { handleRecordingToggle, refetchMeetings } = useSidebar();
  const { openImportDialog } = useImportDialog();
  const { betaFeatures } = useConfig();

  const [density, setDensity] = useState<'comfortable' | 'compact'>('comfortable');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [isBulkWorking, setIsBulkWorking] = useState(false);

  const pageViewTrackedRef = useRef(false);
  useEffect(() => {
    if (pageViewTrackedRef.current) return;
    pageViewTrackedRef.current = true;
    Analytics.trackPageView('library');
  }, []);

  // Grouping happens after the server-side sort, so a non-date sort orders
  // rows within their date group - same behaviour as the mockup.
  const groups = useMemo<Group[]>(() => {
    const now = new Date();
    const buckets = new Map<GroupLabel, Group['items']>();
    for (const item of library.items) {
      const date = new Date(item.created_at);
      const label = groupLabelFor(date, now);
      const entry = { item, timeLabel: timeLabelFor(date, label, now) };
      const bucket = buckets.get(label);
      if (bucket) bucket.push(entry);
      else buckets.set(label, [entry]);
    }
    return GROUP_ORDER.filter((label) => buckets.has(label)).map((label) => ({
      label,
      items: buckets.get(label)!,
    }));
  }, [library.items]);

  const openMeeting = useCallback(
    (id: string) => {
      router.push(`/meeting-details?id=${id}`);
    },
    [router],
  );

  const toggleSelect = useCallback((id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const clearSelection = useCallback(() => setSelected(new Set()), []);

  // Infinite scroll: fetch the next page when the sentinel becomes visible.
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const loadMoreRef = useRef(library.loadMore);
  loadMoreRef.current = library.loadMore;
  const canLoadMore = library.status === 'ready' && library.hasMore && !library.isLoadingMore;
  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel || !canLoadMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) loadMoreRef.current();
      },
      { rootMargin: '300px' },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [canLoadMore]);

  const handleBulkExport = useCallback(async () => {
    const ids = [...selected];
    setIsBulkWorking(true);
    let exported = 0;
    let firstError: string | null = null;
    for (const id of ids) {
      try {
        await invoke('api_export_meeting', { meetingId: id });
        exported++;
      } catch (error) {
        if (!firstError) firstError = error instanceof Error ? error.message : String(error);
      }
    }
    setIsBulkWorking(false);
    clearSelection();
    if (exported > 0) {
      toast.success(`Exported ${exported} of ${ids.length} meetings to Obsidian`);
    }
    if (firstError) {
      toast.error(`${ids.length - exported} meetings failed to export`, {
        description: firstError,
      });
    }
  }, [selected, clearSelection]);

  const handleBulkDelete = useCallback(async () => {
    const ids = [...selected];
    setConfirmDelete(false);
    setIsBulkWorking(true);
    let deleted = 0;
    let firstError: string | null = null;
    for (const id of ids) {
      try {
        await invoke('api_delete_meeting', { meetingId: id });
        Analytics.trackMeetingDeleted(id);
        deleted++;
      } catch (error) {
        if (!firstError) firstError = error instanceof Error ? error.message : String(error);
      }
    }
    setIsBulkWorking(false);
    clearSelection();
    if (deleted > 0) {
      toast.success(deleted === 1 ? 'Meeting deleted' : `${deleted} meetings deleted`);
    }
    if (firstError) {
      toast.error(`${ids.length - deleted} meetings failed to delete`, {
        description: firstError,
      });
    }
    await library.refetch();
    await refetchMeetings();
  }, [selected, clearSelection, library, refetchMeetings]);

  const showSkeletons = library.status === 'loading' && library.items.length === 0;
  const showError = library.status === 'error';
  const showEmptyFirstRun =
    library.status === 'ready' && library.total === 0 && !library.isNarrowed;
  const showNoResults =
    library.status === 'ready' && library.total === 0 && library.isNarrowed;
  const showList = !showSkeletons && !showError && !showEmptyFirstRun && !showNoResults;

  const anySelected = selected.size > 0;

  return (
    <div className="flex flex-col h-full bg-background relative">
      {/* Header */}
      <div className="flex items-center gap-3 px-5 pt-4 pb-2.5">
        <h1 className="text-[19px] font-bold">Library</h1>
        <div className="text-xs text-muted-foreground">
          {library.status === 'ready' ? `${library.total} meetings` : ''}
        </div>
        <div className="flex-1" />
        {betaFeatures.importAndRetranscribe && (
          <button
            onClick={() => openImportDialog()}
            className="flex items-center gap-1.5 px-3 py-[7px] rounded-lg border border-border bg-card text-[13px] font-medium hover:bg-muted transition-colors"
          >
            <Upload className="w-3.5 h-3.5" />
            Import
          </button>
        )}
        <button
          onClick={handleRecordingToggle}
          className="flex items-center gap-1.5 px-3.5 py-[7px] rounded-lg bg-recording text-white text-[13px] font-semibold hover:opacity-90 transition-opacity"
        >
          <Circle className="w-3 h-3" fill="currentColor" />
          Record
        </button>
      </div>

      <LibraryToolbar
        search={library.search}
        onSearchChange={library.setSearch}
        filters={library.filters}
        onFilterChange={library.setFilter}
        onResetFilters={library.resetFilters}
        activeFilterCount={library.activeFilterCount}
        sort={library.sort}
        onToggleSort={library.toggleSort}
        density={density}
        onDensityChange={setDensity}
      />

      {/* Content */}
      <div className="flex-1 min-h-0 overflow-y-auto px-5 pb-24">
        {showSkeletons &&
          [70, 55, 80, 45, 65, 50, 75, 60].map((w, i) => (
            <div key={i} className="flex items-center gap-3 py-3 border-b border-border">
              <Skeleton className="w-4 h-4 rounded" />
              <Skeleton className="h-3 rounded" style={{ width: `${w}%` }} />
              <Skeleton className="w-16 h-2.5 rounded ml-auto" />
            </div>
          ))}

        {showError && (
          <CenteredState
            icon={<AlertCircle className="w-8 h-8 text-destructive" strokeWidth={1.5} />}
            title="Couldn't load your library"
            description="Something went wrong reading the local database."
            action={
              <button
                onClick={() => library.refetch()}
                className="mt-1.5 px-4 py-2 rounded-lg border border-border bg-card text-[13px] font-semibold hover:bg-muted"
              >
                Retry
              </button>
            }
          />
        )}

        {showEmptyFirstRun && (
          <CenteredState
            icon={<Inbox className="w-10 h-10 text-muted-foreground" strokeWidth={1.5} />}
            title="No meetings yet"
            description="Halvern will detect your next call automatically, or you can start recording manually."
            action={
              <button
                onClick={handleRecordingToggle}
                className="mt-1.5 px-4 py-2 rounded-lg bg-primary text-primary-foreground text-[13px] font-semibold hover:opacity-90"
              >
                Start recording
              </button>
            }
          />
        )}

        {showNoResults && (
          <CenteredState
            icon={<SearchX className="w-8 h-8 text-muted-foreground" strokeWidth={1.5} />}
            title="No matches"
            description="Try a different search term or clear your filters."
            action={
              <button
                onClick={() => {
                  library.setSearch('');
                  library.resetFilters();
                }}
                className="mt-1.5 px-4 py-2 rounded-lg border border-border bg-card text-[13px] font-semibold hover:bg-muted"
              >
                Clear search & filters
              </button>
            }
          />
        )}

        {showList &&
          groups.map((group) => (
            <div key={group.label}>
              <div className="sticky top-0 z-10 bg-background text-[11px] font-bold uppercase tracking-wider text-muted-foreground pt-3.5 pb-1.5 px-1">
                {group.label} · {group.items.length}
              </div>
              {group.items.map(({ item, timeLabel }) => (
                <MeetingRow
                  key={item.id}
                  item={item}
                  timeLabel={timeLabel}
                  query={library.effectiveSearch}
                  selected={selected.has(item.id)}
                  anySelected={anySelected}
                  compact={density === 'compact'}
                  onOpen={openMeeting}
                  onToggleSelect={toggleSelect}
                />
              ))}
            </div>
          ))}

        {showList && <div ref={sentinelRef} className="h-px" />}
        {library.isLoadingMore && (
          <div className="py-3 text-center text-xs text-muted-foreground">Loading more…</div>
        )}
      </div>

      {/* Bulk actions bar */}
      {anySelected && (
        <div className="absolute bottom-5 left-1/2 -translate-x-1/2 flex items-center gap-2.5 bg-card border border-border rounded-xl shadow-xl pl-4 pr-2.5 py-2">
          <span className="text-[13px] font-semibold whitespace-nowrap">
            {selected.size} selected
          </span>
          <span className="w-px h-5 bg-border mx-1" />
          <button
            onClick={handleBulkExport}
            disabled={isBulkWorking}
            className="px-3 py-1.5 rounded-lg border border-border bg-card text-xs font-medium hover:bg-muted disabled:opacity-50 whitespace-nowrap"
          >
            Export
          </button>
          <button
            onClick={() => setConfirmDelete(true)}
            disabled={isBulkWorking}
            className="px-3 py-1.5 rounded-lg border border-destructive text-destructive text-xs font-medium hover:bg-destructive/10 disabled:opacity-50"
          >
            Delete
          </button>
          <button
            onClick={clearSelection}
            aria-label="Clear selection"
            className="p-1 text-muted-foreground hover:text-foreground"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}

      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete {selected.size === 1 ? 'this meeting' : `${selected.size} meetings`}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              The transcript, summary and recording files will be removed. This cannot be
              undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleBulkDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
