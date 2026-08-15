# Command Panel backend, v1

The contract Processes 2 and 3 build against. Everything here is implemented and tested in
`src/panel/`. Every example below is copied from a real run against the real binary, not written
by hand.

Read this before writing a schema of your own. If something you need is missing, ask rather than
adding it locally: two definitions of a frame is two half systems that do not meet.

## Transport

**A unix domain socket at `~/.carl/panel/panel.sock`, line delimited JSON, both directions.**

Start it with `carl panel`. It prints the path it bound and then serves.

Four things decided this, and they were checked rather than assumed:

- Carl has **no local IPC at all** today. Three services share state through files in `~/.carl`.
  There was no existing HTTP server, no socket, no async runtime, so nothing to match.
- Carl has **no async runtime** and one panel. An executor would be more machinery than the thing
  it runs. Blocking threads are what the rest of Carl is.
- **The socket file is the authentication.** It sits in a `0700` directory and is itself `0600`,
  so only JJ's own processes can reach it, enforced by the kernel rather than by a check. A TCP
  port on loopback would be reachable by every process and every user on the machine.
- `aosd` in the sibling AOS repository already solved this exact problem the same way, and two
  bug fixes are baked into the order of operations in `listen.rs`. Reinventing it would mean
  finding them again.

**Framing.** One JSON object per line, `\n` terminated, UTF-8. Both directions. It is readable
with `nc -U ~/.carl/panel/panel.sock` when the backend is the thing that is broken.

**If your UI is a browser page**, it cannot open a unix socket. Write a small bridge process that
relays socket lines to a WebSocket. Do not move the backend to a TCP port: the file permissions
are the whole security model, and a bridge keeps the compromise at the edge where it is visible.

## Every frame carries `v`

`v` is `1`. A frame whose version the backend does not know is refused with both versions named:

```json
{"v":1,"reply":"refused","why":"this backend speaks panel protocol 1 and that frame said 99"}
```

Bumped when a frame changes shape in a way an older panel would misread. Not bumped for adding a
field an older panel ignores.

## Panel to backend

```json
{"v":1,"id":"<your id>","ask":"ping"}
{"v":1,"id":"<your id>","ask":"snapshot"}
{"v":1,"id":"<your id>","ask":"subscribe","since":0}
{"v":1,"id":"<your id>","ask":"command","command":{"kind":"say","text":"hello"}}
```

`id` is yours. It comes back on the reply so several requests in flight can be told apart.

## Backend to panel

`reply` is one of `pong`, `snapshot`, `event`, `live`, `gap`, `done`, `speaking`, `refused`.

Replies to a request carry your `id`. **Stream frames have no `id`**, because nothing asked for
them. Do not treat one as a late answer to a request you made.

## The four calls

### `ping`

```json
{"v":1,"id":"a","reply":"pong"}
```

### `snapshot`

The whole world at one moment, with the journal sequence it was built from.

```json
{"v":1,"id":"a","reply":"snapshot","snapshot":{
  "seq":6, "at":1755266400,
  "carl":{"status":{"known":"unknown"},"pending":[],"objectives":[],"recent_delegations":[]},
  "agents":[...], "tasks":[...], "projects":[], "diagnostics":[]
}}
```

`snapshot.seq` is the join to the stream. Subscribe from exactly it and the next frame is the
next record, with nothing repeated and nothing skipped.

### `subscribe`

`since` is the last sequence you already have. `0` means from the beginning.

The backend replays everything after `since`, then sends `live`, then streams. **`live` is always
sent, even when the replay was empty**, so you never have to infer that you have caught up from a
quiet connection.

```json
{"v":1,"reply":"event","event":{"seq":1,"at":1755266400,"kind":"intervened",
  "entity":{"entity":"agent","name":"nora"},
  "record":{"seq":1,"at":1755266400,"actor":"jj","event":"intervened",
            "intervention":"override","agent":"nora","instruction":"check the belt"}}}
{"v":1,"id":"sub","reply":"live","seq":6}
```

**Subscribing takes over the connection.** Open a second connection for commands.

`entity` is who the frame is **about**, which is not always who acted. A `notified` frame is about
the person notified; an `intervened` frame is about the agent it reached. Both have
`actor: "jj"`. File by `entity`, not by actor, or every JJ intervention lands on JJ's row and the
agent it happened to shows nothing.

`record` is the untouched journal line. Read it rather than parsing prose out of anything.

`kind` is one of: `delegated`, `moved`, `submitted`, `reviewed`, `refused`,
`emergency_declared`, `decided`, `intervened`, `notified`.

### `gap`

