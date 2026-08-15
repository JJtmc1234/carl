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

**This step is deliberately not faked.** A shortcut that silently only works when you are
already looking at the window is worse than one that says it needs a line of setup, because
the first kind is discovered while Factorio is fullscreen and the panel will not come.

Restoring keeps the tab, the selected agent, the selected project, the open workspace, both
half typed boxes, and whether the conversation was pinned to the bottom. That set is a struct,
`app::Kept`, so the test asserts on all of it at once rather than on a list somebody will
forget to extend.

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
