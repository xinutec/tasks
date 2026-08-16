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

⚠ **A session is shown its own open tasks and the pile — not what another
conversation is holding.** The first shape filtered on repository alone, which
was inherited rather than chosen: one `TASKS.md` per repo meant both parties'
work sat in one file because there was nowhere else to put it. Carried into a
database it made every session pay, on every turn, for tasks it could not act
on — 132 open across 13 repos, 12,371 bytes, half the budget spent describing
other people's work.

The repository itself went in `0004`, and the holder is now the whole of the
question. A session spans checkouts — fleet work is `xinutec-infra` and
`nixos-config` together — so it was never a question with one answer, and
selecting on the set a session had *claimed* made an unclaimed session's empty
digest indistinguishable from a broken service. A task handed to the right
session in the wrong repo was invisible to the receiver, and `task edit` had no
`--repo` to correct it.

The pile stays, and that is the part worth stating: it is how a task is handed to
whichever conversation is around rather than to a named one, so a digest narrowed
to strictly its own would make work Pippijn left for anybody invisible to
everybody. Looking across holders is a thing you ask for: `task list --all` for
every open task, `task sessions` for who is carrying what.

⚠ **The pile is capped in the digest at `PILE_LINES`, because it is the one part
with a different denominator.** A task a session holds is in one conversation's
prompt; an unheld one is in *every* conversation's, on every turn — so a single
line left for whoever picks it up costs as many prompts as there are live
sessions. This was first argued as affordable from *3 unheld of 134 open*, and
that is a **condition** rather than a property: two days after the cutover the
recall session's digest carried 5 pile lines against its own 3, with nothing
keeping the number down. `MAX_BYTES` is not that guard — it stops a runaway at
some two hundred lines, per session, which the pile would reach only after weeks
of being ruinous. Past the cap the digest says how many more are in the pile and
names `task list`, which is the handover intact and the cost bounded. What a session
*holds* is never capped: growth there is a backlog for that conversation to work
off, not a charge on everybody else.

⚠ **The CLI's default is the same selection, as of 2026-08-09.** `task list`
used to mean every open task there is, which put the cost the digest refuses
behind the one command a session runs to decide what to do next — 135 lines and
12,804 bytes, against one for the session that ran it. It now answers the
digest's question: your own, and the pile. `--mine` drops the pile, `--all` is
the old behaviour, and both are named because all three are real questions.

⚠ **A session can narrow its own digest further, for a few hours** —
`task focus 849 850 --for 4h`, and for four hours its prompt recites those two
and counts the rest. It answers the case the cap above does not: a conversation
holding fifty open tasks pays for all fifty on every turn and is working on two
of them, and nothing else in the service lets it say which two. `src/tasks/focus.rs`
is the only thing here that hides an **open** task, so three rules hold and none
of them is negotiable:

* **It expires**, and cannot be set to "until I say otherwise". A focus nobody
  clears stops applying at its hour; longer than a day is refused, because that
  is not an afternoon's work but a handover, and `task move` is how work changes
  hands where everybody can see it.
* **What is hidden is counted** and the notice names `task focus --clear` — the
  same rule the pile cap follows, for the same reason.
* **P0 and overdue break through**, on the effective rank rather than the chosen
  one, so a focus cannot bury the task an escalation exists to raise.

It touches the digest and nothing else: `task list` still shows everything,
because the digest is the channel nobody asked for and a list somebody typed is
one they wanted.

⚠ **What changed is what happens to a finished task.** The file scheme *deleted*
it, because keeping it is what turned 48 live items into 366, and git recorded the
completion better than a flag did. There is no git here — so the database keeps
done tasks and `task_events` records every move, and the original property is
preserved by the **query** rather than by deletion: nothing injected ever selects
a done row.

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

⚠ **A priority is absent by default, and absence is not a level.** Asked for by
Pippijn on 2026-08-11. There were 700-odd tasks the day the column was added and
none of them were going to be triaged, so a `DEFAULT 'P2'` would have had every
one of them assert something nobody said — a field that is false about most of
its rows is worse than the absent field it replaced.

⚠ **It still has to order, and the whole feature is one `COALESCE`.** Lists sort
by `COALESCE(priority, 'P2'), id`, so an unranked task sorts exactly where an
ordinary one does. `P0` and `P1` rise above the untriaged; `P3` and `P4` **sink
below** it; everything untouched keeps its id order. The obvious alternative —
ranked first, unranked after — gets `P4` backwards, lifting a task marked *when
there is room* above four hundred nobody has read. `Priority::rank` is the same
rule in Rust and `tests/priority.rs` compares the two against a real database,
because a drift between them is silent: every list still returns every task, in
an order nobody notices is wrong until the `P0` is not at the top.

