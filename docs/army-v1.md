# army v1, the shared foundations

Written for the two processes working in parallel on the first delegation chain. This is the
contract. If either of you needs a core type that is not here, ask for it to be added here
rather than defining your own, because two versions of `Task` is the failure that costs a day.

## the chain, and nothing more

```
JJ  ->  Carl  ->  Adrian  ->  Mason  ->  Nora
human   chief     lead        lead       worker
```

Five agents. Not twenty two. The generic roster in `src/army/roster.rs` is the older unnamed
squad and the campaign path still uses it, but nothing new should be built on it.

## what already existed, and what changed

Verified before writing anything, not assumed.

| | |
|---|---|
| `army::Task` | **renamed to `army::Dispatch`** |
| `army::Army`, `Report`, `Summary` | unchanged |
| `army::roster`, `campaign` | unchanged, superseded for new work |

`Dispatch` is one instruction sent to one agent, and it finishes when the model stops talking.
`Task` is now the governed unit. Two types called Task is how two people build two
incompatible halves of one system, which is why the rename happened before anything else.

## the shared types

Everything below is in `src/army/` and exported from `carl::army`.

### `org.rs`, who exists

```rust
pub enum Rank { Human, Chief, Lead, Worker }
pub struct Agent { name, display, rank, reports_to, remit }

pub fn everyone() -> &'static [Agent]
pub fn find(name) -> Option<&'static Agent>
pub fn require(name) -> Result<&'static Agent>      // refuses, and lists who exists
pub fn reports_of(name) -> Vec<&'static Agent>
pub fn chain_to_root(name) -> Vec<&'static Agent>
pub fn may_delegate(from, to) -> bool
pub fn check_delegation(from, to) -> Result<()>
pub fn check_may_implement(name, emergency) -> Result<()>
```

Three rules are in the types rather than in a brief, because a brief is a request.

- **Delegation is to a direct report only.** `check_delegation("carl", "nora")` fails and the
  error names Mason as the route. A chain skipped once stops being a chain.
- **Rank decides who implements.** `Chief` never, whatever happens. `Lead` only when an
  emergency is declared, and declaring one is a recorded event.
- **There is no way to express administrator rights.** No flag, no rank, no field. A test
  fails if the words appear in the code at all.

### `task.rs`, the governed unit

```rust
pub struct TaskId(String)                      // Serialize, from /dev/urandom
pub struct Verification { must: Vec<String> }  // refuses an empty list
pub enum Status { Assigned, InHand, Submitted, ChangesRequested, Accepted, Abandoned }
pub struct Task { id, goal, verification, status, owner, created_by, parent, attempts, workspace }

pub const MAX_ATTEMPTS: u32 = 3;

Task::assign(created_by, owner, goal, verification) -> Result<Task>
Task::split_from(parent, created_by, owner, goal, verification) -> Result<Task>
task.advance(by, next) -> Result<()>
task.must_escalate() -> bool
task.attempts_left() -> u32
Status::may_become(next) -> bool
may_take_on(worker, held) -> Result<()>
```

- `Task::assign` calls `check_delegation` itself. There is one place work is created and one
  place the chain is enforced.
- `advance` checks **who** is moving it. Only the owner may take it in hand or submit it. Only
  the creator may accept or request changes. **A worker cannot accept her own work**, which is
  the most tempting shortcut in the design.
- Nothing reaches `Accepted` except from `Submitted`. Nothing is accepted without review.
- `attempts` increments on each submission. `Accepted` and `Abandoned` are final: reopening is
  a new task, so the old one's history stays true.
- `workspace: Option<String>` is carried now and filled when coding tasks get their own
  worktree and branch, so that change does not touch this type or its callers.

### the two governance rules that live here

Both come from `recent_changes.json`, and both are in the shared layer because two people
each picking a sensible number is two different numbers, and only one of them is being
counted.

- **Two corrections, then escalate.** `MAX_ATTEMPTS` is 3. `must_escalate()` is true only when
  the task is sitting in `ChangesRequested` with three attempts spent. A task on its third
  attempt that has not been judged yet has been *tried* three times, not *failed* three times,
  and escalating it early takes it off somebody about to finish it.

  Escalation is deliberately **not** a status. It is something a lead does about a task, not
  somewhere the task goes, and a seventh state would make every reader handle a case that has
  nothing to do with their question. Process 2 decides what escalating does.

- **One task at a time.** `may_take_on(worker, held)` refuses if that worker already has
  something `InHand`. Submitted and awaiting review does not count, because she is not working
  on it and is free to be handed the next one once it is approved.

