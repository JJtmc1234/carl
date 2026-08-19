# the panel for a 24/7 army, planned

Not built. This is what the Command Panel should eventually show once every agent has a
long-running process of its own, and more usefully, what it should not show.

Written now rather than later because the temptation with a persistent runtime is to put
everything on the screen the moment it exists, and the Agents tab has one job that gets harder
with every field added to it.

## the constraint that decides everything

**The Agents tab must stay readable at twenty agents and beyond.**

Today five agents fill 230 pixels of a fullscreen panel. Twenty fill 920. Forty do not fit, and
the tab stops answering the question it exists for, which is "what is everyone doing" answered
in a couple of seconds without reading.

So the row budget is fixed at what it is now. Anything added to a row has to be worth more than
the thing it pushes out, and almost nothing is.

The rule this gives, and it settles most of the arguments below:

> **A row carries what changes your next action. The detail carries what explains it.**

An agent's session id explains a great deal and changes nothing you would do. It is not row
material. An agent whose context is nearly full is about to stall, and that changes what you
would do in the next ten minutes, so it is.

## the one genuinely new kind of fact

Everything the runtime adds is descriptive except one thing.

**Context pressure is predictive.** Every other field says what happened: it restarted, it
slept, it holds this task, it wrote this memory. Context pressure says what is about to happen,
which is that this agent will compact and stall soon. That is the only new number that earns a
place where the eye passes over it without being asked.

It earns that place as a threshold marker, not as a number. A percentage on twenty rows is
twenty numbers nobody reads. A marker on the two rows that are near their limit is two things
worth looking at.

## where each thing goes

### the row

Unchanged shape, two lines, plus a fixed flags column of at most two glyphs.

| what | why it is here |
|---|---|
| name and department | identity, already there |
| status word and pip | absorbs the process and sleep states, see below |
| current task, or the blocker when there is one | this is the question the tab answers |
| age of last activity | tells a stalled agent from a busy one |
| **flag: continuity degraded** | changes how much of the rest of the row you believe |
| **flag: needs attention** | context near its limit, or a high severity finding |

Two flags and no more. A flags column that grows becomes a second status column, and a row with
two status columns has neither.

Continuity is a flag rather than detail because it qualifies everything else on the row. An
agent on a fresh fallback session has lost its conversation, so its last activity is not
continuous with what came before and any claim it makes about earlier work is a claim about
something it no longer remembers.

### the expanded detail

One agent, as much room as it needs. Everything that explains the row.

- **session**: reference, age, last resume, continuity, restart history
- **context**: pressure, last compaction, and what compaction cost if known
- **work**: task, verification, attempts, project, approved backlog count and what is next
- **permissions**: current task scoped grants, who issued each, expiry or owning task
- **memory**: summary status, last modified, last author, warnings
- **security**: findings count, highest severity, when it was last audited
- **recent events**, as now

### diagnostics

Diagnostics answers "is the system healthy". Agents answers "what is everyone doing". The split
is not by subject, it is by whether the number is about one agent or about the fleet.

So Diagnostics gets the roll-ups and never the per agent facts:

- how many are running, sleeping, degraded, stopped
- resume failures across the fleet, and how many are flapping
- how many agents are near their context limit
- security findings by severity, across everybody
- the supervisor's own health, which nothing else reports

A count across agents belongs here. The same fact about one agent belongs on that agent.

### the contextual workspace

Anything you would read at length or change.

- open `summary.md`
- browse the memory folder
- what a task changed, as a comparison
- a shell in the agent's worktree
- the detail behind a security finding

## sleep

**Sleeping is healthy and must never be drawn as a fault.** It is not offline, it is not
stopped, and it is not unknown. An agent asleep on schedule is an agent doing exactly what it
was told.

That follows the rule the panel already has: colour means a state you might act on. Sleeping is
not one, so sleeping carries no colour.

