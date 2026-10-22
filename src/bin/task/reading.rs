//! The commands that read: `list`, `show`, `checks`.

use super::*;

pub(crate) async fn list(client: &Client, json: bool, args: ListArgs) -> Result<()> {
    let ListArgs {
        all,
        mine,
        pile,
        handed_out,
        done,
        to,
    } = args;
    // Through the same path as `move`, so `--to x` and `move <id> x` agree.
    let holder = match to {
        Some(to) => Some(client.resolve(to).await?),
        None => None,
    };
    let me = client.me().ok();
    let asked = holder.as_ref().map(|to| match to {
        Target::Nobody => selection::Holder::Nobody,
        Target::Person(id) => selection::Holder::Person(id),
        Target::Session(id) => selection::Holder::Session(id),
        // `--to me` is `--mine`.
        Target::Me => selection::Holder::Session(me.unwrap_or_default()),
    });
    let query = list_query(
        all,
        mine,
        pile,
        handed_out,
        done,
        client.session.as_deref(),
        asked,
    )?;
    let req = client
        .request(reqwest::Method::GET, "/api/tasks")
        .query(&query);
    let tasks = client.send(req).await?.unwrap_or(json!([]));
    // Decoded outside the closure, so an unreadable list is an error, not a wrong row.
    let shown: Vec<Task> = serde_json::from_value(tasks.clone())
        .context("the service answered with a list this CLI could not read")?;
    emit(json, &tasks, || {
        if shown.is_empty() {
            // Which question came back empty: an empty pile and an empty plate differ.
            println!(
                "{}",
                if pile {
                    "the pile is empty"
                } else if handed_out {
                    // Good news: everything handed out is closed.
                    "nothing you handed out is still open"
                } else {
                    "nothing open"
                }
            );
        }
        for task in &shown {
            println!("{}", line(task));
        }
    });
    Ok(())
}

pub(crate) async fn show(client: &Client, json: bool, args: ShowArgs) -> Result<()> {
    let ShowArgs {
        id,
        body: only_body,
        previous,
    } = args;
    if previous {
        let was = fetch_previous(client, id).await?;
        if only_body {
            print!("{}", was["body"].as_str().unwrap_or_default());
            return Ok(());
        }
        emit(json, &was, || {
            // The header says when this version was replaced.
            println!(
                "#{} as it stood until {} ({})",
                id.id(),
                was["at"].as_str().unwrap_or(""),
                was["actor"].as_str().unwrap_or("")
            );
            println!("{}", was["subject"].as_str().unwrap_or(""));
            let body = was["body"].as_str().unwrap_or("").trim();
            if !body.is_empty() {
                println!("\n{body}");
            }
            println!("\nput it back: task undo {}", id.id());
        });
    } else {
        let req = client.request(reqwest::Method::GET, &id.path());
        let task = client.send(req).await?.context("no such task")?;
        if only_body {
            // Exactly as stored, no newline added or removed, so it diffs cleanly.
            print!("{}", task["body"].as_str().unwrap_or_default());
            return Ok(());
        }
        let shown: Task = serde_json::from_value(task.clone())
            .context("the service answered with a task this CLI could not read")?;
        emit(json, &task, || {
            println!("{}", line(&shown));
            let body = task["body"].as_str().unwrap_or("").trim();
            if !body.is_empty() {
                println!("\n{body}");
            }
            // Between body and history: it is about the body. Shown until an edit shrinks it.
            if let Some(said) = task["sprawl_said"].as_str().map(str::trim)
                && !said.is_empty()
            {
                println!("\na model read this body, and it is a guess:");
                for finding in said.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    println!("  {finding}");
                }
                println!(
                    "  `task edit {} --body -` is how it gets rewritten.",
                    id.id()
                );
            }
            if let Some(events) = task["events"].as_array()
                && !events.is_empty()
            {
                println!("\nhistory");
                for event in events {
                    println!(
                        "  {}  {}  {}  {}",
                        event["at"].as_str().unwrap_or(""),
                        event["actor"].as_str().unwrap_or(""),
                        event["kind"].as_str().unwrap_or(""),
                        event["detail"].as_str().unwrap_or("")
                    );
                }
            }
        });
    }
    Ok(())
}

pub(crate) async fn checks(client: &Client, json: bool, args: ChecksArgs) -> Result<()> {
    let ChecksArgs { days } = args;
    let req = client
        .request(reqwest::Method::GET, "/api/checks")
        .query(&[("days", days)]);
    let rows = client.send(req).await?.unwrap_or(json!([]));
    let runs: Vec<checks::Ran> =
        serde_json::from_value(rows.clone()).context("reading what the checks did")?;
    // Per session: refusals and overrides.
    let req = client
        .request(reqwest::Method::GET, "/api/checks/overrules")
        .query(&[("days", days)]);
    let overrules: Vec<checks::Overrules> = match client.send(req).await? {
        Some(rows) => serde_json::from_value(rows).context("reading the overrides")?,
        None => Vec::new(),
    };
    emit(json, &rows, || {
        if runs.is_empty() {
            println!("nothing recorded in the last {days} days");
            return;
        }
        for line in checks::tally(&runs) {
            println!("{}", tallied(&line));
        }
        for who in &overrules {
            println!(
                "{:24} refused {:3}, filed past {:3}",
                who.session, who.refused, who.overruled
            );
        }
    });
    Ok(())
}
