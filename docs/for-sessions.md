# The task list, for a session

Your tasks live in a service at `https://tasks.xinutec.org`. The open ones are
already in your context — the `UserPromptSubmit` hook injects them as the block
beginning *"N open task(s)"* — so reading them costs you nothing. `task` is how
you change them.

**Do not use the built-in task tool.** Whatever it stores is re-sent into the
conversation every turn, full bodies and all, which is what this replaced.

⚠ **You will be told otherwise every turn.** A system reminder suggests
`TaskCreate`/`TaskUpdate` on every message. It is wrong here, and the digest at
the top of your context says so every turn too — an instruction read once cannot
outweigh one repeated every message. `task add` is how you file work.

## Two facts about how this works

Neither is guessable from the commands, and getting them wrong produces
confident, wrong tickets.

⚠ **A session never ends.** Conversations go quiet and come back; there is no
terminal state. Nothing here goes stale because nobody is at that keyboard, and
handing work to a conversation with no live process is **queueing** it, not
stranding it. There is nothing to apologise for and nothing to check first.

⚠ **A holder's open tasks are its FUTURE work, not its current work.** Thirty
open against a session is a backlog addressed to that conversation, not thirty
things in flight. `- [>]` is the mark for work actually in hand, and it is the
only one that means that.

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
task list --mine --done   # every task you hold, finished or not
task show <id>            # one task: its prose, and its history
task list --all           # every open task, whoever holds it
task list --handed-out    # what YOU filed and somebody else is holding
task list --to <holder>   # what one other holder is carrying
```

`task list` answers the same question the digest does: your own, and the pile.
`--all` is *what is going on*, and it is the expensive one on purpose.

⚠ **Your prompt shows only the first few of the pile.** Past that it says how
many more there are, and `task list --pile` is where you see them. Everything
*you* hold is always there — the cap is the pile's alone, because an unheld task
is in every conversation's prompt at once and yours are only in yours. So a short
pile in your context is not evidence that the pile is short. Do not count it by
filtering `--all --json` yourself: the holder is `assignee`, an object, and a
guessed `session` field matches every row.

⚠ **`--handed-out` is the only question not about who is holding a task.** It
asks *what did I file that somebody else now has* — precisely the tasks you stop
being able to see, since your prompt never shows another conversation's work.
`--to memview` is not the same question: that is everything memview carries, from
every filer. This is yours, wherever it went — the pile included.

`--json` on any read command prints what the service answered, verbatim, and
`task show <id> --body` prints the stored markdown alone — so a claim about the
data can be checked rather than parsed back out of a human format.

Nothing to set up: the CLI reads `$CLAUDE_CODE_SESSION_ID`, which is already in
every shell you run, so it knows which conversation it is and files your changes
against you. It is installed by home-manager and lives in the nix profile — if
`task` is not found, the fix is `~/.config/home-manager/switch.sh` rather than
anything in your shell.

## What you are called

**Nothing to do.** Lists show the name Claude Code is calling this conversation —
read out of your own transcript by the CLI and sent with every command. Rename the
conversation and lists follow on the next command.

⚠ **`task rename` refuses when Claude Code has a name for you**, because the
derived one would replace it on your very next command. If it lets you through,
you are a conversation with no name of its own, and then it is the only lever.

The id remains the identity and the name is only what a list calls you, so none
of this moves a task: everything you hold stays yours.

## Work the list

```sh
task start <id>                 # you have picked it up  (shows as - [>])
task done <id> [--note -]       # finished; the note goes above the body first
task drop <id> [--reason -]     # closed WITHOUT doing it (shows as - [-])
task reopen <id>                # back to open — it keeps its holder
task move <id> me               # take it — `me` is YOU, this conversation
task move <id> pippijn          # hand it to Pippijn
task move <id> <session>        # hand it to another conversation, by name or id
task move <id> nobody           # put it back in the pile
task add "One line" --priority P2 --body -   # yours, by default
task add "One line" --priority P1 --to nobody --spare "why it is nobody's"
                                    # the pile: refused without a reason
