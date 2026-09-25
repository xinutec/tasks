# tasks

The work Claude sessions and Pippijn hand between each other. One list, reachable
from a phone and from a terminal, where a task can be moved from a person to a
conversation and back.

- **Backend:** Rust (axum) + MariaDB via sqlx. Migrations are embedded and run at
  boot behind a named lock.
- **Frontend:** Angular + Material (zoneless), `frontend/`. Self-contained fonts
  (no third-party fetches) — it must render over the VPN.
- **CLI:** `task`, the same surface for a session that has no browser.
- **Auth:** Nextcloud OAuth2 identity with a stateless HMAC session cookie for
  the person; a shared bearer token plus an `X-Session-Id` header for a session.
  Both are **inert unless configured**: without `SESSION_SECRET` the app serves
  open, which is local dev.

## Why this exists

A task list kept in a file is re-serialised into the conversation every turn,
and grows until it dominates the transcript — which is what happened to the
CLI's built-in list. The fix is a different shape: **inject an index, fetch the
content.** This service replaced a `TASKS.md`-per-repository scheme that did
that, because a file in a repository cannot say which conversation is carrying a
task, or that it has been handed back.

**The property every change here has to keep:**

> What reaches a prompt is one line per OPEN task, and nothing else.

`src/digest.rs` enforces it and is the only module whose output a hook sees.
`tests/digest.rs` is the one test file whose assertions are about cost rather
than correctness — including one that renders 4,000 tasks and fails if the
result is not still small.

## What a prompt is shown

⚠ **A session sees its own open tasks and the pile — never what another
conversation is holding.** Every session paying every turn for work it cannot act
on is the cost this service exists to refuse. There is no repository to narrow
by: a session spans checkouts, so *which repo* never had one answer.

⚠ **The pile stays**, because it is how a task reaches whichever conversation is
around rather than a named one. Strictly *mine* would make work Pippijn left for
anybody invisible to everybody. Looking across holders is something to ask for:
`task list --all`, `task sessions`.

⚠ **The pile is capped at `PILE_LINES`, because it has a different
denominator.** A held task is in one conversation's prompt; an unheld one is in
*every* conversation's, every turn. `MAX_BYTES` is not that guard — it is a
runaway stop per session. Past the cap the digest says how many more there are
and names `task list`. What a session *holds* is never capped: growth there is a
backlog to work off, not a charge on everybody else.

⚠ **`P4` is counted and never recited.** The rank means *nothing is being paid
for it today*, so it gains least from being carried every turn. It is still open
work, and the head still counts it. `P3` is deliberately not trimmed: it means a
workaround is in use, which is still a plan, and hiding it would push everything
to `P2` to stay visible.

⚠ **A session can narrow its own digest for a few hours** — `task focus 849 850
--for 4h` recites those and counts the rest. `src/tasks/focus.rs` is the only
thing that hides an **open** task, so three rules are not negotiable:

* **It expires**, and cannot be set to "until I say otherwise". Longer than a day
  is refused: that is a handover, and `task move` is how work changes hands
  where everybody can see it.
* **What is hidden is counted**, and the notice names `task focus --clear`.
* **P0 and overdue break through**, on the effective rank, so a focus cannot bury
  the task an escalation exists to raise.

A session carrying more than `FOCUS_HINT_LINES` recited lines of its own is told
`focus` exists — reachable only from `--help`, it goes unused. The line states
what `focus` does and stops there. Focus touches the digest and nothing else:
`task list` still shows everything, because a list somebody typed is one they
wanted.

⚠ **Every trim is counted, never silent.** The head gives the full number, so a
short list is explained where it appears.

**The CLI's bare `task list` is the digest's selection**: your own, and the pile.
`--mine` drops the pile, `--pile` is strictly the unheld, `--all` is everything.
⚠ **`--handed-out` is the one selection not about the holder**: what *you* filed
that somebody else now has. Narrowing every view to the caller makes a routing
session blind to its own output, so this answers from the `created` event's
actor rather than from `assignee`.

Done tasks are kept, and `task_events` records every move; the property above is
held by the **query** — nothing injected selects a closed row — not by deletion.

## The model

| thing | what it is |
| --- | --- |
| a **task** | a one-line subject, a markdown body, a status, a holder, and maybe a priority |
| a **status** | `open`, `doing`, `done`, `dropped` — two open states and two ways out |
| a **holder** | nobody, the person, or a session |
| a **priority** | `P0`–`P4`, or none at all — which is the ordinary case |
| a **block** | tasks this one waits for, in `task_blocks` — usually none |
| a **deadline** | a `DATE`, when something outside decides — usually none |
| a **session** | a Claude Code conversation, identified by the CLI's session id |

