# Army Runtime, design note

**Design only. Nothing here is built, and nothing here should be built as part of Command Panel
v1.** This exists so the decisions already made are written down in one place before anybody
starts, and so the next phase argues with a document rather than with somebody's memory.

Command Panel v1 shows an army that does not yet run. Every agent state it draws comes from
folders and a journal that a chain run writes and then exits. The runtime is what makes the army
a thing that is up rather than a thing that is invoked.

## The shape

One dedicated long running Claude process per agent, supervised, with memory on disk.

That single sentence is most of the design, and it is a change in kind rather than in degree.
Today a chain run spawns processes, uses them and drops them, so an agent has no continuity
beyond what it wrote down. A long running session means an agent has a working context that
survives between tasks, which is the difference between a colleague and a contractor.

## Decided

### Processes and sessions

- **One dedicated long running Claude process per agent.** Not a pool, not per task.
- **Native `--resume` is preferred after a restart**, rather than replaying a transcript into a
  fresh session. The CLI already keeps the session; rebuilding one by hand would be a second,
  worse implementation of something that exists.
- **Context compacts under pressure**, not on a timer and not per turn. Compaction is a cost paid
  when the window demands it.

### Memory

- **Every agent has a memory folder.**
- **One permanently known fact per agent: the folder exists, and `summary.md` is the way in.**
  That is deliberately the only thing an agent is told for free. Everything else it must go and
  read, so the brief stays small and the memory stays honest.
- **Markdown for anything a person will read.** Persistent state that only a person reads has no
  business being a format only a program can.

### Supervision

- **A systemd user service starts the Army Runtime Supervisor.** The supervisor keeps agent
  processes alive.
- **Carl controls work. The supervisor controls process existence.** This separation is the whole
  point and is worth stating twice: Carl deciding that Nora should stop working on something is a
  different act from Nora's process exiting, and a design that conflates them gives Carl a kill
  switch he was never meant to have and gives the supervisor opinions about work it cannot judge.
- **Restart policy: immediate on the first crash, then backoff, then degraded session recovery.**
  A process that dies once is unlucky. A process that dies repeatedly is broken, and hammering it
  turns one fault into a busy loop. Degraded recovery is the admission that the session itself may
  be what is wrong.

### Hours

- **The army runs nearly continuously.**
- **Normal agents have scheduled overnight sleep windows.** An agent that never stops is an agent
  whose context and cost grow without anybody choosing it.
- **Carl is currently the only default overnight exception.**

### Work

- **Workers keep two or three lead approved backup tasks.** So a worker blocked on one thing is
  not idle, and the alternatives were agreed in advance rather than invented by a worker who
  wanted something to do.
- **Global priorities with delegated local queues.** The priorities are the organisation's; the
  ordering within them belongs to the lead who owns the work.
- **Leads grant task scoped read and write access.** Scoped to the task, so access ends when the
  task does rather than accumulating.

### What Carl sees

- **A structured live feed, plus richer summaries from leads.** Two channels on purpose: the feed
  is machine readable and complete, the summaries are judged and partial. Carl needs both, and a
  design that gives him only prose cannot count while one that gives him only events cannot tell
  him what mattered.

### Security

Security architecture is a separate document and a separate decision. Nothing here weakens what
already exists: no agent gets sudo, no agent gets a privilege field, and agents run with no home
directory bound.

## Open, and worth settling before anybody writes code

These are not decided and are written down as questions rather than guessed at.

1. **What wakes a sleeping agent early.** A scheduled window and an urgent task will collide on
   the first night.
2. **What a degraded session actually is.** "Recover degraded" is currently a phrase, not a
   behaviour. Probably: a fresh session seeded from `summary.md` with the old one kept for
   inspection, but that is a guess and should be decided rather than defaulted into.
3. **Who reaps a task whose owner's process died mid work.** The task is `InHand` and its owner
   is gone. The lead is the obvious answer and the journal needs an event for it either way.
4. **Whether the supervisor is allowed to refuse to start an agent**, and what the panel shows
   when it does.
5. **How `AgentView.process` becomes answerable.** The supervisor is the only thing that can
   authoritatively associate a process with an agent, so it should publish that association
   rather than have anything infer it from process names. This is what closes the one honest
   `unknown` left in the panel.

## What this note is not

Not a plan, not an ordering, and not a commitment to build any of it next. It is the set of
things already agreed, so that the phase which does start writes code against a decision instead
of rediscovering one.
