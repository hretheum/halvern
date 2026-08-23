import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export interface TemplateInfo {
  id: string;
  name: string;
  description: string;
  is_custom: boolean;
}

export interface TemplateSection {
  title: string;
  instruction: string;
  format: string;
  item_format?: string;
  example_item_format?: string;
}

export interface ParsedTemplate {
  name: string;
  description: string;
  sections: TemplateSection[];
}

/** Derive a filesystem-safe id from a template name, e.g. "Client Kickoff!" -> "client_kickoff". */
export function slugifyTemplateName(name: string): string {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 64);
}

/**
 * Centralizes template list state and the save/delete/validate/import calls
 * so the settings panel and its add/edit dialog can share one source of truth.
 */
export function useTemplateManagement() {
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<TemplateInfo[]>('api_list_templates');
      setTemplates(result);
    } catch (err) {
      console.error('Failed to list templates:', err);
      toast.error('Failed to load templates');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const validateTemplate = useCallback(async (json: string): Promise<ParsedTemplate> => {
    // api_validate_template only returns the name on success; the preview
    // needs the full structure, so parse locally once the backend confirms
    // the JSON is valid. This can't drift from the backend's own parsing:
    // both use the same Template shape, and a real structural mismatch would
    // already have been rejected by api_validate_template itself.
    await invoke<string>('api_validate_template', { templateJson: json });
    return JSON.parse(json) as ParsedTemplate;
  }, []);

  const saveTemplate = useCallback(async (id: string, json: string) => {
    await invoke('api_save_template', { templateId: id, templateJson: json });
    await refresh();
  }, [refresh]);

  const deleteTemplate = useCallback(async (id: string) => {
    await invoke('api_delete_template', { templateId: id });
    await refresh();
  }, [refresh]);

  const getCustomTemplateRaw = useCallback(async (id: string): Promise<string> => {
    return await invoke<string>('api_get_custom_template_raw', { templateId: id });
  }, []);

  const pickTemplateFile = useCallback(async (): Promise<string | null> => {
    return await invoke<string | null>('pick_template_file_command');
  }, []);

  const readTemplateFile = useCallback(async (path: string): Promise<string> => {
    return await invoke<string>('read_template_file_command', { path });
  }, []);

  return {
    templates,
    loading,
    refresh,
    validateTemplate,
    saveTemplate,
    deleteTemplate,
    getCustomTemplateRaw,
    pickTemplateFile,
    readTemplateFile,
  };
}
