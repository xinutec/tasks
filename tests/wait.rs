//! Waking a session that is not speaking.
//!
//! Pippijn, 2026-09-05: *"When there's a problem someone needs to fix, we should
//! have a way for the task tool to wait, so a background job can run and wait
//! until that's done, waking up the session that can then continue"* — and,
//! narrowing it, *"I just need that blocked session to have a way to be notified
//! of a task being closed so they can continue working."*
//!
//! ⚠ **The digest already clears the `⛔` the moment a blocker closes, and that
//! is not a notification.** It is rendered when the session takes a turn, and a
//! blocked session is not taking turns. So the wake has to be something the
//! session is already holding open — a background command that returns — rather
//! than anything delivered to it.
//!
//! What is tested here is the decision, not the sleeping: given what the service
//! says about each blocker, does the wait end, and what is the session told when
//! it comes back.

use std::time::Duration;

use tasks::tasks::types::Status;
use tasks::tasks::wait::{self, Verdict};

/// ⚠ **The bug this exists to prevent**, and it is one line of shell away from
/// being written by every session that rolls its own loop: `status != "open"`
/// reads a blocker somebody has PICKED UP as a blocker that is gone.
#[test]
fn a_blocker_being_worked_on_is_still_a_blocker() {
    assert_eq!(
        wait::verdict(&[(1350, Status::Doing)]),
        Verdict::Waiting(vec![1350])
    );
}

#[test]
fn an_open_blocker_keeps_the_wait_going() {
    assert_eq!(
        wait::verdict(&[(884, Status::Done), (1350, Status::Open)]),
        Verdict::Waiting(vec![1350])
    );
}

#[test]
fn every_blocker_finished_ends_the_wait() {
    assert_eq!(
        wait::verdict(&[(884, Status::Done), (1350, Status::Done)]),
        Verdict::Done
    );
}

/// ⚠ **`drop` is *overtaken, obsolete, decided against* — the problem was NOT
/// fixed.** A waiter that treats it as `done` wakes the session and tells it the
/// opposite of what happened, which is worse than never waking it at all.
#[test]
fn a_dropped_blocker_ends_the_wait_but_not_as_done() {
    assert_eq!(
        wait::verdict(&[(1350, Status::Dropped)]),
        Verdict::Dropped(vec![1350])
    );
}

#[test]
fn one_dropped_among_finished_ones_still_names_itself() {
    assert_eq!(
        wait::verdict(&[
            (884, Status::Done),
            (1350, Status::Dropped),
            (1351, Status::Done),
        ]),
        Verdict::Dropped(vec![1350])
    );
}

/// Waiting reports what is still outstanding rather than just that something is:
/// this string is what the session reads when it eventually gives up.
#[test]
fn waiting_names_all_the_outstanding_ones() {
    assert_eq!(
        wait::verdict(&[
            (884, Status::Open),
            (1350, Status::Done),
            (1351, Status::Doing),
        ]),
        Verdict::Waiting(vec![884, 1351])
    );
}

/// ⚠ **Nothing to wait for is not a successful wait.** Naming no task would
/// otherwise fall through to "every blocker finished" and wake the session with
/// a clean bill of health about a question nobody asked, so the refusal is at
/// the argument, before any of this is reached.
#[test]
fn naming_no_task_is_refused_at_the_argument() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_task"))
        .arg("wait")
        .output()
        .expect("running the CLI");
    assert!(!out.status.success(), "`task wait` with no id was accepted");
}

/// The first minute is cheap and the days after it are not: a blocker closed
/// while somebody is still at the keyboard should wake the session in seconds,
/// and one closed tomorrow can afford a slow poll.
#[test]
fn the_poll_is_eager_at_first_and_lazy_afterwards() {
    assert!(wait::interval(Duration::from_secs(0)) <= Duration::from_secs(5));
    assert!(wait::interval(Duration::from_secs(30)) <= Duration::from_secs(5));
    assert!(wait::interval(Duration::from_secs(600)) >= Duration::from_secs(30));
    assert_eq!(
        wait::interval(Duration::from_secs(86_400)),
        wait::interval(Duration::from_secs(600)),
        "the lazy interval is a ceiling, not a ramp that keeps growing"
    );
}

/// What the woken session actually reads. The exit code says fixed or not; this
/// says which task and how it ended, because the session has been away and the
/// id is the only thing that reconnects the wake to the work it was doing.
#[test]
fn the_verdict_says_which_task_and_how_it_ended() {
    let said = wait::said(&Verdict::Done, &[1350]);
    assert!(said.contains("1350"), "the id is missing: {said}");

    let said = wait::said(&Verdict::Dropped(vec![1350]), &[1350]);
    assert!(said.contains("1350"), "the id is missing: {said}");
    assert!(
        said.contains("dropped"),
        "nothing says the work was not done: {said}"
    );
    // The one id it has, said once. `#1350 closed, but #1350 was dropped` is
    // both ends of the same fact, and the woken session reads this first.
    assert_eq!(
        said.matches("#1350").count(),
        1,
        "the only task is named twice: {said}"
    );

    // With more than one there are two facts, and both belong in the sentence.
    let said = wait::said(&Verdict::Dropped(vec![1350]), &[884, 1350]);
    assert!(
        said.contains("884") && said.contains("1350"),
        "a mixed ending must say what closed and what was dropped: {said}"
    );

    let said = wait::said(&Verdict::Waiting(vec![884, 1351]), &[884, 1350, 1351]);
    assert!(
        said.contains("884") && said.contains("1351"),
        "giving up must name what is still open: {said}"
    );
}
