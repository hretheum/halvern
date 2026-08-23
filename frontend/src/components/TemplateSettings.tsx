'use client';

import React, { useState, useCallback, useEffect, useRef } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Check, FileJson, FolderOpen, Pencil, Plus, ShieldAlert, Trash2, UploadCloud } from 'lucide-react';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { Textarea } from './ui/textarea';
import { Alert, AlertDescription, AlertTitle } from './ui/alert';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from './ui/dialog';
import { ConfirmationModal } from './ConfirmationModel/confirmation-modal';
import {
  ParsedTemplate,
  slugifyTemplateName,
  useTemplateManagement,
} from '@/hooks/useTemplateManagement';
import {
  loadDefaultTemplateId,
  saveDefaultTemplateId,
  FALLBACK_TEMPLATE_ID,
} from '@/lib/template-preferences';

interface TemplateFormState {
  mode: 'add' | 'edit';
  editingId: string | null;
  id: string;
  idTouched: boolean;
  rawJson: string;
  preview: ParsedTemplate | null;
  validatedForJson: string | null;
  validating: boolean;
  saving: boolean;
  error: string | null;
}

const EMPTY_FORM: TemplateFormState = {
  mode: 'add',
  editingId: null,
  id: '',
  idTouched: false,
  rawJson: '',
  preview: null,
  validatedForJson: null,
  validating: false,
  saving: false,
  error: null,
};

