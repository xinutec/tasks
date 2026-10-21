//! The model checks: asking a one-shot `claude`, the duplicate and density reads.

use super::*;

/// The model for checks: the cheapest, since it shares the session's allowance.
/// Named in full, so changing it is a change to this line.
pub(crate) const CHECKER: &str = "claude-haiku-4-5-20251001";

/// How long one check may take. Outside the spread, not at its middle: run time
/// varies severalfold with deliberation, and only the tail pays for the margin.
pub(crate) const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// How much a check may deliberate. Unbounded, it costs several times longer for
/// the same findings; zero returns a fast false `DENSE` on the most tangled
/// bodies. The filing check uses it too.
pub(crate) const DELIBERATION: &str = "1024";

/// The bound for reading a body. Deliberation, not size, makes a read slow. Wider
/// than [`PATIENCE`] because it runs after the write; not much, because a run
/// that hits it produced nothing.
pub(crate) const READING: std::time::Duration = std::time::Duration::from_secs(90);

/// What this conversation's latest refusal of `subject` named, or `None` when
/// nothing has refused it.
pub(crate) async fn licence(client: &Client, subject: &str) -> Result<Option<Vec<u64>>> {
    let req = client
        .request(reqwest::Method::GET, "/api/checks/licence")
        .query(&[("subject", subject)]);
    let got = client
        .send(req)
        .await?
        .context("no licence answer came back")?;
    let said: checks::Licence =
        serde_json::from_value(got).context("reading whether a refusal licenses this")?;
    Ok(said.matched)
}

/// Every open task as `(id, subject)`, all holders (the duplicate that matters
/// is another conversation's), and the ids this session holds. Read before the
/// POST, so both checks refuse while there is nothing to undo.
pub(crate) async fn open_now(client: &Client) -> Result<(Vec<(u64, String)>, Vec<u64>)> {
    let query = list_query(true, false, false, false, false, None, None)?;
    let req = client
        .request(reqwest::Method::GET, "/api/tasks")
        .query(&query);
    let open = client.send(req).await?.unwrap_or(json!([]));
    let rows = open.as_array().map(Vec::as_slice).unwrap_or_default();
    let me = client.session.as_deref();
    let mine = rows
        .iter()
        .filter(|task| {
            task["assignee"]["kind"] == "session"
                && me.is_some_and(|me| task["assignee"]["id"] == me)
        })
        .filter_map(|task| task["id"].as_u64())
        .collect();
    let corpus = rows
        .iter()
        .filter_map(|task| Some((task["id"].as_u64()?, task["subject"].as_str()?.to_string())))
        .collect();
    Ok((corpus, mine))
}

/// Every closed task worth reading, as the cached block. A failure costs no
/// filing (empty list); the skipped count is reported ([`duplicates::worth_reading`]).
pub(crate) async fn settled_now(client: &Client) -> (Vec<duplicates::Settled>, usize) {
    let Ok(query) = list_query(true, false, false, false, true, None, None) else {
        return (Vec::new(), 0);
    };
    let req = client
        .request(reqwest::Method::GET, "/api/tasks")
        .query(&query);
    let Ok(Some(all)) = client.send(req).await else {
        return (Vec::new(), 0);
    };
    let rows = all.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut kept = Vec::new();
    let mut skipped = 0usize;
    for task in rows {
        let Some(status) = task["status"]
            .as_str()
            .and_then(|s| s.parse::<Status>().ok())
        else {
            continue;
        };
        if status.is_open() {
            continue;
        }
        let (Some(id), Some(subject)) = (task["id"].as_u64(), task["subject"].as_str()) else {
            continue;
        };
        if !duplicates::worth_reading(task["detailed"].as_bool().unwrap_or(false)) {
            skipped += 1;
            continue;
        }
        kept.push(duplicates::Settled {
            id,
            subject: subject.to_string(),
            dropped: status == Status::Dropped,
        });
    }
    (kept, skipped)
}

/// What a model names, open or closed, as already saying what is being filed.
pub(crate) async fn already_filed(
    client: &Client,
    corpus: &[(u64, String)],
    settled: &[duplicates::Settled],
    subject: &str,
) -> Result<Vec<duplicates::Match>> {
    if corpus.is_empty() && settled.is_empty() {
        return Ok(Vec::new());
    }
    let asked = duplicates::prompt(subject, corpus, !settled.is_empty());
    // The closed list isn't counted: it is the same cached bytes every filing.
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let prefix = (!settled.is_empty()).then(|| duplicates::settled_block(settled));
    let (said, elapsed_ms) = ask_with(&asked, prefix.as_deref(), PATIENCE).await;
    // Against both lists; `split` decides which this filing is told.
    let known: Vec<(u64, String)> = corpus
        .iter()
        .cloned()
        .chain(settled.iter().map(|t| (t.id, t.subject.clone())))
        .collect();
    let found = match &said {
        Ok(words) => duplicates::parse(words, &known),
        Err(_) => Vec::new(),
    };
    // Only when it named something: the key licenses an override.
    let refused = (!found.is_empty()).then(|| checks::subject_key(subject));
    recorded(
        client,
        checks::Run {
            kind: checks::Kind::Filing,
            task_id: None,
            input_chars,
            accreted: None,
            elapsed_ms,
            outcome: checks::outcome(&said, !found.is_empty()),
            subject_key: refused,
            matched: found.iter().map(|one| one.id).collect(),
            // A filing check keeps no words: there is no task yet.
            said: None,
        },
    )
    .await;
    // The caller prints the failure: a table row doesn't tell the session.
    said?;
    Ok(found)
}

