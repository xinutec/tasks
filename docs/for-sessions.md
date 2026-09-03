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
task list --handed-out    # what YOU filed and somebody else is holding
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

Which leaves five questions with five names. `task list` is *what could I pick
up*; `--mine` is *what am I holding*; `--pile` is *what is going spare*; `--all`
is *what is going on*, and it is the expensive one on purpose.

⚠ **`--handed-out` is the fifth, and the only one that is not about who is
holding a task.** It asks *what did I file that somebody else now has* — and it
exists because those are precisely the tasks you stop being able to see. Your
prompt shows your own work and the pile and deliberately never another
conversation's, so the moment you route something to `memview` it leaves your
sight. `--to memview` is not the same question: that is everything memview
carries, from every filer. This is yours, wherever it went — the pile included,
since a task you left for whoever picks it up is out of your hands too.

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
task add "One line" --priority P2 --body -   # yours, by default
task add "One line" --priority P1 --to nobody --spare "why it is nobody's"
                                    # the pile: refused without a reason
task add "One line" --unassessed    # not yours to judge; whoever takes it decides
task add "One line" --priority P2 --no-duplicate-check   # skip the check below
task edit <id> --prepend "DONE in <sha>."  # add ABOVE the body, keeping all of it
task edit <id> --append -       # add BELOW it; `-` reads stdin
task edit <id> --body -         # REPLACE the prose entirely — this is how you consolidate
task edit <id> --no-density-check  # skip the read a grown body gets, below
task edit <id> --priority P0    # rank one that already exists
task edit <id> --blocked-on 697 --blocked-on 14   # what it waits for
task edit <id> --unblock        # it is not waiting any more
task edit <id> --due 2026-09-01 # when it has to be done by
task edit <id> --no-due         # no deadline after all
task close 42 / task update 42  # the same as `done` / `edit`, since sessions type these
task checks                     # what the two model checks have been doing
```

⚠ **`--body` replaces everything; `--prepend` is how you record an outcome.**
Closing a task usually means writing a paragraph and keeping thousands of
characters of filing, and `--body` deletes the filing unless you read it out and
paste it back first. On 2026-08-15 that read was skipped twice in one afternoon
by the session that maintains this tool — once caught by the guard, once at 52%
kept, which is under the threshold and which nothing catches:

    task edit 42 --prepend "DONE in a2c3ab6 — deployed, verified on prod."

Above rather than below, because a body grows in the order things happened and
what is still true sinks to the bottom. The addition is resolved server-side
against the row it locks, so two conversations adding to one task cannot lose
each other's text — do NOT rebuild this by reading the body and sending it back.

⚠ **A task you deal with is YOURS, without your having to say so.** Filing one
takes it on, and `task start` claims it **out of the pile** — the same way `task
done` already put your name on it. It never takes one off somebody else: if a
task is already held, starting it moves the status and nothing else, and taking
it on properly is `task move <id> me`. If you want it in the pile instead, say
`--to nobody` AND `--spare "<why>"` — the pile is a decision, and since
2026-09-03 one you have to argue: it was corrected 47 times out of 47, so a
reason is now required and a bare `--to nobody` is refused.

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

### What is safe to cut, measured rather than judged

⚠ **Tested 2026-09-01 on twelve bodies.** Each was cut, then a question set was
written from the ORIGINAL and answered twice — once by a reader holding only the
original, once by a reader holding only the cut — neither knowing the other
existed. 72 question-pairs. Two cuts of 65-77% came out AHEAD of their originals
and two tied, so a large cut is not itself the risk.

**Do not ask "what is missing".** The first attempt compared the two texts and
asked what the short one lacked. A reader holding the original always finds
absences and always rationalises them: on one task it named six things as needed
and the paired test showed the real losses were two, neither on its list. Ask
instead whether a reader can ANSWER the same question from either.

Safe to cut, no regression across twelve bodies:

  * **Chronology** — `REPHRASED 2026-08-10`, `STOPPED BEFORE BUILDING IT`, the
    order in which you learned things.
  * **Restating the subject** in the first paragraph.
  * **Narrative colour about your own process** — who pushed back, what you
    nearly concluded.
  * **Measurement tables** where one line is the operative number. Keep that
    line.

⚠ **NOT safe, and each cost an answer:**

  * **A rejected alternative.** Cutting the argument is right; cutting the FACT
    that it was on the table and lost means it gets re-proposed.
  * **A DECISION wearing the clothes of a derivation.** "Retention needs no
    policy, the FK cascades" reads as something you could work out, and cutting
    it left a reader unable to say whether pruning was in scope — because it is
    not a derivation, it is a decision not to build one.

    ⚠ **The test is not *is this obvious*, it is *does this stop somebody doing
    a wrong thing*.** `// increment the counter` above `n += 1` stops nothing and
    goes. A sentence that forecloses work stays, however derivable it looks. I
    had this as "obvious things are dangerous to cut", which is backwards: what
    was dangerous was mislabelling a constraint as obvious.
  * **Who a decision belongs to**, and whether it may be overruled.
  * **A commit sha.** The DATE is chronology; the SHA is the artefact pointer,
    and a reader wants it so it does not redo shipped work.
  * **The open tail.** Every one of the four worst cuts was a body that settled
    something AND left something open. Cutting the narrative took the open half
    with it.

