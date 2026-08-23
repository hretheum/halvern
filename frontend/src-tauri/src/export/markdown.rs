//! Pure rendering of a meeting into Obsidian-flavoured Markdown.
//!
//! Everything in this module is deliberately free of database access, Tauri
//! handles and filesystem I/O so that it can be unit tested without a running
//! application. Writing the rendered bundle to disk lives in [`super::write_bundle`].

/// Meeting metadata needed to render a note. Deliberately decoupled from
/// `database::models::MeetingModel` so the renderer stays testable.
#[derive(Debug, Clone)]
pub struct ExportMeeting {
    pub id: String,
    pub title: String,
    /// ISO 8601 timestamp as stored in `meetings.created_at`.
    pub created_at: String,
    /// Recording length in minutes, when known.
    pub duration_min: Option<f64>,
}

/// One transcript segment as rendered into the note.
#[derive(Debug, Clone)]
pub struct ExportSegment {
    /// Audio source or speaker label from `transcripts.speaker`.
    /// Before speaker diarisation exists this holds `mic` / `system`.
    pub speaker: Option<String>,
    /// Offset from the start of the recording, in seconds.
    pub audio_start_time: Option<f64>,
    pub text: String,
}

/// A rendered meeting, ready to be written to disk.
#[derive(Debug, Clone)]
pub struct ExportBundle {
    /// Filename stem without the `-transcript` / `-summary` suffix,
    /// e.g. `2026-08-11-design-brief-procredit`.
    pub stem: String,
    pub transcript_md: String,
    /// `None` when the meeting has no generated summary yet.
    pub summary_md: Option<String>,
}

/// Which kind of note a front matter block describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Transcript,
    Summary,
}

impl NoteKind {
    fn as_str(self) -> &'static str {
        match self {
            NoteKind::Transcript => "transcript",
            NoteKind::Summary => "summary",
        }
    }
}

/// Maximum slug length. Long meeting titles produce unwieldy filenames and some
/// sync clients dislike very long names, so the slug is truncated on a word
/// boundary where possible.
const MAX_SLUG_LEN: usize = 60;

/// Transliterates a single Polish diacritic to its ASCII base letter.
/// Returns `None` for characters that need no transliteration.
fn transliterate_pl(c: char) -> Option<char> {
    Some(match c {
        'ą' => 'a',
        'ć' => 'c',
        'ę' => 'e',
        'ł' => 'l',
        'ń' => 'n',
        'ó' => 'o',
        'ś' => 's',
        'ź' | 'ż' => 'z',
        'Ą' => 'A',
        'Ć' => 'C',
        'Ę' => 'E',
        'Ł' => 'L',
        'Ń' => 'N',
        'Ó' => 'O',
        'Ś' => 'S',
        'Ź' | 'Ż' => 'Z',
        _ => return None,
    })
}

/// Converts a meeting title into a filesystem-safe slug.
///
/// Lowercase, Polish diacritics transliterated, whitespace and separators
/// collapsed into single hyphens, everything outside `[a-z0-9-]` dropped.
/// Falls back to `meeting` when nothing usable remains.
pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_was_hyphen = true; // suppresses a leading hyphen

    for raw in title.chars() {
        let c = transliterate_pl(raw).unwrap_or(raw);
        let lowered = c.to_ascii_lowercase();

        if lowered.is_ascii_alphanumeric() {
            out.push(lowered);
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            out.push('-');
            last_was_hyphen = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    if out.len() > MAX_SLUG_LEN {
        out.truncate(MAX_SLUG_LEN);
        // Avoid cutting mid-word when a hyphen is close to the limit.
        if let Some(pos) = out.rfind('-') {
            if pos >= MAX_SLUG_LEN / 2 {
                out.truncate(pos);
            }
        }
        while out.ends_with('-') {
            out.pop();
        }
    }

    if out.is_empty() {
        "meeting".to_string()
    } else {
        out
    }
}

/// Extracts the `YYYY-MM-DD` prefix from an ISO 8601 timestamp.
///
/// Falls back to `unknown-date` rather than guessing, so a malformed timestamp
/// is visible in the filename instead of silently becoming today's date.
pub fn date_prefix(created_at: &str) -> String {
    let candidate: String = created_at.chars().take(10).collect();
    let looks_like_date = candidate.len() == 10
        && candidate.as_bytes()[4] == b'-'
        && candidate.as_bytes()[7] == b'-'
        && candidate
            .chars()
            .enumerate()
            .all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() });

    if looks_like_date {
        candidate
    } else {
        "unknown-date".to_string()
    }
}

