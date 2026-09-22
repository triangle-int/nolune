//! Availability negotiation (#111): the pure planner behind an accepted
//! availability query. It turns what this owner keeps (the deadlines of
//! their open commitments and their quiet hours) into the free spans a
//! paired companion may learn, and nothing else: no calendar, no busy
//! span, no reason, no name. A commitment contributes the moment or the
//! window it falls due and nothing more; quiet hours contribute the hours
//! the owner is not to be disturbed. What a peer gets is the complement,
//! clipped to the window it asked about, rounded inward to a granularity
//! the disclosure class sets (an hour at `availability`, a quarter of an
//! hour at `personal` and above, never finer), and capped, so no moment
//! is disclosed exactly and a long window discloses no more than a short
//! one.
//!
//! Nothing here reads a clock, a file, or the network; the delivery in
//! `services::peer_delivery` gathers the inputs and hands them in.

use chrono::{TimeZone, Timelike, Utc};

use crate::domain::{
    commitment::{Commitment, Deadline},
    federation_intent::{
        AvailabilityState, AvailabilityWindow, MAX_AVAILABILITY_WINDOWS, TimeWindow,
    },
    federation_policy::{DisclosureClass, QuietHoursPolicy},
};

/// The coarsest granularity: whole hours, for the `availability` class.
pub const HOUR_SECS: u64 = 60 * 60;
/// The finest granularity any class discloses: a quarter of an hour.
pub const QUARTER_HOUR_SECS: u64 = 15 * 60;
/// How long a commitment due at one moment keeps the owner busy from that
/// moment on.
pub const MOMENT_BUSY_SECS: u64 = HOUR_SECS;

/// A half-open span of Unix seconds the owner is not free in.
pub type Span = (u64, u64);

/// The granularity free spans are rounded to at `disclosure`: hours at
/// `availability` (and below, which never reaches here), quarter hours at
/// `personal` and `sensitive`. Never finer than a quarter of an hour.
pub fn granularity_secs(disclosure: DisclosureClass) -> u64 {
    match disclosure {
        DisclosureClass::None | DisclosureClass::Availability => HOUR_SECS,
        DisclosureClass::Personal | DisclosureClass::Sensitive => QUARTER_HOUR_SECS,
    }
}

/// The spans inside `window` that the owner's open commitments take: a
/// commitment due at a moment is busy for [`MOMENT_BUSY_SECS`] from it, one
/// due inside a window is busy for that window. A closed commitment and
/// one without a deadline contribute nothing; nothing but the deadline is
/// read.
pub fn busy_spans(commitments: &[Commitment], window: TimeWindow) -> Vec<Span> {
    commitments
        .iter()
        .filter(|commitment| commitment.is_open())
        .filter_map(|commitment| commitment.deadline)
        .map(|deadline| match deadline {
            Deadline::At { at } => {
                let from = unsigned(at);
                (from, from.saturating_add(MOMENT_BUSY_SECS))
            }
            Deadline::Window { start, end } => (unsigned(start), unsigned(end)),
        })
        .filter_map(|span| clip(span, window))
        .collect()
}

