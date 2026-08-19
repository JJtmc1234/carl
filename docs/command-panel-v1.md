# Command Panel v1, frozen

Built by three processes working in parallel: the backend and shared model, the egui interface,
and the providers. This says what it is, what it deliberately is not, and what is known to be
missing so that nobody rediscovers it as a surprise.

**Frozen means: bugs may be fixed, and no new v1 features.** Anything larger belongs to a later
phase. The point of freezing is that the next thing built on top has something that stops moving.

## What it is

A fullscreen local control surface for the army, live, with four tabs and a contextual workspace.

```
  carl panel                      the backend, a unix socket at ~/.carl/panel/panel.sock
  carl-panel                      the interface. --home to point it elsewhere, --mock to work
                                  on it with no backend, --windowed, --frames N, --seconds N
```

Three rules run through the whole thing and are each enforced by tests rather than by intention.

**Existing state stays authoritative.** The panel holds no store. Tasks are folded from the
journal on demand, because a task is never written to disk anywhere else. The organisation is the
compiled table in `army::org`. There is no second event log and no second task database, so there
is nothing that can drift and no moment where somebody has to decide which copy was right.

**Unknown is a value, not a blank.** A reading nobody could take is `Reading::Unknown` and never
zero, an event driven fact carries no timestamp because it is true until something changes it,
and a sampled one always carries when it was read. A panel that showed an unreadable disk and a
full disk as the same number would be worse than one that showed neither.

**Observing changes nothing.** Opening a panel on a home reads it and writes nothing except the
socket, which has to live somewhere. This was not true at first and is now proved on a fresh home
in `panel_contract.rs` and `tests/providers.rs`.

## The ordering promise, which is the load bearing one

Army events are ordered by the journal sequence and that is the only ordering there is.
Telemetry frames carry **no sequence**, because a CPU reading is not a thing that happened. A
panel replaces the diagnostics it holds and leaves its resume point alone.

Proved end to end against the real binaries, on a temporary home:

```text
snapshot seq 6  agents 4  projects 1  diagnostics 19
  sample  at 1787101845  last_seq still 6
  event   seq  7  delegated   last_seq now 7
  sample  at 1787101847  last_seq still 7
  link    Reconnecting                        <- backend killed
  link    Disconnected
  link    Connected                           <- backend restarted
  event   seq  8  delegated   last_seq now 8  <- happened while disconnected
events seen: [7, 8]   no duplicates and in order: true
```

`cargo run --release --example v1_endtoend -- <home>` runs that scenario against any home.

## Known limitations in v1

These are honest gaps, not defects. Each is here so it is not rediscovered as a surprise.

**Carl's conversation history is not loaded.** The Carl tab starts empty and fills only with what
is said during that panel session. Carl's earlier turns live in the conversation record on the
surfaces they happened on, and the panel is not given them. The tab now says so in as many words
rather than showing a blank pane, because blank reads as "Carl has said nothing" when the truth
is "nothing from before this panel opened is here". Nothing is invented from the army journal to
fill it: a delegation is not something Carl said, and dressing one up as conversation would make
the tab lie about the one thing it exists to show.

**`AgentView.process` is always unknown.** The provider can list claude processes, but nothing
anywhere associates a process with an agent. Turning "some claude is running" into "Nora's claude
is running" would be a guess presented as a fact on the one screen somebody would open to check
exactly that. It becomes answerable when the runtime supervisor exists, which is
`docs/army-runtime.md`.

**The global shortcut needs one line of desktop setup.** F9 works when the panel has focus, which
is all a client can do on GNOME under Wayland. `carl-panel --toggle` flips a running panel, and a
custom GNOME shortcut pointed at that makes it genuinely global.

**Dead terminal sessions are kept until reaped.** That is deliberate: a row that vanished the
instant its shell exited would never get to say why. Read `is_alive`, and call `reap()` when you
redraw. Do not treat disappearance as the signal that something died.

**A subscribed connection is never idle**, because telemetry keeps arriving. `nc -U -w` will not
time out on one.

## What was verified, and how

847 tests, `cargo fmt --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, nothing ignored or skipped. The provider oracle, `cargo test --test providers`, passes on
its own as a first class gate at 11.

Several of the guards were proved by breaking the thing they guard and watching them fail:

- the terminal environment scrub, with the removal loop disabled, failed naming `LD_LIBRARY_PATH`
  and the value that would have reached the shell. The list integrity test passed throughout,
  which is exactly why checking a list is not enough.
- the `needs_screen` pointer rule, with the old rule restored, failed on the real sentence from
  the record that caused the original defect.

## The eleven defects this work found

They are in `bug-list.md` in full. The ones worth remembering as shapes rather than incidents:

- **A torn append corrupts a boundary, and a boundary is shared.** An interrupted write to the
  milestone file left it with no trailing newline, so the next append was glued to the broken line
  and one crash cost two records. Anything that appends lines and does not check how the file ends
  has this bug.
- **A read that creates is not a read.** `Personnel::open` created the army directory, so opening
  a panel on a home with no army left one behind, which is the difference between "never founded"
  and "founded and everybody left".
- **A failed connect is not proof of absence.** Treating `EAGAIN` from a full backlog as a dead
  socket would have deleted a live backend's socket and stranded it.
- **A test that reads a list tests the list.** The scrub guard checked the configured names rather
  than the child's environment, so it would have passed with the scrub removed.
