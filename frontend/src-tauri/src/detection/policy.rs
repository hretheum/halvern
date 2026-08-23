//! Deciding whether the audio activity we just saw is a meeting.
//!
//! Everything here is pure and clock-free — time is passed in — so the whole
//! rule set can be exercised in unit tests without a real call, real hardware
//! or a running application.

use crate::audio::system_detector::DetectedApp;

/// Screen recorders and streaming tools. These genuinely use the microphone
/// and the speakers at the same time, so the input-and-output rule alone would
/// classify them as meetings. They have to be excluded by identifier.
pub const DEFAULT_IGNORED: &[&str] = &[
    "com.obsproject.obs-studio",
    "so.cap.desktop",
    "com.apple.QuickTimePlayerX",
    "com.telestream.screenflow9",
    "com.techsmith.camtasia2021",
    "com.reincubate.camostudio",
    "com.ecamm.EcammLive",
    "com.loom.desktop",
    "us.zoom.ringcentral",
];

/// Applications treated as a meeting on output alone. The microphone often
/// joins a second or two after the call window opens, and waiting for it would
/// clip the opening of the recording.
pub const DEFAULT_ALWAYS_MEETING: &[&str] = &[
    "us.zoom.xos",
    "com.microsoft.teams2",
    "com.microsoft.teams",
    "com.cisco.webexmeetingsapp",
    "com.hnc.Discord",
    "com.tinyspeck.slackmacgap",
];

/// Tunable behaviour of the detector.
///
/// Serialised as one JSON blob into `settings.meetingDetectionConfig`, with
/// camelCase keys so the frontend can consume it unchanged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionConfig {
    pub enabled: bool,
    pub ignored_bundle_ids: Vec<String>,
    pub always_meeting_bundle_ids: Vec<String>,
    /// How long the activity must persist before recording starts. Stops a
    /// three-second preview from producing a meeting.
    pub min_duration_seconds: u64,
    pub show_notifications: bool,

    // The four auto-stop fields carry serde defaults because the config is one
    // JSON blob in the settings table — blobs written before auto-stop existed
    // must keep parsing, and the blob format was chosen precisely to avoid a
    // migration per field.
    /// Master switch for everything below. Recording never stops on its own
    /// while this is off.
    #[serde(default = "default_auto_stop_enabled")]
    pub auto_stop_enabled: bool,
    /// How long the meeting candidate must stay absent before a stop is
    /// proposed. The counterpart of `min_duration_seconds`: one debounces the
    /// start, the other debounces the end.
    #[serde(default = "default_silence_duration_seconds")]
    pub silence_duration_seconds: u64,
    /// How long an unanswered stop proposal waits before the recording stops
    /// anyway. No answer means nobody is at the machine, which is exactly the
    /// case that produced a seven-hour recording of an empty room.
    #[serde(default = "default_confirmation_timeout_seconds")]
    pub confirmation_timeout_seconds: u64,
    /// Hard cap on recording length, in minutes. Enforced by the recording
    /// manager for every recording — including manually started ones the
    /// detector never sees.
    #[serde(default = "default_max_recording_minutes")]
    pub max_recording_minutes: u64,
}

fn default_auto_stop_enabled() -> bool {
    true
}

fn default_silence_duration_seconds() -> u64 {
    120
}

fn default_confirmation_timeout_seconds() -> u64 {
    120
}

fn default_max_recording_minutes() -> u64 {
    240
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ignored_bundle_ids: DEFAULT_IGNORED.iter().map(|s| s.to_string()).collect(),
            always_meeting_bundle_ids: DEFAULT_ALWAYS_MEETING
                .iter()
                .map(|s| s.to_string())
                .collect(),
            min_duration_seconds: 15,
            show_notifications: true,
            auto_stop_enabled: default_auto_stop_enabled(),
            silence_duration_seconds: default_silence_duration_seconds(),
            confirmation_timeout_seconds: default_confirmation_timeout_seconds(),
            max_recording_minutes: default_max_recording_minutes(),
        }
    }
}

/// Why an application was accepted as a meeting candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateReason {
    /// Listed in `always_meeting_bundle_ids`.
    KnownMeetingApp,
    /// Rendering and capturing audio at the same time.
    InputAndOutput,
}

impl CandidateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CandidateReason::KnownMeetingApp => "known meeting application",
            CandidateReason::InputAndOutput => "using microphone and speakers at once",
        }
    }
}

/// An application that qualifies as a meeting, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub key: String,
    pub name: String,
    pub reason: CandidateReason,
}

/// Trailing words macOS appends to helper process names. Observed on a live
/// Teams call, which reports "Microsoft Teams ModuleHost" and
/// "Microsoft Teams WebView" rather than "Microsoft Teams".
const HELPER_NAME_SUFFIXES: &[&str] = &[
    "ModuleHost",
    "WebView",
    "Helper (Renderer)",
    "Helper (GPU)",
    "Helper (Plugin)",
    "Helper",
    "Renderer",
];