⚠ **A layered body actively misleads, which is worse than being long.** Two
originals held a settled answer at the top and a superseded "still open" section
below it; readers given the full text concluded the dead question was live, and
readers given the cut got it right. If you find both in one body, the fix is not
to hoist — it is to delete the stale layer.

## When a body has grown without anyone rewriting it

⚠ **`--prepend` is cheap and reading is not, so bodies accrete.** Once a task's
body has gained 3,000 characters since the last edit that made it smaller, your
next `task edit` puts the whole thing to a model against three rules and prints
what it found. It has already written your edit: this is advice on stderr, it
refuses nothing, and `--no-density-check` skips the wait.

The three rules, which are also under `task edit --help`:

1. Every paragraph earns its place one of three ways: it tells the holder what
   to do, it is the evidence for that, or it records a refutation that stops
   somebody redoing dead work.
2. No claim without its measurement, and no sentence that only restates one.
3. Deletion beats compression — look for what a later section has superseded,
   not for words to trim.

**The answer is to rewrite, not to trim.** #749 went from 16,405 characters to
3,928 because 76% of it was superseded, including a section headed *"read this
before the 08-12 table below, which is stale"* — a session had noticed and
prepended anyway. Nothing of value was lost. Rewording would have saved a fifth
of that.

⚠ **Do NOT aim for "as few words as possible".** Cut to a word budget and the
numbers go first, because prose reads like the argument — and a body is believed
for its measurements. Aim for density and let the length follow.

## What the tool itself is costing you

`task timings --days 7` — how long each command has been taking, busiest first.
Every row is a command somebody actually ran; nothing polls and nothing is
sampled, so an empty table means the tool went unused rather than that a probe
died. `task checks --days 7` is the same for the two model checks.

⚠ **`edit` is the expensive one and it is not the service.** Its median is a
fifth of a second and its p90 is tens of seconds, because an edit that trips the
density read waits for a model. `--no-density-check` skips it.

## Filing something that is already on the list

⚠ **You cannot see the other conversations' lists, so you will file a duplicate
eventually.** `task add` checks before it files: the local `claude` reads your
subject against every open task — except the ones you declared `--blocked-on`,
since saying a task waits for another is saying they are two — and a match
**ends the command with nothing written**. It looks like this:

```text
Error: already filed, by a model's reading of the titles — nothing was filed:
  #961  both about prepend/append operations on tasks and body change tracking
`task show <id>` to check one. If this really is different work, re-run the same command with --no-duplicate-check.
```

