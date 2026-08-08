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

## 1. Find your tasks

```sh
task list                 # everything open, in the repos you have claimed
task list --mine          # only what is assigned to you
task list --done          # including what is finished
task show <id>            # one task: its prose, and its history
```

Nothing to set up: the CLI reads `$CLAUDE_CODE_SESSION_ID`, which is already in
every shell you run, so it knows which conversation it is and files your changes
against you. It is installed by home-manager and lives in the nix profile — if
`task` is not found, or says *"a token but no session id"*, the fix is
`~/.config/home-manager/switch.sh` rather than anything in your shell.

Your old numbers were kept **where they were free**. They were unique only
inside your own session, and 124 numbers were claimed by more than one — so 46%
had to change. Every migrated task says what it used to be called:

```sh
task show 79
# - [ ] #79   Some subject   [health]
#   was health#79            ← it really is the one you remember
```

If a number moved, find it by what it was:

```sh
task list --done | grep -i 'part of the subject you remember'
```

## 2. Check the list looks like yours

Compare against `~/.claude/tasks/<your-session-id>/` before deleting anything.
The counts should match, subjects verbatim, and every `description` is now the
task's body. If something is missing, **stop and say so** — the old files are
still there, and that is the only reason recovery is possible.

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

⚠ **Delete nothing to "tidy up".** A finished task is kept here on purpose:
under the old file scheme it was deleted because git recorded the finishing
better, and there is no git behind a database. `task done` is the whole of it.

## Which repos you see

The hook shows the repos **you have claimed**, not everything:

```sh
~/Code/xinutec-infra/mac-mini/claude_tasks.py --session <your-id> --claim ~/Code/one ~/Code/two
```

Your session id is printed by the hook itself when you have never claimed
anything. `--claim none` if you want to be left alone.

## If it is not answering

The service is on isis, over the VPN. The hook keeps a cache and prints the last
known list rather than an error, so a quiet spell is invisible; the CLI will say
`no answer — check the VPN`. Nothing is lost — the list is on isis, not in your
context.

`task --help` is the authority on the commands. `README.md` beside this file is
the authority on why any of it is shaped this way.
