import { readPinnedSummaryLanguageDefault } from '@/lib/summary-language-preferences';

/**
 * Speaker labels per language, keyed by the base of a BCP-47 tag.
 *
 * The data layer stores `mic` and `system` — the same values the
 * `transcripts.speaker` column and its migration use. Human-readable text is
 * produced only on the way out, which is why this table lives here and not in
 * the database.
 *
 * English is the fallback for every language not listed. Adding a language is a
 * matter of one line, but a wrong translation is worse than the fallback, so
 * only entries someone can vouch for belong here.
 */
const LABELS: Record<string, { mic: string; system: string }> = {
  en: { mic: 'Me', system: 'Others' },
  pl: { mic: 'Ja', system: 'Rozmówcy' },
  de: { mic: 'Ich', system: 'Andere' },
  es: { mic: 'Yo', system: 'Otros' },
  fr: { mic: 'Moi', system: 'Autres' },
  it: { mic: 'Io', system: 'Altri' },
  pt: { mic: 'Eu', system: 'Outros' },
  nl: { mic: 'Ik', system: 'Anderen' },
  cs: { mic: 'Já', system: 'Ostatní' },
  uk: { mic: 'Я', system: 'Інші' },
};

/**
 * Reduces a BCP-47 tag to the key used above: `pt-BR` and `pt_PT` both give `pt`.
 */
function languageKey(tag: string | null | undefined): string {
  if (!tag) return 'en';
  const base = tag.toLowerCase().replace('_', '-').split('-')[0];
  return base in LABELS ? base : 'en';
}

/**
 * Turns a raw source value into the label shown to a person.
 *
 * Without `language` the caller gets the user's pinned summary language, which is
 * the setting that already decides what language output is produced in. Passing
 * it explicitly matters where the meeting has its own preference that differs
 * from the global default.
 *
 * One place instead of four copies of the same condition: the transcript view,
 * the text handed to the summary model, and two clipboard paths.
 */
export function speakerLabel(
  raw: string | null | undefined,
  language?: string | null,
): string | null {
  if (!raw) return null;

  const key = languageKey(language ?? readPinnedSummaryLanguageDefault());
  const labels = LABELS[key];

  switch (raw) {
    case 'mic':
      return labels.mic;
    case 'system':
      return labels.system;
    // Anything outside the known two passes through — future diarisation will put
    // real names here.
    default:
      return raw;
  }
}

/**
 * A transcript-line prefix, ready to concatenate with the text.
 * Empty when the source is unknown, since older meetings carry no labels.
 */
export function speakerPrefix(
  raw: string | null | undefined,
  language?: string | null,
): string {
  const label = speakerLabel(raw, language);
  return label ? `${label}: ` : '';
}

/**
 * Reduces a raw source value to one of the two known ones, or `null`.
 *
 * Needed when restoring from IndexedDB: a `StoredTranscript` can be a raw
 * `TranscriptUpdate` written before this branch existed, when the source field was
 * hardcoded to the literal "Audio". Without this filter such a value would reach
 * the `speaker` column directly and surface as an "Audio" label in the UI, the
 * export and the summary prompt.
 */
export function normalizeSpeaker(raw: string | null | undefined): 'mic' | 'system' | null {
  return raw === 'mic' || raw === 'system' ? raw : null;
}
