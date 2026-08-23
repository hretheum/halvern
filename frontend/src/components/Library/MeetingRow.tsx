'use client';

import { memo } from 'react';
import { Check } from 'lucide-react';
import { Checkbox } from '@/components/ui/checkbox';
import type { MeetingListItem } from '@/hooks/useMeetingLibrary';
import {
  formatDuration,
  languageLabelFor,
  sourceLabelFor,
  splitHighlight,
  splitSnippetMarkers,
  type TextPart,
} from './format';

function Parts({ parts }: { parts: TextPart[] }) {
  return (
    <>
      {parts.map((p, i) =>
        p.match ? (
          <mark key={i} className="bg-accent text-accent-foreground font-bold rounded-[3px]">
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}

interface MeetingRowProps {
  item: MeetingListItem;
  timeLabel: string;
  query: string;
  selected: boolean;
  anySelected: boolean;
  compact: boolean;
  onOpen: (id: string) => void;
  onToggleSelect: (id: string) => void;
}

export const MeetingRow = memo(function MeetingRow({
  item,
  timeLabel,
  query,
  selected,
  anySelected,
  compact,
  onOpen,
  onToggleSelect,
}: MeetingRowProps) {
  return (
    <div
      onClick={() => onOpen(item.id)}
      // Asymmetric on purpose. The left 4px is bleed so the hover and
      // selected backgrounds round past the checkbox; the right needs real
      // clearance because the last cell is right-aligned text and the row's
      // own border-b runs the full width, so at 4px the language label ends
      // flush with the separator and reads as falling off the row.
      className={`grid grid-cols-[22px_1fr_88px_64px_26px_110px_36px] items-center gap-3 border-b border-border cursor-pointer rounded-md pl-1 pr-3 ${
        compact ? 'py-1' : 'py-2.5'
      } ${selected ? 'bg-accent' : 'hover:bg-muted/60'}`}
    >
      <span
        onClick={(e) => e.stopPropagation()}
        className={`flex items-center transition-opacity ${anySelected ? 'opacity-100' : 'opacity-55'}`}
      >
        <Checkbox
          checked={selected}
          onCheckedChange={() => onToggleSelect(item.id)}
          aria-label={`Select ${item.title}`}
        />
      </span>

      <span className="min-w-0">
        <span
          className={`block font-medium whitespace-nowrap overflow-hidden text-ellipsis ${
            compact ? 'text-[12.5px]' : 'text-[13.5px]'
          }`}
        >
          <Parts parts={splitHighlight(item.title, query)} />
        </span>
        {item.snippet && (
          <span className="block text-[11px] text-muted-foreground whitespace-nowrap overflow-hidden text-ellipsis mt-px">
            <Parts parts={splitSnippetMarkers(item.snippet)} />
          </span>
        )}
      </span>

      <span className="text-xs text-muted-foreground whitespace-nowrap">{timeLabel}</span>
      <span className="text-xs text-muted-foreground whitespace-nowrap">
        {formatDuration(item.duration_seconds)}
      </span>

      <span className="flex justify-center" aria-label={item.has_summary ? 'Has summary' : 'No summary'}>
        {item.has_summary ? (
          <Check className="w-3.5 h-3.5 text-primary" strokeWidth={2.5} />
        ) : (
          <span className="w-[5px] h-[5px] rounded-full bg-border" />
        )}
      </span>

      <span className="text-[11px] text-muted-foreground whitespace-nowrap overflow-hidden text-ellipsis">
        {sourceLabelFor(item)}
      </span>
      <span className="text-[11px] text-muted-foreground text-right">{languageLabelFor(item)}</span>
    </div>
  );
});