/// The spans inside `window` that fall in the owner's quiet hours, read in
/// the policy's zone (UTC when unset or unknown), local hour by local
/// hour from where the window starts; empty without quiet hours. Each
/// step runs to the next local hour boundary (the seconds since local
/// midnight say how far that is, as the gate reads quiet hours), so a
/// zone whose UTC offset is not a whole hour (Kolkata, Kathmandu,
/// Adelaide, St John's) keeps its quiet hours where the owner set them
/// rather than shifted to the UTC hour grid, and a moment inside a
/// declared quiet hour is never left free.
pub fn quiet_spans(quiet: Option<&QuietHoursPolicy>, window: TimeWindow) -> Vec<Span> {
    let Some(quiet) = quiet else {
        return Vec::new();
    };
    let zone: chrono_tz::Tz = quiet
        .timezone
        .as_deref()
        .and_then(|name| name.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let mut spans = Vec::new();
    let mut at = window.from;
    while at < window.to {
        let local = Utc
            .timestamp_opt(i64::try_from(at).unwrap_or(i64::MAX), 0)
            .single()
            .map(|utc| utc.with_timezone(&zone));
        // The next local hour boundary in the offset in force at `at`:
        // between one second and one hour away, so the walk always moves.
        let until = local
            .as_ref()
            .map_or(at.saturating_add(HOUR_SECS), |local| {
                let into_hour = u64::from(local.num_seconds_from_midnight() % 3600);
                at.saturating_add(HOUR_SECS - into_hour)
            });
        if local.is_some_and(|local| quiet.contains(local.hour()))
            && let Some(span) = clip((at, until), window)
        {
            spans.push(span);
        }
        at = until;
    }
    spans
}

/// The free spans inside `window` once `busy` is taken out, each rounded
/// inward to a multiple of `granularity` seconds (so a span never starts
/// before the owner is free or ends after they stop being free), empty
/// spans dropped, at most [`MAX_AVAILABILITY_WINDOWS`] of them from the
/// start of the window, every one marked free. There is no other state a
/// span can carry out of here.
pub fn free_windows(
    window: TimeWindow,
    busy: &[Span],
    granularity: u64,
) -> Vec<AvailabilityWindow> {
    let granularity = granularity.max(QUARTER_HOUR_SECS);
    let mut taken: Vec<Span> = busy.iter().filter_map(|span| clip(*span, window)).collect();
    taken.sort_unstable();
    let mut free = Vec::new();
    let mut cursor = window.from;
    for (from, to) in taken.into_iter().chain([(window.to, window.to)]) {
        if from > cursor {
            let start = cursor.div_ceil(granularity) * granularity;
            let end = from / granularity * granularity;
            if start < end {
                free.push(AvailabilityWindow {
                    from: start,
                    to: end,
                    state: AvailabilityState::Free,
                });
                if free.len() == MAX_AVAILABILITY_WINDOWS {
                    break;
                }
            }
        }
        cursor = cursor.max(to);
    }
    free
}

/// `span` cut down to `window`, or `None` when nothing of it is inside.
fn clip((from, to): Span, window: TimeWindow) -> Option<Span> {
    let from = from.max(window.from);
    let to = to.min(window.to);
    (from < to).then_some((from, to))
}

/// A deadline as Unix seconds; a moment before the epoch is the epoch.
fn unsigned(secs: i64) -> u64 {
    u64::try_from(secs).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::commitment::{
        COMMITMENT_FORMAT_VERSION, CommitmentStatus, Owner, Provenance,
    };

    /// 2027-01-15 08:00 UTC.
    const T0: u64 = 1_800_000_000;
    const DAY: u64 = 24 * HOUR_SECS;

    fn commitment(deadline: Option<Deadline>, status: CommitmentStatus) -> Commitment {
        Commitment {
            version: COMMITMENT_FORMAT_VERSION,
            id: "cmt_1".into(),
            promise: "Dentist appointment with Dr Vane".into(),
            owner: Owner::User,
            status,
            deadline,
            dependencies: Vec::new(),
            waiting_on: None,
            next_check: None,
            continuity_ids: Vec::new(),
            provenance: Provenance::Manual,
            completion: None,
            snoozed_until: None,
            snooze_count: 0,
            last_check: None,
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
        }
    }

    fn window(from: u64, to: u64) -> TimeWindow {
        TimeWindow { from, to }
    }

    #[test]
    fn a_commitment_contributes_its_deadline_only_and_only_while_open() {
        let all = [
            commitment(
                Some(Deadline::At {
                    at: (T0 + 600) as i64,
                }),
                CommitmentStatus::Active,
            ),
            commitment(
                Some(Deadline::Window {
                    start: (T0 + 7_200) as i64,
                    end: (T0 + 9_000) as i64,
                }),
                CommitmentStatus::Due,
            ),
            commitment(
                Some(Deadline::At {
                    at: (T0 + 20_000) as i64,
                }),
                CommitmentStatus::Completed,
            ),
            commitment(None, CommitmentStatus::Active),
            commitment(Some(Deadline::At { at: -5 }), CommitmentStatus::Active),
        ];
        assert_eq!(
            busy_spans(&all, window(T0, T0 + DAY)),
            [
                (T0 + 600, T0 + 600 + MOMENT_BUSY_SECS),
                (T0 + 7_200, T0 + 9_000)
            ]
        );
        // Clipped to the window, and nothing outside it.
        assert_eq!(
            busy_spans(&all, window(T0 + 8_000, T0 + 8_500)),
            [(T0 + 8_000, T0 + 8_500)]
        );
        assert!(busy_spans(&all, window(T0 + 100_000, T0 + 200_000)).is_empty());
    }

    #[test]
    fn quiet_hours_are_read_in_the_policy_zone_and_wrap_midnight() {
        let utc = QuietHoursPolicy {
            start_hour: 22,
            end_hour: 6,
            timezone: None,
        };
        // 08:00 to 08:00 the next day: quiet from 22:00 to 06:00.
        assert_eq!(
            quiet_spans(Some(&utc), window(T0, T0 + DAY)),
            (14..22)
                .map(|hour| (T0 + hour * HOUR_SECS, T0 + (hour + 1) * HOUR_SECS))
                .collect::<Vec<_>>()
        );
        // Berlin is UTC+1 in January: quiet 22:00-06:00 local is 21:00-05:00 UTC.
        let berlin = QuietHoursPolicy {
            timezone: Some("Europe/Berlin".into()),
            ..utc.clone()
        };
        let spans = quiet_spans(Some(&berlin), window(T0, T0 + DAY));
        assert_eq!(
            spans.first(),
            Some(&(T0 + 13 * HOUR_SECS, T0 + 14 * HOUR_SECS))
        );
        assert_eq!(
            spans.last(),
            Some(&(T0 + 20 * HOUR_SECS, T0 + 21 * HOUR_SECS))
        );
        // A window starting mid-hour is clipped, not widened.
        assert_eq!(
            quiet_spans(
                Some(&utc),
                window(T0 + 14 * HOUR_SECS + 600, T0 + 15 * HOUR_SECS)
            ),
            [(T0 + 14 * HOUR_SECS + 600, T0 + 15 * HOUR_SECS)]
        );
        assert!(quiet_spans(None, window(T0, T0 + DAY)).is_empty());
        let unknown_zone = QuietHoursPolicy {
            timezone: Some("Mars/Olympus".into()),
            ..utc
        };
        assert_eq!(
            quiet_spans(Some(&unknown_zone), window(T0, T0 + DAY)).len(),
            8,
            "an unknown zone reads as UTC"
        );
    }

    #[test]
    fn quiet_hours_in_a_zone_with_a_fractional_offset_start_on_the_local_hour() {
        let policy = |zone: &str| QuietHoursPolicy {
            start_hour: 22,
            end_hour: 6,
            timezone: Some(zone.into()),
        };
        let minutes = |count: u64| count * 60;
        // Kolkata is UTC+5:30: quiet 22:00-06:00 IST is 16:30-00:30 UTC,
        // eight local hours from T0 + 8h30.
        let kolkata = quiet_spans(Some(&policy("Asia/Kolkata")), window(T0, T0 + DAY));
        assert_eq!(kolkata.len(), 8, "{kolkata:?}");
        assert_eq!(
            kolkata.first(),
            Some(&(
                T0 + 8 * HOUR_SECS + minutes(30),
                T0 + 9 * HOUR_SECS + minutes(30)
            )),
            "quiet starts at 22:00 IST, not at the UTC hour after it"
        );
        assert_eq!(
            kolkata.last(),
            Some(&(
                T0 + 15 * HOUR_SECS + minutes(30),
                T0 + 16 * HOUR_SECS + minutes(30)
            )),
            "quiet ends at 06:00 IST, not at the UTC hour before it"
        );
        // Kathmandu is UTC+5:45: quiet starts at 16:15 UTC.
        let kathmandu = quiet_spans(Some(&policy("Asia/Kathmandu")), window(T0, T0 + DAY));
        assert_eq!(kathmandu.len(), 8, "{kathmandu:?}");
        assert_eq!(
            kathmandu.first(),
            Some(&(
                T0 + 8 * HOUR_SECS + minutes(15),
                T0 + 9 * HOUR_SECS + minutes(15)
            ))
        );
        // A window starting inside a local hour is clipped, not widened,
        // and the next span still starts on the local hour boundary.
        let clipped = quiet_spans(
            Some(&policy("Asia/Kolkata")),
            window(T0 + 9 * HOUR_SECS, T0 + 11 * HOUR_SECS),
        );
        assert_eq!(
            clipped,
            [
                (T0 + 9 * HOUR_SECS, T0 + 9 * HOUR_SECS + minutes(30)),
                (
                    T0 + 9 * HOUR_SECS + minutes(30),
                    T0 + 10 * HOUR_SECS + minutes(30)
                ),
                (T0 + 10 * HOUR_SECS + minutes(30), T0 + 11 * HOUR_SECS),
            ]
        );
        // What a peer at `availability` gets: the free span before quiet
        // ends on the last whole UTC hour inside the free time (16:00,
        // T0 + 8h) and the one after starts on the first whole hour after
        // it (01:00 next day, T0 + 17h); the half hours 22:00-22:30 IST and
        // 05:30-06:00 IST are never disclosed as free.
        let free = free_windows(window(T0, T0 + DAY), &kolkata, HOUR_SECS);
        let spans: Vec<Span> = free.iter().map(|span| (span.from, span.to)).collect();
        assert_eq!(
            spans,
            [(T0, T0 + 8 * HOUR_SECS), (T0 + 17 * HOUR_SECS, T0 + DAY)]
        );
    }

    #[test]
    fn free_spans_are_the_rounded_complement_and_never_carry_anything_else() {
        let busy = [
            (T0 + 2 * HOUR_SECS + 20 * 60, T0 + 3 * HOUR_SECS + 20 * 60),
            (T0 + 6 * HOUR_SECS, T0 + 7 * HOUR_SECS + 30 * 60),
            // Overlapping and out of order spans merge.
            (T0 + 6 * HOUR_SECS + 600, T0 + 6 * HOUR_SECS + 1_200),
            (T0 + 14 * HOUR_SECS, T0 + 22 * HOUR_SECS),
        ];
        let hourly = free_windows(window(T0, T0 + DAY), &busy, HOUR_SECS);
        let spans: Vec<Span> = hourly.iter().map(|span| (span.from, span.to)).collect();
        assert_eq!(
            spans,
            [
                (T0, T0 + 2 * HOUR_SECS),
                (T0 + 4 * HOUR_SECS, T0 + 6 * HOUR_SECS),
                (T0 + 8 * HOUR_SECS, T0 + 14 * HOUR_SECS),
                (T0 + 22 * HOUR_SECS, T0 + DAY),
            ]
        );
        assert!(
            hourly
                .iter()
                .all(|span| span.state == AvailabilityState::Free)
        );
        let fine = free_windows(window(T0, T0 + DAY), &busy, QUARTER_HOUR_SECS);
        assert_eq!(fine[0].to, T0 + 2 * HOUR_SECS + 15 * 60);
        assert_eq!(fine[1].from, T0 + 3 * HOUR_SECS + 30 * 60);
        assert_eq!(fine[2].from, T0 + 7 * HOUR_SECS + 30 * 60);
        // Never finer than a quarter of an hour, whatever is asked.
        assert_eq!(free_windows(window(T0, T0 + DAY), &busy, 1), fine);
        assert_eq!(granularity_secs(DisclosureClass::Availability), HOUR_SECS);
        assert_eq!(
            granularity_secs(DisclosureClass::Personal),
            QUARTER_HOUR_SECS
        );
        assert_eq!(
            granularity_secs(DisclosureClass::Sensitive),
            QUARTER_HOUR_SECS
        );
        // A gap shorter than the granularity is not disclosed at all.
        let tight = [(T0 + 20 * 60, T0 + DAY)];
        assert!(free_windows(window(T0, T0 + DAY), &tight, HOUR_SECS).is_empty());
        // Nothing free is nothing.
        assert!(free_windows(window(T0, T0 + DAY), &[(T0, T0 + DAY)], HOUR_SECS).is_empty());
        // Capped at the wire's bound, from the start of the window.
        let comb: Vec<Span> = (0..100)
            .map(|hour| {
                (
                    T0 + (2 * hour + 1) * HOUR_SECS,
                    T0 + (2 * hour + 2) * HOUR_SECS,
                )
            })
            .collect();
        let capped = free_windows(window(T0, T0 + 200 * HOUR_SECS), &comb, HOUR_SECS);
        assert_eq!(capped.len(), MAX_AVAILABILITY_WINDOWS);
        assert_eq!(capped[0].from, T0);
    }
}
