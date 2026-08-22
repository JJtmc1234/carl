# Army Runtime

This started as a design note written before anything existed, saying what had been decided so
that the phase which built it would argue with a document rather than with somebody's memory.
The first slice is now built, so this is no longer that note. It is a description of what runs,
followed by the decisions the note left open and what was actually chosen, followed by what is
still not built.

Command Panel v1 showed an army that did not run. Every agent state it drew came from folders and
a journal that a chain run wrote and then exited. The runtime is what makes the army a thing that
is up rather than a thing that is invoked.

## What is built

One dedicated long running `claude` process per agent, supervised, with memory on disk.

```text
  army/nora/identity.json     who this is, forever                    never rewritten
  army/nora/memory/           what she keeps between conversations    hers to write
  run/agents/a-....json       which session, which process            the supervisor's
  run/events.jsonl            what happened, in order                 append only
  the claude process          the thing doing the work                replaceable
```

**Three lifetimes, and each outlives the one below it.** That column is the whole design. The
agent id is minted once when the agent is given a folder and never changes. The session outlives
any number of processes and is what `--resume` continues. The process is the most disposable
thing in the system and is expected to be replaced, which costs the agent nothing because the
conversation it was serving is resumed into the next one.

Squashing any two of those together is how a restart quietly becomes a different agent, or how a
replaced process quietly loses a conversation.

### The pieces

| where | what it is |
|---|---|
| `army::personnel::identity` | the agent id, and the journal record that created it |
| `army::personnel::memory` | the folder, and the one sentence an agent is told for free |
| `army::runtime::record` | what is known about one agent's process |
| `army::runtime::policy` | what to do next about it, pure |
| `army::runtime::continuity` | how much of what an agent had is still with it |
| `army::runtime::store` | the records on disk, with one writer |
| `army::personnel::hours` | when an agent is meant to be off |
| `army::runtime::lock` | one supervisor per home |
| `army::runtime::supervisor` | the part that spawns, kills, wakes and carries a message |
| `carl supervise` | the loop |
| `etc/systemd/carl-army.service` | the unit that keeps the loop alive and starts it at boot |

### The properties worth knowing

**The runtime record is not in the agent's folder.** An agent writes its own `state.json` every
turn. A process record next to that file would be a thing an agent could write down about itself
and be believed. It lives under `run/`, next to the journal, which is the other thing in this
system that describes agents and is not written by them.

**Records are named by agent id, not by name.** Names are expected to become changeable. A record
keyed by name would orphan the first time one did, and the orphan would look exactly like an
agent that had never been started.

**A pid is not a name.** Every record carries the process start time from `/proc/<pid>/stat`
alongside the pid, so a reused pid is not mistaken for a survivor. A zombie is not running either,
which matters more than it sounds: a child that has exited and not been reaped keeps its pid, its
`/proc` entry and its start time, so a supervisor comparing start times alone watches an agent
exit and calls it healthy forever, kept alive by its own failure to reap it.

**One supervisor per home, enforced.** Two would each read the other's records, find processes
they did not own, decide those were orphans of a dead supervisor, and end them. All night. Both
behaving exactly as designed. The lock is a pid file carrying a start time, so a lock left by a
process that was killed is recognised as stale rather than held forever by whatever inherits
the pid.

**The supervisor knows nothing about work.** There is no way from it to give an agent a task. It
can start a process, end one, wake a stopped agent when told what for, and carry a sentence
somebody else composed. A test reads the supervisor's own source and fails if the words `Task`,
`Board`, `delegate` or `Status` appear in it.

**One journal, one numbering.** The records under `run/agents/` answer what is true now. The
journal answers what happened, and "the worker crashed, and then the task was reported finished"
is a sentence somebody has to be able to read in order. Two files cannot be read in order, so
there is one, and the sequence numbering is locked so two writers cannot claim one place in it.

### The unit

`carl-army.service` is a user unit, like the other three, and it is the only one with no
business with a microphone or a screen, so it can genuinely run with nobody logged in. That
needs lingering, which `install.sh` explains.

Two numbers are wider than the other units', for the same reason. A supervisor restart is not
free: every start ends the agent processes the last one left and starts replacements resuming
their conversations, which is four `claude` startups. So it waits thirty seconds rather than
five, and five restarts in ten minutes is the limit rather than five in two.

**Stopping the unit stops every agent with it**, and that is the default rather than something
clever. systemd signals the whole control group, which is the supervisor and every process it
started. Without it, a stop would kill only the supervisor and leave four models running with
nothing able to talk to them, because their pipes went with their parent. Verified rather than
read off the manual page: run under a transient unit, four agents appear in the cgroup, and
stopping it leaves none.

`install.sh` installs the unit always and starts it only once an army has been founded.
Starting a supervisor on a home with no agents gives a service saying "nobody to run" every few
seconds forever, and enabling it by surprise starts four models that stay up and bill for it.

### Hours

Normal agents are off between 23:00 and 07:00 local. Carl is the exception and keeps no window
at all, because he is what JJ talks to and an assistant who is off between eleven and seven is
off exactly when somebody remembers something at midnight.