⚠ **`repo::list` is the only sort in the service.** `digest::render` preserves
the order it is handed, so the prompt, the CLI and the app all inherit one rule
rather than three. The rank costs five bytes on a line that has one and nothing
at all on a line that does not, which is what makes it affordable in the digest.

⚠ **Nothing can clear a priority**, on `PATCH` or from the CLI. Absence means
*leave it alone* for every field on that endpoint, and the exception —
`Option<Option<Priority>>`, a field whose null is meaningful — costs more than
the gesture is worth. Ranking it again is the correction.

⚠ **A deadline is a DATE and it reorders nothing.** Asked for on 2026-08-11,
straight after the first full ranking pass found the gap: #260 was ranked `P0`
and does not pass the `P0` test — nothing about it accrues hourly, it fails all
at once on a date somebody else chose, and there was nowhere to write that date
down. The rank was carrying an argument the column could not hold.

`repo::list` stays the only sort. A deadline is evidence for a rank, not a
competing answer to *what next* — how long the work takes is the term that would
decide, and nothing records it, so floating a `P4` above a `P1` because a date is
near would replace a human decision with an arithmetic one. What it does instead
is show on the line and shout once the day has passed: **overdue is a fact and
needs no threshold, where "due soon" would need one**, which is why there is no
such notice.

The one constraint is the deadline twin of the rank rule: a task may not be due
before something it is blocked on. That is arithmetic rather than judgement, and
it is refused at both ends like its twin.

⚠ **A blocked task says which ticket, and the link carries a rule.** Pippijn,
2026-08-11: *"It can be the same, but not higher priority than the thing it's
blocked on."* Claiming *do this next* about work you cannot start is the single
move that inflates a scale — everything downstream drifts up while the thing
actually holding it sits at `P3` — and it is the one shape a machine can catch.
Refused at both ends: ranking the blocked task up, and demoting the blocker.
Refused rather than cascaded, because quietly re-ranking rows nobody asked about
is worse than saying no and naming the pair.

⚠ **A table, and the first cut was a column.** The measurement said no open task
named more than one blocker; that counted the absence of the feature rather than
the shape of the work, since there was nowhere to record even one. With a single
slot the workaround for a second blocker is the body — which is the staleness
this replaced, applied to the tasks with the most dependencies.

⚠ **No cycles, walked rather than checked one step.** `A → B → A` is the case
everybody thinks of; `A → B → C → A` arrives as three separate edits that each
look fine, and only a traversal of the whole graph refuses it. A loop would make
the rank rule unsatisfiable, not merely odd.

⚠ **An unranked task is never in violation, and that asymmetry is deliberate.**
It sorts as `P2` — that is the ordering — but it asserts nothing, and the rule is
about assertions. Applying it to untriaged tasks would mean recording a
dependency is refused until you rank the dependent, turning a fact into a
decision. That is the pressure that ends with everything ranked to satisfy a
field, and a value that was satisfied rather than chosen says nothing.

⚠ **A session's id is its identity and its name is an attribute.** A rename is
an `UPDATE` of one column and every task assigned to that session stays
assigned; making the name the key would have re-pointed the whole list.

⚠ **That is about storage, and it was allowed to answer a question it does not
address: where the name COMES from.** From it, "so the session pushes the name"
was taken to follow, and it does not. Until 2026-08-10 `sessions.name` was
written by `task rename` alone, so a conversation that never typed it was a uuid
for ever — including the one holding twenty-nine open tasks, which Claude Code
had been calling `memview` all along. The CLI writes
`{"type":"agent-name","agentName":"…","sessionId":"…"}` into the transcript and
appends another on every rename, so the answer was already on the disk the CLI
runs on. It now reads it and sends it with every request (`src/agent_name.rs`),
and the column is a cache of that rather than a self-report.

**Derived beats stored, and that was measured rather than assumed:** of the
fourteen sessions the service knew, thirteen had a stored name and all thirteen
matched what is derived. None disagreed, and the fourteenth was the one with
none. A stored column is still needed — a session whose transcript is
unreachable must render, and history rows are written at render time on purpose
— so this fills it, and `task rename` survives for a conversation the CLI has
not named. It refuses when there is a name to derive, because the next command
would overwrite it.

