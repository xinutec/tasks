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
task list --mine --done   # YOUR list — every task you hold, finished or not
task list --mine          # just what is still on your plate
task show <id>            # one task: its prose, and its history
task list                 # everyone's open work, every repo — not yours
```

⚠ **`task list` is not your list.** It is every open task in the service,
across every repo, and it does not honour the repo claim the way the prompt
digest does — that filtering happens in the hook, not here. `--mine` is the
one that answers "what am I holding".

Nothing to set up: the CLI reads `$CLAUDE_CODE_SESSION_ID`, which is already in
every shell you run, so it knows which conversation it is and files your changes
against you. It is installed by home-manager and lives in the nix profile — if
`task` is not found, or says *"a token but no session id"*, the fix is
`~/.config/home-manager/switch.sh` rather than anything in your shell.

Your old numbers were kept **where they were free**. They were unique only
inside your own session, and 124 numbers were claimed by more than one — so 178
of the 620 (29%) had to move; open tasks got first refusal, and 114 of 125 are
on the number they had. Every migrated task says what it used to be called:

```sh
task show 79
# - [ ] #79   Exercise the share-sheet upload against Isis  [recall]  (recall)
#   was recall#79            ← so this is recall's #79, not health's
```

⚠ **Four sessions had a `#79`.** If a number you remember now shows a subject you
do not, that is why — and the `was` line is how you tell in one command.

**If a number moved, use the old name — it still works.** Every command that
takes a task accepts `79`, `#79`, or `recall#79`:

```sh
task show recall#79       # the one recall called #79, wherever it lives now
task done health#12
```

That pair is the permanent handle. The service's own id can be renumbered; what
a session called it cannot.

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
task move <id> me               # hand it to Pippijn
task move <id> <session-id>     # hand it to another conversation
task move <id> nobody           # put it back in the pile
task add "One line" --repo <repo> --body - --to nobody   # body on stdin
task edit <id> --body -         # rewrite the prose
```

⚠ **A subject is one line and at most 200 characters**, because the subject is
the only part that reaches a prompt — and it reaches one on every turn for as
long as the task is open. Everything else goes in the body, which is read only
when somebody opens the task. The service refuses a subject that is really a
body, and says so.

**`task done` puts your name on it.** Finishing a task makes you its holder, so
every list afterwards says who did it — pass `--to me` (or anyone) in the same
breath if it should go somewhere else instead.

⚠ **Delete nothing to "tidy up".** A finished task is kept here on purpose:
under the old file scheme it was deleted because git recorded the finishing
better, and there is no git behind a database. `task done` is the whole of it.

## Which repos you see

The hook shows the repos **you have claimed**, not everything:

```sh
~/Code/xinutec-infra/mac-mini/claude_tasks.py --session <your-id> --claim ~/Code/one ~/Code/two
```

Your own id is `$CLAUDE_CODE_SESSION_ID` — `echo` it, or read it off the hook,
which prints it when you have never claimed anything. `--claim none` if you want
to be left alone.

⚠ **The claim moves your PROMPT, not your list.** A session that has claimed
nothing gets an empty digest, which looks exactly like a failed migration —
while `task list --mine` still shows everything it holds. Check with the CLI
before concluding anything is lost.

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