task add "One line" --unassessed    # not yours to judge; whoever takes it decides
task edit <id> --prepend "DONE in <sha>."  # add ABOVE the body, keeping all of it
task edit <id> --append -       # add BELOW it; `-` reads stdin
task edit <id> --body -         # REPLACE the prose entirely — this is how you consolidate
task edit <id> --subject "..."  # the one line every list and prompt shows
task edit <id> --priority P0    # rank one that already exists
task edit <id> --blocked-on 697 --blocked-on 14   # what it waits for
task edit <id> --unblock        # it is not waiting any more
task edit <id> --due 2026-12-01 # when it has to be done by
task edit <id> --no-due         # no deadline after all
task undo <id>                  # put back what the last edit replaced
task close 42 / task update 42  # the same as `done` / `edit`, since sessions type these
task checks                     # what the two model checks have been doing
```

⚠ **`--body` replaces everything; `--prepend` is how you record an outcome.**
Closing a task usually means writing a paragraph and keeping the filing, and
`--body` deletes the filing unless you read it out and paste it back first. That
read gets skipped, and the guard on `--body` only catches a body cut to a
fraction of itself:

    task edit 42 --prepend "DONE in a2c3ab6 — deployed, verified on prod."

Above rather than below, because a body grows in the order things happened and
what is still true sinks to the bottom. The addition is resolved server-side
against the row it locks, so two conversations adding to one task cannot lose
each other's text — do NOT rebuild this by reading the body and sending it back.

⚠ **`task undo` reverts THE last edit, not YOUR last edit** — one version is kept
per task. When the last edit was somebody else's it refuses and says whose;
`--anyway` is how to mean it.

⚠ **A task you deal with is YOURS, without your having to say so.** Filing one
takes it on, `task start` claims it **out of the pile**, and `task done` or
`task drop` puts your name on it. It never takes one off somebody else: if a task
is already held, starting it moves the status and nothing else, and taking it on
properly is `task move <id> me`. If you want it in the pile instead, say
`--to nobody` AND `--spare "<why>"` — filings to the pile were almost always
corrected later, so a bare `--to nobody` is refused.

⚠ **A pile line ending in `(from health)` is telling you where the work lives.**
That is the session that filed it, there so you can rule a task out without
opening it. Treat it as a hint: it is silent when Pippijn filed it or the filer
has no name, and wrong when one conversation files work for another, which the
subject usually gives away. It is in `task list` and deliberately not in your
prompt.

⚠ **Two parentheticals, and they are not the same thing.** `(from health)` is
who *filed* it and appears only on a pile line; a bare `(coach)` is who *holds*
it, and that one does reach your prompt. In your own digest the bare one is
always your own name — the pile has no holder — so it is what tells your work
apart from the work going spare.

⚠ **A task in the pile can be `- [>]`, and it is yours to take.** Somebody
started it and handed it back without closing it — the question is still open,
the approach is not — so the status says work happened, not that anybody is
doing it now. Read the body before you begin: it is where the previous holder
left what they found. `task start` takes it on.

⚠ **`me` is you.** Handing work to the person is `pippijn`. Nothing means him
implicitly.

⚠ **A subject is one line and at most 200 characters**, because the subject is
the only part that reaches a prompt — every turn, for as long as the task is
open. Everything else goes in the body, which is read only when somebody opens
the task. The service refuses a subject that is really a body, and says so.

⚠ **Put where it stands in the subject when it fits, and lead the body with it
when it does not.** A body is written in the order things happened, so the
current conclusion ends up last — a verdict at the foot of a long ticket, or a
plan in the closing paragraph of a ticket about something else. A subject like
*"gap 1 CLOSED, exact on 35/35 in shadow — the `on` path is what is left"* is
read by everybody, every turn, without opening anything.

So: **the present tense at the top, the history under it.** Whoever opens this
next wants to know where it stands, not how it got here.

⚠ **And the subject is MAINTAINED, not chosen once.** An edit that moves where
the task stands rewrites the subject in the SAME edit. A `--prepend` that changes
the body and leaves the subject asking a question the body has answered leaves
the one line that reaches a prompt every turn saying the old thing.

⚠ **Closing is a subject edit too.** A closed subject is what the duplicate check
and every later search see first, and it is read by people who will never open
the task.

A close with nothing written since the task last moved says so, with the `task
edit` to run. `task done --note -` writes and closes in one command.

### What is safe to cut

Tested by cutting bodies and asking one reader holding only the original and
another holding only the cut the same questions. Large cuts were not the risk:
several came out ahead of their originals.

**Do not ask "what is missing".** A reader holding the original always finds
absences and always rationalises them, and names the wrong ones. Ask instead
whether a reader can ANSWER the same question from either.

Safe to cut:

  * **Chronology** — `REPHRASED 08-10`, `STOPPED BEFORE BUILDING IT`, the order
    in which you learned things.
  * **Restating the subject** in the first paragraph.
  * **Narrative colour about your own process** — who pushed back, what you
    nearly concluded.
  * **Measurement tables** where one line is the operative number. Keep that
    line.

⚠ **NOT safe:**

  * **A rejected alternative.** Cutting the argument is right; cutting the FACT
    that it was on the table and lost means it gets re-proposed.
  * **A DECISION wearing the clothes of a derivation.** "Retention needs no
    policy, the FK cascades" reads as something you could work out, but it is a
    decision not to build one, and cutting it leaves scope unanswered.

    ⚠ **The test is not *is this obvious*, it is *does this stop somebody doing
    a wrong thing*.** `// increment the counter` above `n += 1` stops nothing and
    goes. A sentence that forecloses work stays, however derivable it looks.
  * **Who a decision belongs to**, and whether it may be overruled.
  * **A commit sha.** A date is chronology; the SHA is the artefact pointer, and a
    reader wants it so it does not redo shipped work.
  * **The open tail.** A body that settled something AND left something open
    loses the open half when the narrative is cut.