You asked to continue from a sequence the record cannot honour. Take a fresh snapshot and
resubscribe from its `seq`. This is sent instead of a stream, never alongside one.

```json
{"v":1,"id":"sub","reply":"gap","asked_for":9999,"have_from":1,"have_to":6,
 "why":"that sequence is past the end of this record, so it is not the record you were reading. Take a fresh snapshot."}
```

### `command`

```json
{"v":1,"id":"c1","reply":"done","seq":1,"what":"recorded as a JJ intervention, told carl and mason"}
```

`seq` is where it landed in the journal, absent for commands that write nothing.

## Commands

Tagged by `kind`.

| kind | fields | writes to the journal |
|---|---|---|
| `say` | `text` | no, it is a conversation |
| `objective` | `text` | `intervened`/`objective`, and Carl is told |
| `answer` | `seq`, `text` | `intervened`/`answered` |
| `inspect` | `agent` | no, it changes nothing |
| `jj_message` | `agent`, `text` | `intervened`/`message` |
| `jj_instruct` | `agent`, `instruction` | `intervened`/`override` |
| `jj_stop` | `agent`, `why` | `intervened`/`stopped` |
| `jj_replace` | `agent`, `goal`, `why` | `intervened`/`replaced` |

**There is no actor field on a command, and there must never be one.** The socket already answered
who is sending, so a field would only be somewhere to write `"mason"` and be believed. An unknown
field is rejected rather than ignored, so a hopeful caller finds out.

`say` streams. You get `speaking` frames as Carl produces text, then a `done` with the whole
answer. It goes through the same `turn` machinery as the terminal, the microphone and Slack, in a
thread called `panel`. Same Carl, same memory, same rules.

## JJ interventions

JJ has absolute authority and the chain rules do not apply to him. That is different from a
security bypass, and the record keeps them apart:

- Written as `intervened` with `actor: "jj"`. **Never** as a `delegated` from the agent's lead.
  "Mason reassigned her" and "JJ reassigned her over Mason's head" are different facts.
- Carl is always told, because he is accountable for an army that just changed under him.
- The affected agent's lead is told, because they thought they knew what their report was doing.
- Notifications carry `about: <seq>` and point back at the intervention rather than repeating it,
  so they cannot come to disagree with it.
- Nobody is told twice, and JJ is never notified of his own intervention.

## Unknown is a value

`Maybe<T>` is `{"known":"unknown"}` or `{"known":"known","value":...}`.

Render those differently. A blank that means "no process running" and a blank that means "nobody
looked" read the same on screen, and that is how a dead agent looks merely idle.

**Always `unknown` in v1**, waiting on Process 3: `AgentView.process`, `CarlView.status`.
**Always empty in v1**: `projects`, `diagnostics`. Carl has no project model and measures nothing
yet. They are empty rather than invented, because a milestone guessed from a git commit is a made
up answer that looks like a real one.

## What is authoritative, and what is derived

| shown | comes from | notes |
|---|---|---|
| name, display, rank, remit, reports_to | `army::org`, compiled in | never unknown, never editable |
| department, sub_department, model, enlisted | the agent's folder on disk | absent until the army is founded |
| what an agent is holding | the folder's `state.json` | it is what survives a restart |
| every task | **folded from the journal** | tasks are never written to disk anywhere |
| everything live | the journal | the only ordering authority |

**The panel holds no store of its own.** Views are folded from the record on demand and thrown
away. Do not build a task table on your side either: keep the last snapshot, apply events to it,
and re-snapshot whenever you get a `gap`.

## For Process 2: use the client, not the protocol

**Everything above this line is implementation detail you should not need.** `carl::panel::live`
does all of it. Your `LivePanelDataSource` is meant to be thin and boring.

```rust
use carl::panel::live::{LivePanel, Update, Health};
use carl::panel::{socket_path, PanelCommand};

let (mut live, snapshot) = LivePanel::open(&socket_path(&home))?;
// `snapshot` is a PanelSnapshot. Draw it.

loop {
    match live.next_update() {
        Update::Event(e)      => apply(e),          // in order, no holes, ever
        Update::Resynced(s)   => redraw_from(s),    // throw away what you had
        Update::Health(h)     => show_link(h),      // Connected/Reconnecting/Stale/Disconnected
    }
}
```

`LivePanel` also implements `Iterator<Item = Update>`, so `for update in live` works. It never
returns `None`: a panel that has lost its backend keeps trying, because a screen going blank is
not the answer.

Run it on a thread and forward `Update`s to your UI. It only returns when something actually
happened, so there are no ticks to filter out.

### What it does for you

