//! Reading the system calendar so a recording can be named after its meeting.
//!
//! # Why this route and not Microsoft Graph
//!
//! The tenant blocks a user from consenting to `Calendars.Read` on their own —
//! measured in Graph Explorer, which came back with
//! `Scope consent failed — access_denied`. Since the block hit a Microsoft app with
//! a verified publisher, our own registration would fare no better.
//!
//! An Exchange account added in System Settings works regardless, because Apple Mail
//! sits on Microsoft's exception list (`microsoft-user-allow-default-consent-apps`).
//! This is not a way around the policy but a path Microsoft left open.
//!
//! # Why the match happens when recording starts
//!
//! The name is set **before** the recording folder exists
//! (`recording_commands` → `set_meeting_name` → `start_accumulation` →
//! `initialize_meeting_folder`). Substituting the title at that moment means the
//! folder, the metadata, the database and the export all pick it up on their own,
//! with nothing to rename afterwards.
//!
//! # The filters, each one verified against a real calendar
//!
//! The `tools/calendar-probe` probe walked 16 recordings and showed that without
//! these three filters the match goes wrong on real data:
//!
//! 1. **all-day events** — birthdays and holidays from personal calendars
//! 2. **cancelled events** — checked through `status()`, not by a title prefix, since
//!    that prefix is localised ("Canceled" / "Anulowano") and parsing it would be brittle
//! 3. **duplicates** — by server-side identifier, because the same series arrives from
//!    several sources with identical title and time

#[cfg(target_os = "macos")]
mod macos;

/// A calendar event matched to the recording that is starting.
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    /// Event title. Replaces names such as "Microsoft Teams — auto".
    pub title: String,
    /// Attendee names, where the calendar exposes them.
    pub participants: Vec<String>,
    /// The invitation body. On the calendar this was tested against it often runs to
    /// two or three thousand characters and carries the purpose of the meeting, so it
    /// is passed to the summary prompt.
    pub agenda: Option<String>,
    /// Server-side identifier, which lets the same event be recognised later.
    pub external_id: Option<String>,
}

/// Looks for a meeting happening right now.
///
/// Returns `None` when calendar access is missing, nothing matches, or the platform is
/// not macOS. **A missing match is never an error** — the caller then uses the name it
/// would have used anyway, so this integration can only improve the outcome, never
/// worsen it.
pub fn find_current_event() -> Option<CalendarEvent> {
    #[cfg(target_os = "macos")]
    {
        macos::find_current_event()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

// Access is requested by `find_current_event` itself, when it finds the "not determined
// yet" state. There is deliberately no separate public function for asking from the
// outside: nobody would call it, and this project already has five cases of code written
// and wired to nothing. Once onboarding has somewhere to ask earlier, the public entry
// point can be added then.