The window lives in each agent's `config.json`, assigned by rank at founding so a new lead gets
the ordinary arrangement without anybody remembering to say so. Hours rather than minutes, and
local rather than UTC, because a person sets it and reads it back.

`keep_hours` is its own pass rather than part of a tick, and it runs first. A tick makes reality
match the record; the timetable decides what the record ought to say, which is the same kind of
act as Carl deciding an agent should stop. Running it second would mean starting agents that were
about to be put down.

**Asleep is not stopped.** They are undone by different things: a stop waits for a person and a
night ends by itself. If they were one state the morning would quietly undo a decision JJ made on
Tuesday, which is the failure the separate state exists to prevent. A degraded agent is not put
to sleep either, because it is the thing somebody is meant to look at.

Sleeping ends the process, which is the whole point, and that includes a process left behind by a
supervisor that has since gone. Nothing here holds its pipes, so it takes the same route a
reclaim does. The alternative is a record saying asleep while a model sits there billing.

A night costs an agent nothing. The exit is recorded at the moment it went down rather than at
the moment it woke, so the morning does not hand it a backoff it never earned, and its attempt
count is untouched. Otherwise an army would degrade itself over a week of ordinary nights.

### Restart policy

Immediate on the first crash, then backoff, then the session is treated as the suspect, then the
supervisor gives up. Each step is a different claim about what is wrong.

- Once is bad luck. Try again now; waiting five seconds to find out helps nobody.
- Repeatedly is a fault, and hammering a fault turns one broken agent into a busy loop. The gap
  doubles, to a five minute cap.
- Repeatedly *while resuming* points at the session. A transcript can be too long, corrupt or
  gone, and retrying the resume fixes none of those, so the session is set aside, kept for
  inspection, and a fresh one is pinned under the same agent id.
- After that there is nothing left to vary. The record says degraded and why, the panel shows
  it, and a person decides.

A process that stayed up for a minute before ending did not fail to start, so its exit does not
count toward any of this. Without that rule an agent restarted once a night declares itself
degraded within a fortnight, having never once failed to start.

## The questions the note left open, and what was chosen

**1. What wakes a sleeping agent early.** The supervisor has a wake, and the reason is a value
rather than a sentence: a task, an incident, or the agent's lead asking. There is deliberately no
variant meaning "in general", so a wake nobody could later justify cannot be written down. Waking
an agent that is already up does nothing and records nothing, because a wake nobody performed is
not something that happened.

The collision the question was really about is the one the timetable answers. An agent woken
inside its own window stays up until the window ends, rather than being put back on the next pass
sixty seconds later. Being woken exempts an agent from the rest of one night and not from every
night after it, so the flag is cleared by the morning rather than by whoever set it.

**2. What a degraded session actually is.** Set aside, kept, and replaced with a fresh one under
the same agent id. Not seeded with anything by the supervisor, and this is where the embedded
memory fact earns its place: every agent is permanently told that its memory folder exists and
that `summary.md` is the way in, so a fresh session reads what the agent knew without the
supervisor having an opinion about what an agent should remember. If a fresh session fails too,
the supervisor stops rather than guessing further.

**3. Who reaps a task whose owner's process died mid work.** The lead blocks it, and the worker
picks it up again after the restart. It is not returned to a queue and not reassigned, because
the task never stopped being that agent's. A second board told to accept the same task is
refused.

**4. Whether the supervisor may refuse to start an agent.** Yes, and it records why. Two ways:
degraded, which it decided itself after enough failures, and stopped, which somebody decided.
The panel shows those as different rows, because an agent that needs a decision and an agent
that is simply not wanted today are not the same thing.

**5. How `AgentView.process` becomes answerable.** From the supervisor's records, which is the
only thing that can authoritatively associate a process with an agent. Unknown still means
unknown: an agent with no runtime record is one nobody has said anything about, which is a
different fact from an agent that is not running.

## Not built

- **Compaction.** Claude Code compacts a session under pressure on its own. Nothing here measures
  how full a conversation is, and the protocol exposes no usable figure for it. A percentage
  nobody measured would be worse than a gap, because somebody would plan around it.
- **Arbitrary hierarchy depth.** `army::org` is still a compiled in table with four named agents
  and a fixed chief, lead, worker set of ranks. Nothing in the runtime layer depends on the
  depth, which is the part that mattered, but the organisation itself does not yet grow.
- **A way to change an agent's hours without editing JSON.** Founding sets them and a text editor
  changes them. That is enough to run on and is not enough to live with.
- **Security officers, detector families, severity routing.** A separate document and a separate
  decision. Nothing built weakens what exists: no agent gets sudo, there is no privilege field
  anywhere, and there is nowhere on disk to write one.

## The rule that everything else hangs off

Carl controls work. The supervisor controls process existence.

Carl deciding that Nora should stop working on something and Nora's process exiting are different
acts. A design that ran them together would give Carl a kill switch he was never meant to have,
and would give the supervisor opinions about work it is in no position to judge.