⚠ **The id is global.** The file scheme numbered per repo and its own hook
documented the cost: *"a bare `#4` means nothing when two repos both have one"*.
One id space means `task show 4` needs nothing else to resolve it — which is what
made dropping the repository in `0004` a deletion rather than a redesign.

## Views

| route | what |
| --- | --- |
| `/` | the open list, in id order, filtered by holder |
| `/t/:id` | one task: its prose, its status, who holds it, its history |
| `/new` | file one |
| `/who` | who holds what: `open/total` per session, for the person, and the pile |

⚠ **`/who` and `task sessions` answer with *holders*, not with every session
row.** A row exists for every conversation that has ever asked for a digest,
which is every conversation that has ever run: **717 of them two days after the
cutover, of which 14 had ever held anything**. Answering with all of them buries
the fourteen under seven hundred `0/0` lines on a screen meant for a phone. The
predicate is *has ever been assigned a task*, not *has anything open* — a
cleared plate still says who cleared it, and a session that dropped its whole
list decided something. `task sessions --all` is every row, which is how a
brand-new conversation's id is found in order to hand it work.

⚠ **There is deliberately no liveness anywhere here, and there must not be.** A
session never ends — conversations go quiet and come back — so a task addressed
to one with no live process is *queued*, not stranded, and its open list is the
work waiting for it rather than work in flight. `- [>]` is the only mark that
means in hand.

Neither fact was written down for the life of the project, and on 2026-08-10 a
session that had been using the tool all day inferred the opposite: it measured
which conversations had live processes (5 of 12), and filed a ticket arguing that
assigning to an offline one "silently reduces visibility", proposing a liveness
column and a warning on `move`. Both would have trained every session to prefer
whoever is online over whoever owns the work, which is the opposite of what an
addressed list is for. That ticket is #713, dropped. **The remedy is stating the
model, not warning about the consequence** — so it is now in `task --help`, in
`move --help`, and in `docs/for-sessions.md`, and a liveness signal should be
refused if it is proposed again.

## The CLI

`docs/for-sessions.md` is this same surface written for the reader who uses it
most — a Claude session — and it is the one to point a new conversation at. This
section says what the commands are; that one says which question each answers,
and which of them a session gets wrong.

```sh
task list [--all|--mine|--pile] [--done]  # yours and the pile; wider; narrower; spare
task show <id> [--body]                   # one task, its prose and its history
task sessions [--all]                     # who holds what, as open/total
<any read command> --json                 # what the service answered, verbatim
task add "<subject>" [--body -] [--to me|pippijn|<session>|nobody] [--priority P1]
task start <id> / task done <id> [--to W] # move it along
task drop <id>                            # close it without doing it
task reopen <id>                          # back to open; it keeps its holder
task move <id> me|pippijn|<session>|nobody  # hand it over
task edit <id> [--subject S] [--body -] [--priority P0]   # change the words, rank it
task edit <id> --prepend "DONE in <sha>."                 # put text ABOVE the body, keeping it
task edit <id> --append -                                 # and BELOW; `-` reads stdin
task focus <id>… --for 4h                 # for four hours my prompt shows only these
task focus [--clear]                      # what I am on, how long is left; end it now
task digest                               # exactly what a prompt receives
task rename <name>                        # tell the service what I call myself
```

**`task add` REFUSES what something already covers.** Before the filing goes
anywhere, the local `claude` is asked — as Haiku, on the caller's own
subscription rather than an API key — whether the new subject is one of the open
tasks in different words. That is the duplicate no one can catch by hand:
sessions cannot see each other's lists, and the two spellings of one problem
share no words. A match ends the command, `nothing was filed`, naming what it
matched and the override in the same breath: `--no-duplicate-check` re-runs it
past both halves, and the caller is still holding the body it tried to file.

⚠ **Only a model that names something refuses.** A failed, slow or missing
`claude`, or a list that could not be read, prints `duplicate check did not run:
…` on stderr and **files the task** — a session that cannot write things down is
worse than any duplicate. That distinction is the whole of the design; it was
advisory until 2026-08-14, and `src/tasks/duplicates.rs` carries both the
measurement that argued for advisory and the one that overruled it.

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

**A write drops the prompt hook's cached digest** (`src/hook.rs`, and
`~/.cache/claude-tasks/<session>.txt` at the other end). The hook reads that file
before the network and treats anything under a minute as current, which is right
for reading and wrong for the moment after a write: a session that files a task
and is then shown a digest without it has been given a reason to file it twice —
the one mistake the pile's visibility exists to prevent. Every non-`GET` clears
it, centrally in `Client::send` rather than per command, and silently, because
the write has already succeeded and the cost of missing it is one stale prompt.

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
| **starting** | `start` claims it **out of the pile** — never off another holder |
| **closing** | `done` and `drop` alike hand it to whoever closed it |