/// Strips a helper suffix so recordings are named after the application rather
/// than one of its subprocesses.
pub fn friendly_name(raw: &str) -> String {
    let trimmed = raw.trim();
    for suffix in HELPER_NAME_SUFFIXES {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            let stripped = stripped.trim();
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// True when `id` is `entry` itself or one of its subprocesses.
///
/// Audio does not come from the main application bundle. A Teams call surfaces
/// as `com.microsoft.teams2.modulehost`, so exact matching against
/// `com.microsoft.teams2` would never fire.
pub(crate) fn matches_bundle(id: &str, entry: &str) -> bool {
    id == entry || id.strip_prefix(entry).is_some_and(|rest| rest.starts_with('.'))
}

/// Groups subprocesses of one application under a single key.
///
/// Used for debouncing: `…teams2.modulehost` and `…teams2.helper` must count as
/// the same candidate, otherwise the pair alternating would reset the timer and
/// a real call might never cross the threshold.
fn bundle_group(id: &str) -> String {
    id.split('.').take(3).collect::<Vec<_>>().join(".")
}

/// Picks the best meeting candidate from what the detector reported.
///
/// A known meeting application always wins over a generic input-and-output
/// match, so that a Zoom call with a browser playing music in the background
/// is attributed to Zoom. Within either group, a process that uses the
/// microphone wins over one that only renders, because that is the process
/// actually carrying the conversation.
pub fn pick_candidate(apps: &[DetectedApp], cfg: &DetectionConfig) -> Option<Candidate> {
    let matched_entry = |app: &DetectedApp, list: &[String]| -> Option<String> {
        let id = app.bundle_id.as_deref()?;
        list.iter().find(|entry| matches_bundle(id, entry)).cloned()
    };

    let key_of = |app: &DetectedApp, known_entry: Option<&String>| -> String {
        match (known_entry, app.bundle_id.as_deref()) {
            // Group under the configured entry, so every helper of one app agrees.
            (Some(entry), _) => entry.clone(),
            (None, Some(id)) => bundle_group(id),
            (None, None) => format!("name:{}", app.name),
        }
    };

    let mut known: Option<(Candidate, bool)> = None;
    let mut generic: Option<(Candidate, bool)> = None;

    for app in apps {
        if matched_entry(app, &cfg.ignored_bundle_ids).is_some() {
            continue;
        }

        let known_entry = matched_entry(app, &cfg.always_meeting_bundle_ids);

        if known_entry.is_some() && (app.uses_output || app.uses_input) {
            let candidate = Candidate {
                key: key_of(app, known_entry.as_ref()),
                name: friendly_name(&app.name),
                reason: CandidateReason::KnownMeetingApp,
            };
            // Prefer whichever helper actually holds the microphone.
            if known.as_ref().is_none_or(|(_, has_input)| !has_input) {
                known = Some((candidate, app.uses_input));
            }
            continue;
        }

        if app.uses_output && app.uses_input && generic.is_none() {
            generic = Some((
                Candidate {
                    key: key_of(app, None),
                    name: friendly_name(&app.name),
                    reason: CandidateReason::InputAndOutput,
                },
                true,
            ));
        }
    }

    known.or(generic).map(|(candidate, _)| candidate)
}

/// Tracks how long the current candidate has been present — and, while a
/// recording runs, how long it has been absent.
///
/// Time is supplied by the caller as monotonic seconds, so tests do not sleep.
#[derive(Debug, Default)]
pub struct Debouncer {
    current: Option<(String, u64)>,
    /// While recording: when the meeting candidate was first observed absent.
    /// `None` means it is present, or was present at the last observation.
    absent_since: Option<u64>,
    /// True once a stop has been proposed for the current absence episode.
    /// Prevents re-asking every observation cycle: a declined proposal stays
    /// declined until the candidate reappears and a fresh episode begins.
    stop_proposed: bool,
}

impl Debouncer {
    /// Feeds one observation and reports whether the candidate has now been
    /// present long enough to act on.
    ///
    /// Returns `true` exactly once per sustained candidate: after firing, the
    /// entry is cleared so that a still-running call does not retrigger.
    pub fn observe(
        &mut self,
        candidate: Option<&Candidate>,
        now_secs: u64,
        min_duration_seconds: u64,
    ) -> bool {
        let Some(candidate) = candidate else {
            self.current = None;
            return false;
        };

        match &self.current {
            Some((key, first_seen)) if key == &candidate.key => {
                if now_secs.saturating_sub(*first_seen) >= min_duration_seconds {
                    self.current = None;
                    true
                } else {
                    false
                }
            }
            _ => {
                // New or changed candidate: restart the clock. Zero threshold
                // means fire immediately.
                if min_duration_seconds == 0 {
                    self.current = None;
                    true
                } else {
                    self.current = Some((candidate.key.clone(), now_secs));
                    false
                }
            }
        }
    }

    /// Forgets any pending candidate, e.g. when audio stopped entirely.
    ///
    /// Deliberately leaves absence tracking alone: `evaluate` calls this on
    /// every observation while a recording runs, and wiping the absence clock
    /// each time would mean a stop could never be proposed.
    pub fn reset(&mut self) {
        self.current = None;
    }

    /// Feeds one observation made *while a recording runs* and reports whether
    /// the candidate has now been absent long enough to propose stopping.
    ///
    /// Fires exactly once per continuous absence episode. A single flap — the
    /// device blinking off for one observation and back — resets the clock the
    /// moment the candidate reappears, so a live call is never interrupted by
    /// the flapping that `on_audio_stopped` documents.
    pub fn observe_absence(
        &mut self,
        candidate_present: bool,
        now_secs: u64,
        silence_duration_seconds: u64,
    ) -> bool {
        if candidate_present {
            // The episode is over; the next absence starts its own clock and
            // may propose again.
            self.absent_since = None;
            self.stop_proposed = false;
            return false;
        }

        let since = *self.absent_since.get_or_insert(now_secs);

        if self.stop_proposed {
            // Already proposed for this episode. Whatever the answer was, the
            // service layer owns the follow-up; the policy stays quiet.
            return false;
        }

        if now_secs.saturating_sub(since) >= silence_duration_seconds {
            self.stop_proposed = true;
            true
        } else {
            false
        }
    }

    /// Forgets any absence episode. Called when no recording is running, so a
    /// stale episode cannot fire into the next recording.
    pub fn clear_absence(&mut self) {
        self.absent_since = None;
        self.stop_proposed = false;
    }
}

/// Tracks the one stop proposal that may be outstanding at a time.
///
/// Pure state so the expiry rules are testable without a clock or an app
/// handle: the service holds one of these behind a mutex, opens a proposal
/// when `Decision::Stop` arrives, and resolves it from exactly one of three
/// places — the user's answer, the confirmation timeout, or the meeting
/// audibly resuming.
#[derive(Debug, Default)]
pub struct ProposalLedger {
    pending: Option<u64>,
    next_id: u64,
    /// True while this proposal is the one that paused capture. A recording
    /// the user had already paused by hand is not ours to resume, so the flag
    /// stays false and answering the proposal leaves their pause standing.
    owns_pause: bool,
}

impl ProposalLedger {
    /// Opens a new proposal, superseding any previous one, and returns its id.
    pub fn open(&mut self) -> u64 {
        self.next_id += 1;
        self.pending = Some(self.next_id);
        // A claim belongs to the proposal that made it. Clearing here keeps a
        // claim from an abandoned episode out of the new one.
        self.owns_pause = false;
        self.next_id
    }

    /// Records that this proposal paused capture, so resolving it may resume.
    pub fn claim_pause(&mut self) {
        self.owns_pause = true;
    }

    /// Reports whether this proposal paused capture, and spends the claim.
    ///
    /// Every resolution path calls this exactly once: the paths that keep
    /// recording resume on `true`, the paths that stop ignore the answer —
    /// stopping clears the pause anyway. Spending it means no second resume
    /// can fire for the same episode.
    pub fn take_pause_claim(&mut self) -> bool {
        std::mem::take(&mut self.owns_pause)
    }

    /// Closes proposal `id` if it is still the pending one.
    ///
    /// Returns `true` only in that case. A timeout task calls this with the id
    /// it was armed for, so a timeout belonging to an already-answered or
    /// superseded proposal stands down instead of stopping a recording it no
    /// longer speaks for.
    pub fn resolve(&mut self, id: u64) -> bool {
        if self.pending == Some(id) {
            self.pending = None;
            true
        } else {
            false
        }
    }

    /// Closes whatever proposal is pending. Returns `true` when one was.
    ///
    /// Used by the user's answer (which does not know the id) and by the
    /// meeting-resumed path.
    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Resolves proposal `id` with the user's answer.
    ///
    /// The dialog is opened carrying the id of the proposal it was shown for,
    /// so an answer that arrives after that proposal was already superseded —
    /// timeout, meeting resumed, or a fresh proposal opened in between — is
    /// stale and must change nothing, including a pause claim that by then
    /// belongs to a later proposal it was never shown.
    pub fn answer(&mut self, id: u64) -> ProposalResolution {
        let was_pending = self.resolve(id);
        // Only the answer that actually resolves the proposal spends the
        // claim; a stale dialog must not resume anything.
        let owns_pause = was_pending && self.take_pause_claim();
        ProposalResolution { was_pending, owns_pause }
    }

    /// Withdraws the pending proposal because the meeting audibly resumed.
    ///
    /// Withdrawal happens only when a proposal stands *and* a meeting candidate
    /// is present again. Silence with a pending proposal leaves it standing —
    /// its confirmation timeout still speaks for it.
    pub fn withdraw_for_resumed_meeting(&mut self, candidate_present: bool) -> ProposalResolution {
        if self.is_pending() && candidate_present && self.cancel() {
            ProposalResolution { was_pending: true, owns_pause: self.take_pause_claim() }
        } else {
            ProposalResolution { was_pending: false, owns_pause: false }
        }
    }

    /// Resolves proposal `id` because its confirmation timeout expired.
    ///
    /// Returns `true` only when `id` is still the pending proposal; a timeout
    /// armed for an already-answered or superseded proposal stands down. On
    /// expiry the pause claim is spent rather than acted on — stopping clears
    /// the pause by itself.
    pub fn expire(&mut self, id: u64) -> bool {
        let resolved = self.resolve(id);
        if resolved {
            self.take_pause_claim();
        }
        resolved
    }
}

/// How a resolution path left the ledger, and what the caller now owes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProposalResolution {
    /// Whether a proposal was actually pending. A stale answer or a resumed
    /// meeting without a pending proposal resolves nothing.
    pub was_pending: bool,
    /// Whether this resolution owns the capture pause and must resume it.
    pub owns_pause: bool,
}

/// What the caller should do with an observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Begin recording for this application.
    ///
    /// `key` is the candidate's grouping key — the configured bundle entry, or
    /// the first three components of the identifier. The capture layer uses it
    /// to tap only this application's processes, so that whatever else the
    /// machine plays during the meeting stays out of the recording.
    Start {
        name: String,
        key: String,
        reason: CandidateReason,
    },
    /// A candidate exists but has not persisted long enough yet.
    Waiting,
    /// Nothing here looks like a meeting.
    Ignore,
    /// Detection is switched off in settings.
    Disabled,
    /// A recording is already running; leave it alone.
    AlreadyRecording,
    /// The meeting candidate has been absent for the configured window while a
    /// recording runs. **A proposal, not an order**: the service asks the user
    /// and only stops on confirmation or after the confirmation times out.
    Stop,
}

