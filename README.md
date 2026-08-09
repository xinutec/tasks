# tasks

The work Claude sessions and Pippijn hand between each other. One list, reachable
from a phone and from a terminal, where a task can be moved from a person to a
conversation and back.

- **Backend:** Rust (axum) + MariaDB via sqlx. Migrations are embedded and run at
  boot behind a named lock.
- **Frontend:** Angular 22 + Material (zoneless), `frontend/`. Self-contained
  fonts (no third-party fetches) — it must render over the VPN.
- **CLI:** `task`, the same surface for a session that has no browser.
- **Auth:** Nextcloud OAuth2 identity with a stateless HMAC session cookie for
  the person; a shared bearer token plus an `X-Session-Id` header for a session.
  Both are **inert unless configured**: without `SESSION_SECRET` the app serves
  open, which is local dev.

## Why this exists

Every task list this project has had was thrown away for the same reason: it was
re-serialised into the conversation on every turn. The CLI's built-in list
reached **527 kB a turn** on one session — 93% of it a `description` field the
prompt never renders — and became 73% of a 3.7 GB transcript. The fix was never a
shorter list; it was a different shape: **inject an index, fetch the content.**

That produced the file scheme this service replaces — a `TASKS.md` per repository
with bodies in `docs/tasks/<id>.md`, injected by a `UserPromptSubmit` hook
(`xinutec-infra/mac-mini/claude_tasks.py`, whose docstring is the authority on
the measurements). It worked, and it could not do the one thing wanted next: a
task belongs to a *holder*, and a file in a repository has no way to say that a
particular conversation is carrying it, or that it has been handed back.

**The constraint survives the move, and it is the property every change here has
to keep:**

> What reaches a prompt is one line per OPEN task, and nothing else.

`src/digest.rs` is where that is enforced, and it is the only module whose output
a hook ever sees. `tests/digest.rs` is the only test file in the repository whose
assertions are about cost rather than correctness — including one that renders
4,000 tasks and fails if the result is not still small.

⚠ **A session is shown its own open tasks and the pile, in the repos it claimed
— not what another conversation is holding.** The first shape filtered on
repository alone, which was inherited rather than chosen: one `TASKS.md` per repo
meant both parties' work sat in one file because there was nowhere else to put
it. Carried into a database it made every session pay, on every turn, for tasks
it could not act on — 132 open across 13 repos, 12,371 bytes, half the budget
spent describing other people's work.

The pile stays, and that is the part worth stating: it is how a task is handed to
whichever conversation is around rather than to a named one, so a digest narrowed
to strictly its own would make work Pippijn left for anybody invisible to
everybody. Looking across holders is a thing you ask for — `task list --repo R`
for every holder in a repo, `task sessions` for who is carrying what.

⚠ **What changed is what happens to a finished task.** The file scheme *deleted*
it, because keeping it is what turned 48 live items into 366, and git recorded the
completion better than a flag did. There is no git here — so the database keeps
done tasks and `task_events` records every move, and the original property is
preserved by the **query** rather than by deletion: nothing injected ever selects
a done row.

## The model

| thing | what it is |
| --- | --- |
| a **task** | a one-line subject, a markdown body, a status, and a holder |
| a **status** | `open`, `doing`, `done`, `dropped` — two open states and two ways out |
| a **holder** | nobody, the person, or a session |
| a **session** | a Claude Code conversation, identified by the CLI's session id |
| a **repo** | which checkout the work is in, as a bare name; may be absent |

⚠ **Status and holder are independent, and that is why there are four states and
not seven.** "New" is `open` with no holder; "assigned" is `open` with one;
"accepted" is `doing`. Chaining those into one ladder would have made handing a
task back to the pile a *status* change, which would then have to un-say that
somebody had started it — and the whole point of this service is that work moves
between a person and a conversation repeatedly.

⚠ **`dropped` is a closed task that was never done**, for one that has been
overtaken, has gone out of date, or has been decided against. It exists because
the two alternatives are both worse: leaving it open for ever, or closing it as
`done` and having every later list credit somebody with work nobody did. It buys
nothing anywhere else — nothing injected selects a closed row of either kind —
and there is deliberately no *reason* field beside it, because a reason is prose
and the body is where prose goes.

