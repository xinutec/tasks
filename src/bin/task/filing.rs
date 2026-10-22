//! `task add`: the duplicate checks, then the filing.

use super::*;

pub(crate) async fn add(client: &Client, json: bool, args: AddArgs) -> Result<()> {
    let AddArgs {
        subject,
        body: raw,
        to,
        spare,
        priority,
        // Handled by clap's group: `--unassessed` is the absence of `--priority`.
        unassessed: _,
        blocked_on,
        blocks,
        due,
        repo,
        project,
        subject_flag,
        not_duplicate_of,
        no_duplicate_check,
        check_only,
    } = args;
    if repo.is_some() || project.is_some() {
        return Err(commands::declined(
            "a task has no repo and no project — the field went in migration 0004, \
             and the holder is the whole of an assignment. What this touches goes in \
             the subject, where every list shows it: `task list` prints one line per \
             task and that line is all most sessions will read.",
        ));
    }
    let subject = match (subject, subject_flag) {
        (Some(subject), None) => subject,
        (None, Some(said)) => {
            return Err(commands::declined(format!(
                "the subject is the first argument, not a flag: \
                 `task add {said:?} --priority P2`. Nothing was filed."
            )));
        }
        (Some(_), Some(_)) => {
            return Err(commands::declined(
                "the subject was given twice, as the argument and as `--subject`. \
                 Nothing was filed, because there is no way to tell which one you meant.",
            ));
        }
        (None, None) => {
            return Err(commands::declined(
                "a filing needs a subject: `task add \"one line\" --priority P2`.",
            ));
        }
    };
    // Before the duplicate check, so a filing the service would refuse spends no
    // model call (`repo::spare_note`). Trimmed as the service trims.
    let reason = spare
        .as_deref()
        .map(str::trim)
        .filter(|why| !why.is_empty());
    match holder::pile_verdict(matches!(to, Some(To::Nobody)), reason.is_some()) {
        holder::PileVerdict::Fits => {}
        refused => return Err(commands::declined(refused.said_to_cli())),
    }
    client.writing()?;
    // Skipped only for an override the service will accept: a recent refusal of
    // this subject, and naming every task it named. Otherwise the check runs, so
    // asking to skip first time gains nothing.
    let asked = !not_duplicate_of.is_empty() || no_duplicate_check;
    let skip = match (asked, check_only) {
        (false, _) | (_, true) => false,
        (true, false) => match licence(client, &subject).await {
            Ok(refused) => match checks::overrule(true, &not_duplicate_of, refused.as_deref()) {
                checks::Overrule::Skip => true,
                checks::Overrule::Unnamed(missing) => {
                    return Err(commands::declined(checks::unnamed(&missing)));
                }
                checks::Overrule::Check => {
                    eprintln!(
                        "(nothing has refused this subject yet, so the duplicate check runs)"
                    );
                    false
                }
            },
            Err(why) => {
                eprintln!(
                    "(could not ask whether a refusal licenses this, so the check runs: {why:#})"
                );
                false
            }
        },
    };
    // Two refusing questions before the POST: an open task with this exact
    // subject, and a model seeing the same problem in other words. `--check-only`
    // asks both and stops. An unreadable list costs a note, never a filing.
    let (corpus, mine) = match skip {
        true => (Vec::new(), Vec::new()),
        false => open_now(client).await.unwrap_or_else(|why| {
            eprintln!("(duplicate check did not run: {why:#})");
            (Vec::new(), Vec::new())
        }),
    };
    if let Some(already) = duplicates::same_subject(&subject, &corpus) {
        // Recorded like a model's refusal, so its override is accepted.
        recorded(
            client,
            checks::Run {
                kind: checks::Kind::Collision,
                task_id: None,
                input_chars: 0,
                accreted: None,
                elapsed_ms: 0,
                outcome: checks::Outcome::Spoke,
                subject_key: Some(checks::subject_key(&subject)),
                matched: vec![already],
                said: None,
            },
        )
        .await;
        return Err(commands::declined(duplicates::collision(already)));
    }
    // Before the POST: a refusal afterwards is not one. Edges narrow only what the
    // model reads, after the exact check over everything.
    let candidates = duplicates::edged(&corpus, &blocked_on, &blocks);
    // The closed list isn't narrowed: nothing waits on a closed task.
    let (settled, unread) = match skip {
        true => (Vec::new(), 0),
        false => settled_now(client).await,
    };
    if !skip {
        match already_filed(client, &candidates, &settled, &subject).await {
            Ok(found) if !found.is_empty() => {
                let (open, over) = duplicates::split(&found, &settled);
                // Open wins when both are named: folding into live work beats reopening.
                // `declined`, never `bail!`, or the commonest refusal prints as a failure.
                let said = match open.is_empty() {
                    false => duplicates::refusal(&open, &candidates, &mine),
                    true => duplicates::reopen_instead(&over, settled.len(), unread),
                };
                return Err(commands::declined(said));
            }
            Ok(_) => {}
            Err(why) => eprintln!("(duplicate check did not run: {why:#})"),
        }
    }
    // Only reached when nothing was named.
    if check_only {
        println!("NONE — nothing open or closed was named");
        return Ok(());
    }
    let mut payload = json!({
        "subject": subject,
        "body": raw.as_deref().map(body).transpose()?.unwrap_or_default(),
        // Always sent, so the service can tell a skip from an old client.
        "checked": !skip,
    });
    if skip {
        payload["not_duplicate_of"] = json!(not_duplicate_of);
    }
    // Always present, null when unassessed; the service refuses a missing key.
    payload["priority"] = match priority {
        Some(priority) => json!(priority.as_str()),
        None => Value::Null,
    };
    if !blocked_on.is_empty() {
        payload["blocked_on"] = json!(blocked_on);
    }
    if let Some(due) = due {
        payload["due"] = json!(due.to_string());
    }
    if let Some(to) = to {
        let to = client.resolve(to).await?;
        payload["assignee"] = assignee(&to, client.me()?);
    }
    // As given: refused above if it didn't fit, and by the service's type.
    if let Some(spare) = spare {
        payload["spare"] = json!(spare);
    }
    let req = client
        .request(reqwest::Method::POST, "/api/tasks")
        .json(&payload);
    let task = client.send(req).await?.context("no task came back")?;
    let created: Created = serde_json::from_value(task.clone())
        .context("the service answered with a task this CLI could not read")?;
    emit(json, &task, || {
        let now = chrono::Utc::now();
        let hints = created
            .closed
            .iter()
            .map(|closed| lifecycle::reopen_hint(closed, created.task.id, now));
        println!("{}", lifecycle::on_the_line(line(&created.task), hints));
    });
    // After the POST, which gives the id. Recorded on the other task: `blocked_on`
    // belongs to the waiter.
    if let Some(filed) = task["id"].as_u64() {
        for blocked in &blocks {
            if let Err(why) = unblocking(client, *blocked, filed).await {
                eprintln!("(filed, but #{blocked} was not marked as waiting for it: {why:#})");
            }
        }
    }
    Ok(())
}