/// Report one run; silent on failure, since the call is over.
pub(crate) async fn recorded(client: &Client, run: checks::Run) {
    let req = client
        .request(reqwest::Method::POST, "/api/checks")
        .json(&run);
    let _ = client.send(req).await;
}

/// Whether this process waited for a model, set where the waiting happens so
/// timeouts count.
pub(crate) static WAITED_FOR_A_MODEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Ask a one-shot session and leave nothing behind: each call leaves a
/// transcript, deleted as soon as the answer is in, failures included. `prefix`
/// is the cached part and must not vary ([`duplicates::settled_block`]); it goes
/// via a file, like the prompt ([`call`]).
pub(crate) async fn ask_with(
    prompt: &str,
    prefix: Option<&str>,
    patience: std::time::Duration,
) -> (Result<String>, u32) {
    WAITED_FOR_A_MODEL.store(true, std::sync::atomic::Ordering::Relaxed);
    let named = named();
    let carried = prefix.and_then(|text| {
        let path = std::env::temp_dir().join(format!("task-settled-{named}.txt"));
        std::fs::write(&path, text).ok().map(|()| path)
    });
    let started = std::time::Instant::now();
    let said = call(prompt, &named, carried.as_deref(), patience).await;
    // Before `discard`, which isn't part of what is measured.
    let took = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    discard(&named);
    if let Some(path) = carried {
        let _ = std::fs::remove_file(path);
    }
    (said, took)
}

/// The call itself. The prompt goes on stdin: it carries every open title, too
/// big for an argument and visible in process listings.
pub(crate) async fn call(
    prompt: &str,
    named: &str,
    prefix: Option<&std::path::Path>,
    patience: std::time::Duration,
) -> Result<String> {
    let mut command = tokio::process::Command::new("claude");
    command
        .current_dir(std::env::temp_dir())
        .arg("-p")
        .args(["--session-id", named])
        .args(["--model", CHECKER]);
    if let Some(path) = prefix {
        command.arg("--append-system-prompt-file").arg(path);
    }
    let mut child = command
        // The setting that decides a check's cost ([`DELIBERATION`]).
        .env("MAX_THINKING_TOKENS", DELIBERATION)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("no `claude` on PATH")?;
    let mut stdin = child.stdin.take().context("claude took no stdin")?;
    stdin
        .write_all(prompt.as_bytes())
        .await
        .context("writing the prompt")?;
    stdin.flush().await.context("writing the prompt")?;
    // Closed: `-p` reads to end of file.
    drop(stdin);
    let out = tokio::time::timeout(patience, child.wait_with_output())
        .await
        .with_context(|| format!("no answer in {}s", patience.as_secs()))?
        .context("waiting for claude")?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A v4 UUID for one of these calls, by hand: `rand` is already here.
pub(crate) fn named() -> String {
    let mut bytes = [0u8; 16];
    rand::fill(&mut bytes);
    // Version 4, variant 1: the CLI validates it.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Remove the call's transcript; nothing to remove is not a failure.
pub(crate) fn discard(named: &str) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    if let Some(path) = tasks::agent_name::transcript_of(&projects, named) {
        let _ = std::fs::remove_file(path);
    }
}

/// A model's read of a grown body; every failure is silence, since the edit landed.
pub(crate) async fn accreting(client: &Client, id: TaskRef, updated: &Value) {
    let Some(accreted) = updated["replaced"]["accreted"].as_u64() else {
        return;
    };
    if !density::worth_asking(accreted as usize) {
        return;
    }
    // Re-read: additions carry only fragments, and the whole document is judged.
    let req = client.request(reqwest::Method::GET, &id.path());
    let Ok(Some(task)) = client.send(req).await else {
        return;
    };
    let Some(body) = task["body"].as_str() else {
        return;
    };
    let asked = density::prompt(id.id(), accreted as usize, body);
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let (said, elapsed_ms) = ask_with(&asked, None, READING).await;
    let advice = said
        .as_ref()
        .ok()
        .and_then(|words| density::advice(words, id.id()));
    recorded(
        client,
        checks::Run {
            kind: checks::Kind::Density,
            task_id: Some(id.id()),
            input_chars,
            accreted: Some(accreted.min(u64::from(u32::MAX)) as u32),
            elapsed_ms,
            outcome: checks::outcome(&said, advice.is_some()),
            subject_key: None,
            matched: Vec::new(),
            // What the model said, not this command's wrapper.
            said: advice
                .is_some()
                .then(|| said.as_ref().ok().map(|words| words.trim().to_string()))
                .flatten(),
        },
    )
    .await;
    if let Some(advice) = advice {
        eprintln!("{advice}");
    }
}