⚠ **A layered body actively misleads, which is worse than being long.** A settled
answer on top of a superseded "still open" section below it makes readers of the
full text conclude the dead question is live. The fix is not to hoist — it is to
delete the stale layer.

⚠ **`--prepend` is how bodies get that way, and it is the cheap thing to do.**
Putting the correction on top costs one command; deleting what it refutes costs
reading the rest. Leading with the verdict does not fix a layered body — it is
what produces one. When you prepend a correction, delete what it corrects in the
same edit.

### Closing it says how it came out

⚠ **`task done` over a body that ends at "the fix, roughly" is a plan wearing a
finished status.** The outcome goes in before the close — what was done, and the
sha, so nobody redoes shipped work: `task done <id> --note "DONE in <sha>. …"`.

⚠ **A tail left open at close needs a ticket number, not a sentence.** "Still
open: X" inside a closed body is invisible from every list there is — no prompt
recites it, no `task list` shows it, and the duplicate check will not match it.
Many closed bodies carry such a phrase, and some of them really are owed work
with no ticket.

⚠ **But read, do not grep.** A later section can answer an earlier one — a
"SIGNED OFF" section further down, or *"reopen this if that trade is worth
taking"*, a deliberate park naming its own condition. The phrase is a place to
look and the body is the evidence; getting this wrong invents work for somebody.

So the test is not whether you mention it. It is whether a reader who never opens
this task can find it. File it, or say why it is deliberate — and let the close
mean closed.

## When a body has grown without anyone rewriting it

⚠ **`--prepend` is cheap and reading is not, so bodies accrete.** Once a task's
body has grown past a threshold since the last edit that made it smaller, your
next `task edit` puts the whole thing to a model against three rules and prints
what it found. It has already written your edit: this is advice on stderr, it
refuses nothing, and `--no-density-check` skips the wait. What it said stays on
the task — `task show` prints it, and your prompt marks the task `[sprawl …]` —
until an edit makes the body smaller or a later read finds it dense.

The three rules, which are also under `task edit --help`:

1. Every paragraph earns its place one of three ways: it tells the holder what
   to do, it is the evidence for that, or it records a refutation that stops
   somebody redoing dead work.
2. No claim without its measurement, and no sentence that only restates one.
3. Deletion beats compression — look for what a later section has superseded,
   not for words to trim.

**The answer is to rewrite, not to trim.** A body that is mostly superseded by its
own later layers can lose most of its length and nothing of value; rewording saves
a little.

⚠ **Do NOT aim for "as few words as possible".** Cut to a word budget and the
numbers go first, because prose reads like the argument — and a body is believed
for its measurements. Aim for density and let the length follow.

## What the tool itself is costing you

`task timings --days 7` — how long each command has been taking, busiest first.
Every row is a command somebody actually ran; nothing polls, so an empty table
means the tool went unused rather than that a probe died. `task checks --days 7`
is the same for the two model checks.

⚠ **`edit` is the expensive one and it is not the service.** An edit that trips
the density read waits for a model, so its p90 is many times its median.
`--no-density-check` skips it.