⚠ **A session's id is its identity and its name is an attribute.** A session
renames itself as its job changes and pushes the new name here; that is an
`UPDATE` of one column, and every task assigned to it stays assigned. Making the
name the key would have re-pointed the whole list on a rename.

⚠ **The id is global, not per repository.** The file scheme numbered per repo and
its own hook documented the cost: *"a bare `#4` means nothing when two repos both
have one"*. One id space means `task show 4` needs no repo.

⚠ **A repo filter never returns the tasks that belong to no repo.** A session asks
by repo, so that is what keeps Pippijn's own items — which have no checkout — out
of every prompt.

## Views

| route | what |
| --- | --- |
| `/` | the open list, grouped by repository, filtered by holder |
| `/t/:id` | one task: its prose, its status, who holds it, its history |
| `/new` | file one |
| `/who` | who holds what: `open/total` per session, for the person, and the pile |

## The CLI

```sh
task list [--repo R] [--mine] [--done]    # what is open
task show <id> [--body]                   # one task, its prose and its history
task sessions                             # who holds what, as open/total
<any read command> --json                 # what the service answered, verbatim
task add "<subject>" [--repo R] [--body -] [--to me|pippijn|<session>|nobody]
task start <id> / task done <id> [--to W] # move it along
task drop <id>                            # close it without doing it
task move <id> me|pippijn|<session>|nobody  # hand it over
task edit <id> [--subject S] [--body -]   # change the words
task digest [--repo R]                    # exactly what a prompt receives
task rename <name>                        # tell the service what I call myself
```

`--body -` reads stdin, which is how a session writes a long one without fighting
shell quoting. `TASKS_TOKEN`, or `~/.config/tasks/token`, is the shared secret.
**Never on argv** — a token in a command line is in every process listing on the
machine and in the transcript of the session that typed it.

**Identity needs no setup.** Claude Code sets `$CLAUDE_CODE_SESSION_ID` in every
shell it runs, and the CLI reads it: `--session`, then `$TASKS_SESSION`, then
that. A session therefore cannot forget to say who it is, nor mistype *another*
conversation's id into its own history. There is no anonymous mode for reads
either — the service needs both halves of the credential to answer at all — so a
bare token gets a 401 that names the missing half rather than the generic one
that once read as a bad token.

### Installing it

```sh
nix build .#task            # just the binary, here
```

On the Mac it is installed through home-manager (`pippijn/mac-config`) like every
other tool, pinned to this repo's committed HEAD. ⚠ **A commit here is not an
installed CLI**: `~/.config/home-manager/switch.sh` re-locks and activates, and
until it runs every session is holding the previous build. The gate has a row for
the package, so the flake cannot rot unnoticed between switches.

**Naming a task.** Every command that takes one accepts `79`, or `#79` as the
digest prints it — the hash is accepted because the digest puts one on every
line of every prompt, and a session copying an id out of its own context must
not be corrected for it.

⚠ **There was a second spelling, and spending it was the work of retiring it.**
`recall#79` named a task by what a session called it before the migration, held
resolvable by `origin_session` / `origin_number`, because 178 of the 620
imported tasks could not keep their number. Pippijn confirmed on 2026-08-09 that
every session had moved, so the columns went — but only after every reference
that depended on them was rewritten to a live id: 29 machine-written
`blockedBy` / `blocks` footers and 21 citations in ordinary prose. Deleting the
mapping first would have turned all fifty into dead references with nothing
failing. `migrations/0003_drop_origin.sql` records what was checked.

**A task belongs to whoever is dealing with it, and the service works that out
rather than waiting to be told.** Three moments infer a holder, all meaning the
same thing by it and sharing one function (`actor_holder`):

| moment | the rule |
| --- | --- |
| **filing** | a new task is the filer's, unless the call says where it goes |
| **starting** | moving one into `doing` claims it **out of the pile** — never off another holder |
| **closing** | `done` and `drop` alike hand it to whoever closed it |

