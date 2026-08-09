# Your tasks moved. Here is how to find them, and how to clear the old ones.

Read this once, do the four steps, and carry on.

Your task list is no longer in `~/.claude/tasks/<your-session-id>/`. It is in a
service at `https://tasks.xinutec.org`, and the open items are already being
injected into your prompt by the `UserPromptSubmit` hook — that is the block at
the top of your context beginning *"N open task(s)"*.

**Do not use the built-in task tool.** Whatever it stores is re-sent as a
`task_reminder` attachment 1.75 times per message, carrying every task ever
created with full bodies. On one session that reached 527 kB a turn, 93% of it a
`description` field the prompt never renders. That is what this replaced.

⚠ **You will be told otherwise every turn.** A system reminder suggests
`TaskCreate`/`TaskUpdate` on every message, whether or not the old store still
exists. It is wrong here, and the digest at the top of your context now says so
on every turn too — because an instruction read once cannot outweigh one
repeated every message. `task add` is how you file work.

## 1. Find your tasks

```sh
task list                 # yours, and the pile — what you could pick up
task list --mine          # strictly what you are holding, without the pile
task list --mine --done   # YOUR list — every task you hold, finished or not
task show <id>            # one task: its prose, and its history
task list --all           # every open task, whoever holds it
```

⚠ **`task list` is about you now** (2026-08-09), and it did not use to be. It
answered with every open task in the service — 135 lines and 12,804 bytes when
that was measured, against one line for the session that ran it — which is the
cost the digest exists to refuse, reached through the one command you run when
you want to know what to do next. It now asks the same question the digest does:
your own, and the pile.

Which leaves three questions with three names. `task list` is *what could I pick
up*; `--mine` is *what am I holding*; `--all` is *what is going on*, and it is
the expensive one on purpose.

Nothing to set up: the CLI reads `$CLAUDE_CODE_SESSION_ID`, which is already in
every shell you run, so it knows which conversation it is and files your changes
against you. It is installed by home-manager and lives in the nix profile — if
`task` is not found, or says *"a token but no session id"*, the fix is
`~/.config/home-manager/switch.sh` rather than anything in your shell.

Your old numbers were kept **where they were free**. They were unique only
inside your own session, and 124 numbers were claimed by more than one — so 178
of the 620 (29%) had to move; open tasks got first refusal, and 114 of 125 are
on the number they had.

⚠ **The `recall#79` spelling is GONE (2026-08-09), and so is the `was` line.**
It existed to carry you across the migration, Pippijn confirmed every session had
made it, and a second id space that nothing needs is a second id space to keep
level. Every command takes `79` or `#79`, and that is now the whole of it.

Nothing was orphaned by the removal: the mapping was spent before the columns
were dropped. 29 tasks whose bodies carried machine-written `blockedBy` /
`blocks` numbers in the old space, and 21 further citations in ordinary prose,
were rewritten to live ids first.

**If you are holding an old number in your own notes**, and `task show <n>` shows
a subject you do not recognise, that is the four-sessions-had-a-`#79` problem and
there is no longer a lookup for it. Search instead — `task list --mine --done`
prints every task you have ever held, and the subject is what you actually
remember.

## 2. Check the list looks like yours

Compare `task list --mine --done` against `~/.claude/tasks/<your-session-id>/`
before deleting anything — one line per `<n>.json`, subjects verbatim, and every
`description` now the task's body:

```sh
# Every subject, both sides, sorted — a real diff rather than a count.
task list --mine --done --json | jq -r '.[].subject' | sort > /tmp/now
jq -r .subject ~/.claude/tasks/$CLAUDE_CODE_SESSION_ID/*.json | sort > /tmp/was
diff /tmp/was /tmp/now && echo "verbatim"
```

And a body, if you want to check one:

```sh
task show <id> --body | diff - <(jq -r .description ~/.claude/tasks/$CLAUDE_CODE_SESSION_ID/<n>.json)
```

`--json` prints what the service answered, verbatim, on any read command; the
bodies gained a footer where the import kept an `activeForm` or a `blockedBy`,
so expect that one addition and nothing else.

If something is missing, **stop and say so** — the old files are still there,
and that is the only reason recovery is possible.

## 3. Delete your built-in tasks

Only after step 2, and only your own directory:

```sh
rm ~/.claude/tasks/<your-session-id>/*.json
```

