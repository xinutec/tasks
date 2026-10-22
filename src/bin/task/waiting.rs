//! Waiting on other work: `wait` and `focus`.

use super::*;

pub(crate) async fn wait(client: &Client, args: WaitArgs) -> Result<()> {
    let WaitArgs { ids, period } = args;
    // Checked first, so a typo is a refusal, not a day-long wait.
    let bound = focus::parse(&period)?
        .to_std()
        .context("a wait cannot be negative")?;
    let asked: Vec<u64> = ids.iter().map(TaskRef::id).collect();
    // A missing task fails the first pass rather than being polled.
    let started = std::time::Instant::now();
    let verdict = loop {
        let verdict = wait::verdict(&statuses(client, &ids).await?);
        if !matches!(verdict, wait::Verdict::Waiting(_)) {
            break verdict;
        }
        let waited = started.elapsed();
        if waited >= bound {
            break verdict;
        }
        tokio::time::sleep(wait::interval(waited).min(bound - waited)).await;
    };
    let said = wait::said(&verdict, &asked);
    if verdict == wait::Verdict::Done {
        println!("{said}");
    } else {
        bail!(said);
    }
    Ok(())
}

pub(crate) async fn focus(client: &Client, json: bool, args: FocusArgs) -> Result<()> {
    let FocusArgs { ids, period, clear } = args;
    if clear {
        let req = client.request(reqwest::Method::DELETE, "/api/focus");
        let was = client.send(req).await?.unwrap_or(json!({}));
        emit(json, &was, || match was["was"].is_null() {
            false => println!("focus ended — your prompt shows everything open again"),
            true => println!("there was no focus on"),
        });
    } else if ids.is_empty() {
        let req = client.request(reqwest::Method::GET, "/api/focus");
        let focus = client.send(req).await?.unwrap_or(Value::Null);
        emit(json, &focus, || match parse_focus(&focus) {
            Some(focus) => println!("{}", describe(&focus)),
            None => println!(
                "not focused — your prompt shows everything open. \
                 `task focus <id>… --for 4h` narrows it."
            ),
        });
    } else {
        let period = period.context(
            "how long? `--for 4h`. There is no default: the expiry is what makes \
             hiding an open task safe.",
        )?;
        let period = focus::parse(&period)?;
        let body = json!({
            "tasks": ids.iter().map(|id| id.id()).collect::<Vec<_>>(),
            "minutes": period.num_minutes(),
        });
        let req = client
            .request(reqwest::Method::POST, "/api/focus")
            .json(&body);
        let focus = client.send(req).await?.unwrap_or(Value::Null);
        emit(json, &focus, || match parse_focus(&focus) {
            Some(focus) => println!("{}", describe(&focus)),
            // A 2xx this can't read is an API disagreement; never report "focused"
            // unconfirmed.
            None => println!("the service accepted the focus but did not describe it"),
        });
    }
    Ok(())
}
