//! The HTTP client: credentials, who is asking, and resolving a holder.

use super::*;

/// The shared secret, from the environment or the Mac's file; never argv, which
/// is visible in process listings and transcripts. The CLI's only: the pod reads
/// `AGENT_TOKEN`, and must not hold the credential it verifies.
pub(crate) fn token() -> Option<String> {
    if let Ok(value) = std::env::var("TASKS_TOKEN")
        && !value.trim().is_empty()
    {
        return Some(value.trim().to_string());
    }
    let path = std::path::Path::new(&std::env::var("HOME").ok()?)
        .join(".config")
        .join("tasks")
        .join("token");
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Which conversation this is: `$TASKS_SESSION` for scripts, else `$CLAUDE_CODE_SESSION_ID`.
pub(crate) fn session_id() -> Option<String> {
    let set = |v: Option<String>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    set(std::env::var("TASKS_SESSION").ok())
        .or_else(|| set(std::env::var("CLAUDE_CODE_SESSION_ID").ok()))
}

/// What Claude Code calls this conversation now, from the transcript
/// ([`tasks::agent_name`]); silent on failure.
pub(crate) fn called_now(session: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    tasks::agent_name::from_projects(&projects, session)
}

/// `Clone` so `main` can report the timing after the command; cheap, and
/// `reqwest::Client` shares its connection pool.
#[derive(Clone)]
pub(crate) struct Client {
    pub(crate) http: reqwest::Client,
    pub(crate) base: String,
    pub(crate) token: Option<String>,
    pub(crate) session: Option<String>,
    /// What Claude Code calls this conversation, read once per command.
    pub(crate) called: Option<String>,
}

impl Client {
    pub(crate) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if let Some(session) = &self.session {
            req = req.header("X-Session-Id", session);
        }
        // On every request, reads too, so a rename in Claude Code reaches the service by itself.
        if let Some(called) = &self.called {
            req = req.header(tasks::access::SESSION_NAME_HEADER, called);
        }
        req
    }

    pub(crate) async fn send(&self, req: reqwest::RequestBuilder) -> Result<Option<Value>> {
        self.identified()?;
        // Built first so the method is visible: any non-GET changed the list, so the
        // hook's cached digest is dropped here, centrally, where no command can forget.
        let req = req.build().context("building the request")?;
        let wrote = req.method() != reqwest::Method::GET;
        let res = self
            .http
            .execute(req)
            .await
            .context("reaching the tasks service")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            // The service's message, which says what was wrong.
            let said = match serde_json::from_str::<Value>(&body) {
                Ok(parsed) => parsed["error"].as_str().map(str::to_string).unwrap_or(body),
                // Not JSON (an ingress page, a proxy timeout): the body is the message.
                Err(_) => body,
            };
            // 4xx is the service deciding, 5xx it failing; counted apart (`commands::Ended`).
            if status.is_client_error() {
                return Err(commands::declined(format!("{status}: {said}")));
            }
            bail!("{status}: {said}");
        }
        if wrote {
            self.forget_cached_digest();
        }
        if body.trim().is_empty() {
            return Ok(None);
        }
        // An unreadable success means CLI and service disagree about the API: an error,
        // never "nothing came back".
        Ok(Some(serde_json::from_str(&body).with_context(|| {
            format!("the service answered {status} with something this CLI could not read")
        })?))
    }

    pub(crate) async fn text(&self, req: reqwest::RequestBuilder) -> Result<String> {
        self.identified()?;
        let res = req.send().await.context("reaching the tasks service")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{status}: {body}");
        }
        Ok(body)
    }

    /// A typed holder as a session id ([`holder::resolve`]), refused with the known names
    /// when unresolved.
    pub(crate) async fn resolve(&self, to: To) -> Result<Target> {
        let typed = match to {
            To::Nobody => return Ok(Target::Nobody),
            To::Me => return Ok(Target::Me),
            To::Human => return self.human().await.map(Target::Person),
            To::Session(typed) => typed,
        };
        let typed = &typed;
        // Holders first: the short list, and the usual case.
        let mut known = known_sessions(
            &self
                .send(self.request(reqwest::Method::GET, "/api/holders"))
                .await?
                .unwrap_or(json!([])),
        );
        if !known.iter().any(|(id, name)| matches(id, name, typed)) {
            // The full session list only if that didn't answer.
            known = known_sessions(
                &self
                    .send(self.request(reqwest::Method::GET, "/api/sessions"))
                    .await?
                    .unwrap_or(json!([])),
            );
        }
        let pairs: Vec<(&str, Option<&str>)> = known
            .iter()
            .map(|(id, name)| (id.as_str(), name.as_deref()))
            .collect();
        match holder::resolve(pairs, typed) {
            Holder::Session(id) => Ok(Target::Session(id)),
            Holder::Unknown(names) => bail!(
                "no session called `{typed}`, and it is not an id this service knows. \
                 Assigning it anyway would hand the task to a conversation that is not \
                 there, which leaves every list but `--all`. Known: {}",
                names.join(", ")
            ),
            Holder::Ambiguous(ids) => bail!(
                "`{typed}` answers to {} conversations, as a name or an id, so this \
                 would guess: {}. Give the full id instead — `task sessions` prints both.",
                ids.len(),
                ids.join(", ")
            ),
        }
    }

    /// The person's account, as the service's holder list names it.
    pub(crate) async fn human(&self) -> Result<String> {
        let rows = self
            .send(self.request(reqwest::Method::GET, "/api/holders"))
            .await?
            .unwrap_or(json!([]));
        person_id(&rows).context("the service names no person to hand work to")
    }

    /// Drop the hook's cached digest; silent on failure.
    pub(crate) fn forget_cached_digest(&self) {
        let (Some(session), Ok(home)) = (&self.session, std::env::var("HOME")) else {
            return;
        };
        tasks::hook::forget_digest(std::path::Path::new(&home), session);
    }

    /// Refuse before the round trip when this CLI holds a token but no identity,
    /// a certain 401. Holding neither is the service's call. Called per request, not
    /// in `main`, so a command that sends nothing needs no identity. ⚠ Tests clear
    /// both variables: passing `--session` to get past this tests nothing.
    pub(crate) fn identified(&self) -> Result<()> {
        if self.token.is_some() && self.session.is_none() {
            bail!(
                "a token but no session id: this conversation is not saying who it is. \
                 Claude Code normally sets $CLAUDE_CODE_SESSION_ID; outside it, \
                 pass --session or set $TASKS_SESSION."
            );
        }
        Ok(())
    }

    /// This conversation's id, what `me` resolves to, needed before building a request body.
    pub(crate) fn me(&self) -> Result<&str> {
        self.session.as_deref().context(
            "no session id: pass --session or set TASKS_SESSION. \
             Claude Code normally sets $CLAUDE_CODE_SESSION_ID.",
        )
    }

    pub(crate) fn writing(&self) -> Result<()> {
        if self.token.is_none() {
            bail!(
                "no token: set TASKS_TOKEN or write ~/.config/tasks/token. \
                 Writing is never anonymous, so there is no unguarded case here."
            );
        }
        if self.session.is_none() {
            bail!(
                "no session id: pass --session or set TASKS_SESSION. \
                 Claude Code normally sets $CLAUDE_CODE_SESSION_ID."
            );
        }
        Ok(())
    }
}

