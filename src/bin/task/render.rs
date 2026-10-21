//! Service answers as the lines a session reads.

use super::*;

/// One task as one line, the digest's shape, so a list and a prompt look like
/// one system. Unlike the digest, a pile row says who filed it.
pub(crate) fn line(task: &Task) -> String {
    let marker = task.status.marker();
    // Rank first, padded so lines align; `!` marks a rank a deadline raised.
    let rank = match task.escalated_to {
        Some(raised) => format!("{}!", raised.as_str()),
        None => task
            .priority
            .map(|p| p.as_str().to_string())
            .unwrap_or_default(),
    };
    let mut out = format!("{marker} #{:<4} {rank:<3} {}", task.id, task.subject);
    if let Some(due) = task.due {
        if task.overdue {
            out.push_str(&format!("  OVERDUE {due}"));
        } else {
            out.push_str(&format!("  due {due}"));
        }
    }
    if task.blocked && !task.blocked_on.is_empty() {
        let ids: Vec<String> = task.blocked_on.iter().map(|v| format!("#{v}")).collect();
        out.push_str(&format!("  ⛔{}", ids.join(",")));
    }
    // On `task show`'s first line, so `head` still gets the count.
    match task.body_lines {
        0 => {}
        1 => out.push_str("  [1 line]"),
        n => out.push_str(&format!("  [{n} lines]")),
    }
    // The digest's marker, from the same field.
    if let Some(chars) = task.sprawl_chars {
        out.push_str(&format!("  [sprawl {}]", tasks::digest::thousands(chars)));
    }
    // `label()` is name, else id.
    if task.assignee.kind != AssigneeKind::Nobody {
        out.push_str(&format!("  ({})", task.assignee.label()));
    } else if let Some(from) = &task.filed_by {
        out.push_str(&format!("  (from {from})"));
    }
    out
}

/// Print an answer: verbatim with `--json`, else drawn. One helper, so no command ignores the flag.
pub(crate) fn emit(json: bool, value: &Value, human: impl FnOnce()) {
    if json {
        // Can't fail on a parsed Value; compact is still correct if it did.
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        );
    } else {
        human();
    }
}

/// The focus a route answered with; `null` is ordinary.
pub(crate) fn parse_focus(value: &Value) -> Option<tasks::tasks::focus::Focus> {
    serde_json::from_value(value.clone()).ok()
}

/// A focus as one line, with a countdown: fine here, never in the cached digest.
pub(crate) fn describe(focus: &tasks::tasks::focus::Focus) -> String {
    let ids: Vec<String> = focus.tasks.iter().map(|id| format!("#{id}")).collect();
    format!(
        "focused on {} until {} UTC — {} left. P0 and overdue still break through; \
         `task list` still shows everything.",
        ids.join(" "),
        focus.until.format("%H:%M"),
        tasks::tasks::focus::spell(focus.until - chrono::Utc::now()),
    )
}

/// What an edit landed on, told with its undo ([`Replaced`](tasks::tasks::types::Replaced)).
pub(crate) fn displaced(task: &Value, id: u64) -> Option<String> {
    let was = task.get("replaced")?;
    let (before, after) = (was["was"].as_u64()?, was["now"].as_u64()?);
    Some(format!(
        "replaced text last written {} by {} ({before} → {after} chars) — task undo {} puts it back",
        was["at"].as_str().unwrap_or(""),
        was["by"].as_str().unwrap_or(""),
        id
    ))
}

/// One kind of check per line. A timeout always prints: quiet and timeout both end in silence.
pub(crate) fn tallied(line: &checks::Tally) -> String {
    let kind = match line.kind {
        checks::Kind::Filing => "filing",
        checks::Kind::Density => "density",
        checks::Kind::Collision => "collision",
    };
    let outcomes: Vec<String> = [
        ("quiet", line.quiet),
        ("spoke", line.spoke),
        ("timeout", line.timeout),
        ("error", line.error),
    ]
    .into_iter()
    .filter(|(_, n)| *n > 0)
    .map(|(what, n)| format!("{n} {what}"))
    .collect();
    format!(
        "{kind:8} {:3} runs · {} · {} median, {} p90, {} worst",
        line.runs,
        outcomes.join(", "),
        spell(line.median_ms),
        spell(line.p90_ms),
        spell(line.worst_ms),
    )
}

/// One command's line; the run count first, as its weight.
pub(crate) fn timed_line(line: &commands::Tally) -> String {
    let failed = if line.failed > 0 {
        format!(", {} failed", line.failed)
    } else {
        String::new()
    };
    format!(
        "{:10} {:4} runs{failed} · {} median, {} p90, {} worst",
        line.verb,
        line.runs,
        spell(line.median_ms),
        spell(line.p90_ms),
        spell(line.worst_ms),
    )
}

/// Milliseconds as somebody would say them.
pub(crate) fn spell(ms: u32) -> String {
    let seconds = f64::from(ms) / 1000.0;
    if seconds < 10.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}s", seconds.round() as u32)
    }
}