⚠ **Status and holder are independent**, which is why there are four states and
not seven. "New" is `open` with no holder; "assigned" is `open` with one;
"accepted" is `doing`. One ladder would make handing a task back to the pile a
*status* change that un-says that somebody started it — and work here moves
between a person and a conversation repeatedly.

⚠ **`dropped` is a closed task that was never done**: overtaken, out of date, or
decided against. Without it an obsolete task stays open for ever, or is closed as
`done` and credits somebody with work nobody did. There is no *reason* field: a
reason is prose, and `task drop --reason` writes it above the body.

⚠ **A priority is absent by default, and absence is not a level.** A
`DEFAULT 'P2'` would have every unranked task assert something nobody said.

⚠ **It still has to order, and the whole feature is one `COALESCE`.** Lists sort
by `COALESCE(priority, 'P2'), id`, so an unranked task sorts where an ordinary one
does: `P0` and `P1` rise above the untriaged, `P3` and `P4` **sink below** it,
and everything untouched keeps its id order. "Ranked first, unranked after"
would lift *when there is room* above everything nobody has read.
`Priority::rank` is the same rule in Rust, and `tests/priority.rs` compares the
two against a real database, because a drift between them is silent.

⚠ **`repo::list` is the only sort in the service.** `digest::render` preserves
the order it is handed, so the prompt, the CLI and the app inherit one rule.

⚠ **Nothing can clear a priority.** Absence means *leave it alone* for every
field on `PATCH`, and a meaningful null (`Option<Option<Priority>>`) makes every
client guess. Ranking again is the correction.