## Filing something that is already on the list

⚠ **You cannot see the other conversations' lists, so you will file a duplicate
eventually.** `task add` checks before it files: a subject identical to an open
one is refused outright, and the local `claude` reads your subject against every
open and closed task — except the ones you named in `--blocked-on` or
`--blocks`, since saying a task waits for another is saying they are two. A match
**ends the command with nothing written**:

```text
  #961  both about prepend/append operations on tasks and body change tracking
NOT FILED — a model reading the open titles says this is already one of them. `task show <id>` to check one, or re-run the same command with --no-duplicate-check if this really is different work.
```

⚠ **Read that last line rather than moving on.** Nothing was filed, so there is
no task to come back to and no `task drop` to do — if you treat this like a
warning you have lost the filing. Two answers, both one command: `task show 961`
and fold your text into that one, or re-run **the same command** with
`--no-duplicate-check`, which is what you want when it matched your topic rather
than your work. You are still holding the body; it is a re-run, not a rewrite.

⚠ **`--no-duplicate-check` is for overruling a refusal you have just seen, and
only that.** Passed without one, the service refuses the filing and tells you to
drop the flag.

⚠ **A CLOSED match stops you too, and the way past it is `task reopen`.** The
remedy differs, so the message does:

```text
  #689  k8s Dhall model generation and apply convergence check already completed — already done
NOT FILED — a model reading the closed titles says this work already exists. `task reopen <id>` if it is the same work and carry on in that task, or re-run the same command with --no-duplicate-check if it really is different (read against 984 closed tasks; 11 skipped as having no body).
```

Nothing was filed here either. Two answers, both one command: `task reopen 689`
and carry on in that task, or re-run **the same command** with
`--no-duplicate-check` when it matched your topic and not your work.

⚠ **`dropped` is not a verdict.** A drop records a status and no reason, so a
dropped twin does not mean the work was rejected — read the task and find out.

**Ask what it WOULD say, without filing:** `task add "…" --priority P4
--check-only` runs both halves, prints what a filing would meet, and writes
nothing. A clean subject prints `NONE`; a match comes back as the refusal itself,
on the same exit code a real filing would get.

**It is a guess from titles, so it is often wrong about what is related.** Two
tasks on one subsystem read as one task to it. That is the price of catching the
duplicate nobody can catch by hand, and it is paid by the override being cheap.

**Silence means it looked and found nothing.** A check that could not run says
so explicitly (`duplicate check did not run: …`) **and files the task anyway** —
only a model that actually names something refuses, because a session that
cannot write things down is worse than any duplicate. The check costs seconds,
and it costs them *before* the task exists rather than after.

**Naming a task you closed today gets a reopen hint.** If the subject or body
cites `#89` and you closed #89 in the last day, the filing lands and says so,
with both commands: `task reopen 89`, and `task drop` for the task just filed. A
follow-up to finished work usually belongs in the old task.

## What to do next: P0 to P4

Five levels, and `task --help` glosses each one — read them there rather than
guessing, because a level two conversations read differently ranks nothing.

⚠ **FILING ONE MEANS SAYING.** `task add` takes `--priority`, or `--unassessed`
for work that is not yours to judge — filing into another session's domain is the
ordinary case, and that is what the escape is for. Leaving both off is refused,
by the CLI and by the service both. Pippijn: *"I want everything to have a
priority."*

⚠ **Reach for `--unassessed` rather than a reflex `P2`.** They sort identically,
so nothing is lost by being honest; what differs is the claim. `P2` says *I read
this and it is ordinary*. `--unassessed` says *I have not judged it*. A required
field whose safe answer is obvious gets filled with that answer, and then the
rank carries no information — the same failure as everything being `P0`.

⚠ **UNASSESSED is not a sixth level.** An unassessed task sorts exactly where
`P2` does. So `P0` and `P1` rise above the untriaged and `P3` and `P4` **sink
below** it, and everything nobody has judged keeps its place — oldest first,
which is what makes old work get fixed rather than buried.

⚠ **`P4` leaves your prompt.** A task ranked `P4` is counted in the digest and
never listed, because nothing is being paid for it today. It is still open, still
in `task list`, and a deadline brings it straight back. So `P4` is right for real
work that costs nothing to defer, and wrong for work you simply are not doing
this week — that is `P3`, which is still recited.

