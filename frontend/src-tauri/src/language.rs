//! What each transcription engine can hear, and what the user's answer about
//! their meeting language implies for the models installed at first run.
//!
//! This module exists because the two engines do not cover the same ground.
//! Parakeet detects among 25 European languages by itself and takes no language
//! argument at all; Whisper covers around a hundred but has to be told which
//! one, or detect per chunk. Onboarding used to install Parakeet
//! unconditionally, which left every user outside those 25 languages with an
//! engine that could not transcribe them, unasked and unwarned.
//!
//! The decisions here are deliberately pure: they take an answer and some
//! numbers and return a choice, with no I/O and no `AppHandle`, so the rules
//! can be tested directly. See `docs/ONBOARDING_LANGUAGE_MODEL.md`.

use serde::{Deserialize, Serialize};

/// The languages Parakeet TDT 0.6B v3 was trained on, as ISO 639-1 codes.
///
/// Source: the model card at <https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3>.
/// v3 extended v2 from English-only to these 25 European languages, and detects
/// among them without prompting — which is why `ParakeetProvider::transcribe`
/// ignores its `language` argument rather than being unfinished.
///
/// Kept sorted so that a reader can check membership by eye and a diff against
/// a future model version stays legible.
pub const PARAKEET_V3_LANGUAGES: [&str; 25] = [
    "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv", "mt",
    "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
];

/// Whether Parakeet can transcribe this language at all.
///
/// Unknown or malformed codes answer `false`, which routes them to Whisper —
/// the safe direction, since Whisper covers far more and a wrong `true` here
/// produces unusable transcripts rather than merely slower ones.
pub fn parakeet_supports(code: &str) -> bool {
    let code = normalise_code(code);
    PARAKEET_V3_LANGUAGES.contains(&code.as_str())
}

/// Reduce a locale-ish string to a bare ISO 639-1 code: `pt-BR` becomes `pt`,
/// `EN_us` becomes `en`. The pickers hand us bare codes today, but OS locales
/// arrive in the longer form and this is the boundary that meets them.
fn normalise_code(code: &str) -> String {
    code.trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Which speech recognition engine to install and use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeechEngine {
    Parakeet,
    Whisper,
}

/// The provider strings stored in the transcript config.
///
/// These are the only spellings `audio::transcription::engine` accepts for live
/// recording, and they are not guessable — the Whisper one is `localWhisper`,
/// not `whisper`. They live here so that the value written at onboarding and
/// the value matched at record time come from one definition; writing an
/// unrecognised provider fails only later, when someone presses record.
pub const PROVIDER_PARAKEET: &str = "parakeet";
pub const PROVIDER_WHISPER: &str = "localWhisper";

impl SpeechEngine {
    /// The provider string stored in the transcript config, matching what
    /// `SettingsRepository::save_transcript_config` and the engine dispatch in
    /// `audio::transcription` expect.
    pub fn provider(self) -> &'static str {
        match self {
            SpeechEngine::Parakeet => PROVIDER_PARAKEET,
            SpeechEngine::Whisper => PROVIDER_WHISPER,
        }
    }

    /// The model onboarding downloads for this engine.
    ///
    /// The Whisper choice is `large-v3-turbo-q5_0` rather than a smaller tier
    /// because this path exists precisely for the languages Parakeet cannot
    /// serve, and a weak model there would trade one bad transcript for
    /// another. At 547 MB it is also close to Parakeet's 670 MB, so choosing a
    /// language does not surprise the user with a much heavier first run.
    pub fn default_model(self) -> &'static str {
        match self {
            SpeechEngine::Parakeet => crate::config::DEFAULT_PARAKEET_MODEL,
            SpeechEngine::Whisper => "large-v3-turbo-q5_0",
        }
    }

    /// Download size in MiB, for telling the user what the first run costs
    /// before it starts. Kept next to the model choice so the two cannot drift.
    pub fn download_size_mb(self) -> u32 {
        match self {
            SpeechEngine::Parakeet => 670,
            SpeechEngine::Whisper => 547,
        }
    }
}

