# read this before your next edit

You branched before the shared foundations existed, so you have been building against a base
that did not have them. That is my fault, not yours: I published the contract after you
started. Your branch has now been fast forwarded onto `main`, and nothing you wrote was
touched.

`docs/army-v1.md` is the contract. The short version of what it means for you.

## types that now exist, which you have written your own copies of

| you defined | use instead | where |
|---|---|---|
| `chain::Rank` | `carl::army::Rank` | `src/army/org.rs` |
| `chain::ledger::Status` | `carl::army::Status` | `src/army/task.rs` |
| `chain::ledger::Event` | `carl::army::Event` | `src/army/event.rs` |

Please delete yours and use these. Not because mine are better, but because two definitions of
`Status` is two half systems that do not meet, and one of us has to give way. I am the
integrator, so it is me who has to say which, and the answer has to be the shared one.

If any of the three is missing something you need, say so and I will add it to the shared type
once. Do not extend your local copy.

## what is already decided, so you do not have to

- `Status` is `Assigned, InHand, Submitted, ChangesRequested, Accepted, Abandoned`. Six.
  Nothing reaches `Accepted` except from `Submitted`.
- `Task::advance(by, next)` checks **who** is moving it. A worker cannot accept her own work.
  Do not assign `task.status` directly, and it is private to `advance` for a reason.
- `Task::assign` already calls `check_delegation`, so you do not need to.
- `attempts` is incremented by `advance` on submission. That is your retry counter.
- `Event` has a closed variant list, and `Journal` owns sequence numbers. Do not append to the
  file yourself.

## what is yours, and what I will not touch

Driving a task from `Assigned` to `Accepted` through real agents. What a lead sends downward,
what it does with what comes back, the review decision, the retry loop, and when to give up.

I own `org.rs`, `task.rs` and `event.rs`. If you need a change in one, ask.

## the one thing worth checking before you go further

`army::Task` used to be something else: one instruction to one agent. It is now
`army::Dispatch`. If you wrote code against the old `Task` expecting `role` and `instruction`
fields, that is `Dispatch` now, and the new `Task` is the governed unit with a status.

## pull again before you go much further

Two commits landed after your branch was moved onto the foundations.

`74e8ae3` adds the two governance rules from `recent_changes.json` that constrain the shared
types, because two people each picking a sensible number is two different numbers.

- `MAX_ATTEMPTS` is 3, and `task.must_escalate()` is true only when a task is sitting in
  `ChangesRequested` with three attempts spent. A task on its third attempt that has not been
  judged has been tried three times, not failed three times.
- `may_take_on(worker, held)` refuses a worker who already has something `InHand`. Creating a
  second task is fine. Handing it over while she is working is not: a lead may queue work and
  may not pile it on.

The next commit adds `tests/chain.rs`, which walks JJ to Carl to Adrian to Mason to Nora with
the real types and no model anywhere. Read it before building: it is the shortest description
of how these types are meant to fit together, including how escalation is expressed without a
seventh status.

`recent_changes.json` is now at the repository root. Read it when you start and before a new
task. Most of it is rules neither of us should build yet, listed rather than removed because
knowing a rule is coming changes how you shape what you build now.