| you would have had to | it does |
|---|---|
| open and frame a unix socket | `LivePanel::open` |
| track the last sequence | `live.last_seq()` |
| notice a hole in the stream | refuses it as an error rather than accepting it |
| reconnect on failure | automatic, with `Health` reported honestly |
| resume from the right place | resubscribes from the last accepted sequence |
| recover from a `gap` | takes a fresh snapshot and gives you `Update::Resynced` |
| tell a quiet army from a dead backend | pings on a second connection before claiming either |

### Sending commands

```rust
live.command(PanelCommand::JjInstruct { agent: "nora".into(), instruction: "...".into() })?;

// Carl's answer, as he produces it:
live.command_streaming(
    PanelCommand::Say { text: "what is Nora up to".into() },
    &mut |text| print!("{text}"),
)?;
```

Commands go out on a connection of their own, because the subscribed one cannot carry a request.
That is handled for you.

**Streaming is real.** `say` and `objective` produce `speaking` frames while Carl is still
talking, and `command_streaming` hands each one over as it arrives. Nothing splits a finished
answer into pieces afterwards to look like streaming. Every other command produces no `speaking`
frames at all and the callback is simply never called.

### Connection health, and what each state entitles you to claim

| `Health` | what is true | what the screen should say |
|---|---|---|
| `Connected` | contact confirmed within the last few seconds | current |
| `Stale` | connected, quiet, and a check has not yet confirmed the backend is alive | was true a moment ago |
| `Reconnecting` | the connection went and it is trying again | not current |
| `Disconnected` | attempts are failing | not current, and say so plainly |

A quiet army and a dead backend look identical from a subscribed socket, because the backend
sends nothing when nothing is happening. So silence proves nothing, and `LivePanel` opens a
second short lived connection and pings rather than assuming. `since_contact()` tells you how
long since contact was last confirmed.

### If you need the layer below

`carl::panel::client::PanelClient` is one connection, typed: `connect`, `ping`, `snapshot`,
`command`, `command_streaming`, `subscribe`. `subscribe` **consumes** the client and gives back
an `Events`, because a streaming connection cannot carry requests, and making that a type change
rather than a rule in a document means it cannot be got wrong.

Types to generate from, if your UI is not in Rust: `src/panel/view.rs` and `src/panel/wire.rs`.
They serialise exactly as shown above.

### Running example

`cargo run --example panel_watch -- ~/.carl` is a complete panel in forty lines. It has no state
of its own beyond what it prints, which is the point.

## For Process 3

Do not write to the journal from a collector. Diagnostics are measurements, not army history, and
the journal's value is that everything in it is worth reading.

Give me providers with this shape and I will wire them behind the backend:

```rust
fn diagnostics() -> Vec<DiagnosticView>;   // component, health, summary, measured_at, metrics
fn projects()    -> Vec<ProjectView>;      // id, name, goal, phase, department, tasks, blockers
fn process_state(agent: &str) -> Maybe<ProcessState>;
```

`measured_at` must be when you measured, not when you were asked. `Health::Unknown` is the
default and is correct for anything you did not actually check.

Terminal and editor are yours and are not in this protocol. If they need to reach the backend,
tell me what for and I will add a command rather than having you open a second channel.

## Lifecycle

`carl panel` removes its socket when it stops, on a normal return, on a panic unwinding, and on
SIGTERM or SIGINT. systemd stops a service with SIGTERM, so that is the ordinary stop rather than
the rare one, and without it every stop would leave a file that made `ls` suggest a backend was
running.

SIGKILL and a power cut still leave the socket, because nothing can run at that point. That is
what the stale socket recovery is for: a start that finds a socket in its way connects to it, and
clears it only when nothing answers. A live backend is never stranded and a dead one never needs
manual cleanup.

Two backends cannot share a home. The second refuses at bind, naming the path. There is no lock
file, because the socket already answers the question and a second mechanism would be a second
thing that can disagree.

## Verified

447 tests, `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` clean.

The full path, with the real binaries and a real restart, as run:

```text
snapshot at seq 0: 4 agents, 0 tasks
  event  seq  1  delegated   Task { id: "cafe01", actor: "mason" }   <- separate process
  event  seq  2  moved       Task { id: "cafe01", actor: "mason" }
  link   Reconnecting                                                <- backend killed
  link   Disconnected
  link   Connected                                                   <- backend restarted
  event  seq  3  submitted   Task { id: "cafe01", actor: "mason" }   <- happened while away
  event  seq  4  reviewed    Task { id: "cafe01", actor: "mason" }
```

Sequences 3 and 4 were written while nothing was watching, and arrived in order after the
reconnect with nothing repeated and nothing skipped. No refresh was asked for at any point. The
socket was gone from the filesystem between the kill and the restart.
