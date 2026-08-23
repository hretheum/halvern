'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

/** One row of `api_query_meetings`, exactly as the backend serializes it. */
export interface MeetingListItem {
  id: string;
  title: string;
  created_at: string;
  duration_seconds?: number;
  source?: 'manual' | 'auto' | 'imported' | string;
  app_name?: string;
  language?: string;
  folder_path?: string;
  segment_count: number;
  has_summary: boolean;
  /** Excerpt around the search hit, with [brackets] marking matched terms. */
  snippet?: string;
}

export interface LibraryFilters {
  dateRange: 'all' | 'today' | 'week' | 'month';
  summary: 'all' | 'with' | 'without';
  source: 'all' | 'auto' | 'manual' | 'imported';
  language: 'all' | 'en' | 'pl';
}

export const DEFAULT_FILTERS: LibraryFilters = {
  dateRange: 'all',
  summary: 'all',
  source: 'all',
  language: 'all',
};

export type SortKey = 'date' | 'title' | 'duration' | 'segments';
export interface LibrarySort {
  key: SortKey;
  dir: 'asc' | 'desc';
}

const PAGE_SIZE = 100;
const SEARCH_DEBOUNCE_MS = 250;

/**
 * The database stores created_at as RFC3339 with a `+00:00` offset and the
 * backend compares it as text, so filter boundaries must use the same shape
 * for the lexicographic comparison to be a chronological one.
 */
function toDbTimestamp(date: Date): string {
  return date.toISOString().replace('Z', '+00:00');
}

function dateFromFor(range: LibraryFilters['dateRange']): string | undefined {
  const now = new Date();
  switch (range) {
    case 'today':
      return toDbTimestamp(new Date(now.getFullYear(), now.getMonth(), now.getDate()));
    case 'week': {
      const from = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      from.setDate(from.getDate() - 6);
      return toDbTimestamp(from);
    }
    case 'month':
      return toDbTimestamp(new Date(now.getFullYear(), now.getMonth(), 1));
    default:
      return undefined;
  }
}

interface QueryResult {
  items: MeetingListItem[];
  total: number;
}

async function queryMeetings(
  search: string,
  filters: LibraryFilters,
  sort: LibrarySort,
  offset: number,
): Promise<QueryResult> {
  return await invoke<QueryResult>('api_query_meetings', {
    search: search.trim() || undefined,
    dateFrom: dateFromFor(filters.dateRange),
    dateTo: undefined,
    hasSummary:
      filters.summary === 'all' ? undefined : filters.summary === 'with',
    sources: filters.source === 'all' ? undefined : [filters.source],
    language: filters.language === 'all' ? undefined : filters.language,
    sort: sort.key,
    descending: sort.dir === 'desc',
    limit: PAGE_SIZE,
    offset,
  });
}

export type LibraryStatus = 'loading' | 'error' | 'ready';

/**
 * Data source for the library screen: debounced search, composable filters,
 * sorting and incremental page loading, all backed by `api_query_meetings`.
 * Every response is checked against a request counter so a slow page can
 * never overwrite the results of a newer query.
 */
export function useMeetingLibrary() {
  const [search, setSearch] = useState('');
  const [filters, setFilters] = useState<LibraryFilters>(DEFAULT_FILTERS);
  const [sort, setSort] = useState<LibrarySort>({ key: 'date', dir: 'desc' });

  const [status, setStatus] = useState<LibraryStatus>('loading');
  const [items, setItems] = useState<MeetingListItem[]>([]);
  const [total, setTotal] = useState(0);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  // Debounced copy of `search`; queries key off this one.
  const [effectiveSearch, setEffectiveSearch] = useState('');

  const requestIdRef = useRef(0);
  const itemCountRef = useRef(0);
  itemCountRef.current = items.length;

  useEffect(() => {
    const handle = setTimeout(() => setEffectiveSearch(search), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(handle);
  }, [search]);

  const runQuery = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setStatus('loading');
    try {
      const result = await queryMeetings(effectiveSearch, filters, sort, 0);
      if (requestId !== requestIdRef.current) return;
      setItems(result.items);
      setTotal(result.total);
      setStatus('ready');
    } catch (error) {
      if (requestId !== requestIdRef.current) return;
      console.error('Failed to query meetings:', error);
      setStatus('error');
    }
  }, [effectiveSearch, filters, sort]);

  useEffect(() => {
    runQuery();
  }, [runQuery]);

  const loadMore = useCallback(async () => {
    const requestId = requestIdRef.current;
    setIsLoadingMore(true);
    try {
      const result = await queryMeetings(
        effectiveSearch,
        filters,
        sort,
        itemCountRef.current,
      );
      if (requestId !== requestIdRef.current) return;
      setItems((prev) => [...prev, ...result.items]);
      setTotal(result.total);
    } catch (error) {
      console.error('Failed to load more meetings:', error);
    } finally {
      setIsLoadingMore(false);
    }
  }, [effectiveSearch, filters, sort]);

  const setFilter = useCallback(
    <K extends keyof LibraryFilters>(key: K, value: LibraryFilters[K]) => {
      setFilters((prev) => ({ ...prev, [key]: value }));
    },
    [],
  );

  const resetFilters = useCallback(() => setFilters(DEFAULT_FILTERS), []);

  const toggleSort = useCallback((key: SortKey) => {
    setSort((prev) =>
      prev.key === key
        ? { key, dir: prev.dir === 'desc' ? 'asc' : 'desc' }
        : { key, dir: 'desc' },
    );
  }, []);

  const activeFilterCount = Object.entries(filters).filter(
    ([, value]) => value !== 'all',
  ).length;

  // "No results" only makes sense when the user narrowed something down;
  // an untouched query returning nothing means a genuinely empty library.
  const isNarrowed = effectiveSearch.trim() !== '' || activeFilterCount > 0;

  return {
    search,
    setSearch,
    effectiveSearch,
    filters,
    setFilter,
    resetFilters,
    sort,
    toggleSort,
    status,
    items,
    total,
    hasMore: items.length < total,
    isLoadingMore,
    loadMore,
    refetch: runQuery,
    activeFilterCount,
    isNarrowed,
  };
}