`assignee` is the only place a *list* can say any of this — the history records
every actor, and no list renders a history — so a task closed while held by
`nobody` read as "done by nobody" everywhere it was seen again. Dropping counts
on the same argument backwards: who decided a thing was not worth doing belongs
in a list too, and the status beside the name tells the two apart. An explicit
assignee in the same change always wins, and reopening leaves the holder alone.

⚠ **Nothing means Pippijn implicitly.** `me` is whoever is running the command,
so for a session it is that conversation; handing work to the person is
`pippijn`, which says so. It read the other way round until 2026-08-09, together
with a default of the pile on filing and a `start` that claimed nothing — three
separate places where a session's own work was not its own. The visible symptom
was a conversation showing `0 open` while it was hours into a task, because a
holder was recorded on the way out and at no other time.

⚠ **The pile is a decision now, not a default.** `--to nobody` — or "nobody" in
the form — is how work is left for whoever picks it up, which is how Pippijn
hands a task to no conversation in particular. It is still the second thing a
digest carries, deliberately: see below.

⚠ **"Open" is `Status::is_open`, never `status <> 'done'`.** Six queries spelled
it the second way, which was the same thing until it wasn't: a dropped task would
have gone on counting as open in the list, the filter bar and all three `/who`
tallies, and none of them would have failed. `still_open!` is now the only place
that vocabulary appears in SQL, and `a_dropped_task_is_not_open_anywhere` is what
holds the Rust and the SQL halves together.

**`--json` on any read command prints what the service answered, verbatim**, and
`task show <id> --body` prints the stored markdown alone. Both exist so a claim
about the data can be *checked* rather than parsed out of a human format with a
regex — which is what the migration check in `docs/for-sessions.md` had to do
until the health session pointed out that `wc -l` on both sides proves only the
count. The JSON is reprinted rather than rebuilt here, so there is one documented
shape rather than two kept level by hand. `task digest` refuses `--json`: it
answers in text/plain deliberately, being exactly what a prompt receives.

`task digest` prints the byte count on stderr. That number is the per-turn cost of
the whole system, and it is the one worth watching.

## Who may do what

| credential | is | may |
| --- | --- | --- |
| Nextcloud session cookie | the person | everything |
| `AGENT_TOKEN` + `X-Session-Id` | that session | everything except renaming another session |

⚠ **The actor is derived from the credential, never from the request body.** A
write says what to change; it does not get to say who is changing it. That is what
stops a session filing history as though Pippijn had moved a task, and it is
prevented by there being no field to put it in.

⚠ **`AGENT_TOKEN` authenticates the machine, not the conversation.** Every session
on the Mac reads the same value out of the same file, so one holding it can act as
another by declaring a different id. That is not a boundary being lost — they run
as one user on one machine and can read each other's transcripts anyway — but it
must not be described as per-session authentication, because a later change might
rely on that.

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
hand-run cannot report green with none of the SQL exercised.
`ui-check` renders every screen at Pixel width and asserts no text collides and
nothing spills past the right edge.

⚠ **Geometry is not sight, and this app proved it on its first render.** The
layout harness passed while shipping two defects a screenshot makes obvious: a
holder chip capped so tight that `memview` rendered as `memv…`, and a two-line
`mat-hint` overflowing Material's one-line subscript slot onto the field below —
text over a *border*, which no text-overlap check will ever call a collision. So
`pnpm run shots` renders every screen to `ui-snapshots/` for a person to look at.
It is deliberately **not** a gate row: it asserts nothing, and a check nobody
reads is worse than none.

## Deployment

`https://tasks.xinutec.org` — isis, over the WireGuard VPN, behind the Nextcloud
sign-in wall. The manifests are generated from the Dhall model in the
infrastructure repo (`kubes/dhall/apps/tasks.dhall` → `kubes/tasks/k8s/`); that
repo's `tasks/README.md` is the authority on the deployment.

⚠ **A push is not a deploy.** `:latest` is a fixed string and nothing watches it;
`kubectl -n tasks rollout restart deploy/tasks` is required, and manifest changes
need the yaml applying rather than a rollout.