Keep `.highwatermark` and `.lock` — the CLI owns those. Deleting the JSON is
what stops the per-turn attachment; this is the whole point of the exercise.

## 4. Work the list

```sh
task start <id>                 # you have picked it up  (shows as - [>])
task done <id>                  # finished
task drop <id>                  # closed WITHOUT doing it (shows as - [-])
task move <id> me               # take it — `me` is YOU, this conversation
task move <id> pippijn          # hand it to Pippijn
task move <id> <session-id>     # hand it to another conversation
task move <id> nobody           # put it back in the pile
task add "One line" --body -    # yours, by default
task add "One line" --to nobody # for whoever picks it up
task edit <id> --body -         # rewrite the prose
```

⚠ **A task you deal with is YOURS, without your having to say so.** Filing one
takes it on, and `task start` claims it **out of the pile** — the same way `task
done` already put your name on it. It never takes one off somebody else: if a
task is already held, starting it moves the status and nothing else, and taking
it on properly is `task move <id> me`. Until 2026-08-09 none of that was true: a holder was recorded when a
task was *closed* and at no other moment, so a session could show three finished
tasks and `0 open` while it was hours into a fourth. If you want it in the pile
instead, say `--to nobody` — that is now a decision rather than a default.

⚠ **`me` is you.** It used to mean Pippijn even when a session typed it, which
was the one word every conversation reached for. Handing work to the person is
`pippijn`. Nothing means him implicitly any more.

⚠ **A subject is one line and at most 200 characters**, because the subject is
the only part that reaches a prompt — and it reaches one on every turn for as
long as the task is open. Everything else goes in the body, which is read only
when somebody opens the task. The service refuses a subject that is really a
body, and says so.

**`task done` puts your name on it.** Finishing a task makes you its holder, so
every list afterwards says who did it — pass `--to pippijn` (or anyone) in the
same breath if it should go somewhere else instead. `task drop` does the same,
and that is deliberate: who decided a thing was not worth doing is as much a fact
as who did it.

⚠ **A task that has gone out of date is `drop`, not `done`.** Both close it and
both take it out of every prompt; the difference is that `done` credits somebody
with having done it, and a list that says that when nobody did is a list you
stop trusting. Overtaken, obsolete, decided against, superseded by how the code
actually went — all `drop`. If *why* matters, write it: `task edit <id> --body -`.

⚠ **Delete nothing to "tidy up".** A closed task is kept here on purpose: under
the old file scheme it was deleted because git recorded the finishing better,
and there is no git behind a database. `task done` and `task drop` are the whole
of it — between them they cover every reason a task should stop appearing.

## What your prompt shows you

**Your own open tasks, and the pile.** Not what another conversation is holding.
Until 2026-08-09 it was every open task in the repos you had claimed, regardless
of holder, which came from the file scheme rather than from a decision: one
`TASKS.md` per repo meant both parties' work sat in one file. It cost every
session, on every turn, a description of work it could not act on.

⚠ **There is nothing to claim any more, and no repos.** The column went in
`0004`. A session spans checkouts — fleet work is `xinutec-infra` and
`nixos-config` together — so it was never a question with one answer, and a
session that had never claimed got an empty digest that looked exactly like a
broken service. Nothing to configure, no `--claim`. A session holding nothing,
with an empty pile, is answered with silence, which is what `--claim none` used
to be for.

The pile is deliberately still there. It is how a task gets handed to whichever
conversation is around rather than to a named one, so it has to stay visible to
all of them — and it is the answer to "what if I re-file something already in
hand": if it is in hand it is somebody's, and if it is nobody's it is yours to
take.

Looking wider is something you ask for: `task list` shows every open task
whoever holds it, and `task sessions` shows who is carrying what.

## If it is not answering

The service is on isis, over the VPN. The hook keeps a 60-second cache and
prints the last known list rather than an error, so a quiet spell is invisible in
your prompt; the CLI is blunter and reports `reaching the tasks service` with the
cause under it. Check the VPN first — that is the usual answer. Nothing is lost
either way: the list is on isis, not in your context.

`task sessions` shows who holds what — every session, Pippijn and the pile, as
`open/total`. The same thing is at <https://tasks.xinutec.org> for Pippijn, which
is why a task's subject is written to be read by somebody who is not you.

`task --help` is the authority on the commands. `README.md` beside this file is
the authority on why any of it is shaped this way.
