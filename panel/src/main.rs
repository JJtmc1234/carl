//! The Command Panel, as a program.
//!
//! Fullscreen on start, and one key hides and shows it. What that key can reach is the one
//! thing worth being precise about, so it is written down here rather than claimed:
//!
//! **F9 is bound inside the window.** It works whenever the panel has focus, which is what a
//! toolkit can do on its own. A truly global shortcut, one that reaches the panel while JJ is
//! in Factorio, is registered with the desktop rather than with the application. On this
//! machine that is GNOME on Wayland, where a client cannot grab a key it does not have focus
//! for, by design.
//!
//! So the application exposes the action and the desktop binds it. `carl-panel --toggle`
//! flips a running panel, and one custom shortcut in GNOME Settings pointed at that command
//! makes the key global. That step is deliberately not faked here, because a shortcut that
//! silently only works when you are already looking at the window is worse than one that is
//! honest about needing a line of setup.

use carl_panel::source::{LivePanelDataSource, PanelDataSource};
use carl_panel::{App, MockPanelDataSource, launch, theme, ui};
use eframe::egui::{Key, ViewportCommand};

/// How the panel was asked to start.
struct Args {
    /// Windowed rather than fullscreen. For working on the panel without it taking the screen.
    windowed: bool,
    /// Draw this many frames and exit. Turns a manual look into something that can be run in
    /// a check, and proves the whole draw path executes rather than only the state machine.
    frames: Option<u32>,
    /// Walk every tab, open an agent, a project and the workspace, and let the scripted
    /// timeline run underneath it all.
    ///
    /// This is what makes "we opened it" mean something. Unit tests exercise the state and
    /// never the layout, and a panel breaks in the drawing: a row that cannot fit, a borrow
    /// held across a closure, a scroll area nested inside itself. None of that shows up until
    /// something actually paints it.
    tour: bool,
    /// Run for this many seconds and exit.
    ///
    /// Separate from `--frames` because the live timeline runs on a clock. A frame count says
    /// the layout painted, and only elapsed time says the scripted blocker, decision,
    /// disconnection and milestone actually reached the screen.
    seconds: Option<u64>,
    /// Carl's home, when it is not the usual one.
    ///
    /// Added during final integration. Without it the panel can only ever be pointed at the real
    /// `~/.carl`, which makes an end to end run on a temporary home impossible: the only way to
    /// exercise the whole thing was to exercise it against JJ's actual army. A test that can only
    /// be run against production is a test nobody runs twice.
    home: Option<std::path::PathBuf>,
    /// Run against the scripted mock rather than the backend.
    ///
    /// For demonstrating and for working on the interface with no backend running. Never the
    /// default, because a panel that quietly shows invented figures when the army is down is
    /// the worst thing this whole design is trying to avoid.
    mock: bool,
}

fn parse_args() -> Args {
    let all: Vec<String> = std::env::args().collect();
    Args {
        windowed: all.iter().any(|a| a == "--windowed"),
        tour: all.iter().any(|a| a == "--tour"),
        mock: all.iter().any(|a| a == "--mock"),
        home: all
            .iter()
            .position(|a| a == "--home")
            .and_then(|i| all.get(i + 1))
            .map(std::path::PathBuf::from),
        seconds: all
            .iter()
            .position(|a| a == "--seconds")
            .and_then(|at| all.get(at + 1))
            .and_then(|n| n.parse().ok()),
        frames: all
            .iter()
            .position(|a| a == "--frames")
            .and_then(|at| all.get(at + 1))
            .and_then(|n| n.parse().ok()),
    }
}

