//! Who `task move <id> <to>` decides you meant.
//!
//! ⚠ **These pin the DECISION, not the round trip.** What the CLI fetches to
//! answer with — holders first, then every row — is plumbing; which conversation
//! a word picks out is the property, and getting it wrong hands somebody's work
//! to a conversation that is not there. So the resolution is a pure function in
//! the library, exercised here as public API, for the same reason
//! `tests/selection.rs` exists one module over.

use tasks::tasks::holder::{Holder, resolve};

const MEMVIEW: &str = "7c0202eb-080b-40a5-a654-8758b4ca723e";
const DEV_LINT: &str = "f67a35b2-57ff-414b-93ae-4a4b87612ec4";
const HEALTH: &str = "296dae53-3f84-4bd1-afbb-9ddcddedbdbb";

fn fleet() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        (MEMVIEW, Some("memview")),
        (DEV_LINT, Some("dev-lint")),
        (HEALTH, Some("health")),
    ]
}

#[test]
fn a_name_this_tool_prints_is_a_name_it_accepts() {
    // The whole ticket in one assertion: `task list` says `(health)`, so
    // `task move 42 health` has to work. It did not, and translating the one
    // into the other was a `task sessions | grep` every time.
    assert_eq!(
        resolve(fleet(), "health"),
        Holder::Session(HEALTH.to_string())
    );
}

#[test]
fn an_id_still_works_and_needs_no_name() {
    // Ids remain the identity. A conversation nobody has named yet has no other
    // spelling, so this path cannot be allowed to rot.
    assert_eq!(
        resolve(vec![(MEMVIEW, None)], MEMVIEW),
        Holder::Session(MEMVIEW.to_string())
    );
}

#[test]
fn an_id_beats_a_name_that_collides_with_it() {
    // They cannot collide in practice — one is a uuid — but if they ever did,
    // the identity is the answer and the attribute is not.
    let odd = vec![("health", Some("something-else")), (HEALTH, Some("health"))];
    assert_eq!(
        resolve(odd, "health"),
        Holder::Session("health".to_string()),
        "the name won over an exact id"
    );
}

/// Names are reused, so this is a real case rather than a defensive one.
///
/// Two distinct conversations have both been called `memview`: `7c0202eb`, which
/// still is, and `f67a35b2`, which is `dev-lint` now. Resolving to whichever
/// came first in a list would hand work to a conversation on the strength of a
/// name it used to have.
#[test]
fn a_name_two_conversations_share_is_refused_with_both_ids() {
    let overlapping = vec![(MEMVIEW, Some("memview")), (DEV_LINT, Some("memview"))];
    match resolve(overlapping, "memview") {
        Holder::Ambiguous(ids) => {
            assert_eq!(ids.len(), 2, "{ids:?}");
            assert!(ids.contains(&MEMVIEW.to_string()), "{ids:?}");
            assert!(ids.contains(&DEV_LINT.to_string()), "{ids:?}");
        }
        other => panic!("a shared name resolved to one conversation: {other:?}"),
    }
}

/// The half that matters more than the naming.
///
/// ⚠ **Checked, not assumed:** the write would not land either way —
/// `fk_tasks_session` refuses an assignee with no `sessions` row. What falling
/// through to "probably an id" would cost is the *answer*: the constraint
/// arrives as `AppError::Other`, a 500 logged as an internal error and reaching
/// the caller as `moving a task`. Every mistyped name would send somebody to
/// look at the service.
#[test]
fn a_word_that_names_nothing_is_refused_rather_than_sent_as_an_id() {
    match resolve(fleet(), "helth") {
        Holder::Unknown(names) => assert_eq!(names, vec!["dev-lint", "health", "memview"]),
        other => panic!("a typo was accepted as an id: {other:?}"),
    }
}

#[test]
fn the_refusal_says_what_the_alternatives_were() {
    // The reader's next question is always "what should I have typed", and the
    // answer is free here — the list was fetched to decide this in the first
    // place.
    let Holder::Unknown(names) = resolve(fleet(), "nothing-like-it") else {
        panic!("expected a refusal");
    };
    assert!(names.contains(&"health".to_string()), "{names:?}");
    assert!(
        names.windows(2).all(|w| w[0] <= w[1]),
        "the names are unsorted, so the list reads as arbitrary: {names:?}"
    );
}

#[test]
fn unnamed_conversations_do_not_crowd_the_refusal() {
    // Most rows are conversations nobody has named — 717 against 14 when that
    // was split — and listing them as blanks would bury the answer.
    let mostly_anonymous = vec![
        (HEALTH, Some("health")),
        ("11111111-1111-1111-1111-111111111111", None),
        ("22222222-2222-2222-2222-222222222222", None),
    ];
    let Holder::Unknown(names) = resolve(mostly_anonymous, "nope") else {
        panic!("expected a refusal");
    };
    assert_eq!(names, vec!["health"]);
}

#[test]
fn nothing_known_at_all_is_still_a_refusal() {
    // A service that answered with an empty list, or a first run against an
    // empty database. Sending the word on as an id would be the masking
    // fallback this whole function exists to avoid.
    assert_eq!(resolve(vec![], "health"), Holder::Unknown(vec![]));
}
