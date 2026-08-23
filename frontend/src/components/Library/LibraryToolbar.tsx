'use client';

import { Search, X, ListFilter, ArrowUpDown } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import type { LibraryFilters, LibrarySort, SortKey } from '@/hooks/useMeetingLibrary';

const DATE_RANGE_OPTIONS: Array<[LibraryFilters['dateRange'], string]> = [
  ['all', 'All time'],
  ['today', 'Today'],
  ['week', 'This week'],
  ['month', 'This month'],
];
const SUMMARY_OPTIONS: Array<[LibraryFilters['summary'], string]> = [
  ['all', 'Any'],
  ['with', 'Has summary'],
  ['without', 'No summary'],
];
const SOURCE_OPTIONS: Array<[LibraryFilters['source'], string]> = [
  ['all', 'Any'],
  ['auto', 'Detected'],
  ['manual', 'Manual'],
  ['imported', 'Imported'],
];
const LANGUAGE_OPTIONS: Array<[LibraryFilters['language'], string]> = [
  ['all', 'Any'],
  ['en', 'English'],
  ['pl', 'Polish'],
];
const SORT_OPTIONS: Array<[SortKey, string]> = [
  ['date', 'Date'],
  ['title', 'Title'],
  ['duration', 'Duration'],
  ['segments', 'Segments'],
];

function Chip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`px-2.5 py-1 rounded-lg text-xs font-medium border transition-colors ${
        active
          ? 'border-primary bg-accent text-accent-foreground'
          : 'border-border bg-card text-foreground hover:bg-muted'
      }`}
    >
      {label}
    </button>
  );
}

function FilterGroup<T extends string>({
  title,
  options,
  value,
  onChange,
}: {
  title: string;
  options: Array<[T, string]>;
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div>
      <div className="text-[11px] font-semibold text-muted-foreground mb-1.5">{title}</div>
      <div className="flex gap-1.5 flex-wrap">
        {options.map(([v, label]) => (
          <Chip key={v} label={label} active={value === v} onClick={() => onChange(v)} />
        ))}
      </div>
    </div>
  );
}

interface LibraryToolbarProps {
  search: string;
  onSearchChange: (value: string) => void;
  filters: LibraryFilters;
  onFilterChange: <K extends keyof LibraryFilters>(key: K, value: LibraryFilters[K]) => void;
  onResetFilters: () => void;
  activeFilterCount: number;
  sort: LibrarySort;
  onToggleSort: (key: SortKey) => void;
  density: 'comfortable' | 'compact';
  onDensityChange: (density: 'comfortable' | 'compact') => void;
}

export function LibraryToolbar({
  search,
  onSearchChange,
  filters,
  onFilterChange,
  onResetFilters,
  activeFilterCount,
  sort,
  onToggleSort,
  density,
  onDensityChange,
}: LibraryToolbarProps) {
  const sortLabel = SORT_OPTIONS.find(([k]) => k === sort.key)?.[1] ?? 'Date';

  const toolbarButton = (active: boolean) =>
    `flex items-center gap-1.5 px-3 h-[34px] rounded-lg border text-xs font-medium whitespace-nowrap transition-colors ${
      active
        ? 'border-primary bg-accent text-accent-foreground'
        : 'border-border bg-card text-foreground hover:bg-muted'
    }`;

  return (
    <div className="flex items-center gap-2 px-5 pb-3">
      <div className="flex-1 flex items-center gap-2 bg-muted border border-border rounded-lg px-2.5 h-[34px]">
        <Search className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
        <input
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder="Search titles and transcripts…"
          className="flex-1 min-w-0 bg-transparent outline-none text-[13px] text-foreground placeholder:text-muted-foreground"
        />
        {search && (
          <button
            onClick={() => onSearchChange('')}
            aria-label="Clear search"
            className="text-muted-foreground hover:text-foreground p-0.5"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>

      <Popover>
        <PopoverTrigger asChild>
          <button className={toolbarButton(activeFilterCount > 0)}>
            <ListFilter className="w-3.5 h-3.5" />
            Filter{activeFilterCount > 0 ? ` (${activeFilterCount})` : ''}
          </button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-80 flex flex-col gap-3 p-3.5">
          <FilterGroup
            title="Date range"
            options={DATE_RANGE_OPTIONS}
            value={filters.dateRange}
            onChange={(v) => onFilterChange('dateRange', v)}
          />
          <FilterGroup
            title="Summary"
            options={SUMMARY_OPTIONS}
            value={filters.summary}
            onChange={(v) => onFilterChange('summary', v)}
          />
          <FilterGroup
            title="Source"
            options={SOURCE_OPTIONS}
            value={filters.source}
            onChange={(v) => onFilterChange('source', v)}
          />
          <FilterGroup
            title="Language"
            options={LANGUAGE_OPTIONS}
            value={filters.language}
            onChange={(v) => onFilterChange('language', v)}
          />
          <button
            onClick={onResetFilters}
            className="self-start text-xs text-primary hover:underline"
          >
            Reset filters
          </button>
        </PopoverContent>
      </Popover>

      <Popover>
        <PopoverTrigger asChild>
          <button className={toolbarButton(false)}>
            <ArrowUpDown className="w-3.5 h-3.5" />
            Sort: {sortLabel}
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-48 p-1.5 flex flex-col">
          {SORT_OPTIONS.map(([key, label]) => {
            const active = sort.key === key;
            return (
              <button
                key={key}
                onClick={() => onToggleSort(key)}
                className={`flex items-center justify-between px-2.5 py-2 rounded-md text-xs text-left ${
                  active ? 'bg-muted font-semibold' : 'hover:bg-muted'
                }`}
              >
                <span>{label}</span>
                {active && (
                  <span className="text-[11px] text-muted-foreground">
                    {sort.dir === 'asc' ? '↑' : '↓'}
                  </span>
                )}
              </button>
            );
          })}
        </PopoverContent>
      </Popover>

      <div
        className="flex gap-0.5 bg-muted border border-border rounded-lg p-0.5"
        role="group"
        aria-label="Row density"
      >
        {(['comfortable', 'compact'] as const).map((d) => (
          <button
            key={d}
            onClick={() => onDensityChange(d)}
            aria-pressed={density === d}
            className={`px-2 py-1 rounded-md text-[11px] transition-colors ${
              density === d
                ? 'bg-card text-foreground font-semibold shadow-sm'
                : 'text-muted-foreground font-medium hover:text-foreground'
            }`}
          >
            {d === 'comfortable' ? 'Comfortable' : 'Compact'}
          </button>
        ))}
      </div>
    </div>
  );
}
