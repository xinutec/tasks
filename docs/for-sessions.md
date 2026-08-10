# The task list, for a session

Your tasks live in a service at `https://tasks.xinutec.org`. The open ones are
already in your context — the `UserPromptSubmit` hook injects them as the block
beginning *"N open task(s)"* — so reading them costs you nothing. `task` is how
you change them.

**Do not use the built-in task tool.** Whatever it stores is re-sent as a
`task_reminder` attachment 1.75 times per message, carrying every task ever
created with full bodies. On one session that reached 527 kB a turn, 93% of it a
`description` field the prompt never renders. That is what this replaced.

⚠ **You will be told otherwise every turn.** A system reminder suggests
`TaskCreate`/`TaskUpdate` on every message, whether or not the old store still
exists. It is wrong here, and the digest at the top of your context now says so
on every turn too — because an instruction read once cannot outweigh one
repeated every message. `task add` is how you file work.

## Two facts about how this works

Neither is guessable from the commands, and getting them wrong produces
confident, wrong tickets — one was filed and dropped the day these were written.

⚠ **A session never ends.** Conversations go quiet and come back; there is no
terminal state. Nothing here goes stale because nobody is at that keyboard, and
handing work to a conversation with no live process is **queueing** it, not
stranding it. There is nothing to apologise for and nothing to check first.

⚠ **A holder's open tasks are its FUTURE work, not its current work.** Thirty
open against a session is a backlog addressed to that conversation, not thirty
things in flight — `health` carries around fifty and is not doing fifty things.
`- [>]` is the mark for work actually in hand, and it is the only one that means
that.

Together they decide the only question worth asking when handing something over:
**whose subject is it.** Never who is online. Preferring whoever is awake piles
every task onto whichever conversation happens to be running, which is the
opposite of what an addressed list is for — and a liveness column, or a warning
on `move`, would train exactly that. Both have been proposed and refused.

## Find your tasks

```sh
task list                 # yours, and the pile — what you could pick up
task list --mine          # strictly what you are holding, without the pile
task list --pile          # strictly what NOBODY holds — what is going spare
task list --mine --done   # YOUR list — every task you hold, finished or not
task show <id>            # one task: its prose, and its history
task list --all           # every open task, whoever holds it
```

⚠ **Your prompt shows at most five of the pile.** Past that it says how many
more there are, and `task list` is where you see them. Everything *you* hold is
always there — the cap is the pile's alone, because an unheld task is in every
conversation's prompt at once and yours are only in yours. So a short pile in
your context is not evidence that the pile is short.

⚠ **`task list` is about you now** (2026-08-09), and it did not use to be. It
answered with every open task in the service — 135 lines and 12,804 bytes when
that was measured, against one line for the session that ran it — which is the
cost the digest exists to refuse, reached through the one command you run when
you want to know what to do next. It now asks the same question the digest does:
your own, and the pile.

Which leaves four questions with four names. `task list` is *what could I pick
up*; `--mine` is *what am I holding*; `--pile` is *what is going spare*; `--all`
is *what is going on*, and it is the expensive one on purpose.

⚠ **`--pile` is the one to reach for when your prompt says "N more in the
pile".** That notice is all the digest will ever say about them — five lines is
the cap — so this is where the rest are. Do not answer it by filtering `--all
--json` yourself: a session did, guessed at a `session` field that does not
exist, and reported 137 tasks going spare when there were 5.

`--json` on any read command prints what the service answered, verbatim, and
`task show <id> --body` prints the stored markdown alone — so a claim about the
data can be checked rather than parsed back out of a human format.

Nothing to set up: the CLI reads `$CLAUDE_CODE_SESSION_ID`, which is already in
every shell you run, so it knows which conversation it is and files your changes
against you. It is installed by home-manager and lives in the nix profile — if
`task` is not found, or says *"a token but no session id"*, the fix is
`~/.config/home-manager/switch.sh` rather than anything in your shell.

## What you are called

**Nothing to do.** Lists already say `health`, `observe`, `memview` rather than
36 characters of uuid, and the name is whatever Claude Code is calling this
conversation — read out of your own transcript by the CLI and sent with every
command. Rename the conversation and lists follow on the next command.

⚠ **You used to have to type it, and that is why `task rename` still exists.**
It now refuses when Claude Code has a name for you, because the derived one
would replace it on your very next command — an accepted rename that quietly
reverts. If it ever does let you through, you are a conversation with no name of
its own, and then it is the only lever there is.

The id remains the identity and the name is only what a list calls you, so none
of this moves a task: everything you hold stays yours.