| state | pip | word | colour | why |
|---|---|---|---|---|
| Running | filled | WORKING | accent | busy, and the eye should find it |
| Idle | hollow square | IDLE | faint | available, nothing to do |
| Sleeping | hollow bar | SLEEPING | faint | deliberate, healthy, quiet |
| Wake requested | filled | WAKE ASKED | accent dim | somebody asked, nothing has happened yet |
| Waking | filled | WAKING | accent | transitional, and brief |
| Restarting | filled | RESTARTING | warn | ordinary once, a problem repeated |
| Degraded | filled | DEGRADED | warn | working and not properly |
| Resume failed | filled | RESUME FAILED | bad | needs somebody |
| Stopped | hollow | STOPPED | unknown | deliberate and not running |
| Unknown | hollow | UNKNOWN | unknown | nobody has looked |

Idle and sleeping share a colour and must not share a shape, because they are different facts
and the colour rule deliberately refuses to separate them. A hollow square against a hollow bar
does it without adding a hue that would imply something to do.

**Carl is the standing overnight exception, and an exception nobody can see becomes the habit.**
So an agent awake outside its window carries a visible override marker and the reason in detail.
Not an alarm, because it was allowed, but never invisible either.

## resume

The distinction the brief is right to insist on, stated as the panel would say it:

> A new process that started successfully and an original session that resumed successfully are
> not the same event, and drawing them the same way loses the only thing that mattered.

Continuity is its own attribute rather than a process state, because a process can be perfectly
healthy while its continuity is broken.

| continuity | on the row | in detail |
|---|---|---|
| Continuous | nothing | resumed at, session age |
| Fresh fallback | **flag** | what session was lost, when, why the resume failed |
| Resume failed | status is `RESUME FAILED` | the error, and how many attempts |
| Flapping | **flag** | restart count, backoff, the crashes |

A fresh fallback is the case worth being loud about. The process is up, the agent will answer,
everything looks normal, and it has forgotten everything. That is the failure that gets trusted
because it looks like success.

## memory

Not a tab. Memory is a property of an agent and belongs inside that agent's detail, reached
from the row somebody was already looking at. A top level Memory tab would make you go to
memory and then hunt for whose.

Inside agent detail, a MEMORY section:

- **summary status**: present, missing, or malformed
- **last modified**, with relative age
- **last author** when authorship exists, which matters for a management or ancestor update
  because "Carl last wrote this agent's summary" is a different fact from the agent writing it
- **open `summary.md`** into the editor, which already handles read only and the conflict case
- **browse the memory folder**

A missing or malformed `summary.md` is shown as a warning and never as an empty section. The
same reasoning as milestone gaps: silently drawing nothing is indistinguishable from drawing
nothing because there is nothing wrong.

## what this needs that does not exist

The panel cannot show any of it until the wire carries it. The minimum addition to
`panel::view::AgentView`, using `Maybe` throughout so unknown stays a real value:

    process:      Maybe<ProcessState>   // widened: Running, Sleeping, Restarting,
                                        // ResumeFailed, Degraded, Stopped
    continuity:   Maybe<Continuity>     // Continuous | Fresh | Flapping
    context:      Maybe<Context>        // pressure 0..1, last_compaction
    session:      Maybe<SessionRef>     // reference, started_at, last_resume
    sleep:        Maybe<Sleep>          // window, wake_requested, override reason
    backlog:      Maybe<usize>
    memory:       Maybe<MemoryState>    // summary ok, last_modified, last_author
    findings:     Maybe<Findings>       // count, highest severity, audited_at
    grants:       Vec<Grant>            // detail only, empty is a real answer

And one new workspace request, which is the only new seam:

    WorkspaceRequest::Folder { path }   // browse the memory folder

## what was deliberately left out

- **session id on the row.** Explains much, changes nothing you would do.
- **backlog count on the row.** It says what is coming, not what is happening.
- **audit recency anywhere but detail and the fleet roll-up.** It is a property of the checking,
  not of the agent's work.
- **a memory tab**, for the reason above.
- **a per agent context percentage on the row.** The threshold marker carries the actionable
  half and none of the noise.
