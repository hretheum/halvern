'use client';

import { useCallback, useState } from 'react';
import { useRouter } from 'next/navigation';
import { LAYOUT } from '@/lib/layout';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import {
  ChevronLeft,
  FolderOpen,
  FileDown,
  RefreshCw,
  Trash2,
  PanelLeft,
} from 'lucide-react';
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
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { exportMeeting } from '@/services/exportService';
import Analytics from '@/lib/analytics';

export interface WorkshopMeta {
  dateTimeLabel: string;
  durationLabel: string | null;
  sourceLabel: string | null;
  languageLabel: string | null;
  speakersLabel: string | null;
}

interface WorkshopHeaderProps {
  meetingId: string;
  title: string;
  onTitleCommit: (title: string) => void;
  meta: WorkshopMeta;
  onOpenFolder: () => Promise<void>;
  /** Present only when re-transcription is possible for this meeting. */
  onRetranscribe?: () => void;
  isNarrow: boolean;
  onToggleTranscript: () => void;
}

function iconButton(extra = '') {
  return `flex items-center justify-center ${LAYOUT.ICON_BUTTON} rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground transition-colors ${extra}`;
}

/**
 * The workshop's identity header: back to the library, the title edited in
 * place, the facts of the meeting (when, how long, from where, in what
 * language, who), and the meeting-level actions.
 */
export function WorkshopHeader({
  meetingId,
  title,
  onTitleCommit,
  meta,
  onOpenFolder,
  onRetranscribe,
  isNarrow,
  onToggleTranscript,
}: WorkshopHeaderProps) {
  const router = useRouter();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [exporting, setExporting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const startEdit = () => {
    setDraft(title);
    setEditing(true);
  };

  const commitEdit = () => {
    setEditing(false);
    const next = draft.trim();
    if (next && next !== title) onTitleCommit(next);
  };

  // Escape must abandon the draft - the mockup only handled Enter, which
  // left no way to back out of an accidental edit.
  const cancelEdit = () => {
    setEditing(false);
    setDraft('');
  };

  const handleExport = useCallback(async () => {
    Analytics.trackButtonClick('export_to_obsidian', 'workshop');
    setExporting(true);
    try {
      const paths = await exportMeeting(meetingId);
      const names = paths.map((p) => p.split('/').pop()).join(', ');
      toast.success(`Exported to Obsidian: ${names}`);
    } catch (error) {
      // Most common cause is no vault configured; the backend says so explicitly.
      console.error('Obsidian export failed:', error);
      toast.error(String(error));
    } finally {
      setExporting(false);
    }
  }, [meetingId]);

  const handleDelete = useCallback(async () => {
    setConfirmDelete(false);
    setDeleting(true);
    try {
      await invoke('api_delete_meeting', { meetingId });
      Analytics.trackMeetingDeleted(meetingId);
      toast.success('Meeting deleted');
      router.push('/');
    } catch (error) {
      toast.error('Failed to delete meeting', { description: String(error) });
      setDeleting(false);
    }
  }, [meetingId, router]);

  const metaParts = [
    meta.dateTimeLabel,
    meta.durationLabel,
    meta.sourceLabel,
    meta.languageLabel,
    meta.speakersLabel,
  ].filter(Boolean) as string[];

  return (
    <div className={`shrink-0 ${LAYOUT.INSET} pt-3.5 pb-2.5 border-b border-border`}>
      <div className={`flex items-center ${LAYOUT.GAP}`}>
        <button
          onClick={() => router.push('/')}
          aria-label="Back to library"
          className={`flex items-center ${LAYOUT.GUTTER} text-muted-foreground hover:text-foreground shrink-0`}
        >
          <ChevronLeft className="w-[18px] h-[18px]" />
        </button>

        {editing ? (
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commitEdit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
              if (e.key === 'Escape') {
                // Prevent the blur commit: leave editing first, then blur.
                cancelEdit();
              }
            }}
            autoFocus
            className="flex-1 min-w-0 text-base font-bold bg-card border border-ring rounded-md -ml-1.5 px-1.5 py-0.5 outline-none"
          />
        ) : (
          <button
            onClick={startEdit}
            title="Rename meeting"
            className="text-base font-bold cursor-text -ml-1.5 px-1.5 py-0.5 rounded-md whitespace-nowrap overflow-hidden text-ellipsis text-left hover:bg-muted/60"
          >
            {title}
          </button>
        )}

        <div className="flex-1" />

        {isNarrow && (
          <button
            onClick={onToggleTranscript}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-border bg-card text-xs shrink-0"
          >
            <PanelLeft className="w-3.5 h-3.5" />
            Transcript
          </button>
        )}

        <div className="flex items-center gap-0.5 shrink-0">
          <Tooltip>
            <TooltipTrigger asChild>
              <button onClick={() => onOpenFolder()} className={iconButton()} aria-label="Open recording folder">
                <FolderOpen className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent>Open recording folder</TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={handleExport}
                disabled={exporting}
                className={iconButton('disabled:opacity-50')}
                aria-label="Export to Obsidian"
              >
                <FileDown className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{exporting ? 'Exporting…' : 'Export to Obsidian'}</TooltipContent>
          </Tooltip>

          {onRetranscribe && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button onClick={onRetranscribe} className={iconButton()} aria-label="Re-transcribe">
                  <RefreshCw className="w-4 h-4" />
                </button>
              </TooltipTrigger>
              <TooltipContent>Re-transcribe from the recording</TooltipContent>
            </Tooltip>
          )}

          <span className="w-px h-[18px] bg-border mx-1" />

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => setConfirmDelete(true)}
                disabled={deleting}
                className={`flex items-center justify-center ${LAYOUT.ICON_BUTTON} rounded-lg text-destructive hover:bg-destructive hover:text-destructive-foreground transition-colors disabled:opacity-50`}
                aria-label="Delete meeting"
              >
                <Trash2 className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent>Delete meeting</TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div className={`flex gap-2.5 items-center mt-1.5 ${LAYOUT.CONTENT_OFFSET} text-xs text-muted-foreground flex-wrap`}>
        {metaParts.map((part, i) => (
          <span key={i} className="flex items-center gap-2.5">
            {i > 0 && <span aria-hidden>·</span>}
            <span>{part}</span>
          </span>
        ))}
      </div>

      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this meeting?</AlertDialogTitle>
            <AlertDialogDescription>
              The transcript, summary and recording files will be removed. This cannot be
              undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
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
