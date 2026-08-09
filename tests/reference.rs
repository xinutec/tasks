//! How a task may be named.
//!
//! There were two spellings until 2026-08-09: `recall#79` named a task by what
//! a session called it before the migration, because 178 of the 620 imported
//! tasks could not keep their number. It went with the columns behind it once
//! the mapping had been spent — see `migrations/0003_drop_origin.sql`. What is
//! left is one id space, and the only question is whether the hash is optional.

use tasks::tasks::reference::TaskRef;

#[test]
fn the_hash_the_digest_prints_is_optional() {
    assert_eq!("79".parse(), Ok(TaskRef(79)));
    // The digest puts `#79` on every line of every prompt, so a session copying
    // one out of its own context must not be corrected for it. This is the
    // whole reason the type exists rather than a bare `u64` argument.
    assert_eq!("#79".parse(), Ok(TaskRef(79)));
    // Typed by a person, off a phone or out of a sentence.
    assert_eq!(" #79 ".parse(), Ok(TaskRef(79)));
}

#[test]
fn a_name_that_is_not_a_task_says_so() {
    for bad in ["recall", "recall#79", "#", "", "79x", "seventy-nine"] {
        let err = bad.parse::<TaskRef>().expect_err(bad);
        assert!(err.contains("#79"), "{bad:?} said {err:?}");
    }
}

#[test]
fn an_old_name_is_no_longer_a_name() {
    // Not an oversight: `recall#79` used to parse, and prose from before the
    // migration still contains it. It has to FAIL rather than be read as `79`,
    // which would silently answer with health's task when recall's was meant —
    // the four-sessions-had-a-`#79` problem the two columns existed to solve.
    let err = "recall#79".parse::<TaskRef>().expect_err("recall#79");
    assert!(err.contains("is not a task"), "{err:?}");
}

#[test]
fn a_reference_knows_its_url_and_prints_with_the_hash() {
    assert_eq!(TaskRef(79).path(), "/api/tasks/79");
    assert_eq!(TaskRef(79).to_string(), "#79");
}
