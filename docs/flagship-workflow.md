# The flagship workflow

One workflow, made dramatically better over the next year, instead of five layers built before
any of them is known to be useful.

This document exists because of outside feedback from David George. He said Carl, AOS and ME OS
sound like the same thing, that too many layers could get built before one proves useful, and
that Carl becomes a serious security problem the moment ME depends on it. All three are fair,
and two of them are checkable against these repositories rather than a matter of opinion.

## Carl, AOS and ME OS

Three projects that keep getting read as one. The split below is what the code already does,
written down so the public wording can stop drifting.

| | owns | does not own |
|---|---|---|
| **Carl** | work. What should happen, who does it, in what order, reviewed by whom, and what reaches JJ | processes, and what a process is allowed to touch |
| **AOS** | execution. Which programs may start, what they may read and change, what needs a commit first, and what gets recorded | any opinion about whether a task was worth doing |
| **ME OS** | a computer. Boot, display, input, on real hardware eventually | agents, and anything on this page |

The one sentence version: **Carl decides what work happens, AOS decides what a running agent is
allowed to do, ME OS is a separate operating system project.**

The rule that keeps the first two apart is already in the code and already tested. Carl controls
work. The supervisor controls process existence. `army::runtime::supervisor` has a test that
reads its own source and fails if the words `Task`, `Board`, `delegate` or `Status` appear in it.

ME OS is not part of the flagship. It is active, it has its own milestones, and it draws
rectangles in an emulator. Carl running on ME OS is a decade away and nothing about the next
year depends on it.

## The hypothesis

> Carl can take a real ME objective, organise the Agentic Army around it, execute the work
> across ME projects inside boundaries AOS enforces, keep JJ informed, recover from its own
> failures, and hand back a reviewed result with far less manual coordination than doing it by
> hand.

The workflow is ME's own research and engineering work. Not a general productivity assistant.
The difference matters: a general assistant is judged on whether it feels helpful, and this is
judged on whether real ME objectives finish.

Picking ME's own work is not modesty. It is the only workflow where the person judging the
result knows what a good result looks like, and where a bad one costs nothing but a morning.

## The workflow, stage by stage

Twelve stages. The status column is what is true in this repository today, not what is intended.

| # | stage | what has to happen | today |
|---|---|---|---|
| 1 | objective intake | JJ gives Carl an objective, recorded as `Intervened { Objective }` | built |
| 2 | priority | Carl decides what it displaces, and says so | not built |
| 3 | delegation | Carl splits it to a lead, the lead splits it to a worker, nobody skips a level | built and tested |
| 4 | execution | a permanent agent with a durable identity does the work in a long running process | built, first slice |
| 5 | scoped permission | the lead grants one workspace, and something other than Carl enforces it | half. Carl records the grant, AOS enforces, never joined |
| 6 | reporting | structured events for Carl, richer prose for the lead | half. Events built, lead summaries not |
| 7 | review | whoever created the task reviews it, and nobody reviews their own work | built and tested |
| 8 | blockers | a task that cannot move is visible and aging, not silently parked | not built |
| 9 | failure recovery | a crashed worker's task stays its own, and it picks it up after the restart | built and tested |
| 10 | memory continuity | a new process resumes the conversation, or reports exactly what was lost | built and tested |
| 11 | escalation | three rejections goes over the lead's head, and a question Carl cannot answer reaches JJ | half. Up the chain works, reaching JJ does not |
| 12 | final output | a reviewed result, with the record able to say who accepted it and why | built and tested |

Seven built, three half built, two not started. Half means one side works and the other does
not, which is a different thing from a gap and is worth counting separately.

Stage 5 is the half that matters. The grant is recorded in Carl and the enforcement lives in
AOS, and the two have never been pointed at each other on real work. It is also the only gap
that is somebody else's repository, and it is the one David's security concern lands on.

## What "dramatically better" means

Not a longer list of stages. The same twelve, run repeatedly on real objectives, with the
numbers below moving in the right direction over a year of actual use.

## The measures

Nine. Every one is a fold over `run/events.jsonl`, so none of them needs a second system to
record it and none can disagree with the history.