⚠ **Read that last line rather than moving on.** Nothing was filed, so there is
no task to come back to and no `task drop` to do — if you treat this like a
warning you have lost the filing. Two answers are open and both are one command:
`task show 961` and fold your text into that one, or re-run **the same command**
with `--no-duplicate-check`, which is what you want when it matched your topic
rather than your work. You are still holding the body; it is a re-run, not a
rewrite.

**Ask what it WOULD say, without filing:** `task add "…" --priority P4 --check-only`
runs both halves against open and closed alike, prints what a filing would meet,
and writes nothing. Use it when you are unsure whether something already exists
and do not want to find out by having a filing refused.

⚠ **A CLOSED match does not stop you — it tells you and files anyway.** The
finished and abandoned tasks are read as well, and their remedy is different, so
their message is different:

```text
this may already exist, closed — a model's reading of the titles. It was filed anyway:
  #689  k8s Dhall model generation and apply convergence check already completed — already done
`task show <id>` to read one. If it is the same work, `task reopen <id>` and close the one just filed rather than carrying two.
(read against 984 closed tasks; 11 skipped as having no body)
```

Your task exists — this is a note about it, not a refusal of it. If it is the
same work, `task reopen <id>` and drop the one you just filed; carrying two is
the thing this exists to stop. If it is not, ignore it and move on.

⚠ **`dropped` is not a verdict.** Dropping records a status and no reason, so a
dropped twin does not mean the work was rejected — read the task and find out.

**It is a guess from titles, so it is often wrong about what is related.** Two
tasks on one subsystem read as one task to it. That is the price of catching the
duplicate nobody can catch by hand, and it is paid by the override being cheap.

**Silence means it looked and found nothing.** A check that could not run says
so explicitly (`duplicate check did not run: …`) **and files the task anyway** —
only a model that actually names something refuses, because a session that
cannot write things down is worse than any duplicate. `--no-duplicate-check`
skips the wait, which is worth doing when filing a batch: it costs about 9
seconds a filing, and it costs it *before* the task exists rather than after.

⚠ **What that wait is made of, measured 2026-08-23.** Not the reading: the same
16.1 kB prompt over 181 titles, run twice at the same settings, was answered in
16.1 seconds after 1,152 output tokens and in 34.9 after 2,976, both `NONE`. It is the length of the
deliberation, which is now capped — `MAX_THINKING_TOKENS=1024` on both checks,
after an uncapped body read took 220 seconds to reach two findings that the
capped one reaches in 11. Proving that nothing matches is still the expensive
case; a real match costs a third of it. The bound is 120 seconds rather than 60
because five filings out of 280 since 2026-08-14 died on the old one.

Both checks record what they did in `check_run` — kind, size, elapsed,
outcome — so the next statement about either is a distribution rather than
three samples and a grep.

## What to do next: P0 to P4

Asked for by Pippijn on 2026-08-11. Five levels, and `task --help` glosses each
one — read them there rather than guessing, because a level two conversations
read differently ranks nothing.

⚠ **FILING ONE MEANS SAYING.** `task add` takes `--priority`, or `--unassessed`
for work that is not yours to judge — filing into another session's domain is the
ordinary case, and that is what the escape is for. Leaving both off is not an
answer and is refused, by the CLI and by the service both. Asked for by Pippijn
2026-08-11: *"I want everything to have a priority."*

⚠ **Reach for `--unassessed` rather than a reflex `P2`.** They sort identically,
so nothing is lost by being honest; what differs is the claim. `P2` says *I read
this and it is ordinary*. `--unassessed` says *I have not judged it*. A required
field whose safe answer is obvious gets filled with that answer, and then the
rank carries no information — the same failure as everything being `P0`, which is
what the levels were rewritten to prevent.

