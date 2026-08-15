# the delegation chain, v1

What `src/army/chain/` does, and what the integrator needs to know about it. The shared
foundations it is built on are in `docs/army-v1.md` and are not repeated here.

## the path one request takes

```
JJ
 -> carl     ask  ->  objective + DONE WHEN     Task::assign(carl, adrian)
     -> adrian   ask  ->  objective + DONE WHEN     Task::split_from(adrian, mason)
         -> mason    ask  ->  one task + DONE WHEN      Task::split_from(mason, nora)
             -> nora     works, verifies, submits
             <- mason    reruns the verification, ACCEPT or REJECT
         <- mason    reports up
     <- adrian   decides and reports up
 <- carl     tells JJ what happened
```

Each arrow down is a real `Task`, so `check_delegation` is the only route the work can take.
Carl cannot skip Adrian because `Task::assign` refuses it, not because the driver remembers not
to. Each arrow up is `Status::Submitted` followed by the creator accepting or abandoning, so
nothing is marked done by whoever did it.

Every step is a `claude` process held open for the whole run. The conversation is the state:
Mason reviews against the task he wrote rather than against a description of it, and Nora
corrects her own first attempt rather than being re-briefed from cold.

## the files

| file | what is in it |
|---|---|
| `mod.rs` | the brief each agent runs with, and the tool list per rank |
| `words.rs` | exactly what each agent is asked, at each step |
| `handback.rs` | reading a worker report, a verdict, and a DONE WHEN section |
| `run.rs` | `Chain`, one turn per agent, and the bookkeeping around a task |
| `drive.rs` | the campaign, and the retry loop |

## three rules that are enforced rather than requested

**Rank picks the tool list the process starts with.** `tools_for` gives a chief an empty list,
a lead read tools and `Bash`, and a worker an editor as well. `check_may_implement` refuses a
chief who asks to implement, and the empty tool list means a chief who never asks still cannot
write a file. Two guards, because either alone is one forgotten call away from nothing.

Mason having `Bash` and no editor is deliberate and imperfect. A reviewer has to rerun the
verification rather than believe a summary, and a shell can write a file. It is the smallest
grant that still lets a reviewer check rather than trust, and it is the obvious thing to
tighten later.

**A task cannot exist without something checkable on it.** An agent handing work down is asked
for a `DONE WHEN` section and asked once more if it forgets. Nothing here invents a fallback
condition, because a generic "it works" would satisfy `Verification` while defeating the reason
it is required.

**A review that does not clearly say ACCEPT is not an acceptance.** `read_verdict` defaults to
reject. The other default lets a rambling answer with no decision in it pass work nobody
approved, which is the exact failure a review exists to prevent.

## the retry rule

`after_review(accepted, attempts)` is the whole rule, on its own so it can be checked without
spending a model call. One attempt and two corrections. The third rejection abandons the task
and it goes up with its whole history, because a task rejected three times is usually one that
was written wrong rather than one done wrong.

`drive.rs` calls that function rather than repeating the comparison, so what runs is what the
tests check.

## one thing that is mine and probably should not be

**One task at a time** is enforced in `Chain`, as a list of who holds an unfinished task.
`Task` counts attempts and checks who moves what, but nothing in it knows that two tasks exist
at once, so there was nowhere shared to put this.

It belongs shared. It is an organisation rule rather than a property of one run, and right now
a second driver could hand Nora a second task without either of them noticing. Suggested shape,
if you want it in `task.rs`:

```rust
pub fn check_free(open: &[Task], owner: &str) -> Result<()>
```

Say the word and I will move it and delete mine.

## what I did not need, and did not build

`army::Report` is untouched. A worker report here has four parts and a blocker, which is a
different shape from one agent's output, so it is `Handback` in `handback.rs` and it does not
compete with `Report`. If you would rather it were `Report` with a `task` field, that is a
small change and it is yours to call.

There is no `Event` variant for an escalation. A third rejection is recorded as `Moved` to
abandoned plus the reviewer's `Reviewed`, and Adrian's decision as `Decided`. That reads
correctly, but "how often does work get escalated" is exactly the kind of counting a closed
variant list is for, and it currently needs a join across three events to answer. An
`Escalated { task, from, to, why }` variant would make it one filter. Your call.

Nothing from the "not yet" list is here: no layered memory, no scheduling, no periodic reports,
no queue, no dependencies, no onboarding, no interface, no extra agents.

## running it

```
carl chain --workdir DIR "what you want"
```

`--journal` defaults to `events.jsonl` inside the workdir. `DIR` is where the worker changes
files, and per the governance feed it should be a git worktree of its own once that exists.
Nothing here creates one yet, and `Task::workspace` is the field waiting for it.
