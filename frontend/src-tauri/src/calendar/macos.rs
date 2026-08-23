//! macOS implementation: EventKit through the `objc2` bindings.
//!
//! The filtering and selection logic is carried over from `tools/calendar-probe`, where
//! it was checked against 16 real recordings: 2 of 2 meetings matched, 14 of 14 test
//! runs correctly left unmatched, no mistakes.

use log::{debug, info};

use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStatus, EKEventStore};
use objc2_foundation::NSDate;

use super::CalendarEvent;

/// How long before an event starts we still consider a recording to belong to it.
///
/// People join a little early. With auto-detection the recording starts once Teams
/// makes sound, which is usually somewhere around the scheduled start.
const LEAD_TIME_S: f64 = 10.0 * 60.0;

/// How long after an event ends we still tie a recording to it.
///
/// Meetings run over; a recording started just after the scheduled end most likely
/// still belongs to the same meeting.
const GRACE_TIME_S: f64 = 10.0 * 60.0;

/// Bonus for a Teams marker in the location field.
///
/// The app detects a meeting by Teams making sound. When the calendar independently
/// says the event is a Teams meeting, two signals agree — a stronger case than
/// overlapping times alone.
const TEAMS_BONUS: f64 = 2.0;

/// Bonus for having attendees, which separates a meeting from a calendar note.
const PARTICIPANTS_BONUS: f64 = 1.0;

pub fn request_access_if_needed() {
    // SAFETY: the EventKit bindings are `unsafe` because they are raw Objective-C calls.
    unsafe {
        let status = EKEventStore::authorizationStatusForEntityType(EKEntityType::Event);
        if status != EKAuthorizationStatus::NotDetermined {
            return;
        }

        info!("Calendar: requesting access (the system dialog appears asynchronously)");

        let store = EKEventStore::new();
        let block = block2::RcBlock::new(
            |granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {
                if granted.as_bool() {
                    info!("Calendar: access granted — later recordings will get meeting names");
                } else {
                    info!("Calendar: access not granted — names stay as they were");
                }
            },
        );
        store.requestFullAccessToEventsWithCompletion(&*block as *const _ as *mut _);

        // Deliberately not waiting: starting a recording must not depend on how
        // quickly the user clicks the dialog.
        //
        // Since we do not wait, both values would die at the end of this function while
        // the answer arrives later. Apple requires a strong reference to the event store
        // for as long as it is in use — a released store can silently invalidate the
        // request, meaning the dialog never appears. The block is most likely copied by
        // EventKit, as any completion handler invoked later would be, but if it were not,
        // releasing it would be a use-after-free. The price of holding both is two small
        // allocations once per process; the price of getting it wrong is a dialog that
        // never shows and a day of diagnosis. That same class of bug has already cost
        // this project a day and a half over the microphone.
        std::mem::forget(store);
        std::mem::forget(block);
    }
}

pub fn find_current_event() -> Option<CalendarEvent> {
    // SAFETY: as above — kept in one block so `unsafe` does not spread across the file.
    unsafe {
        let status = EKEventStore::authorizationStatusForEntityType(EKEntityType::Event);
        if status != EKAuthorizationStatus::FullAccess {
            debug!("Calendar: no full access ({status:?}), name left unchanged");
            request_access_if_needed();
            return None;
        }

        let store = EKEventStore::new();
        let now = NSDate::date().timeIntervalSince1970();

        let from = NSDate::dateWithTimeIntervalSince1970(now - GRACE_TIME_S);
        let to = NSDate::dateWithTimeIntervalSince1970(now + LEAD_TIME_S);

        let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&from, &to, None);
        let events = store.eventsMatchingPredicate(&predicate);

        let mut seen: Vec<String> = Vec::new();
        let mut best: Option<(f64, CalendarEvent)> = None;

        for i in 0..events.count() {
            let e = events.objectAtIndex(i);

            // Filter 1: all-day entries are birthdays and holidays, not meetings.
            if e.isAllDay() {
                continue;
            }

            // Filter 2: cancelled events stay in the calendar when nobody removes them.
            if e.status() == EKEventStatus::Canceled {
                continue;
            }

            // Filter 3: de-duplicate by server-side identifier.
            let external_id = e.calendarItemExternalIdentifier().map(|s| s.to_string());
            if let Some(id) = &external_id {
                if seen.contains(id) {
                    continue;
                }
                seen.push(id.clone());
            }

            let start = e.startDate().timeIntervalSince1970();
            let end = e.endDate().timeIntervalSince1970();

            // The recording has to start inside the event's window, with slack on both sides.
            if now < start - LEAD_TIME_S || now > end + GRACE_TIME_S {
                continue;
            }

            let location = e.location().map(|l| l.to_string()).unwrap_or_default();
            let is_teams = location.contains("Teams");

            let participants: Vec<String> = e
                .attendees()
                .map(|list| {
                    (0..list.count())
                        .filter_map(|j| list.objectAtIndex(j).name().map(|n| n.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // The closer to the event's start, the more confident the match — a recording
            // usually begins when the meeting does.
            let distance_min = ((now - start).abs() / 60.0).min(60.0);
            let mut score = 60.0 - distance_min;
            if is_teams {
                score += TEAMS_BONUS * 10.0;
            }
            if participants.len() > 1 {
                score += PARTICIPANTS_BONUS * 10.0;
            }

            let candidate = CalendarEvent {
                title: e.title().to_string(),
                participants,
                agenda: e.notes().map(|n| n.to_string()).filter(|s| !s.trim().is_empty()),
                external_id,
            };

            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, candidate));
            }
        }

        match best {
            Some((score, event)) => {
                info!(
                    "Calendar: matched '{}' (score {:.0}, {} participants, {} chars of agenda)",
                    event.title,
                    score,
                    event.participants.len(),
                    event.agenda.as_ref().map_or(0, |a| a.chars().count())
                );
                Some(event)
            }
            None => {
                debug!("Calendar: no matching event, name left unchanged");
                None
            }
        }
    }
}