⚠ **UNASSESSED is not a sixth level.** An unassessed
task sorts exactly where `P2` does. So `P0` and `P1` rise above the untriaged and
`P3` and `P4` **sink below** it, and everything nobody has judged keeps its
place — oldest first, which is what makes old work get fixed rather than buried.
Sorting the ranked first and the rest after would have got `P4` backwards:
marking a task *when there is room* would have lifted it above four hundred
tickets nobody had read.

⚠ **`P4` has a consequence the other levels do not: it leaves your prompt.** A
task ranked `P4` is counted in the digest and never listed, because the level
says it is a record rather than a plan and reciting it every turn would say the
opposite. It is still open, still in `task list`, and a deadline or an
escalation brings it straight back. So `P4` is the right answer for work that
may never happen, and the wrong one for work you simply are not doing this week
— that is `P3`, which is still recited.

⚠ **Do not rank things to tidy up.** There were 700-odd tasks the day the column
was added and none of them were ranked; that is the correct state, and it is why
the column is nullable rather than defaulted. A rank is worth something because
most tasks do not have one. Rank a task when you have actually decided its
place, and leave the rest alone.

⚠ **There is no way to unrank.** Absence means *leave it alone* for every field
`task edit` takes, and a task ranked wrongly is corrected by ranking it again.
The service will not clear one for you.

It shows up wherever a task is drawn — `task list`, `task show`, your prompt, and
the app — and it costs nothing on a task that has none.

## What it is waiting for

⚠ **Say it as a ticket number, not in the body.** Twelve open tasks named a
blocker in prose the day this was added, in six different spellings — and **five
of the twelve named a blocker that was already closed**, so a reader who believed
the body thought the task was stuck when it was ready to start. `--blocked-on`
joins to the live row and cannot go stale that way.

⚠ **A task may not be ranked more urgently than what blocks it.** Equal is fine,
higher is refused, and the refusal names the other task. With several blockers
the bound is the LEAST urgent one still open — that is the one deciding when you
can actually start. Ranking the BLOCKER down is refused for the same reason.

⚠ **Nothing may block itself, and no loop is allowed** — not `A → B → A`, and not
`A → B → C → A`, which arrives as three edits that each look fine on their own.

⚠ **An unranked task is never in violation.** It has claimed nothing, so
recording *"this waits for that"* never forces you to rank anything first. The
rule starts applying the moment somebody states a rank.

The link is kept when the blocker closes — it is a record of how the work went —
but it stops constraining anything and stops being drawn. `⛔#697` in your prompt
means *still waiting*; nothing there means nothing is in the way.

## When it has to be done by

⚠ **Inside the last week, a deadline RAISES the rank to `P0`.** Pippijn's rule,
2026-08-11. The line reads `P0!` — the `!` says the level is not the one anybody
set, and `task show` prints both. Nothing is written: the raise is recomputed
from the date every time it is read, so it appears on its own and goes away on
its own.

⚠ **Further out than a week it reorders NOTHING.** A deadline is evidence for a
rank rather than a competing answer to what-next: how long the work takes is the
term that would decide, and nothing anywhere records it. So a far date is an
argument for ranking something up and a person makes that call; a near one is
Pippijn's stated rule, which is a decision rather than arithmetic.

⚠ **A task may not be due before something it is blocked on.** That one is
arithmetic rather than judgement — you cannot finish before the thing you are
waiting for — so it is refused, with the other task named. Equal is allowed.

Almost nothing has a deadline, and that is right: a date belongs here when
something OUTSIDE decides it. #260 is the case this was built for — *retire the
Fitbit Web API before Sep 2026* — where the rank was carrying an argument the
data could not hold.

Your prompt shows `due 2026-09-01`, or `OVERDUE 2026-09-01` once the day has
passed. The date rather than a countdown, because the digest is cached and read
minutes later, and a date is the same fact whenever it is read.

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
about what *this* conversation is doing, and there is nobody else who could make
it.

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