/// Formats a recording offset as `MM:SS`, or `H:MM:SS` past one hour.
pub fn format_timestamp(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/// Quotes a string for a YAML scalar. Titles routinely contain colons and
/// quotation marks, both of which break unquoted YAML.
fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
        .replace('\r', "");
    format!("\"{}\"", escaped)
}

fn front_matter(meeting: &ExportMeeting, kind: NoteKind) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("meeting_id: {}\n", yaml_quote(&meeting.id)));
    fm.push_str(&format!("title: {}\n", yaml_quote(&meeting.title)));
    fm.push_str(&format!("date: {}\n", yaml_quote(&meeting.created_at)));
    if let Some(d) = meeting.duration_min {
        fm.push_str(&format!("duration_min: {:.1}\n", d));
    }
    fm.push_str("source: halvern\n");
    fm.push_str(&format!("type: {}\n", kind.as_str()));
    fm.push_str(&format!("tags: [meeting, halvern, {}]\n", kind.as_str()));
    fm.push_str("---\n\n");
    fm
}

/// Speaker labels per language, keyed by the base of a BCP-47 tag.
///
/// Mirrors `src/lib/speaker-labels.ts` on the frontend. The two tables are kept in
/// step by hand: they are short, they change rarely, and sharing them would mean
/// generating one from the other for the sake of twenty lines.
///
/// English is the fallback for every language not listed. A wrong translation is
/// worse than the fallback, so only entries someone can vouch for belong here.
const SPEAKER_LABELS: &[(&str, &str, &str)] = &[
    // (language, mic, system)
    ("en", "Me", "Others"),
    ("pl", "Ja", "Rozmówcy"),
    ("de", "Ich", "Andere"),
    ("es", "Yo", "Otros"),
    ("fr", "Moi", "Autres"),
    ("it", "Io", "Altri"),
    ("pt", "Eu", "Outros"),
    ("nl", "Ik", "Anderen"),
    ("cs", "Já", "Ostatní"),
    ("uk", "Я", "Інші"),
];

/// Note headings per language, same keys and same fallback rule as `SPEAKER_LABELS`.
const NOTE_STRINGS: &[(&str, &str, &str, &str)] = &[
    // (language, transcript suffix, summary suffix, empty-transcript line)
    ("en", "transcript", "summary", "_The transcript is empty._"),
    ("pl", "transkrypt", "podsumowanie", "_Transkrypt jest pusty._"),
    ("de", "Transkript", "Zusammenfassung", "_Das Transkript ist leer._"),
    ("es", "transcripción", "resumen", "_La transcripción está vacía._"),
    ("fr", "transcription", "résumé", "_La transcription est vide._"),
    ("it", "trascrizione", "riassunto", "_La trascrizione è vuota._"),
    ("pt", "transcrição", "resumo", "_A transcrição está vazia._"),
    ("nl", "transcript", "samenvatting", "_Het transcript is leeg._"),
    ("cs", "přepis", "shrnutí", "_Přepis je prázdný._"),
    ("uk", "транскрипт", "підсумок", "_Транскрипт порожній._"),
];

/// Reduces a BCP-47 tag to the key both tables use: `pt-BR` and `pt_PT` give `pt`.
fn language_key(language: Option<&str>) -> String {
    language
        .map(|tag| {
            tag.to_ascii_lowercase()
                .replace('_', "-")
                .split('-')
                .next()
                .unwrap_or("en")
                .to_string()
        })
        .unwrap_or_else(|| "en".to_string())
}

fn note_strings(language: Option<&str>) -> &'static (&'static str, &'static str, &'static str, &'static str) {
    let key = language_key(language);
    NOTE_STRINGS
        .iter()
        .find(|(code, _, _, _)| *code == key)
        .unwrap_or(&NOTE_STRINGS[0])
}

