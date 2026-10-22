//! The commands that change a task's words: `edit` and `undo`.

use super::*;

pub(crate) async fn edit(client: &Client, json: bool, args: EditArgs) -> Result<()> {
    let EditArgs {
        id,
        subject,
        body: raw,
        prepend,
        append,
        no_density_check,
        replace_body,
        priority,
        blocked_on,
        unblock,
        due,
        no_due,
    } = args;
    let mut change = json!({});
    if let Some(priority) = priority {
        change["priority"] = json!(priority.as_str());
    }
    // An empty list clears, so `--unblock` and `--blocked-on` share the field.
    if unblock {
        change["blocked_on"] = json!([] as [u64; 0]);
    } else if !blocked_on.is_empty() {
        change["blocked_on"] = json!(blocked_on);
    }
    // A date has no empty value, so clearing needs its own field.
    if no_due {
        change["clear_due"] = json!(true);
    } else if let Some(due) = due {
        change["due"] = json!(due.to_string());
    }
    if let Some(subject) = subject {
        change["subject"] = json!(subject);
    }
    if let Some(raw) = raw {
        change["body"] = json!(body(&raw)?);
        // Only with a body, so the flag can't linger on a line that writes none.
        if replace_body {
            change["replace_body"] = json!(true);
        }
    }
    // One stdin, so only one of these may read it; a second would read nothing.
    if prepend.as_deref() == Some("-") && append.as_deref() == Some("-") {
        bail!("only one of --prepend and --append can read stdin — give the other inline");
    }
    if let Some(text) = prepend {
        change["prepend"] = json!(body(&text)?);
    }
    if let Some(text) = append {
        change["append"] = json!(body(&text)?);
    }
    if change.as_object().is_none_or(|o| o.is_empty()) {
        bail!("nothing to change: pass --subject or --body");
    }
    let updated = patch(client, json, id, change).await?;
    // After the write: advice, never a refusal.
    if !no_density_check && !json {
        accreting(client, id, &updated).await;
    }
    Ok(())
}

pub(crate) async fn undo(client: &Client, json: bool, args: UndoArgs) -> Result<()> {
    let UndoArgs { id, anyway } = args;
    client.writing()?;
    let was = fetch_previous(client, id).await?;
    // Read first, so a refusal costs nothing; `mine` is the service's answer on
    // stored identity.
    if !anyway {
        let was: tasks::tasks::types::Revision = serde_json::from_value(was.clone())
            .context("the service's previous version did not parse")?;
        if tasks::tasks::undo::needs_saying(&was) {
            return Err(commands::declined(tasks::tasks::undo::refusal(
                &was,
                id.id(),
            )));
        }
    }
    // `replace_body`: see `Change::replace_body`.
    patch(
        client,
        json,
        id,
        json!({
            "subject": was["subject"],
            "body": was["body"],
            "replace_body": true,
        }),
    )
    .await?;
    Ok(())
}
