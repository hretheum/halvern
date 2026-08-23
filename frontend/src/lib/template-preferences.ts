/**
 * Which summary template new generations start from.
 *
 * Stored in localStorage like the auto-summary switch: it is a UI preference
 * of this installation, not meeting data. The workshop reads it once per
 * visit as the initial selection; picking a different template there stays a
 * per-visit override and never writes back.
 */
const KEY = 'defaultSummaryTemplate';

/** Built-in template every fresh install has; the safety net when the stored id is gone. */
export const FALLBACK_TEMPLATE_ID = 'standard_meeting';

export function loadDefaultTemplateId(): string {
  try {
    return localStorage.getItem(KEY) || FALLBACK_TEMPLATE_ID;
  } catch {
    return FALLBACK_TEMPLATE_ID;
  }
}

export function saveDefaultTemplateId(id: string): void {
  try {
    localStorage.setItem(KEY, id);
  } catch {
    // Preference persistence is best-effort; generation falls back cleanly.
  }
}