| measure | from | why this one |
|---|---|---|
| objectives accepted | `Reviewed { accepted }` on a parentless task | the only measure that says the thing worked |
| objectives finished without JJ | objectives with no `Intervened` other than the opening one | the difference between a tool and a colleague |
| interventions per objective | `Intervened` per objective | falling is the whole point. A rising number is the honest early answer |
| review rejection rate | `Reviewed { accepted: false }` over all reviews | zero means the leads are rubber stamping |
| retry rate | `Submitted { attempt }` above 1 | work handed down badly, seen from the other end |
| escalations | `EmergencyDeclared`, and tasks at `must_escalate` | how often the chain could not resolve it itself |
| crash recovery | `AgentCrashed` followed by `AgentStarted`, against `AgentGaveUp` | whether a failure is an event or an outage |
| continuity failures | `ContinuityChanged` | an agent that came back with less than it had |
| refusals | `Refused` | a rule nothing ever hits is not protecting anything |

Deliberately not measured: time saved. It would have to be estimated, an estimate would be
flattering, and a flattering number on a page like this is worse than a gap.

Two of these are meant to look bad first. Interventions per objective and review rejection rate
both go up when the army starts doing real work, because the alternative is JJ not looking and
leads not reading. A metric that only ever improves is a metric nobody is really measuring.

Read them with `carl army metrics`.

## Security, now and later

Taken seriously and split, because a plan that lists biometrics next to audit logs is not a
security plan. It is a way of doing neither.

### Now

Each of these either exists or is the next thing in its area. Nothing here is future work.

| | state |
|---|---|
| no sudo, no admin, anywhere | done. There is no privilege field in the format and nowhere on disk to write one, and a test fails if the word appears |
| task scoped write access | granted by a lead, enforced by `aos-files` with a read root and a narrower write root |
| secret isolation | `aos-files` refuses `.ssh`, `.aws`, `.env`, `.carl`, `.claude`, private keys and the rest, on the name asked for and on the name it resolves to |
| audit log | `run/events.jsonl`, append only, single numbering, refusals recorded with their reason |
| human override | JJ reaching past the chain is its own event rather than an ordinary one, so an override can never be mistaken for a decision Carl made |
| authority boundaries | `org.rs` refuses a shortcut and names the route instead, and the refusal is recorded |
| agent identity separate from session | three lifetimes, tested. A restart is not a new agent and a new session is not a new agent |
| kill switch | `aos stop-all`, and stopping the systemd unit takes every agent process with it, verified against a real cgroup |
| runtime monitoring | supervisor records, backoff, and a degraded state a person has to clear |
| memory integrity | not done. Nothing checks that an agent's memory folder was written by that agent |
| security officers | Serena holds the department and leads nobody. A department recorded before there is work for it, not a claim that auditing happens |

### Later

Not started, not scheduled, and listed so nobody mistakes them for the plan.

Biometrics. Physical command centre security. Risk scoring. Hardware authentication. Facility
controls.

Every one of them protects a building or a person, and ME has neither a building nor anybody in
it. Building them now would be security theatre in the exact sense of the phrase.

### The concern that is real

David is right that Carl becomes a serious vulnerability if ME depends on it for devices, labs
and operations. The answer is not more gates around Carl. It is that Carl is not the thing that
enforces anything.

Carl records that a lead granted a worker one directory. `aos-files` is what refuses the path,
in a different process, holding no opinion about the work. If Carl is wrong, compromised or
simply confused, what an agent can reach does not change. That separation is the security
design, and everything else is detail underneath it.

Where it does not hold yet is stage 5 above. The grant is recorded in Carl and the enforcement
lives in AOS, and the two are not yet wired together on a real objective. Until they are, the
boundary is two correct halves that have not been joined, which is worth being plain about.

## The first year, as gates

No dates. A stage is finished when the sentence next to it is true.

| | gate |
|---|---|
| 1 | the army survives a reboot with every agent resuming its own conversation |
| 2 | one real ME objective goes intake to accepted result with no manual step |
| 3 | a worker's write is refused by AOS, from a grant Carl recorded, and the refusal is in the log |
| 4 | a lead rejects real work and the worker's second attempt is accepted |
| 5 | a crash mid task is followed by the same agent finishing the same task |
| 6 | a lead summary reaches Carl that JJ did not have to ask for |
| 7 | a blocked task announces its own age |
| 8 | `carl army metrics` has ten real objectives in it |
| 9 | the Command Panel shows a live objective from the BOOX without a second backend |
| 10 | thirty objectives, and the intervention rate over the last ten is below the first ten |

Gate 10 is the one that matters. The nine above it are how you get there.

## What is not in the flagship

Holoprojector and Employee Bracers stay experiments, and staying an experiment is a real
status rather than a demotion. They answer questions the flagship does not ask: spatial
interaction, input that is not a keyboard, accessibility, and what a human interface looks like
when the computer is not on a desk. Holoprojector has six verified milestones and should keep
earning them at its own pace.

They do not get flagship effort, and nothing in the list above waits on them.
