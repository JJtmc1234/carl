# the command panel

A fullscreen operations interface for the army. Four tabs, one adapter boundary, and no state
model of its own.

## the framework, and why

**egui with eframe 0.29**, pure Rust, in a workspace member crate `panel` that depends on
`carl` by path.

Chosen after checking the machine rather than by preference. What was actually verified:

| option | finding |
|---|---|
| Tauri | `webkit2gtk-4.1` and `4.0` both missing from pkg-config. Installing needs sudo, which agents do not get. |
| gtk4-rs, gtk3 | dev packages missing, same problem |
| iced | would work, also winit based, but a second GUI stack for no gain |
| **eframe** | **probe crate built clean in 28s and ran and exited 0 on this session** |

The session is Wayland (`wayland-0`) with XWayland on `:0`, Intel GPU with direct rendering.
Every runtime library winit loads by hand is present: libX11, libxkbcommon, libwayland-client,
libXcursor, libXrandr, libXi, libEGL, libGL. That is why the missing dev packages do not
matter, and it is the reason this choice needs nothing installed on JJ's machine.

Two consequences worth knowing. egui is immediate mode, so the interface is redrawn every
frame and there is no widget tree to fall out of step with the data. And it adds about 200
crates, which is why the panel is its own member rather than a feature of `carl`.

## how to launch

```
cargo run -p carl-panel                          fullscreen, mock data
cargo run -p carl-panel -- --windowed            windowed, for working on it
cargo run -p carl-panel -- --tour --frames 300   drives every tab and exits
cargo run -p carl-panel -- --seconds 62          runs the whole scripted minute
cargo run -p carl-panel -- --toggle              flips a running panel and exits
```

The root `cargo build` and `cargo test` still cover only `carl`, because `default-members` is
set to `["."]`. Nobody who wanted the CLI has to compile a GUI. `--workspace` covers both.

## the shortcut, honestly

**F9 toggles the panel, and it is bound inside the window.** It works whenever the panel has
focus, which is all a toolkit can do on its own.

A truly global shortcut is registered with the desktop rather than with the application. This
is GNOME on Wayland, where a client cannot grab a key it does not have focus for, by design,
and no library changes that. So the application exposes the action instead:

```
carl-panel --toggle
```

That flips a running panel and exits. Bind it in Settings, Keyboard, Custom Shortcuts, and the
key becomes global.

**One shortcut opens it whether or not it is running.** A desktop shortcut can only run a
command, so a `--toggle` that merely flipped a live panel would do nothing on the first press
of the day, which is exactly when somebody wants it. It now starts one if none is up.

A pid file in the temp directory says which process is the panel, and the liveness check reads
the command line as well as the id: process ids are reused, and a stale file naming a stranger
would send the flip somewhere no panel would ever appear.

**The Wayland grab is deliberately not faked.** A shortcut that silently only works when you
are already looking at the window is worse than one that says it needs a line of setup, because
the first kind is discovered while Factorio is fullscreen and the panel will not come.

Restoring keeps the tab, the selected agent, the selected project, the open workspace, both
half typed boxes, and whether the conversation was pinned to the bottom. That set is a struct,
`app::Kept`, so the test asserts on all of it at once rather than on a list somebody will
forget to extend.

## attached to the real backend

`LivePanelDataSource` implements the same four methods the mock does, so nothing above the
seam can tell which it is on. The UI never sees a socket, a sequence, a gap or a frame.

**Two threads, because the client blocks and a frame loop must not.**

The reader owns the `LivePanel` and sits in `next_update()`, which returns only when something
actually happened. Blocking is what that thread is for. The commander runs one command at a
time on its own short lived `PanelClient`, for the reason Process 1 documented on
`LivePanel::command`: a subscribed connection cannot carry a request, and the reader is parked
inside a blocking call and cannot be borrowed to send one.

`poll()` drains with `try_recv` and returns, always. There is a test that fails if it ever
takes more than 50ms on a silent backend, because a panel that freezes whenever the army goes
quiet is frozen most of the day.

| backend | what the panel does |
|---|---|
| initial snapshot | replaces the whole model |
| `Update::Event` | applies only what the record states |
| `Update::Resynced` | replaces the whole model, never merges |
| `Update::Health` | moves the badge and nothing else |

A journal record says what happened, not what everything looks like afterwards, so a `moved`
frame moves that task and nothing else. Guessing at the rest is how a screen drifts and stays
confidently wrong until the next snapshot.

The conversation is the one thing carried across a resync. It is this session's talking, the
backend keeps none of it, and wiping what JJ just said to punish him for a dropped socket would
be the wrong way round.

## shared types, and the ones that were deleted

The panel had its own `Diagnostic` and `Project`. Both are gone. Main carries Process 3's as
canonical and both of mine were worse in the same way: a metric held a formatted string, so an
unreadable disk and a full one arrived as the same kind of thing, and a phase was an enum, so a
project whose phase was worded differently drew as unknown rather than as what it said.

What is left on this side is what the screen genuinely has to decide.

**Which board a component sits on.** `model::render::group_of`. `system.` is the machine and
everything else is the army, which survives Process 3's rename without a list of legacy
prefixes: `army.agent.nora` and the older `agent.nora` both land in the right place. When
`Diagnostic::group()` merges this becomes a call to it.

**Whether a reading has an age.** Now `Kind` from the backend rather than guessed from a
prefix. Sampled carries a moment and goes stale after 30s. Event driven carries none, because
it is true until something changes it and a clock beside it would say it decays.