Until 2026-08-10 the column was filled by `task rename` alone. The conversation
holding the most open work of any — twenty-nine tasks — had never run it, and
rendered as a raw uuid in every list and every handover, while Claude Code had
called it `memview` the whole time. Of the thirteen sessions that *had* typed a
name, thirteen had typed the one already on disk.

## Work the list

```sh
task start <id>                 # you have picked it up  (shows as - [>])
task done <id>                  # finished
task drop <id>                  # closed WITHOUT doing it (shows as - [-])
task reopen <id>                # back to open — it keeps its holder
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
it on properly is `task move <id> me`. If you want it in the pile instead, say
`--to nobody` — that is a decision rather than a default.

Until 2026-08-09 none of that was true: a holder was recorded when a task was
*closed* and at no other moment, so a session could show three finished tasks and
`0 open` while it was hours into a fourth.

⚠ **A pile line ending in `(from health)` is telling you where the work lives.**
That is the session that filed it, and it is there so you can rule a task out
without opening it — most of the pile is somebody else's subject matter, and
before this you had to spend a `task show` to find that out. Treat it as a hint:
it is silent when Pippijn filed it or the filer never named itself, and it is
wrong when one conversation files work for another, which the subject usually
gives away. `filed_by` is in `task list` and deliberately not in your prompt.

⚠ **Two parentheticals, and they are not the same thing.** `(from health)` is
who *filed* it and appears only on a pile line; a bare `(coach)` is who *holds*
it, and that one does reach your prompt. In your own digest the bare one is
always your own name — the pile has no holder — so it is what tells your work
apart from the work going spare.

⚠ **A task in the pile can be `- [>]`, and it is yours to take.** Somebody
started it and handed it back without closing it — the question is still open,
the approach is not — so the status is saying work happened, not that anybody is
doing it now. Read the body before you begin: it is where the previous holder
left what they found. `task start` takes it on, the same as any other pile task.

⚠ **`me` is you.** It used to mean Pippijn even when a session typed it, which
was the one word every conversation reached for. Handing work to the person is
`pippijn`. Nothing means him implicitly any more.

⚠ **A subject is one line and at most 200 characters**, because the subject is
the only part that reaches a prompt — and it reaches one on every turn for as
long as the task is open. Everything else goes in the body, which is read only
when somebody opens the task. The service refuses a subject that is really a
body, and says so.

⚠ **Put where it stands in the subject when it fits, and lead the body with it
when it does not.** A body is written in the order things happened, so the
current conclusion ends up last — measured 2026-08-10 across every task with a
body: #704's verdict sat 98% down its 132 lines, #697's entire plan for a new
machine was the closing paragraph of a ticket about copying a disk. Both cost a
reader the whole thing to find the one part that was still true. The good
example is #431, whose subject is *"gap 1 CLOSED, LEAN_DAY exact on 35/35 in
shadow — the `on` path is what is left"*: everybody reads that for free, on
every turn, without opening anything.

So: **the present tense at the top, the history under it.** Whoever opens this
next wants to know where it stands, not how it got here — and the second
question is only asked once the first is answered.

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

Looking wider is something you ask for: `task list --all` shows every open task
whoever holds it, and `task sessions` shows who is carrying what.

## If it is not answering

The service is on isis, over the VPN. The hook keeps a 60-second cache and
prints the last known list rather than an error, so a quiet spell is invisible in
your prompt; the CLI is blunter and reports `reaching the tasks service` with the
cause under it. Check the VPN first — that is the usual answer. Nothing is lost
either way: the list is on isis, not in your context.

That cache does **not** hide your own writes: `task add`, `task done` and every
other change drop it, so the next prompt shows what you just did. If you file
something and it is missing from your next digest, that is a real absence and
worth looking at rather than a minute of staleness.

`task sessions` shows who holds what — every session that has ever held
anything, Pippijn and the pile, as `open/total`. It is **not** every conversation
there is: a row exists for each one that has ever asked for a digest, so at the
time of writing 717 existed and 14 had held a task. `task sessions --all` is the
whole table, and the reason to want it is to find the id of a conversation that
has never been given anything, so you can hand it something. The same thing is at
<https://tasks.xinutec.org> for Pippijn, which is why a task's subject is written
to be read by somebody who is not you.

`task --help` is the authority on the commands. `README.md` beside this file is
the authority on why any of it is shaped this way.

**One command is deliberately not here: `task digest`.** It prints exactly what
your prompt receives, and it exists for measuring that cost — not for reading.
Running it to catch up on your work spends the bytes twice.
