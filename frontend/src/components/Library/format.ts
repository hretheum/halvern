import type { MeetingListItem } from '@/hooks/useMeetingLibrary';

export const GROUP_ORDER = ['Today', 'Yesterday', 'This week', 'This month', 'Older'] as const;
export type GroupLabel = (typeof GROUP_ORDER)[number];

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function groupLabelFor(date: Date, now: Date): GroupLabel {
  const dayDiff = Math.floor(
    (startOfDay(now).getTime() - startOfDay(date).getTime()) / 86_400_000,
  );
  if (dayDiff <= 0) return 'Today';
  if (dayDiff === 1) return 'Yesterday';
  if (dayDiff < 7) return 'This week';
  if (date.getMonth() === now.getMonth() && date.getFullYear() === now.getFullYear()) {
    return 'This month';
  }
  return 'Older';
}

/**
 * Today and yesterday read best as a clock time; anything older needs the
 * date - the mockup showed a bare time for every row, which tells you
 * nothing about a meeting from March.
 */
export function timeLabelFor(date: Date, group: GroupLabel, now: Date): string {
  if (group === 'Today' || group === 'Yesterday') {
    return date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  }
  const sameYear = date.getFullYear() === now.getFullYear();
  return date.toLocaleDateString([], {
    month: 'short',
    day: 'numeric',
    ...(sameYear ? {} : { year: 'numeric' }),
  });
}

export function formatDuration(seconds: number | undefined): string {
  if (seconds === undefined || !Number.isFinite(seconds)) return '—';
  const minutes = Math.max(1, Math.round(seconds / 60));
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `${h}h ${m}m` : `${h}h`;
}

/** Honest labels: an old meeting without a recorded source shows a dash. */
export function sourceLabelFor(item: MeetingListItem): string {
  switch (item.source) {
    case 'auto':
      return item.app_name || 'Detected';
    case 'manual':
      return 'Manual';
    case 'imported':
      return 'Imported';
    default:
      return '—';
  }
}

export function languageLabelFor(item: MeetingListItem): string {
  return item.language ? item.language.toUpperCase() : '—';
}

export interface TextPart {
  text: string;
  match: boolean;
}

/** Split text into parts, marking case-insensitive occurrences of `query`. */
export function splitHighlight(text: string, query: string): TextPart[] {
  const q = query.trim().toLowerCase();
  if (!q) return [{ text, match: false }];
  const lower = text.toLowerCase();
  const parts: TextPart[] = [];
  let i = 0;
  while (i < text.length) {
    const idx = lower.indexOf(q, i);
    if (idx === -1) {
      parts.push({ text: text.slice(i), match: false });
      break;
    }
    if (idx > i) parts.push({ text: text.slice(i, idx), match: false });
    parts.push({ text: text.slice(idx, idx + q.length), match: true });
    i = idx + q.length;
  }
  return parts;
}

/**
 * FTS snippets arrive with `[` `]` around matched terms (see the snippet()
 * call in the meetings repository). Turn them into the same parts shape the
 * title highlighter produces.
 */
export function splitSnippetMarkers(snippet: string): TextPart[] {
  const parts: TextPart[] = [];
  const re = /\[([^\]]*)\]/g;
  let last = 0;
  for (const m of snippet.matchAll(re)) {
    if (m.index! > last) parts.push({ text: snippet.slice(last, m.index), match: false });
    parts.push({ text: m[1], match: true });
    last = m.index! + m[0].length;
  }
  if (last < snippet.length) parts.push({ text: snippet.slice(last), match: false });
  return parts;
}