/// Turns a raw source value into the label a person reads, in `language`.
///
/// `language` is a BCP-47 tag; `pt-BR` and `pt_PT` both resolve to `pt`. An unknown
/// or missing tag falls back to English, which is also what the UI ships in.
///
/// Anything outside the known two values passes through untouched — future
/// diarisation will put real names here.
fn speaker_label<'a>(raw: &'a str, language: Option<&str>) -> &'a str {
    let key = language_key(language);

    let (_, mic, system) = SPEAKER_LABELS
        .iter()
        .find(|(code, _, _)| *code == key)
        .unwrap_or(&SPEAKER_LABELS[0]);

    match raw {
        "mic" => mic,
        "system" => system,
        other => other,
    }
}

fn render_transcript(
    meeting: &ExportMeeting,
    segments: &[ExportSegment],
    language: Option<&str>,
) -> String {
    let mut out = front_matter(meeting, NoteKind::Transcript);
    let strings = note_strings(language);
    out.push_str(&format!("# {} — {}\n\n", meeting.title, strings.1));

    if segments.is_empty() {
        out.push_str(strings.3);
        out.push('\n');
        return out;
    }

    for seg in segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }

        let stamp = seg.audio_start_time.map(format_timestamp);
        let label = seg
            .speaker
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|raw| speaker_label(raw, language));

        match (stamp, label) {
            (Some(t), Some(l)) => out.push_str(&format!("**[{}] {}:** {}\n\n", t, l, text)),
            (Some(t), None) => out.push_str(&format!("**[{}]** {}\n\n", t, text)),
            (None, Some(l)) => out.push_str(&format!("**{}:** {}\n\n", l, text)),
            (None, None) => out.push_str(&format!("{}\n\n", text)),
        }
    }

    out
}

fn render_summary(
    meeting: &ExportMeeting,
    summary_markdown: &str,
    language: Option<&str>,
) -> String {
    let mut out = front_matter(meeting, NoteKind::Summary);
    out.push_str(&format!("# {} — {}\n\n", meeting.title, note_strings(language).2));
    out.push_str(summary_markdown.trim());
    out.push('\n');
    out
}