export function TemplateSettings() {
  const {
    templates,
    loading,
    validateTemplate,
    saveTemplate,
    deleteTemplate,
    getCustomTemplateRaw,
    pickTemplateFile,
    readTemplateFile,
  } = useTemplateManagement();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [defaultTemplateId, setDefaultTemplateId] = useState<string>(loadDefaultTemplateId);
  const [form, setForm] = useState<TemplateFormState>(EMPTY_FORM);
  const [isDragOver, setIsDragOver] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);

  // Trimmed once, used everywhere downstream (safety check, collision check,
  // save) so a stray leading/trailing space can't make the collision check
  // miss a match that the backend — which trims before writing — would
  // actually hit.
  const trimmedId = form.id.trim();

  const openAddDialog = useCallback(() => {
    setForm(EMPTY_FORM);
    setDialogOpen(true);
  }, []);

  const openEditDialog = useCallback(async (id: string, name: string) => {
    try {
      const rawJson = await getCustomTemplateRaw(id);
      setForm({
        ...EMPTY_FORM,
        mode: 'edit',
        editingId: id,
        id,
        idTouched: true,
        rawJson,
      });
      setDialogOpen(true);
    } catch (err) {
      toast.error(`Failed to load "${name}" for editing`, {
        description: typeof err === 'string' ? err : String(err),
      });
    }
  }, [getCustomTemplateRaw]);

  // Only the Templates tab's Add/Edit dialog listens for drag-drop, and only
  // while it's open — Tauri's drag-drop event is window-wide, so this can
  // fire alongside the app's audio-import drop handler in layout.tsx. If the
  // "Import Audio & Retranscribe" beta feature happens to be on, dropping a
  // .json file here may also trigger that handler's "please drop an audio
  // file" toast alongside this one. Known, accepted overlap — not fixed here
  // since it belongs to an unrelated feature's global listener.
  useEffect(() => {
    if (!dialogOpen) return;

    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    const setup = async () => {
      const unlistenEnter = await listen('tauri://drag-enter', () => {
        if (!cancelled) setIsDragOver(true);
      });
      if (cancelled) { unlistenEnter(); return; }
      unlisteners.push(unlistenEnter);

      const unlistenLeave = await listen('tauri://drag-leave', () => {
        if (!cancelled) setIsDragOver(false);
      });
      if (cancelled) { unlistenLeave(); unlisteners.forEach(u => u()); return; }
      unlisteners.push(unlistenLeave);

      const unlistenDrop = await listen<{ paths: string[] }>('tauri://drag-drop', async (event) => {
        if (cancelled) return;
        setIsDragOver(false);
        const jsonPath = event.payload.paths.find(p => p.toLowerCase().endsWith('.json'));
        if (!jsonPath) {
          toast.error('Please drop a .json template file');
          return;
        }
        try {
          const content = await readTemplateFile(jsonPath);
          setForm(prev => ({ ...prev, rawJson: content, preview: null, validatedForJson: null, error: null }));
        } catch (err) {
          toast.error('Failed to read dropped file', {
            description: typeof err === 'string' ? err : String(err),
          });
        }
      });
      if (cancelled) { unlistenDrop(); unlisteners.forEach(u => u()); return; }
      unlisteners.push(unlistenDrop);
    };

    setup();

    return () => {
      cancelled = true;
      unlisteners.forEach(u => u());
    };
  }, [dialogOpen, readTemplateFile]);

  const handleBrowse = useCallback(async () => {
    try {
      const content = await pickTemplateFile();
      if (content !== null) {
        setForm(prev => ({ ...prev, rawJson: content, preview: null, validatedForJson: null, error: null }));
      }
    } catch (err) {
      toast.error('Failed to open file', {
        description: typeof err === 'string' ? err : String(err),
      });
    }
  }, [pickTemplateFile]);

  const handleRawJsonChange = useCallback((value: string) => {
    setForm(prev => {
      // Auto-fill the id from the pasted/imported name, but only until the
      // user has touched the id field themselves — after that, their choice
      // always wins.
      let nextId = prev.id;
      if (!prev.idTouched) {
        try {
          const parsed = JSON.parse(value);
          if (typeof parsed?.name === 'string' && parsed.name.trim()) {
            nextId = slugifyTemplateName(parsed.name);
          }
        } catch {
          // Not valid JSON yet (e.g. mid-paste) — leave the id as is.
        }
      }
      return { ...prev, rawJson: value, id: nextId, preview: null, validatedForJson: null, error: null };
    });
  }, []);

  const handleIdChange = useCallback((value: string) => {
    setForm(prev => ({ ...prev, id: value, idTouched: true, preview: null, validatedForJson: null }));
  }, []);

  const handleValidate = useCallback(async () => {
    setForm(prev => ({ ...prev, validating: true, error: null }));
    try {
      const parsed = await validateTemplate(form.rawJson);
      setForm(prev => ({
        ...prev,
        validating: false,
        preview: parsed,
        validatedForJson: prev.rawJson,
      }));
    } catch (err) {
      const message = typeof err === 'string' ? err : (err as any)?.message || String(err);
      setForm(prev => ({ ...prev, validating: false, preview: null, validatedForJson: null, error: message }));
    }
  }, [form.rawJson, validateTemplate]);

  const handleSave = useCallback(async () => {
    setForm(prev => ({ ...prev, saving: true, error: null }));
    try {
      await saveTemplate(trimmedId, form.rawJson);
      toast.success(`Template "${form.preview?.name || trimmedId}" saved`);
      setDialogOpen(false);
      setForm(EMPTY_FORM);
    } catch (err) {
      const message = typeof err === 'string' ? err : (err as any)?.message || String(err);
      setForm(prev => ({ ...prev, saving: false, error: message }));
    }
  }, [trimmedId, form.rawJson, form.preview, saveTemplate]);

  const markAsDefault = useCallback((id: string, name: string) => {
    saveDefaultTemplateId(id);
    setDefaultTemplateId(id);
    toast.success('Default template set', {
      description: `New summaries will start from "${name}"`,
    });
  }, []);

  const handleConfirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    try {
      await deleteTemplate(deleteTarget.id);
      toast.success(`Template "${deleteTarget.name}" deleted`);
      // A deleted default must not linger as a dangling id.
      if (deleteTarget.id === defaultTemplateId) {
        saveDefaultTemplateId(FALLBACK_TEMPLATE_ID);
        setDefaultTemplateId(FALLBACK_TEMPLATE_ID);
        toast.info('Default template reset to Standard Meeting Notes');
      }
    } catch (err) {
      toast.error(`Failed to delete "${deleteTarget.name}"`, {
        description: typeof err === 'string' ? err : String(err),
      });
    } finally {
      setDeleteTarget(null);
    }
  }, [deleteTarget, deleteTemplate, defaultTemplateId]);

  const isValidated = form.validatedForJson !== null && form.validatedForJson === form.rawJson;
  const idIsSafe = /^[a-zA-Z0-9_-]+$/.test(trimmedId);
  const canSave = isValidated && idIsSafe && !form.saving;

  const collision = isValidated
    ? templates.find(t => t.id === trimmedId && !(form.mode === 'edit' && t.id === form.editingId))
    : undefined;

  return (
    <div className="space-y-4 pb-6">
      <div className="flex items-center justify-between">
        <div>
          <Label className="block text-sm font-medium text-foreground">
            Summary Templates
          </Label>
          <p className="text-xs text-muted-foreground mt-0.5">
            Built-in templates are read-only. Add your own for summaries shaped the way you need.
          </p>
        </div>
        <Button onClick={openAddDialog} size="sm">
          <Plus className="h-4 w-4 mr-1" />
          Add Template
        </Button>
      </div>

      <div className="space-y-2">
        {loading && templates.length === 0 && (
          <p className="text-sm text-muted-foreground">Loading templates...</p>
        )}
        {templates.map((template) => {
          const isDefault = template.id === defaultTemplateId;
          return (
            <div
              key={template.id}
              role="button"
              tabIndex={0}
              aria-pressed={isDefault}
              onClick={() => {
                if (!isDefault) markAsDefault(template.id, template.name);
              }}
              onKeyDown={(e) => {
                if ((e.key === 'Enter' || e.key === ' ') && !isDefault) {
                  e.preventDefault();
                  markAsDefault(template.id, template.name);
                }
              }}
              title={isDefault ? 'Default template' : 'Click to use as the default template'}
              className={`flex items-center justify-between gap-3 rounded-lg border p-3 cursor-pointer transition-colors ${
                isDefault
                  ? 'border-primary bg-accent/40'
                  : 'border-border bg-card hover:bg-muted/60'
              }`}
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-foreground truncate">{template.name}</span>
                  <span
                    className={`text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded ${
                      template.is_custom
                        ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300'
                        : 'bg-muted text-muted-foreground'
                    }`}
                  >
                    {template.is_custom ? 'Custom' : 'Built-in'}
                  </span>
                  {isDefault && (
                    <span className="flex items-center gap-1 text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-primary text-primary-foreground font-semibold">
                      <Check className="h-3 w-3" strokeWidth={3} />
                      Default
                    </span>
                  )}
                </div>
                <p className="text-xs text-muted-foreground truncate">{template.description}</p>
              </div>
              {template.is_custom && (
                <div className="flex items-center gap-1 shrink-0">
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={(e) => {
                      e.stopPropagation();
                      openEditDialog(template.id, template.name);
                    }}
                    title="Edit template"
                  >
                    <Pencil className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeleteTarget({ id: template.id, name: template.name });
                    }}
                    title="Delete template"
                  >
                    <Trash2 className="h-4 w-4 text-red-600 dark:text-red-400" />
                  </Button>
                </div>
              )}
            </div>
          );
        })}
      </div>

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-[600px] max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <FileJson className="h-5 w-5" />
              {form.mode === 'edit' ? 'Edit Template' : 'Add Template'}
            </DialogTitle>
            <DialogDescription>
              {form.mode === 'edit'
                ? 'Edit the JSON below and re-validate before saving.'
                : 'Paste JSON, drop a .json file below, or browse for one.'}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <Alert>
              <ShieldAlert className="h-4 w-4" />
              <AlertTitle className="text-sm font-semibold">Instructions go straight to your model</AlertTitle>
              <AlertDescription className="text-xs">
                Each section's instruction is sent directly to your summarization model.
                Only use templates from sources you trust, and review the instructions
                in the preview below before saving.
              </AlertDescription>
            </Alert>

            <div>
              <div className="flex items-center justify-between mb-1">
                <Label className="text-sm font-medium text-foreground">Template JSON</Label>
                <Button type="button" variant="outline" size="sm" onClick={handleBrowse}>
                  <FolderOpen className="h-3.5 w-3.5 mr-1" />
                  Browse...
                </Button>
              </div>
              <div
                className={`rounded-md border-2 border-dashed transition-colors ${
                  isDragOver ? 'border-blue-500 bg-blue-50 dark:bg-blue-950/30' : 'border-transparent'
                }`}
              >
                <Textarea
                  value={form.rawJson}
                  onChange={(e) => handleRawJsonChange(e.target.value)}
                  placeholder={'{\n  "name": "...",\n  "description": "...",\n  "sections": [...]\n}'}
                  className="font-mono text-xs min-h-[180px]"
                  disabled={form.saving}
                />
              </div>
              {isDragOver && (
                <p className="text-xs text-blue-600 dark:text-blue-400 flex items-center gap-1 mt-1">
                  <UploadCloud className="h-3.5 w-3.5" />
                  Drop the .json file to import
                </p>
              )}
            </div>

            <div>
              <Label className="text-sm font-medium text-foreground">Template ID</Label>
              <Input
                value={form.id}
                onChange={(e) => handleIdChange(e.target.value)}
                placeholder="e.g. client_kickoff"
                disabled={form.mode === 'edit' || form.saving}
                className="mt-1 font-mono text-xs"
              />
              <p className="text-xs text-muted-foreground mt-1">
                {form.mode === 'edit'
                  ? "The id can't be changed when editing — delete and re-add to rename."
                  : 'Letters, numbers, "_" and "-" only. Auto-filled from the template name.'}
              </p>
            </div>

            {collision && (
              <Alert className="border-amber-500 bg-amber-50 dark:bg-amber-950/40">
                <ShieldAlert className="h-4 w-4 text-amber-600 dark:text-amber-400" />
                <AlertTitle className="text-sm text-amber-900 dark:text-amber-200">
                  This will {collision.is_custom ? 'overwrite' : 'shadow'} an existing template
                </AlertTitle>
                <AlertDescription className="text-xs text-amber-800 dark:text-amber-300">
                  {collision.is_custom
                    ? `Saving will replace your existing "${collision.name}" template.`
                    : `Saving will hide the built-in "${collision.name}" template whenever this id is used. Delete this custom template later to bring the built-in one back.`}
                </AlertDescription>
              </Alert>
            )}

            {form.error && (
              <div className="bg-red-50 dark:bg-red-950/40 border border-red-200 dark:border-red-900 rounded-lg p-3">
                <p className="text-sm text-red-800 dark:text-red-300">{form.error}</p>
              </div>
            )}

            {isValidated && form.preview && (
              <div className="space-y-2 rounded-lg border border-green-300 dark:border-green-800 bg-green-50 dark:bg-green-950/30 p-3">
                <p className="text-sm font-semibold text-green-900 dark:text-green-200">
                  {form.preview.name}
                </p>
                <p className="text-xs text-green-800 dark:text-green-300">{form.preview.description}</p>
                <div className="space-y-1.5 pt-1">
                  {form.preview.sections.map((section, i) => (
                    <div key={i} className="text-xs">
                      <span className="font-medium text-green-900 dark:text-green-200">{section.title}</span>
                      <span className="text-green-700 dark:text-green-400"> ({section.format}): </span>
                      <span className="text-green-800 dark:text-green-300">{section.instruction}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)} disabled={form.saving}>
              Cancel
            </Button>
            {!isValidated ? (
              <Button
                onClick={handleValidate}
                disabled={!form.rawJson.trim() || form.validating}
              >
                {form.validating ? 'Validating...' : 'Validate & Preview'}
              </Button>
            ) : (
              <Button onClick={handleSave} disabled={!canSave} className="bg-blue-600 hover:bg-blue-700">
                {form.saving ? 'Saving...' : 'Save Template'}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmationModal
        isOpen={deleteTarget !== null}
        text={`Delete the "${deleteTarget?.name}" template? This can't be undone.`}
        onConfirm={handleConfirmDelete}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  );
}