/// Choose the engine for the languages the user says their meetings are in.
///
/// Parakeet only when it covers *every* language named, because it decides for
/// itself which one it is hearing and cannot be steered; one unsupported
/// language in the set is enough to make it the wrong engine for that user.
///
/// An empty list means the user declined to say — they skipped the question or
/// chose "several languages" without narrowing it. That answers Whisper, which
/// covers around a hundred languages: not knowing must fail towards the engine
/// that can probably cope, never towards the one that probably cannot.
pub fn choose_engine(languages: &[String]) -> SpeechEngine {
    if languages.is_empty() {
        return SpeechEngine::Whisper;
    }

    if languages.iter().all(|code| parakeet_supports(code)) {
        SpeechEngine::Parakeet
    } else {
        SpeechEngine::Whisper
    }
}

/// Families of languages that summarization models tend to handle alike.
///
/// The grouping exists to keep the bake-off finite: scoring 4 models against 32
/// languages is 128 measurements, against 5 groups it is 20. It is a testing
/// convenience, not a claim that every member of a group behaves identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageGroup {
    English,
    WesternEuropean,
    Slavic,
    Cjk,
    Other,
}

/// Map a language code to its group. Unknown codes land in `Other`, which is
/// the group whose ranking must assume the least.
pub fn language_group_for_code(code: &str) -> LanguageGroup {
    match normalise_code(code).as_str() {
        "en" => LanguageGroup::English,
        "de" | "fr" | "es" | "it" | "pt" | "nl" | "ca" | "da" | "sv" | "no" | "nb" | "fi"
        | "is" | "el" | "ro" | "hu" | "et" | "lv" | "lt" | "mt" | "ga" | "cy" | "eu" | "gl" => {
            LanguageGroup::WesternEuropean
        }
        "pl" | "cs" | "sk" | "sl" | "hr" | "sr" | "bs" | "bg" | "mk" | "ru" | "uk" | "be" => {
            LanguageGroup::Slavic
        }
        "zh" | "ja" | "ko" | "yue" => LanguageGroup::Cjk,
        _ => LanguageGroup::Other,
    }
}

/// The group to rank summary models against, given every language the user
/// named. A single language uses its own group; a mixed set falls to `Other`,
/// which is the honest answer — no group's ranking describes a model that has
/// to do two families at once.
pub fn summary_language_group(languages: &[String]) -> LanguageGroup {
    let mut groups = languages.iter().map(|code| language_group_for_code(code));

    match groups.next() {
        None => LanguageGroup::Other,
        Some(first) if groups.all(|g| g == first) => first,
        Some(_) => LanguageGroup::Other,
    }
}