/// Renders a meeting into a bundle of Markdown notes.
///
/// `summary_markdown` is the already-extracted Markdown body, not the raw
/// `summary_processes.result` JSON blob.
/// Renders the pair of notes for one meeting.
///
/// `language` is the BCP-47 tag the notes are written in — it comes from the same
/// summary-language preference the user already sets, so an exported note matches
/// the summary it sits next to. `None` falls back to English.
pub fn render_meeting(
    meeting: &ExportMeeting,
    segments: &[ExportSegment],
    summary_markdown: Option<&str>,
    language: Option<&str>,
) -> ExportBundle {
    let stem = format!(
        "{}-{}",
        date_prefix(&meeting.created_at),
        slugify(&meeting.title)
    );

    ExportBundle {
        stem,
        transcript_md: render_transcript(meeting, segments, language),
        summary_md: summary_markdown
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|md| render_summary(meeting, md, language)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting() -> ExportMeeting {
        ExportMeeting {
            id: "meeting-1".to_string(),
            title: "Design Brief for Procredit Holding".to_string(),
            created_at: "2026-08-11T13:24:04.675549+00:00".to_string(),
            duration_min: Some(13.7),
        }
    }

    #[test]
    fn slug_transliterates_polish_diacritics() {
        assert_eq!(
            slugify("Zażółć gęślą jaźń"),
            "zazolc-gesla-jazn",
            "every Polish diacritic must fold to ASCII"
        );
        assert_eq!(slugify("Świętość ŁÓDŹ"), "swietosc-lodz");
    }

    #[test]
    fn slug_collapses_separators_and_strips_punctuation() {
        assert_eq!(slugify("Design Brief:  Procredit / Holding!"), "design-brief-procredit-holding");
        assert_eq!(slugify("  --- leading and trailing --- "), "leading-and-trailing");
    }

    #[test]
    fn slug_falls_back_when_nothing_usable_remains() {
        assert_eq!(slugify("!!! ???"), "meeting");
        assert_eq!(slugify(""), "meeting");
    }

    #[test]
    fn slug_is_truncated_on_a_word_boundary() {
        let long = "a".repeat(40) + " " + &"b".repeat(40);
        let s = slugify(&long);
        assert!(s.len() <= MAX_SLUG_LEN, "slug length was {}", s.len());
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn date_prefix_extracts_or_flags_malformed() {
        assert_eq!(date_prefix("2026-08-11T13:24:04+00:00"), "2026-08-11");
        assert_eq!(date_prefix("not a date"), "unknown-date");
        assert_eq!(date_prefix(""), "unknown-date");
    }

    #[test]
    fn timestamps_switch_to_hours_past_one_hour() {
        assert_eq!(format_timestamp(0.0), "00:00");
        assert_eq!(format_timestamp(65.4), "01:05");
        assert_eq!(format_timestamp(3725.0), "1:02:05");
        assert_eq!(format_timestamp(-5.0), "00:00");
    }

    #[test]
    fn yaml_quoting_survives_quotes_and_colons() {
        let m = ExportMeeting {
            title: "Re: \"Q3\" plan\\draft".to_string(),
            ..meeting()
        };
        let fm = front_matter(&m, NoteKind::Transcript);
        assert!(fm.contains(r#"title: "Re: \"Q3\" plan\\draft""#), "got:\n{}", fm);
    }

    #[test]
    fn meeting_without_summary_yields_none() {
        let bundle = render_meeting(&meeting(), &[], None, None);
        assert!(bundle.summary_md.is_none());
        assert_eq!(bundle.stem, "2026-08-11-design-brief-for-procredit-holding");
        assert!(bundle.transcript_md.contains("_The transcript is empty._"));
    }

    #[test]
    fn blank_summary_is_treated_as_absent() {
        let bundle = render_meeting(&meeting(), &[], Some("   \n  "), None);
        assert!(bundle.summary_md.is_none());
    }

    #[test]
    fn transcript_renders_timestamp_and_speaker() {
        let segments = vec![
            ExportSegment {
                speaker: Some("mic".to_string()),
                audio_start_time: Some(65.0),
                text: "  Zaczynamy spotkanie.  ".to_string(),
            },
            ExportSegment {
                speaker: None,
                audio_start_time: None,
                text: "Bez metadanych.".to_string(),
            },
            ExportSegment {
                speaker: Some("system".to_string()),
                audio_start_time: Some(2.0),
                text: "   ".to_string(),
            },
        ];
        let bundle = render_meeting(&meeting(), &segments, Some("## Decisions\n\n- none"), Some("pl"));

        assert!(bundle.transcript_md.contains("**[01:05] Ja:** Zaczynamy spotkanie."));
        assert!(bundle.transcript_md.contains("\nBez metadanych.\n"));
        assert!(
            !bundle.transcript_md.contains("system"),
            "a segment with empty text must not appear"
        );

        let summary = bundle.summary_md.expect("the summary should exist");
        assert!(summary.contains("type: summary"));
        assert!(summary.contains("## Decisions"));
    }

    #[test]
    fn front_matter_omits_unknown_duration() {
        let m = ExportMeeting { duration_min: None, ..meeting() };
        let fm = front_matter(&m, NoteKind::Summary);
        assert!(!fm.contains("duration_min"));
        assert!(fm.contains("type: summary"));
    }

    #[test]
    fn etykiety_zrodel_tlumacza_sie_na_polskie() {
        assert_eq!(speaker_label("mic", Some("pl")), "Ja");
        assert_eq!(speaker_label("system", Some("pl")), "Rozmówcy");
        assert_eq!(speaker_label("mic", Some("de")), "Ich");
        assert_eq!(speaker_label("mic", None), "Me", "no language falls back to English");
        assert_eq!(speaker_label("mic", Some("qq")), "Me", "an unknown language falls back too");
        assert_eq!(speaker_label("system", Some("pt-BR")), "Outros", "a regional tag resolves to its base");
        // An unknown value stays as it is — better to show it raw than to lose it.
        assert_eq!(speaker_label("Anna", Some("pl")), "Anna");
    }
}
