# planning

The work in chunks, with what has to be true before a chunk counts as finished.

Effort is in sessions, where a session is roughly an afternoon.

## what this is aimed at

One workflow, made dramatically better over the next year, rather than five layers built before
any of them is known to be useful. It is written up in
[docs/flagship-workflow.md](docs/flagship-workflow.md), along with the nine measures and the
security split, and the test for taking on a second area is in
[docs/three-year-expansion-gate.md](docs/three-year-expansion-gate.md).

The short version. **Carl owns work. AOS owns execution. ME OS is a separate operating system
and is not part of this.** The flagship is Carl taking a real ME objective, organising the army
around it, executing inside boundaries AOS enforces, and handing back a reviewed result.

Phases 1 to 7 below built the assistant. Phases 8 to 11 are the rest of that assistant and are
still worth doing. The army work underneath them is what the flagship is actually made of, and
the gates for it are in the flagship document rather than repeated here.

## done

| phase | effort | what | done when |
|---|---|---|---|
| 1 | 1 | the record, threads, memory, the claude driver | a question asked twice in one thread is answered in context, and the question survives the answer failing |
| 2 | 1 | screen capture, and knowing when to look | "should I put it here" looks, "what should I research" does not |
| 3 | 1 | the ear: mic, wake word, whisper, voice | "hey carl" wakes him and he answers out loud |
| 4 | 1 | echo cancellation and streaming | he can be interrupted mid sentence, and a spoken exchange takes 16s rather than 100s |
| 5 | 1 | Slack over socket mode | mentions, direct messages and his own name, answered in thread |
| 6 | 1 | A2A, so two agents can talk without running away | specified in `docs/a2a.md`, implemented, and a hello sent |
| 7 | 1 | memory that actually gets written | a fact given in one conversation is known in a different one |

## next

### phase 8, feedback while he thinks

Claude takes five to twenty five seconds and Slack shows nothing at all during it, so a
question looks ignored until it is suddenly answered. The voice does not have this problem,
because a voice arriving late is obviously still coming.

Post a placeholder immediately and replace it with the answer, or use the typing indicator.
The first is more honest, since a placeholder can say what he is doing.

Done when no Slack question sits with no visible response for more than two seconds.

Effort: half a session.

### phase 9, forgetting

There is no way to remove a note. A wrong fact is worse than a missing one, because it comes
back on every turn and is stated with confidence.

`carl memory forget` exists for the terminal. What is missing is Carl doing it himself when
corrected, which is the same shape as `[remember]` and probably a `[forget]` line matching on
the note text.

Done when telling Carl he was wrong about something removes the note, and a second
conversation confirms it is gone.

Effort: half a session.

### phase 10, Alex

The protocol exists and Alex does not speak it yet. Hunter has the spec.

Done when Carl and Alex complete a full exchange, hello through done, in a channel, and the
turn limit is never reached because the conversation ended on its own.

Effort: unknown, and mostly not mine.

### phase 11, running unattended

All three services run and restart. Two of them stop at logout, which is correct, because
they are PipeWire clients and there is no microphone when nobody is logged in. Slack needs
`loginctl enable-linger` and that needs root.

What is actually missing is noticing when something is wrong. A service that restarts every
fifteen seconds forever is technically running.

Done when a failure that persists for five minutes reaches JJ somewhere he will see it.

Effort: half a session.

## the army, which is where the flagship lives

Not a phase list, because the gates are in
[docs/flagship-workflow.md](docs/flagship-workflow.md) and two copies of a plan disagree within
a month.

What is built: the organisation and its delegation rules, tasks with review and escalation, the
event journal with one numbering, per agent identity, memory and hours, the runtime supervisor
with restart policy and crash recovery, the Command Panel, and the nine measures folded out of
the journal by `carl army metrics`.

What is not: priority setting, blocked task aging, lead summaries that arrive without being
asked for, the escalation path that reaches JJ, and the join between a grant Carl records and
the capability layer in AOS that enforces it. That last one is the gap David's security concern
actually lands on.

## not planned, and why

**A sandbox for python.** Worth doing and much bigger than it looks. AOS phase 3 is building
the pieces, and Carl should borrow them rather than grow a second version.

**A GUI.** Three surfaces is enough.

**Multiple users.** Carl knows names now and answers anybody in the workspace, but memory is
one pile with no notion of whose fact is whose. That is fine for one household and wrong for
anything larger, and the fix is not worth building before the problem exists.

## how a phase is judged finished

Not when it compiles and not when it is typechecked.

- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, all three run and
  the output actually read
- every bug fixed has a test that fails against the old code, proven by reverting the fix
- the thing has been run for real, not only in tests, because every bug of consequence so far
  came from running it