/// Hand the frontend Parakeet's coverage so the pickers can warn when a chosen
/// language is outside the installed engine's reach.
///
/// A command rather than a constant mirrored in TypeScript: two copies of this
/// list would drift, and the drift would be invisible until someone's meeting
/// came back as nonsense.
#[tauri::command]
pub fn parakeet_supported_languages() -> Vec<String> {
    PARAKEET_V3_LANGUAGES
        .iter()
        .map(|code| code.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parakeet_covers_exactly_the_twenty_five_languages_of_the_model_card() {
        // Spelled out rather than derived from the constant, so that editing
        // the constant cannot quietly edit the expectation with it.
        for code in [
            "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv",
            "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
        ] {
            assert!(parakeet_supports(code), "{code} should be supported");
        }

        assert_eq!(PARAKEET_V3_LANGUAGES.len(), 25);
    }

    #[test]
    fn parakeet_does_not_cover_the_languages_that_motivated_this_work() {
        for code in ["ja", "zh", "ko", "ar", "tr", "hi", "he", "th", "vi", "id"] {
            assert!(!parakeet_supports(code), "{code} must route to Whisper");
        }
    }

    #[test]
    fn unknown_and_malformed_codes_are_not_claimed_by_parakeet() {
        for code in ["", "   ", "xx", "klingon", "-", "??"] {
            assert!(!parakeet_supports(code), "{code:?} must not be claimed");
        }
    }

    #[test]
    fn locale_shaped_codes_resolve_to_their_base_language() {
        assert!(parakeet_supports("pt-BR"));
        assert!(parakeet_supports("en_US"));
        assert!(parakeet_supports("  DE  "));
        assert!(!parakeet_supports("ja-JP"));
    }

    #[test]
    fn only_the_leading_subtag_decides_support() {
        // Deliberately lenient: anything after the first subtag is a region or
        // script qualifier and cannot change which language is being spoken, so
        // an unrecognised tail is ignored rather than treated as unsupported.
        assert!(parakeet_supports("en-whatever-follows"));
        assert!(!parakeet_supports("ja-whatever-follows"));
    }

    #[test]
    fn a_single_european_language_chooses_parakeet() {
        assert_eq!(choose_engine(&codes(&["pl"])), SpeechEngine::Parakeet);
        assert_eq!(choose_engine(&codes(&["en"])), SpeechEngine::Parakeet);
    }

    #[test]
    fn a_single_language_outside_the_set_chooses_whisper() {
        assert_eq!(choose_engine(&codes(&["ja"])), SpeechEngine::Whisper);
        assert_eq!(choose_engine(&codes(&["ar"])), SpeechEngine::Whisper);
    }

    #[test]
    fn an_all_european_mix_still_chooses_parakeet() {
        assert_eq!(
            choose_engine(&codes(&["de", "fr", "nl"])),
            SpeechEngine::Parakeet
        );
    }

    #[test]
    fn one_unsupported_language_in_the_mix_forces_whisper() {
        // The case this whole change exists for: a team whose meetings are
        // mostly English would otherwise get Parakeet, and their Japanese
        // meetings would come back as nonsense.
        assert_eq!(choose_engine(&codes(&["en", "ja"])), SpeechEngine::Whisper);
    }

    #[test]
    fn declining_to_answer_chooses_whisper() {
        assert_eq!(choose_engine(&[]), SpeechEngine::Whisper);
    }

    #[test]
    fn the_whisper_provider_string_is_the_one_live_recording_accepts() {
        // Spelled out because it is the one value here that cannot be guessed:
        // audio::transcription::engine matches "localWhisper", and writing
        // "whisper" would pass onboarding and then fail at the first recording,
        // for exactly the users this whole change exists to serve.
        assert_eq!(PROVIDER_WHISPER, "localWhisper");
        assert_eq!(PROVIDER_PARAKEET, "parakeet");
    }

    #[test]
    fn engine_metadata_matches_what_the_download_step_needs() {
        assert_eq!(SpeechEngine::Parakeet.provider(), PROVIDER_PARAKEET);
        assert_eq!(SpeechEngine::Whisper.provider(), PROVIDER_WHISPER);
        assert_eq!(
            SpeechEngine::Parakeet.default_model(),
            crate::config::DEFAULT_PARAKEET_MODEL
        );
        assert_eq!(SpeechEngine::Whisper.default_model(), "large-v3-turbo-q5_0");
    }

    #[test]
    fn whisper_onboarding_model_exists_in_the_catalog_with_the_advertised_size() {
        let entry = crate::config::WHISPER_MODEL_CATALOG
            .iter()
            .find(|(name, ..)| *name == SpeechEngine::Whisper.default_model())
            .expect("the onboarding Whisper model must be a real catalog entry");

        assert_eq!(entry.2, SpeechEngine::Whisper.download_size_mb());
    }

    #[test]
    fn language_groups_sort_the_codes_the_pickers_offer() {
        assert_eq!(language_group_for_code("en"), LanguageGroup::English);
        assert_eq!(
            language_group_for_code("de"),
            LanguageGroup::WesternEuropean
        );
        assert_eq!(language_group_for_code("pl"), LanguageGroup::Slavic);
        assert_eq!(language_group_for_code("ja"), LanguageGroup::Cjk);
        assert_eq!(language_group_for_code("ar"), LanguageGroup::Other);
        assert_eq!(language_group_for_code("xx"), LanguageGroup::Other);
    }

    #[test]
    fn one_language_ranks_against_its_own_group() {
        assert_eq!(
            summary_language_group(&codes(&["pl"])),
            LanguageGroup::Slavic
        );
    }

    #[test]
    fn languages_from_one_group_keep_that_group() {
        assert_eq!(
            summary_language_group(&codes(&["pl", "cs", "uk"])),
            LanguageGroup::Slavic
        );
    }

    #[test]
    fn languages_from_different_groups_fall_back_to_other() {
        assert_eq!(
            summary_language_group(&codes(&["en", "ja"])),
            LanguageGroup::Other
        );
        assert_eq!(summary_language_group(&[]), LanguageGroup::Other);
    }
}