⚠ **A deadline is a DATE, and it reorders nothing — except inside the last
week**, where it raises the task to `P0` (Pippijn's rule). The raise is derived
at read time and never written, drawn as `P0!` so it does not look chosen, and
`task show` gives both levels. Further out, a date is evidence for a rank, not a
competing answer to *what next* — how long the work takes would decide, and
nothing records it. Overdue is shown as a fact; there is no "due soon", which
would need a threshold.

⚠ **A blocked task names its blockers, and the link carries rules.** Pippijn:
*"It can be the same, but not higher priority than the thing it's blocked on."*
Claiming *do this next* about work you cannot start is the move that inflates a
scale, and one a machine can catch. Refused at both ends — ranking the blocked
task up, and demoting the blocker — rather than cascaded, because silently
re-ranking rows nobody asked about is worse than naming the pair. With several
blockers the bound is the LEAST urgent open one. A due date earlier than an open
blocker's is refused too: that is arithmetic, not judgement.

* **A table, not a column**: with one slot, a second blocker goes in the body and
  goes stale there.
* **No cycles, walked over the whole graph**: `A → B → C → A` arrives as three
  edits that each look fine.
* **An unranked task is never in violation.** The rule binds a claim, and
  recording *"this waits for that"* must not require ranking anything first.
* **Kept when the blocker closes**, as a record of how the work went; it stops
  constraining anything and stops being drawn.

⚠ **A session's id is its identity and its name is an attribute.** A rename is an
`UPDATE` of one column and no task moves. The name is not a self-report: the CLI
reads what Claude Code calls the conversation out of its own transcript
(`src/agent_name.rs`) and sends it with every request, so the column is a cache.
`task rename` refuses when there is a name to derive, because the next command
would overwrite it.

⚠ **The id is global**, so `task show 4` needs nothing else to resolve it.

## Views

| route | what |
| --- | --- |
| `/` | the open list, in the backend's order, filtered by holder |
| `/t/:id` | one task: its prose, its status, who holds it, its history |
| `/new` | file one |
| `/who` | who holds what: `open/total` per session, for the person, and the pile |

⚠ **`/who` and `task sessions` answer with *holders*, not every session row.** A
row exists for every conversation that ever asked for a digest, and nearly all
of them never held anything. The predicate is *has ever been assigned a task*,
not *has anything open*, so a cleared plate still says who cleared it.
`task sessions --all` is every row — how a brand-new conversation's id is found.

⚠ **There is deliberately no liveness anywhere here, and there must not be.** A
session never ends — conversations go quiet and come back — so a task addressed
to one with no live process is *queued*, not stranded, and its open list is the
work waiting for it. `- [>]` is the only mark that means in hand. A liveness
column or a warning on `move` would train every session to prefer whoever is
online over whoever owns the work; **the remedy is stating the model** — in
`task --help`, `move --help` and `docs/for-sessions.md` — and a liveness signal
should be refused if it is proposed again.

## The CLI

`docs/for-sessions.md` is this surface written for the reader who uses it most —
a Claude session — and is the one to point a new conversation at. This section
says what the commands are and why they are shaped so; `task --help` is the
authority on the flags.

```sh
task list [--all|--mine|--pile] [--done]  # yours and the pile; wider; narrower; spare
task list --handed-out                    # what you filed and another holder has
task show <id> [--body]                   # one task, its prose and its history
task sessions [--all]                     # who holds what, as open/total
<any read command> --json                 # what the service answered, verbatim
task add "<subject>" [--body -] [--to me|pippijn|<session>|nobody] [--priority P1]
                                          # `--to nobody` needs `--spare "<why>"`
task start <id> / task done <id> [--to W] [--note -]   # move it along
task drop <id> [--reason -]               # close it without doing it
task reopen <id>                          # back to open; it keeps its holder
task move <id> me|pippijn|<session>|nobody  # hand it over
task wait <id>… [--for 4h] &              # in the BACKGROUND: block until they close,
                                          # and the command returning wakes this session
task edit <id> [--subject S] [--body -] [--priority P0]   # change the words, rank it
task edit <id> --prepend "DONE in <sha>."                 # put text ABOVE the body, keeping it
task edit <id> --append -                                 # and BELOW; `-` reads stdin
task focus <id>… --for 4h                 # for four hours my prompt shows only these
task focus [--clear]                      # what I am on, how long is left; end it now
task digest                               # exactly what a prompt receives
task checks / task timings                # what the model checks and the commands cost
```

### The duplicate check

**`task add` refuses what something already covers.** Before the filing is sent,
the local `claude` (the cheapest model, on the caller's own subscription) is
asked whether the subject is one of the open tasks in different words — the
duplicate nobody can catch by hand, since sessions cannot see each other's lists.
An exact subject is caught by string equality first. A match ends the command
with `NOT FILED`, naming what it matched. **`--no-duplicate-check` overrules a
refusal you have just seen, and only then**: the service refuses the flag unless
this session was recently refused that exact subject, because passed
pre-emptively it means the check never runs. The caller still holds the body, so
overruling is one re-run.

⚠ **Only a model that names something refuses.** A failed, slow or missing
`claude`, or a list that could not be read, prints `duplicate check did not run:
…` and **files the task** — a session that cannot write things down is worse than
any duplicate.

**It reads the closed tasks too, and those refuse as well — with a different
remedy.** The closed list rides in the cached half of the call, and a filing that
repeats finished work is sent to reopen it:

```text
  #689  k8s Dhall model generation and apply convergence check already completed — already done
NOT FILED — a model reading the closed titles says this work already exists. `task reopen <id>` if it is the same work and carry on in that task, or re-run the same command with --no-duplicate-check if it really is different (read against 984 closed tasks; 11 skipped as having no body).
```

The closed half is the weaker reader, so it refuses correct filings more often;
that is the price of not leaving the cleanup to whoever notices later. The
verdict is always the LAST line, because sessions pipe this to `tail -3`.

* **`dropped` asserts no decision.** The status carries no reason, and a model
  asked about one will invent it — so both closed statuses refuse alike and point
  at the task.
* **A closed row with no body is not read** — mostly this tool's own probes.
  The filter is what a row *says*, never how fast it closed. What it skips is
  counted and printed.
* **A declared edge is exempt**: a task named in `--blocked-on` or `--blocks` is
  not compared, because a blocker resembles what it unblocks by construction.
* **`--check-only`** runs both halves and files nothing, so the check can be
  tried without probe rows in a shared tracker.

### The density read

**A body that has grown without being rewritten is read back to you.** Once one
has gained `density::SAMPLER` characters since the last edit that made it
smaller, `task edit` puts it to the local model against the three rules `task
edit --help` prints, and says what came back on stderr. It never refuses: the
edit has landed, so a missing, slow or unreadable `claude` is silence, and
`--no-density-check` skips the wait. The unit is characters since the last
consolidation — a size cannot tell a body just rewritten from one that has
doubled unread. What a read said is stored on the task, shown by `task show`, and
marked `[sprawl …]` in the digest until an edit makes the body smaller or a
later read finds it dense.

**Both checks cap how long the model may deliberate** (`MAX_THINKING_TOKENS=1024`).
Uncapped, a read runs many times longer to reach the same findings; at zero, the
most tangled bodies come back `DENSE` in seconds — a false all-clear on the task
that most needs the read.

**Both checks write down what they did** (`check_run`): kind, characters put to
the model, elapsed, and `quiet`, `spoke`, `timeout` or `error`. The table has no
foreign keys, because an instrument its subject can refuse or delete is not one.
`task checks` folds the last week into one line per kind, the abandoned calls
included — leaving them out makes a bound that fires look comfortable.

**The tool records what it costs you, from real use.** Every invocation writes its
wall clock to `command_run`; `task timings` reads it back. Nothing polls: a timer
times a command nobody runs, from a cold process. One real command an hour
carries the numbers to fleetwatch, chosen by a conditional `UPDATE` so concurrent
sessions cannot all send.

### Identity, the token and the cache

`TASKS_TOKEN`, or `~/.config/tasks/token`, is the shared secret. **Never on
argv** — a token in a command line is in every process listing and in the
transcript of the session that typed it.

**Identity needs no setup.** Claude Code sets `$CLAUDE_CODE_SESSION_ID` in every
shell it runs; `--session`, then `$TASKS_SESSION`, override it. A session cannot
forget to say who it is, nor mistype another conversation's id. There is no
anonymous mode for reads either, and a token without a session id is refused by
the CLI with a message naming the missing half.

**A write drops the prompt hook's cached digest** (`src/hook.rs`, and
`~/.cache/claude-tasks/<session>.txt` at the other end). The hook serves its last
answer for a short while, which is right for reading and wrong right after a
write: a session shown a digest without the task it just filed has a reason to
file it twice. Every non-`GET` clears it, centrally in `Client::send`, silently.

**Naming a task.** Every command that takes one accepts `79`, or `#79` as the
digest prints it — a session copying an id out of its own context must not be
corrected for it.

### Who holds a task

**A task belongs to whoever is dealing with it, and the service works that out.**
Three moments infer a holder, sharing one function (`actor_holder`):

| moment | the rule |
| --- | --- |
| **filing** | a new task is the filer's, unless the call says where it goes |
| **starting** | `start` claims it **out of the pile** — never off another holder |
| **closing** | `done` and `drop` alike hand it to whoever closed it |

`assignee` is the only place a *list* can say who did something — no list
renders a history — so a closed task must not read as done by nobody, and a
closed task cannot be handed to the pile. An explicit assignee always wins, and
reopening leaves the holder alone.

⚠ **Nothing means Pippijn implicitly.** `me` is whoever is running the command;
handing work to the person is `pippijn`.

⚠ **The pile is argued for.** `--to nobody` needs `--spare "<why>"` beside it:
filings to the pile were almost always corrected later, so an unheld task is
something stated rather than a way of not choosing. A reason WITHOUT the pile is
refused too — it would say nothing true about a held task.

⚠ **`doing` and `nobody` together is a real state, not a leftover.** A session
that stops work hands the task back without closing it — the question is still
open, the approach is not — so the status says work happened, and the body says
how far. This is why the starting rule reads the holder and nothing else: a rule
reading the status would make `start` a silent no-op there.

**A pile row says who filed it.** `filed_by` is the filing session's name, read
from `task_events`, so there is nothing to set and nothing to drift. It is drawn
only where there is no holder, as plain text rather than the holder's pill. It is
a hint about where the work lives, not a filter, and **never in the digest**,
where a word on every pile line would be a charge on every session every turn —
`the_digest_never_says_who_filed_a_task` is the guard. And the filer is not
always the place: one conversation files work for another.

### What a write answers

⚠ **"Open" is `Status::is_open`, never `status <> 'done'`**, which counts a
dropped task as open, silently. `still_open!` is the only place the vocabulary
appears in SQL, and `a_dropped_task_is_not_open_anywhere` holds the Rust and SQL
halves together.

**A write answers with what it moved.** `PATCH /api/tasks/{id}` returns the task
plus `changed`: the `task_events` kinds it wrote, empty when it wrote none, and
the CLI prints *nothing changed — it was already like that*. **Reported, not
refused**: a no-op is often correct — starting a task already yours is meant to
be quiet — but must not answer exactly like a write that worked.
`a_write_that_moves_nothing_says_so` holds both halves.

**An edit that replaced text says whose, and when**, with `task undo` beside it.
It refuses nothing — sessions rewrite each other's words by standing permission —
but a writer told the text was rewritten recently by somebody else can stop.
`task undo` restores the one previous version kept per task, and refuses without
`--anyway` when the edit it would revert is somebody else's.

**`--json` on any read command prints what the service answered, verbatim**, so
a claim about the data can be checked rather than parsed out of the human
format; `task show <id> --body` prints the stored markdown alone. The JSON is
reprinted, not rebuilt, so there is one shape. The holder is `assignee`, an
object of `{kind, id, name}` — there is no top-level `session` field, and
guessing one matches every row. `task digest` refuses `--json`: it is exactly
what a prompt receives, in text/plain, and prints its byte count on stderr —
the per-turn cost of the whole system.

### Installing it

```sh
nix build .#task            # just the binary, here
```

On the Mac it is installed through home-manager (`pippijn/mac-config`), pinned to
this repo's committed HEAD. ⚠ **A commit here is not an installed CLI**:
`~/.config/home-manager/switch.sh` re-locks and activates, and until it runs every
session holds the previous build. The gate has a row for the package, so the
flake cannot rot unnoticed between switches.

## Who may do what

| credential | is | may |
| --- | --- | --- |
| Nextcloud session cookie | the person | everything but a focus, which belongs to a conversation |
| `AGENT_TOKEN` + `X-Session-Id` | that session | everything, but it may rename and focus only itself |

⚠ **The actor is derived from the credential, never from the request body.** A
write says what to change, not who is changing it, so a session cannot file
history as though Pippijn had moved a task: there is no field to put it in.

⚠ **`AGENT_TOKEN` authenticates the machine, not the conversation.** Every session
on the Mac reads the same file, so one can act as another by declaring a
different id. No boundary is lost — they run as one user and can read each
other's transcripts anyway — but it must never be relied on as per-session
authentication.

## Run (dev, Mac)

```sh
cd frontend && pnpm install && pnpm run build   # once, and after UI changes
./scripts/dev.sh                                # → http://192.168.1.81:8092
```

`scripts/dev.sh` brings up a throwaway MariaDB for the length of the run; set
`DATABASE_URL` to point at a lasting one instead. `ng serve` (in `frontend/`)
proxies `/api` to `127.0.0.1:8092` for UI work.

## Environment

| var | default | meaning |
| --- | --- | --- |
| `DATABASE_URL` | (required) | full `mysql://` DSN |
| `BIND_ADDR` | `0.0.0.0:8092` | listen address |
| `STATIC_DIR` | unset | built SPA to serve; unset = API-only |
| `AGENT_TOKEN` | unset | the shared secret a session presents; unset = the agent API is closed |
| `SESSION_SECRET` | unset | enables the browser wall; HMAC key for cookies |
| `NC_BASE_URL` / `NC_CLIENT_ID` / `NC_CLIENT_SECRET` / `NC_REDIRECT_URI` | — | NC OAuth2 client (required once the wall is up) |
| `NC_INTERNAL_URL` | unset | server-side NC base (cluster Service DNS; sends `Host:` of `NC_BASE_URL`) — the isis hairpin fix |
| `ALLOWED_USERS` | — | comma-separated NC user allow-list; fail-closed |

## Verify

```sh
nix run ../dev-lint#gate -- . gate.json   # what the pre-commit hook runs
./scripts/setup-hooks.sh                  # install the hook, once per clone
```

`gate.dhall` is the gate; `gate.json` is rendered from it and committed, so
running the gate needs no `dhall`, and one of the checks re-renders and diffs the
two.

**Two rows carry the weight.** `tests` brings up a throwaway MariaDB, because the
SQL here is runtime strings and running the queries is the only check on them —
and the tests **panic rather than skip** when no database is supplied, so a
hand-run cannot report green with none of the SQL exercised. `ui-check` renders
every screen at Pixel width and asserts no text collides and nothing spills past
the right edge.

⚠ **Geometry is not sight.** A truncated chip label, or a hint drawn over a
field's border rather than over text, passes every measurement. So `pnpm run
shots` renders every screen to `ui-snapshots/` for a person to look at. It is
deliberately **not** a gate row: it asserts nothing, and a check nobody reads is
worse than none.

## Deployment

`https://tasks.xinutec.org` — isis, over the WireGuard VPN, behind the Nextcloud
sign-in wall. The manifests are generated from the Dhall model in the
infrastructure repo (`kubes/dhall/apps/tasks.dhall` → `kubes/tasks/k8s/`); that
repo's `tasks/README.md` is the authority on the deployment.

⚠ **A push is not a deploy.** `:latest` is a fixed string and nothing watches it;
`kubectl -n tasks rollout restart deploy/tasks` is required, and manifest changes
need the yaml applying rather than a rollout.