**The agent overlay.** Unenlisted is unknown, blocked beats whatever the task says, and unknown
stays unknown. Nothing turns a process indicator green because some Claude process exists
somewhere.

`metric_pairs()` renders an unreadable value as the word unknown, so a gap cannot read as a
zero even for a consumer that only ever sees the flat form.

`TaskView::project` is the link the projects pane walks, project to task to agent. A project
with no tasks shows none rather than the ones that happen to read like it.

## the one boundary

Everything above `PanelDataSource` draws. Everything below it fetches. No widget anywhere knows
where its data came from.

```rust
pub trait PanelDataSource {
    fn snapshot(&mut self) -> Snapshot;
    fn poll(&mut self) -> Vec<PanelEvent>;
    fn submit(&mut self, command: Command) -> Result<(), String>;
    fn link(&self) -> Link;
    fn describe(&self) -> String;
}
```

**For Process 1**, the integration point is exactly one thing: write
`LivePanelDataSource` implementing that trait, and change one line in `panel/src/main.rs`:

```rust
let mut app = App::new(Box::new(MockPanelDataSource::new()));
//                              ^ becomes LivePanelDataSource::connect(...)
```

Nothing else in the panel changes. `PanelEvent` is a closed list on purpose, for the same
reason `army::event::Event` is: a free string means every writer invents its own wording and no
reader can count anything.

Three behaviours the live source must honour, because the panel is built on them.

**A snapshot is authoritative and complete.** The panel takes one at startup and another after
every reconnection, and it replaces what was on screen rather than merging. Events missed while
disconnected were never delivered, so patching the next one onto old state leaves a version of
the world nobody ever sent.

**Unknown is a value.** Return `Health::Unknown`, `None`, an empty metric list. Do not return a
zero for something nothing measured. The panel draws the gap.

**Refuse commands while the link is down** rather than queueing them silently. The panel
refuses first and says so, but a source that accepted an intervention nobody received would be
worse than one that failed loudly.

## the workspace, for Process 3

The panel owns the container and the interaction model. It spawns no shells and reads no files.

The whole seam is one enum:

```rust
pub enum WorkspaceRequest {
    File { path: String, line: Option<u32> },
    Diff { task: TaskId },
    Terminal { cwd: String },
    Investigate { component: String },
    Close,
}
```

It is emitted as `Command::Workspace(..)` and also opens the pane locally. Today the pane names
what it would open and says it is not attached. Filling it means setting
`app::Workspace::content`, and the docked pane already lays out around it.

It is bottom docked rather than a fifth tab on purpose. A terminal or an editor is a tool you
open from something you were already looking at, and making it a destination inverts that: you
would go to the editor and then hunt for the file.

Triggers already wired: an agent's worktree opens a terminal, a task opens its diff, a
diagnostic row opens an investigation.

## the visual language

One accent, a cold amber on near black. Accent means live, selected, or needs you. If
everything glowed, glowing would mean nothing, so most of the screen is grey on black and the
eye goes to the few things that are not.

Colour carries state and nothing else. Never decoration, never a category. Health and agent
status are the only things that get a hue, so a coloured thing on this screen is always
something you might have to act on. Unknown is deliberately colourless, because a gap in what
was measured is not a fault and treating it as one trains somebody to ignore the screen.

Depth comes from four background steps rather than shadows. No gradients, no glass, no bevels,
no hexagons, no radar. Structure comes from hairline rules and indentation.

Monospace throughout, because nearly everything here is an identifier, a figure or a state.
Nothing below 11px, and the small labels are letter spaced rather than shrunk, which reads as
deliberate where tiny type reads as cramped.

Motion is reserved for meaning. A row that just changed carries a dim left bar for two seconds
and nothing else animates.

**The text was invisible for three commits, and it was never a colour.**

`eframe` is declared `default-features = false`, and `default_fonts` is one of those defaults.
Without it egui has no fonts at all. Every shape still draws, so the panel came up with the
right colours, borders, status pips and hairlines and not one glyph, which looks exactly like
black text on a black background and is nothing of the sort.

Three wrong diagnoses before anyone looked at the manifest. The desktop theme was blamed, then
the OpenGL backend, and wgpu changed nothing because the renderer was never involved. What
solved it was a screenshot: shapes present, glyphs absent, identical under two renderers, which
is a font question and not a colour one.

The check that missed it inspected the colour each text shape asked for, which was always
correct, and so never noticed there was nothing to paint in it.
`there_are_fonts_and_they_produce_actual_glyphs` lays out four letters in every text role and
both families and counts the glyphs, and it fails without `default_fonts`.

`glow` is the default renderer. `--features wgpu` is kept for a machine where OpenGL genuinely
is at fault, which this one was not.

**The panel forces dark and stops following the desktop.** JJ's desktop is in light mode, egui
follows the desktop by default and reapplies that preference every frame, and the first build
painted every label that did not name its own colour black on near black. Setting the visuals
once was not enough, because the preference won a fraction of a second later.
`theme::install` now sets `ThemePreference::Dark` and runs every frame, and
`the_installed_theme_is_dark_whatever_the_desktop_prefers` fails against the old code.

## what is not built here

No layered memory, no scheduling, no periodic reports, no queue, no dependency scheduler, no
notifications, no diagnostics collectors, no project discovery, no terminal process management,
no filesystem writes. The panel reads and asks.

`docs/army-v1.md` line 197 lists a dashboard and diagnostics under what not to build yet. JJ
has since asked for both directly, so the newer instruction supersedes that line. Noted here
rather than left as something two processes happen to have understood.
