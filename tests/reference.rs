//! How a task may be named, and where each spelling goes.
//!
//! Worth its own file because the second spelling is load-bearing: 46% of the
//! 598 imported tasks could not keep their number, so `recall#79` is the only
//! handle the prose written before the migration contains.

use tasks::tasks::reference::TaskRef;

#[test]
fn a_task_can_be_named_three_ways() {
    assert_eq!("79".parse(), Ok(TaskRef::Id(79)));
    // What the digest prints on every line of every prompt, so a session
    // copying one out of its own context is not corrected for it.
    assert_eq!("#79".parse(), Ok(TaskRef::Id(79)));
    assert_eq!(
        "recall#79".parse(),
        Ok(TaskRef::Origin("recall".into(), 79))
    );
    // Typed by a person, off a phone or out of a sentence.
    assert_eq!(
        " recall #79 ".parse(),
        Ok(TaskRef::Origin("recall".into(), 79))
    );
}

#[test]
fn a_name_that_is_not_a_task_says_so() {
    for bad in ["recall", "recall#", "#", "", "79x", "recall#seventy-nine"] {
        let err = bad.parse::<TaskRef>().expect_err(bad);
        assert!(err.contains("recall#79"), "{bad:?} said {err:?}");
    }
}

#[test]
fn an_old_name_goes_to_the_two_segment_route() {
    // Never `/api/tasks/recall%2379`: `#` is the fragment delimiter, so an
    // unescaped one truncates the request to `/api/tasks/recall`, and an escaped
    // one works only for the callers that remember to escape it.
    assert_eq!(TaskRef::Id(79).path(), "/api/tasks/79");
    assert_eq!(
        TaskRef::Origin("recall".into(), 79).path(),
        "/api/tasks/by/recall/79"
    );
}

#[test]
fn a_reference_prints_as_it_is_typed() {
    assert_eq!(TaskRef::Id(79).to_string(), "#79");
    assert_eq!(
        TaskRef::Origin("recall".into(), 79).to_string(),
        "recall#79"
    );
}