⚠ **`P4` is not a shelf for decided questions.** A settled ticket is CLOSED —
`done` if it answered what it asked, `drop` if it was decided against — and
`task list --done` still finds it. An open task is work.

⚠ **Do not rank things to tidy up.** Most tasks are unranked, and that is the
correct state — it is why the column is nullable rather than defaulted. Rank a
task when you have actually decided its place, and leave the rest alone.

⚠ **There is no way to unrank.** A task ranked wrongly is corrected by ranking it
again.

It shows up wherever a task is drawn — `task list`, `task show`, your prompt, and
the app — and costs nothing on a task that has none.

## What it is waiting for

⚠ **Say it as a ticket number, not in the body.** A blocker named in prose goes
stale when the blocker closes, and a reader who believes the body thinks the task
is stuck when it is ready to start. `--blocked-on` joins to the live row and
cannot go stale that way.

⚠ **A task may not be ranked more urgently than what blocks it.** Equal is fine,
higher is refused, and the refusal names the other task. With several blockers
the bound is the LEAST urgent one still open — that is the one deciding when you
can actually start. Ranking the BLOCKER down is refused for the same reason.

⚠ **Nothing may block itself, and no loop is allowed** — not `A → B → A`, and not
`A → B → C → A`, which arrives as three edits that each look fine on their own.

⚠ **An unranked task is never in violation.** It has claimed nothing, so
recording *"this waits for that"* never forces you to rank anything first.

The link is kept when the blocker closes — it is a record of how the work went —
but it stops constraining anything and stops being drawn. `⛔#697` in your prompt
means *still waiting*; nothing there means nothing is in the way.

### When the blocker is not a ticket

⚠ **Say it in the body, and that is correct rather than a workaround.** `hands on
the hardware`, `Pippijn present for device signing`, `an external image rebuild`,
`sweep after some weeks` — none of these is a task row, so `--blocked-on` has
nothing to join to. Write them in prose.

⚠ **The rule above is about a blocker that IS a ticket, and does not transfer.**
That failure needs a row whose status can change behind the sentence's back.
"Pippijn present for device signing" has no row and no status; it stays true
until it is done, and cannot be quietly contradicted.

⚠ **This is why there is no free-text blocker field.** `blocked` is derived —
*any blocker still open* — so a free-text condition would either block for ever
or need somebody to clear it by hand, and the `⛔` would stop meaning anything.

**So: if the thing you are waiting for could be a ticket, file it and link it. If
it could not, write it down and move on.**

### Waiting for it, rather than checking back

⚠ **`⛔#697` is not a notification, and it cannot be one.** It is drawn when you
take a turn, and a conversation that has stopped for a blocker is not taking
turns. Nothing reaches a session that is not speaking.

So if the blocker is genuinely what stops you, hold the wait open instead:

```
task wait 697 &        # in the BACKGROUND, then end your turn
```

Claude Code brings a session back when one of its background commands exits, so
the command returning is the wake. Nothing is delivered to you and no
conversation addresses another; you simply have an unfinished job, and it
finishes when somebody closes the task.

⚠ **Read the exit before continuing.** `0` means the blockers were *done*.
Anything else means they were not, and the reason is on stderr — a blocker that
was `drop`ped was overtaken, obsolete or decided against, so the thing you
stopped for did not happen. Resuming as if it had is the one way this makes
things worse than not waiting at all.

⚠ **It gives up after a day** unless `--for` says otherwise, and giving up is not
failure: it wakes you, names what is still open, and you can wait again. There is
no way to wait for ever, because a wait nobody finds out about is worse than a
bounded one.

The wait lives in your own process, so a Claude Code restart loses it: the
`--blocked-on` edge and the `⛔` survive, but the automatic wake does not. Not
something to plan around — Pippijn: *"Sessions are long lived on Mac, no need to
worry about that."*

## When it has to be done by

⚠ **Inside the last week, a deadline RAISES the rank to `P0`.** Pippijn's rule.
The line reads `P0!` — the `!` says the level is not the one anybody set, and
`task show` prints both. Nothing is written: the raise is recomputed from the
date every time it is read, so it appears and goes away on its own.

⚠ **Further out than a week it reorders NOTHING.** A deadline is evidence for a
rank rather than a competing answer to what-next: how long the work takes is the
term that would decide, and nothing records it. So a far date is an argument for
ranking something up, and a person makes that call.

