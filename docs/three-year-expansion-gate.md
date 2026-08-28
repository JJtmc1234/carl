# What has to be true before ME takes on a second major area

David George asked it directly: what needs to be true in three years for ME to confidently
launch its second major area of focus?

This is the answer. It is a gate rather than a date, because a date can arrive whether or not
anything is ready and a gate cannot.

## The shape of the answer

ME does not promise to launch a second area in three years. It commits to a test. If the test
passes, expanding is a decision that can be made with evidence. If it does not, the honest move
is another year on the first one.

Nothing below says which second area. Choosing it is what the evidence condition is for.

## The six conditions

All six. Not four of six, and not five with the sixth argued around.

### 1. Reliability

- Fifty real ME objectives have gone through the flagship workflow. Real means work JJ wanted
  done anyway, not a demonstration built to be demonstrated.
- Over the most recent twenty, at least three in four were accepted.
- Over the most recent twenty, at least half finished with no JJ intervention after the opening
  objective.
- Interventions per objective over the last twenty is lower than over the first twenty. The
  direction matters more than the number.
- Crash recovery is boring. Every `AgentCrashed` in the last quarter was followed by the same
  agent resuming the same task, or by an `AgentGaveUp` somebody looked at within a day.

### 2. Security

- Every task that touched a file did it through a scoped capability. No agent has ever been
  handed a raw shell, and there is still nowhere in the format to write a privilege.
- Every gated call in the last quarter is in the audit log, refusals included, and the log has
  one numbering that nothing has been able to fork.
- No unresolved high severity finding about authority or scope. A finding is resolved when a
  test exists that fails against the old code.
- Human override has been used for real, not only tested. JJ has stopped a running objective and
  the record shows it as an intervention rather than as a decision Carl made.
- Memory integrity is checked. An agent's memory folder can be shown to have been written by
  that agent.

### 3. Operations

- Carl and AOS run ME's own research and engineering work as the normal way it happens, not as
  an alternative to doing it by hand.
- Objectives have landed successfully across at least three separate ME repositories.
- The army has run for a quarter without a week where JJ went back to doing it manually because
  the army was not worth the trouble.

### 4. Capacity

- There is enough capacity to split attention without the flagship slowing down. Today that
  means one person, so this condition is mostly about the first area needing less of him than it
  does now.
- Measured rather than felt: the flagship's own metrics keep improving across a quarter in which
  meaningful effort went elsewhere.

### 5. Reuse

- The flagship has produced infrastructure the second area would inherit rather than rebuild.
  The supervisor, the event log, the capability servers, the policy handshake and the identity
  model are the candidates, and the condition is that at least one of them has already been used
  by something it was not written for.
- If nothing has been reused inside ME, the claim that a second area gets a head start is
  untested.

### 6. Evidence

- Somebody outside ME has used or asked for the second area, or an ME objective was blocked by
  its absence more than once.
- The case is written down, with what would falsify it.
- The first area does not need the second in order to be worth having. Expanding to rescue a
  flagship that is not working is how one unfinished thing becomes two.

## What fails the gate

Stated so it cannot be quietly argued away later.

- A demonstration that only works when watched.
- A completion rate held up by objectives written small enough to pass.
- Any of the six conditions met "in spirit".
- A second area chosen because it is more interesting than the first one is at that point.

## If the gate passes

ME can expand with evidence. The first area keeps its own measures and does not get quietly
starved to feed the second.

## If it does not

Another year on Carl and AOS, and this document gets updated with which condition failed and
why. Not deleted, and not rewritten to be easier.

## Why a gate rather than a date

Three years from now is 2029. The formal launch target is around 2035, which makes this gate a
checkpoint roughly a third of the way through the runway rather than a launch.

A date says when to look. A gate says what to look at. Only one of them can be failed.