fn main() -> eframe::Result<()> {
    // What a desktop shortcut is pointed at. One key, whether or not a panel is up: flip the
    // one that is running, or start one if none is. A shortcut that only worked once a panel
    // already existed would do nothing on the first press of the day, which is exactly when
    // somebody wants it.
    if std::env::args().any(|a| a == "--toggle") {
        match launch::wanted() {
            launch::Wanted::Flip(pid) => match launch::ask_toggle() {
                Ok(()) => println!("asked the panel to show or hide, pid {pid}"),
                Err(e) => eprintln!("could not reach the running panel: {e}"),
            },
            launch::Wanted::Start => match launch::start_detached() {
                Ok(pid) => println!("no panel was running, started one, pid {pid}"),
                Err(e) => eprintln!("could not start a panel: {e}"),
            },
        }
        return Ok(());
    }

    let args = parse_args();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("AOS Command Panel")
            .with_inner_size([1600.0, 940.0])
            .with_fullscreen(!args.windowed),
        // Only when built for it. OpenGL was blamed for the missing text and was innocent.
        #[cfg(feature = "wgpu")]
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    // Says this process is the panel, so a later shortcut press finds it instead of starting
    // a second one on top.
    launch::claim(std::process::id());

    let mut app = App::new(source(&args));
    let mut drawn = 0u32;
    let began = std::time::Instant::now();
    let mut said_at = 0u64;

    let ran = eframe::run_simple_native("carl-panel", options, move |ctx, _frame| {
        // Every frame rather than once. Setting it at startup left the desktop free to put a
        // light theme back over the top, and the panel would rather spend a few microseconds
        // than be unreadable.
        theme::install(ctx);

        // Ask for fullscreen again on the first frame the window is really up.
        //
        // The builder hint alone is not enough. Under Wayland the window is not mapped when
        // that hint is read, so the compositor is free to ignore it and hand back an ordinary
        // window, which is what it was doing. Sending the command once the window exists is
        // what actually makes it fullscreen, and it costs one command on one frame.
        if drawn == 0 && !args.windowed {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        }

        // Live by polling the source every frame, never by asking a person to refresh.
        app.tick();

        if ctx.input(|i| i.key_pressed(Key::F9)) || launch::toggle_asked() {
            app.toggle_visible();
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(app.visible));
            ctx.send_viewport_cmd(ViewportCommand::Minimized(!app.visible));
        }

        if args.tour {
            tour(&mut app, drawn);
        }

        if app.visible {
            ui::draw(&mut app, ctx);
        }
        drawn += 1;

        // While a timed run is going, say what is on screen every few seconds, so the live
        // changes are visible in the output of a run nobody is watching.
        if args.seconds.is_some() {
            let elapsed = began.elapsed().as_secs();
            if elapsed >= said_at + 5 {
                said_at = elapsed;
                // The two kinds of freshness side by side, because the whole point is that one
                // moves and the other does not.
                let unknown = app
                    .snapshot
                    .diagnostics
                    .iter()
                    .filter(|d| d.health == carl_panel::model::Health::Unknown)
                    .count();
                println!(
                    "{elapsed:>3}s  link {:<16} nora {:<8} decisions {}  milestones {}  \
                     seq {}  sampled {}  unknown {unknown}",
                    app.link.label(),
                    app.snapshot
                        .agent("nora")
                        .map(|a| a.status.label())
                        .unwrap_or("gone"),
                    app.snapshot.decisions.len(),
                    app.snapshot
                        .projects
                        .iter()
                        .map(|p| p.milestones.len())
                        .sum::<usize>(),
                    app.last_seq(),
                    app.sampled_at
                        .map(|s| s.to_string())
                        .unwrap_or("never".into()),
                );
            }
        }

        if let Some(limit) = args.seconds
            && began.elapsed().as_secs() >= limit
        {
            println!("ran {limit}s, drew {drawn} frames, no panic");
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        // The self check. Draws for real, reports what it saw, and leaves.
        if let Some(limit) = args.frames
            && drawn >= limit
        {
            println!(
                "drew {drawn} frames. tab {:?}, link {}, {} agents, {} diagnostics, \
                 {} projects, {} conversation turns, {} decisions",
                app.tab,
                app.link.label(),
                app.snapshot.agents.len(),
                app.snapshot.diagnostics.len(),
                app.snapshot.projects.len(),
                app.snapshot.conversation.len(),
                app.snapshot.decisions.len(),
            );
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        // The panel is live, so it redraws on a clock rather than only on input. Modest, so an
        // idle panel is not a busy loop.
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    });

    // Forgets this process, so the next shortcut press starts a fresh panel rather than asking
    // one that has gone to flip. A crash leaves the file behind, which is why `running` checks
    // the process is genuinely a panel rather than trusting the id.
    launch::release();
    ran
}

/// Whatever the panel should be attached to.
///
/// The live backend unless told otherwise, and if it cannot be reached the panel says so and
/// falls back to the mock **while saying which one it is on**, in the rail, at all times. The
/// alternative is refusing to start, which leaves JJ with no way to see that the army is down.
fn source(args: &Args) -> Box<dyn PanelDataSource> {
    if args.mock {
        return Box::new(MockPanelDataSource::new());
    }

    let socket = match &args.home {
        Some(home) => carl::panel::socket_path(home),
        None => LivePanelDataSource::default_socket(),
    };
    match LivePanelDataSource::open(&socket) {
        Ok(live) => {
            println!("attached to {}", socket.display());
            Box::new(live)
        }
        Err(why) => {
            eprintln!(
                "could not reach the backend at {}: {why}\n\
                 start it with `carl panel`, or run with --mock to work on the interface.\n\
                 falling back to the mock, and the rail will say so.",
                socket.display()
            );
            Box::new(MockPanelDataSource::new())
        }
    }
}

/// Drives the panel through everything worth painting, on a schedule.
///
/// Deliberately touches the states that only exist for a moment: a blocked worker, a decision
/// waiting, the link down, and the workspace open over the top of a tab.
fn tour(app: &mut App, frame: u32) {
    use carl_panel::{Tab, WorkspaceRequest};

    match frame {
        30 => app.select_tab(Tab::Agents),
        45 => app.select_agent("nora"),
        70 => app.select_agent("carl"),
        90 => app.select_tab(Tab::Diagnostics),
        120 => app.select_tab(Tab::Projects),
        135 => app.select_project("jjtorio"),
        160 => app.select_project("command panel"),
        180 => app.open_workspace(WorkspaceRequest::Terminal {
            cwd: "/home/jj_tmc/Projects/carl".into(),
        }),
        210 => app.select_tab(Tab::Agents),
        225 => app.open_workspace(WorkspaceRequest::File {
            path: "/home/jj_tmc/Projects/carl/src/army/org.rs".into(),
            line: Some(42),
        }),
        250 => app.close_workspace(),
        270 => app.select_tab(Tab::Carl),
        _ => {}
    }
}