⚠ **A task may not be due before something it is blocked on.** That is arithmetic
rather than judgement, so it is refused, with the other task named. Equal is
allowed.

Almost nothing has a deadline, and that is right: a date belongs here when
something OUTSIDE decides it — *retire the Fitbit Web API before Sep 2026* — where
the rank would otherwise be carrying an argument the data cannot hold.

Your prompt shows `due 2026-12-01`, or `OVERDUE 2026-12-01` once the day has
passed. The date rather than a countdown, because the digest is cached and read
later, and a date is the same fact whenever it is read.

## Closing it

**`task done` puts your name on it.** Finishing a task makes you its holder, so
every list afterwards says who did it — pass `--to pippijn` (or anyone) in the
same command if it should go somewhere else. `task drop` does the same, and that
is deliberate: who decided a thing was not worth doing is as much a fact as who
did it.

⚠ **A task that has gone out of date is `drop`, not `done`.** Both close it and
take it out of every prompt; the difference is that `done` credits somebody with
having done it, and a list that says that when nobody did is a list you stop
trusting. Overtaken, obsolete, decided against, superseded by how the code
actually went — all `drop`, and `--reason` says why.

⚠ **Delete nothing to "tidy up".** A closed task is kept on purpose, and
`task done` and `task drop` between them cover every reason a task should stop
appearing.

## What your prompt shows you

**Your own open tasks, and the pile.** Not what another conversation is holding,
and not by repository — there are no repos and nothing to claim. A session
holding nothing, with an empty pile, is answered with silence.

The pile is deliberately still there. It is how a task gets handed to whichever
conversation is around rather than to a named one, so it has to stay visible to
all of them — and it is the answer to "what if I re-file something already in
hand": if it is in hand it is somebody's, and if it is nobody's it is yours to
take.

Looking wider is something you ask for: `task list --all` shows every open task
whoever holds it, and `task sessions` shows who is carrying what.

## Working on two things when you are holding fifty

    task focus 849 850 --for 4h

For four hours your prompt recites those two and **counts** the rest. Use it
when you settle into something: a plate of fifty is fifty lines on every turn,
and forty-eight of them are about an afternoon that is not this one.

`task focus` alone says what you are on and how long is left. `task focus
--clear` ends it early. It ends by itself regardless — that is the point, and it
is why there is no way to say *until I say otherwise*. Over a day is refused: if
the other work is not yours this afternoon but somebody else's altogether, the
honest way to say that is `task move`, where everybody can see it.

Three things it does **not** do, and you can rely on all three:

* **`task list` is untouched.** It still shows everything, focused or not — the
  question "what should I pick up next" is never answered with silence.
* **A P0 still reaches you**, and so does anything past its deadline. A focus
  cannot bury the one task that was meant to interrupt it.
* **It hides nothing quietly.** The digest says how many it left out and how to
  stop, so a short list is never something you have to check by hand.

Nobody else can focus you, and you cannot focus anybody else: it is a claim
about what *this* conversation is doing.

## If it is not answering

The service is on isis, over the VPN. The hook keeps a short cache and prints the
last known list rather than an error, so a quiet spell is invisible in your
prompt; the CLI is blunter and reports `reaching the tasks service` with the
cause under it. Check the VPN first — that is the usual answer. Nothing is lost
either way: the list is on isis, not in your context.

That cache does **not** hide your own writes: `task add`, `task done` and every
other change drop it, so the next prompt shows what you just did. If you file
something and it is missing from your next digest, that is a real absence and
worth looking at.

`task sessions` shows who holds what — every session that has ever held
anything, Pippijn and the pile, as `open/total`. It is **not** every conversation
there is: a row exists for each one that ever asked for a digest, and most never
held a task. `task sessions --all` is the whole table, and the reason to want it
is to find the id of a conversation that has never been given anything, so you
can hand it something. The same list is at <https://tasks.xinutec.org> for
Pippijn, which is why a task's subject is written to be read by somebody who is
not you.

`task --help` is the authority on the commands. `README.md` beside this file is
the authority on why any of it is shaped this way.

**One command is deliberately not here: `task digest`.** It prints exactly what
your prompt receives, and it exists for measuring that cost — not for reading.
Running it to catch up on your work spends the bytes twice.