/// Full decision for one detector event.
pub fn evaluate(
    apps: &[DetectedApp],
    cfg: &DetectionConfig,
    debouncer: &mut Debouncer,
    now_secs: u64,
    is_recording: bool,
) -> Decision {
    if is_recording {
        // Checked before the `enabled` switch on purpose: `enabled` governs
        // *starting* recordings, while auto-stop has its own switch and serves
        // manually started recordings too — those deserve the same
        // end-of-meeting watch whether or not start detection is on.
        //
        // Do not let a running recording be restarted, and do not accumulate
        // debounce state behind it.
        debouncer.reset();

        if !cfg.auto_stop_enabled {
            // Also drop any half-built absence episode, so switching the
            // feature back on does not fire from stale state.
            debouncer.clear_absence();
            return Decision::AlreadyRecording;
        }

        let candidate = pick_candidate(apps, cfg);
        if debouncer.observe_absence(candidate.is_some(), now_secs, cfg.silence_duration_seconds) {
            return Decision::Stop;
        }
        return Decision::AlreadyRecording;
    }

    if !cfg.enabled {
        return Decision::Disabled;
    }

    // Not recording: any absence episode belongs to a recording that no longer
    // exists and must not fire into the next one.
    debouncer.clear_absence();

    let candidate = pick_candidate(apps, cfg);

    if debouncer.observe(candidate.as_ref(), now_secs, cfg.min_duration_seconds) {
        let candidate = candidate.expect("debouncer only fires with a candidate");
        Decision::Start {
            name: candidate.name,
            key: candidate.key,
            reason: candidate.reason,
        }
    } else if candidate.is_some() {
        Decision::Waiting
    } else {
        Decision::Ignore
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(bundle: Option<&str>, name: &str, input: bool, output: bool) -> DetectedApp {
        DetectedApp {
            bundle_id: bundle.map(str::to_string),
            name: name.to_string(),
            uses_input: input,
            uses_output: output,
        }
    }

    fn cfg() -> DetectionConfig {
        DetectionConfig {
            enabled: true,
            ..DetectionConfig::default()
        }
    }

    #[test]
    fn music_playback_alone_is_not_a_meeting() {
        let apps = vec![app(Some("com.spotify.client"), "Spotify", false, true)];
        assert!(pick_candidate(&apps, &cfg()).is_none());
    }

    #[test]
    fn microphone_alone_is_not_a_meeting() {
        // A dictation tool captures without rendering.
        let apps = vec![app(Some("com.example.dictate"), "Dictate", true, false)];
        assert!(pick_candidate(&apps, &cfg()).is_none());
    }

    #[test]
    fn output_and_input_together_is_a_meeting() {
        let apps = vec![app(Some("com.google.Chrome"), "Google Chrome", true, true)];
        let found = pick_candidate(&apps, &cfg()).expect("a candidate was expected");
        assert_eq!(found.reason, CandidateReason::InputAndOutput);
        assert_eq!(found.key, "com.google.Chrome");
    }

    #[test]
    fn known_meeting_app_qualifies_on_output_alone() {
        // Zoom often opens audio output before the microphone joins.
        let apps = vec![app(Some("us.zoom.xos"), "zoom.us", false, true)];
        let found = pick_candidate(&apps, &cfg()).expect("a candidate was expected");
        assert_eq!(found.reason, CandidateReason::KnownMeetingApp);
    }

    #[test]
    fn screen_recorders_are_excluded_despite_using_both_directions() {
        let apps = vec![app(Some("com.obsproject.obs-studio"), "OBS", true, true)];
        assert!(
            pick_candidate(&apps, &cfg()).is_none(),
            "a screen recorder uses both directions but is not a meeting"
        );
    }

    #[test]
    fn known_meeting_app_wins_over_a_generic_match() {
        let apps = vec![
            app(Some("com.google.Chrome"), "Google Chrome", true, true),
            app(Some("us.zoom.xos"), "zoom.us", true, true),
        ];
        let found = pick_candidate(&apps, &cfg()).unwrap();
        assert_eq!(found.name, "zoom.us");
        assert_eq!(found.reason, CandidateReason::KnownMeetingApp);
    }

    #[test]
    fn app_without_bundle_id_falls_back_to_its_name() {
        let apps = vec![app(None, "Weird Daemon", true, true)];
        let found = pick_candidate(&apps, &cfg()).unwrap();
        assert_eq!(found.key, "name:Weird Daemon");
    }

    /// Exactly what a live Teams call reported on macOS 26.5: two helper
    /// processes, and the main bundle identifier nowhere to be seen.
    fn real_teams_call() -> Vec<DetectedApp> {
        vec![
            app(
                Some("com.microsoft.teams2.modulehost"),
                "Microsoft Teams ModuleHost",
                true,
                true,
            ),
            app(
                Some("com.microsoft.teams2.helper"),
                "Microsoft Teams WebView",
                false,
                true,
            ),
        ]
    }

    #[test]
    fn teams_helpers_match_the_configured_parent_bundle() {
        let found = pick_candidate(&real_teams_call(), &cfg()).expect("Teams is a meeting");
        assert_eq!(
            found.reason,
            CandidateReason::KnownMeetingApp,
            "prefix matching must recognise the helper process"
        );
        assert_eq!(found.key, "com.microsoft.teams2");
        assert_eq!(found.name, "Microsoft Teams", "the name drops the helper-process suffix");
    }

    #[test]
    fn teams_helpers_share_one_debounce_key() {
        // Both helpers must group together, otherwise the pair alternating
        // would keep resetting the timer and a real call would never fire.
        let apps = real_teams_call();
        let only_webview = vec![apps[1].clone()];

        let a = pick_candidate(&apps, &cfg()).unwrap();
        let b = pick_candidate(&only_webview, &cfg()).unwrap();
        assert_eq!(a.key, b.key);
    }

    #[test]
    fn the_helper_holding_the_microphone_is_preferred() {
        // Reversed order: the speakers-only helper comes first in the list.
        let mut apps = real_teams_call();
        apps.reverse();
        let found = pick_candidate(&apps, &cfg()).unwrap();
        assert_eq!(found.name, "Microsoft Teams");
    }

    #[test]
    fn prefix_matching_does_not_leak_across_similar_identifiers() {
        // `com.microsoft.teams` must not swallow `com.microsoft.teams2.*`,
        // and a longer unrelated identifier must not match either.
        assert!(matches_bundle("com.microsoft.teams2.modulehost", "com.microsoft.teams2"));
        assert!(!matches_bundle("com.microsoft.teams2.modulehost", "com.microsoft.teams"));
        assert!(!matches_bundle("com.evil.zoom.xos", "us.zoom.xos"));
        assert!(matches_bundle("us.zoom.xos", "us.zoom.xos"));
    }

    #[test]
    fn ignored_list_also_covers_subprocesses() {
        let apps = vec![app(
            Some("com.obsproject.obs-studio.helper"),
            "OBS Helper",
            true,
            true,
        )];
        assert!(
            pick_candidate(&apps, &cfg()).is_none(),
            "the recorder's helper process must be excluded too"
        );
    }

    #[test]
    fn friendly_name_strips_helper_suffixes() {
        assert_eq!(friendly_name("Microsoft Teams ModuleHost"), "Microsoft Teams");
        assert_eq!(friendly_name("Microsoft Teams WebView"), "Microsoft Teams");
        assert_eq!(friendly_name("Google Chrome Helper (Renderer)"), "Google Chrome");
        assert_eq!(friendly_name("zoom.us"), "zoom.us", "an ordinary name passes through unchanged");
        assert_eq!(friendly_name("WebView"), "WebView", "the name itself must not disappear");
    }

    #[test]
    fn generic_matches_group_subprocesses_too() {
        let apps = vec![app(Some("com.google.Chrome.helper"), "Google Chrome Helper", true, true)];
        let found = pick_candidate(&apps, &cfg()).unwrap();
        assert_eq!(found.key, "com.google.Chrome");
        assert_eq!(found.reason, CandidateReason::InputAndOutput);
    }

    #[test]
    fn debouncer_waits_for_the_configured_duration() {
        let mut d = Debouncer::default();
        let c = Candidate {
            key: "us.zoom.xos".to_string(),
            name: "zoom.us".to_string(),
            reason: CandidateReason::KnownMeetingApp,
        };

        assert!(!d.observe(Some(&c), 100, 15), "the first observation only starts the clock");
        assert!(!d.observe(Some(&c), 110, 15), "10 s is not enough");
        assert!(d.observe(Some(&c), 115, 15), "15 s is enough");
        assert!(
            !d.observe(Some(&c), 200, 15),
            "once fired, it must not fire again for an ongoing call"
        );
    }

    #[test]
    fn debouncer_restarts_when_the_candidate_changes() {
        let mut d = Debouncer::default();
        let zoom = Candidate { key: "us.zoom.xos".into(), name: "zoom.us".into(), reason: CandidateReason::KnownMeetingApp };
        let teams = Candidate { key: "com.microsoft.teams2".into(), name: "Teams".into(), reason: CandidateReason::KnownMeetingApp };

        assert!(!d.observe(Some(&zoom), 100, 15));
        assert!(!d.observe(Some(&teams), 110, 15), "a different candidate restarts the clock");
        assert!(!d.observe(Some(&teams), 120, 15), "counted from 110, so 10 s is not enough");
        assert!(d.observe(Some(&teams), 125, 15));
    }

    #[test]
    fn debouncer_forgets_when_audio_stops() {
        let mut d = Debouncer::default();
        let c = Candidate { key: "us.zoom.xos".into(), name: "zoom.us".into(), reason: CandidateReason::KnownMeetingApp };

        assert!(!d.observe(Some(&c), 100, 15));
        assert!(!d.observe(None, 105, 15), "silence forgets the candidate");
        assert!(!d.observe(Some(&c), 110, 15), "the clock counts from scratch");
        assert!(d.observe(Some(&c), 125, 15));
    }

    #[test]
    fn zero_duration_fires_immediately() {
        let mut d = Debouncer::default();
        let c = Candidate { key: "x".into(), name: "X".into(), reason: CandidateReason::InputAndOutput };
        assert!(d.observe(Some(&c), 0, 0));
    }

    #[test]
    fn evaluate_respects_the_enabled_switch() {
        let mut d = Debouncer::default();
        let apps = vec![app(Some("us.zoom.xos"), "zoom.us", true, true)];
        let disabled = DetectionConfig { enabled: false, ..DetectionConfig::default() };

        assert_eq!(
            evaluate(&apps, &disabled, &mut d, 0, false),
            Decision::Disabled
        );
    }

    #[test]
    fn evaluate_leaves_a_running_recording_alone() {
        let mut d = Debouncer::default();
        let apps = vec![app(Some("us.zoom.xos"), "zoom.us", true, true)];

        assert_eq!(
            evaluate(&apps, &cfg(), &mut d, 0, true),
            Decision::AlreadyRecording
        );
    }

    #[test]
    fn evaluate_walks_from_waiting_to_start() {
        let mut d = Debouncer::default();
        let apps = vec![app(Some("us.zoom.xos"), "zoom.us", true, true)];
        let c = cfg();

        assert_eq!(evaluate(&apps, &c, &mut d, 100, false), Decision::Waiting);
        assert_eq!(evaluate(&apps, &c, &mut d, 105, false), Decision::Waiting);

        match evaluate(&apps, &c, &mut d, 115, false) {
            Decision::Start { name, key, reason } => {
                assert_eq!(name, "zoom.us");
                assert_eq!(
                    key, "us.zoom.xos",
                    "the key travels to the capture layer, which taps this application's processes"
                );
                assert_eq!(reason, CandidateReason::KnownMeetingApp);
            }
            other => panic!("expected Start, got {:?}", other),
        }
    }

    #[test]
    fn evaluate_ignores_pure_playback() {
        let mut d = Debouncer::default();
        let apps = vec![app(Some("com.spotify.client"), "Spotify", false, true)];
        assert_eq!(evaluate(&apps, &cfg(), &mut d, 100, false), Decision::Ignore);
    }

    // Auto-stop ---------------------------------------------------------------
    //
    // Defaults in play: silence_duration_seconds = 120.

    fn meeting() -> Vec<DetectedApp> {
        vec![app(Some("us.zoom.xos"), "zoom.us", true, true)]
    }

    #[test]
    fn a_brief_absence_while_recording_does_not_propose_stopping() {
        let mut d = Debouncer::default();
        let c = cfg();
        let silence: Vec<DetectedApp> = vec![];

        assert_eq!(evaluate(&meeting(), &c, &mut d, 100, true), Decision::AlreadyRecording);
        assert_eq!(
            evaluate(&silence, &c, &mut d, 110, true),
            Decision::AlreadyRecording,
            "10 s of absence is a blip, not the end of the meeting"
        );
        assert_eq!(
            evaluate(&meeting(), &c, &mut d, 120, true),
            Decision::AlreadyRecording,
            "the candidate came back, so the episode is over"
        );
        assert_eq!(
            evaluate(&silence, &c, &mut d, 200, true),
            Decision::AlreadyRecording,
            "a fresh absence starts its own clock rather than inheriting the old one"
        );
        assert_eq!(
            evaluate(&silence, &c, &mut d, 319, true),
            Decision::AlreadyRecording,
            "119 s into the fresh episode is still below the threshold"
        );
        assert_eq!(
            evaluate(&silence, &c, &mut d, 320, true),
            Decision::Stop,
            "120 s of continuous absence crosses it"
        );
    }

    #[test]
    fn a_sustained_absence_proposes_stopping_exactly_once() {
        let mut d = Debouncer::default();
        let c = cfg();
        let silence: Vec<DetectedApp> = vec![];

        assert_eq!(evaluate(&silence, &c, &mut d, 0, true), Decision::AlreadyRecording);
        assert_eq!(evaluate(&silence, &c, &mut d, 120, true), Decision::Stop);
        assert_eq!(
            evaluate(&silence, &c, &mut d, 240, true),
            Decision::AlreadyRecording,
            "the episode already proposed once; continued silence must not nag"
        );

        // The meeting resumes, then ends again: a new episode may propose anew.
        assert_eq!(evaluate(&meeting(), &c, &mut d, 300, true), Decision::AlreadyRecording);
        assert_eq!(evaluate(&silence, &c, &mut d, 310, true), Decision::AlreadyRecording);
        assert_eq!(evaluate(&silence, &c, &mut d, 430, true), Decision::Stop);
    }

    #[test]
    fn auto_stop_can_be_switched_off() {
        let mut d = Debouncer::default();
        let c = DetectionConfig {
            enabled: true,
            auto_stop_enabled: false,
            ..DetectionConfig::default()
        };
        let silence: Vec<DetectedApp> = vec![];

        for t in [0u64, 200, 4_000, 100_000] {
            assert_eq!(
                evaluate(&silence, &c, &mut d, t, true),
                Decision::AlreadyRecording,
                "with auto-stop off, no amount of silence proposes stopping"
            );
        }
    }

    #[test]
    fn absence_does_not_leak_between_recordings() {
        let mut d = Debouncer::default();
        let c = cfg();
        let silence: Vec<DetectedApp> = vec![];

        // 119 s of absence in the first recording — just short of firing.
        assert_eq!(evaluate(&silence, &c, &mut d, 0, true), Decision::AlreadyRecording);
        assert_eq!(evaluate(&silence, &c, &mut d, 119, true), Decision::AlreadyRecording);

        // The recording stops; an observation arrives while idle.
        assert_eq!(evaluate(&silence, &c, &mut d, 125, false), Decision::Ignore);

        // A new recording starts. The old 119 s must not count towards it.
        assert_eq!(
            evaluate(&silence, &c, &mut d, 130, true),
            Decision::AlreadyRecording,
            "the new recording starts a fresh absence clock"
        );
        assert_eq!(evaluate(&silence, &c, &mut d, 249, true), Decision::AlreadyRecording);
        assert_eq!(evaluate(&silence, &c, &mut d, 250, true), Decision::Stop);
    }

    #[test]
    fn auto_stop_works_even_with_start_detection_disabled() {
        // A manual recording with the detector switched off still deserves the
        // end-of-meeting watch. Only auto_stop_enabled governs this path.
        let mut d = Debouncer::default();
        let c = DetectionConfig::default(); // enabled: false, auto_stop_enabled: true
        let silence: Vec<DetectedApp> = vec![];

        assert_eq!(evaluate(&silence, &c, &mut d, 0, true), Decision::AlreadyRecording);
        assert_eq!(evaluate(&silence, &c, &mut d, 120, true), Decision::Stop);
    }

    // ProposalLedger ----------------------------------------------------------

    #[test]
    fn an_unanswered_proposal_expires() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();
        assert!(
            ledger.resolve(id),
            "no answer arrived, so the timeout resolves it and the stop proceeds"
        );
        assert!(!ledger.is_pending());
    }

    #[test]
    fn a_declined_proposal_does_not_stop_on_timeout() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();
        assert!(ledger.cancel(), "the user answered 'keep recording'");
        assert!(
            !ledger.resolve(id),
            "the timeout for a declined proposal must stand down"
        );
    }

    #[test]
    fn a_superseded_proposal_cannot_fire_its_old_timeout() {
        let mut ledger = ProposalLedger::default();
        let first = ledger.open();
        let second = ledger.open();
        assert!(!ledger.resolve(first), "the old timeout no longer speaks for anything");
        assert!(ledger.resolve(second), "the current one does");
    }

    #[test]
    fn cancel_resolves_exactly_once() {
        let mut ledger = ProposalLedger::default();
        ledger.open();
        assert!(ledger.cancel());
        assert!(!ledger.cancel(), "a second cancel finds nothing pending");
    }

    #[test]
    fn a_proposal_that_muted_capture_owns_the_resume() {
        let mut ledger = ProposalLedger::default();
        ledger.open();
        ledger.claim_pause();
        assert!(ledger.take_pause_claim(), "the proposal paused, so it may resume");
        assert!(
            !ledger.take_pause_claim(),
            "the claim is spent; a second resume must not fire"
        );
    }

    #[test]
    fn a_proposal_that_found_capture_already_paused_does_not_resume() {
        // The user had pressed pause by hand before the proposal opened.
        // Answering it must leave their pause alone.
        let mut ledger = ProposalLedger::default();
        ledger.open();
        assert!(
            !ledger.take_pause_claim(),
            "without a claim, answering must not resume someone else's pause"
        );
    }

    #[test]
    fn a_new_proposal_does_not_inherit_the_previous_claim() {
        let mut ledger = ProposalLedger::default();
        ledger.open();
        ledger.claim_pause();

        // The recording stopped and a new one began without the claim ever
        // being taken. The fresh proposal starts from nothing.
        ledger.open();
        assert!(
            !ledger.take_pause_claim(),
            "a stale claim must not resume a recording this proposal never paused"
        );
    }

    // Resolution paths ---------------------------------------------------------
    //
    // Characterization tests written against the sequences service.rs performed
    // inline before the extraction; the assertions freeze that behaviour.

    #[test]
    fn an_answer_resolves_the_pending_proposal_and_takes_its_claim() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();
        ledger.claim_pause();

        let outcome = ledger.answer(id);
        assert!(outcome.was_pending);
        assert!(outcome.owns_pause, "this proposal paused capture, so its answer resumes it");
        assert!(!ledger.is_pending());
    }

    #[test]
    fn an_answer_to_a_proposal_without_a_claim_does_not_resume() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();

        let outcome = ledger.answer(id);
        assert!(outcome.was_pending);
        assert!(!outcome.owns_pause, "capture was paused by hand, not by this proposal");
    }

    #[test]
    fn a_stale_answer_resolves_nothing_and_spends_nothing() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();
        ledger.claim_pause();
        assert!(ledger.expire(id), "the timeout got there first");

        let outcome = ledger.answer(id);
        assert!(!outcome.was_pending, "the dialog was stale");
        assert!(!outcome.owns_pause, "a stale dialog must not resume anything");
    }

    #[test]
    fn an_answer_carrying_a_superseded_id_leaves_the_current_proposal_alone() {
        // Fixed behaviour: the dialog carries the id of the proposal it was
        // shown for. A late answer to a proposal that has since been
        // superseded by a fresh one must not close — or resume the pause of
        // — a proposal the user never saw.
        let mut ledger = ProposalLedger::default();
        let first = ledger.open();
        assert!(ledger.cancel());
        ledger.open();
        ledger.claim_pause();

        let outcome = ledger.answer(first);
        assert!(!outcome.was_pending, "the id belongs to a proposal that is no longer pending");
        assert!(!outcome.owns_pause);
        assert!(ledger.is_pending(), "the current proposal is untouched");
        assert!(ledger.take_pause_claim(), "and so is its claim");
    }

    #[test]
    fn a_resumed_meeting_withdraws_the_proposal_and_takes_its_claim() {
        let mut ledger = ProposalLedger::default();
        ledger.open();
        ledger.claim_pause();

        let outcome = ledger.withdraw_for_resumed_meeting(true);
        assert!(outcome.was_pending);
        assert!(outcome.owns_pause, "the withdrawal resumes the capture this proposal paused");
        assert!(!ledger.is_pending());
    }

    #[test]
    fn continued_silence_leaves_the_proposal_standing() {
        let mut ledger = ProposalLedger::default();
        ledger.open();
        ledger.claim_pause();

        let outcome = ledger.withdraw_for_resumed_meeting(false);
        assert!(!outcome.was_pending, "no candidate returned, nothing to withdraw");
        assert!(!outcome.owns_pause);
        assert!(ledger.is_pending(), "the confirmation timeout still speaks for it");
        assert!(ledger.take_pause_claim(), "and its claim is untouched");
    }

    #[test]
    fn a_resumed_meeting_without_a_pending_proposal_does_nothing() {
        let mut ledger = ProposalLedger::default();
        let outcome = ledger.withdraw_for_resumed_meeting(true);
        assert!(!outcome.was_pending);
        assert!(!outcome.owns_pause);
    }

    #[test]
    fn expiry_of_the_current_proposal_resolves_and_spends_the_claim() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();
        ledger.claim_pause();

        assert!(ledger.expire(id));
        assert!(!ledger.is_pending());
        assert!(
            !ledger.take_pause_claim(),
            "expiry spends the claim without acting on it — stopping clears the pause itself"
        );
    }

    #[test]
    fn expiry_of_a_superseded_proposal_stands_down_and_spends_nothing() {
        let mut ledger = ProposalLedger::default();
        let first = ledger.open();
        ledger.open();
        ledger.claim_pause();

        assert!(!ledger.expire(first), "the old timeout no longer speaks for anything");
        assert!(ledger.is_pending(), "the newer proposal is untouched");
        assert!(ledger.take_pause_claim(), "and so is its claim");
    }

    #[test]
    fn expiry_after_an_answer_stands_down() {
        let mut ledger = ProposalLedger::default();
        let id = ledger.open();
        assert!(ledger.answer(id).was_pending);
        assert!(!ledger.expire(id), "answered first, so the timeout resolves nothing");
    }

    #[test]
    fn stored_config_from_before_auto_stop_still_parses() {
        // A settings blob exactly as written before the auto-stop fields
        // existed. It must parse, and the new fields must take their defaults —
        // the blob format was chosen to avoid a migration per field.
        let json = r#"{
            "enabled": true,
            "ignoredBundleIds": [],
            "alwaysMeetingBundleIds": [],
            "minDurationSeconds": 15,
            "showNotifications": true
        }"#;

        let parsed: DetectionConfig = serde_json::from_str(json).expect("old blob must parse");
        assert!(parsed.auto_stop_enabled);
        assert_eq!(parsed.silence_duration_seconds, 120);
        assert_eq!(parsed.confirmation_timeout_seconds, 120);
        assert_eq!(parsed.max_recording_minutes, 240);
    }
}