### `event.rs`, what happened

```rust
pub enum Event { Delegated, Moved, Submitted, Reviewed, Refused, EmergencyDeclared, Decided }
pub struct Record { seq, at, actor, event }     // flattened, one JSON object per line
pub struct Journal { open(path), append(actor, event) -> Result<Record> }
pub fn read(path) -> Result<Vec<Record>>
pub fn about(records, task) -> Vec<Record>
```

- Append only, flushed on every write, numbering continues across a restart.
- **Refusals are recorded.** Without them nobody can tell a rule that is working from a rule
  nothing has ever hit.
- `Event::moved(task, from, to)` is built from the statuses themselves, so the record and the
  task cannot describe the same change differently.
- One corrupt line is skipped rather than fatal.
- The variant list is closed on purpose. A free string means every writer invents its own
  wording and no reader can count anything, and counting is most of what a record is for.

## the shortest description of how this fits together

`tests/chain.rs` walks JJ to Carl to Adrian to Mason to Nora with the real types and no model
anywhere, in milliseconds. Read it before building. It shows every rule in use rather than
described, including the two that are easy to get wrong:

- **Escalation without a seventh status.** After three rejections, Mason takes it to Adrian by
  creating a new task whose parent is the failed one. The original stays in
  `ChangesRequested`, so its history remains true.
- **Queueing is not piling on.** A lead may create a second task for a busy worker. Handing it
  over is what `may_take_on` refuses.

It also exists to catch a design fault the unit tests cannot: types that each behave correctly
and cannot be composed into the shape JJ asked for. That is much cheaper to find there than
after somebody has written a delegation engine against them.

## process 2, the delegation chain

Build against `task.rs` and `org.rs`.

Yours:

- driving a task from `Assigned` to `Accepted`, through real agents
- what a lead actually sends its report, and what it does with what comes back
- the review decision, and the retry loop when changes are requested
- when to give up, using `attempts`

Use `Task::assign` and `Task::split_from` to create work, and `advance` for every state
change. Do not set `task.status` directly, and do not add a state. If you need a distinction
the six states cannot express, say so and it gets added here once.

Reports: keep using `army::Report` for what an agent produced. If a report needs to be tied to
a task, add `task: Option<TaskId>` to `Report` here rather than wrapping it in a new type.

## process 3, identity and state

Build against `org.rs` and `event.rs`.

Yours:

- agent local state: what an agent knows between turns, on disk, per agent
- hierarchy metadata beyond name and rank, if it is genuinely needed
- writing events at every important action, and reading them back

Use `org::require` at every boundary rather than `find`, so a typo is an error rather than a
silently skipped step. Use `Journal::append` for every event and never write the file
yourself.

Agent local state should live under `<home>/army/<agent>/`, one directory per agent, named by
`Agent::name`. Names are already lowercase and safe for a filename, but validate anything that
arrives from outside the table.

## the governance feed

`recent_changes.json` now lives at the repository root, which is the path the file itself
names. It is the shared record of organisation wide decisions, Carl is the normal writer, and
JJ may replace any entry at any time.

Read it when you start and before a new task. Several entries describe things neither of you
should build yet, and they are listed below rather than removed, because knowing a rule is
coming changes how you shape what you build now.

Of the fifteen entries, three constrain the shared types and are already implemented:
two corrections before escalation, one task at a time, and no administrator rights. The rest
are yours to be aware of and not to act on.

## what neither of you should build yet

Layered memory, scheduling, periodic reports, continuous operation, a dependency scheduler,
popups, an approval interface, a dashboard, diagnostics, the other seventeen agents,
cross department work, or governance beyond what is above.

## conflicts to avoid

1. **Do not define a second `Task`, `Status`, `Event` or `Agent`.** They are here. Extend them
   here.
2. **Do not add a status.** Six states are all anybody has needed. A seventh is another edge
   both of you have to get right.
3. **Do not write `task.status = ...`.** Use `advance`, so the who and the transition are both
   checked.
4. **Do not bypass `check_delegation`.** If a shortcut is genuinely needed, it belongs in
   `org.rs` with a test saying why.
5. **Do not append to the event file directly.** Sequence numbers come from `Journal`.
6. **`Dispatch` is not `Task`.** If you find yourself wanting a `Dispatch` with a status, you
   want a `Task`.
7. **No agent gets administrator rights**, and there is deliberately nowhere to put them.