`assignee` is the only place a *list* can say any of this — the history records
every actor, and no list renders a history — so a task closed while held by
`nobody` read as "done by nobody" everywhere it was seen again. Dropping counts
on the same argument backwards: who decided a thing was not worth doing belongs
in a list too, and the status beside the name tells the two apart. An explicit
assignee in the same change always wins, and reopening leaves the holder alone.

**A pile row says who filed it, and that is the whole of what replaced the repo
column.** `filed_by` is the filing session's name, read out of `task_events` —
there is nothing to set and nothing that can drift, and it was known for 112 of
the 139 open tasks the day it was added. It is drawn only where there is no
holder, in the space a pile row leaves empty, and as plain text rather than the
holder's pill: a chip would say somebody has the task, which is the one thing
that row must not say.

⚠ **A hint, not a filter, and not in the digest.** Dropping the repo column
removed two things at once and only one of them was wrong. *Which sessions
should be shown this* hid work and is gone for good; *where does this work live*
is what a session needs to rule a task out, and without it that cost 2,732 bytes
of `task show` against 548 for seeing the whole pile. The digest stays silent
because most open tasks are in the pile, so a word on each is a per-task charge
on every session on every turn — `the_digest_never_says_who_filed_a_task` is the
guard, because that argument will come back wearing a good suit. And the filer is
not always the place: #683 was filed by the tasks session about memview.

⚠ **`doing` and `nobody` together is a real state, not a leftover.** A session
that stops work deliberately hands the task back without closing it — the
question is still open, the approach is not — so the status is testimony that
work happened rather than a claim that it is happening, and the body is where
the next taker finds out how far it got. #19 is the task that established it,
and it is why the starting rule reads the holder and nothing else: a
`before.status != Doing` clause survived here until 2026-08-09, which made
`start` a silent no-op on the one state where there was nobody to displace.
Refusing the state instead would have been the wrong fix — unlike a task closed
into the pile, which claims a finish nobody made, this one is true.

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

**A write answers with what it moved.** `PATCH /api/tasks/{id}` returns the task
plus `changed`: the `task_events` kinds it wrote — `status`, `assigned`,
`edited` — empty when it wrote none. The CLI prints *nothing changed — it was
already like that* under the task line.

⚠ **Reported, not refused, and the distinction is the whole design.** Three
defects in one day were writes that answered exactly like writes that had
worked: `start` on a task already `doing` in the pile (`07df813`), a rename to a
blank name (`0cf49a5`), closing into the pile (`98157f4`). Each was found by
reproducing it against a scratch task, because success and no-op were
indistinguishable. But a no-op is *often correct* — starting a task already
yours is meant to be quiet — so refusing them would trade a silent success for a
spurious failure. The event rows are the answer rather than a second list kept
level by hand: a write that records no history changed nothing, by definition.
`a_write_that_moves_nothing_says_so` holds both halves, and it is why a body
write now compares before it records.

**`--json` on any read command prints what the service answered, verbatim**, and
`task show <id> --body` prints the stored markdown alone. Both exist so a claim
about the data can be *checked* rather than parsed out of a human format with a
regex — which is what the check that verified the migration had to do, until the
health session pointed out that `wc -l` on both sides proves only the count. The JSON is reprinted rather than rebuilt here, so there is one documented
shape rather than two kept level by hand. `task digest` refuses `--json`: it
answers in text/plain deliberately, being exactly what a prompt receives.

⚠ **The shape is now written down in `--help`, because it was being guessed.** A
task is `{id, subject, status, assignee, detailed, filed_by, created_at,
updated_at, closed_at}`; the holder is `assignee`, an object of `{kind, id,
name}` with `kind` one of `session`/`person`/`nobody`, and there is no top-level
`session` field. A session hand-filtering `--all --json` assumed there was,
matched every row that lacked it, and reported **137** tasks in the pile against
a real **5** — to Pippijn, before anybody checked. A flag whose help documents
its own provenance at length and never says what it returns is half a flag.

**`--pile` exists for the same reason**: that question had no name, so it was
answered by hand. `pile=true` on the wire *widens* a session's plate to include
the unheld; `unheld=true` *narrows* to them. Both are parameters because both are
real questions, and the CLI spends a flag on telling them apart.

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