/// Who a task is handed to, parsed once at the argument boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum To {
    /// Back in the pile, for whoever picks it up.
    Nobody,
    /// Whoever runs this. ⚠ For a session that is itself; the person is `human`.
    Me,
    /// The person; the service says which account.
    Human,
    /// A conversation by id or name, settled by [`Client::resolve`] before sending.
    Session(String),
}

impl std::str::FromStr for To {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "nobody" | "none" | "" => To::Nobody,
            "me" | "self" | "mine" => To::Me,
            "human" => To::Human,
            id => To::Session(id.to_string()),
        })
    }
}

/// A [`To`] with the person's account and session ids known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Nobody,
    Me,
    Person(String),
    Session(String),
}

/// The assignee the API takes. `me` is resolved here: a body says what to
/// change, never who is changing it.
pub(crate) fn assignee(to: &Target, me: &str) -> Value {
    match to {
        Target::Nobody => json!({ "kind": "nobody" }),
        Target::Me => json!({ "kind": "session", "id": me }),
        Target::Person(id) => json!({ "kind": "person", "id": id }),
        Target::Session(id) => json!({ "kind": "session", "id": id }),
    }
}

/// Whether a row answers to what somebody typed, as an id or as a name.
pub(crate) fn matches(id: &str, name: &Option<String>, typed: &str) -> bool {
    id == typed || name.as_deref() == Some(typed)
}

/// The person row's id in `/api/holders`.
pub(crate) fn person_id(rows: &Value) -> Option<String> {
    rows.as_array()?
        .iter()
        .find(|row| row["kind"] == "person")?["id"]
        .as_str()
        .map(str::to_string)
}

/// One row of `/api/holders` or `/api/sessions`, reduced to what naming needs.
pub(crate) fn known_sessions(rows: &Value) -> Vec<(String, Option<String>)> {
    rows.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|row| row["kind"] != "person" && row["kind"] != "nobody")
        .filter_map(|row| {
            let id = row["id"].as_str()?.to_string();
            Some((id, row["name"].as_str().map(str::to_string)))
        })
        .collect()
}
