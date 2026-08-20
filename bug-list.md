# bug list

Every bug of consequence, and the test that stops it coming back. No entry without a test.

| n | bug | how it showed up | guard |
|---|---|---|---|
| 1 | The microphone consumer read 0.6s per pass while a pass cost more than that, so it fell behind real time forever. | "it doesn't pick up what I say" | rewritten with a reader thread, `the_ring_never_grows_past_its_window` |
| 2 | Silence was measured as peak against a fixed floor, and one key press puts peak at 0.24 in a quiet room, so a recording never ended early. | Carl woke and then never answered | `one_click_fools_peak_and_does_not_fool_rms` |
| 3 | Carl heard his own voice through the speakers and answered his own answer. | replies to himself | echo cancellation, and `Devices` falling back to muting |
| 4 | `say` wrote the whole text into piper before starting the player, so a long answer deadlocked both. | never fired, found by reading | the player starts before any text is written |
| 5 | Slack was sent a JSON body for `users.info`, which only reads form parameters, so the id was dropped and Slack reported `user_not_found`. | Carl could not work out anybody's name | `user_not_found_mentions_the_encoding_trap` |
| 6 | A bare name in a channel produced an empty question, which the CLI refuses. | Carl posted a raw CLI error into a channel, twice | `an_empty_question_is_refused_here_rather_than_by_the_cli` |
| 7 | Two echo cancellers ran at once, so naming a device picked an unpredictable half of an unpredictable pair and nothing was cancelled. | Carl answered himself in a loop while reporting echo cancelled audio | `two_cancellers_are_refused_rather_than_gambled_on` |

| 8 | Overhaul mod families were matched with spaces removed from the mod name but not from the pattern, so any pattern containing a space could never match. | Space Exploration was installed and silently never reported | `no_overhaul_stem_can_be_impossible_to_match` |

| 9 | Mods were read from the directory rather than from `mod-list.json`, so mods present but switched off were reported as active. | 88 mods on disk and 4 enabled, so Carl was told a vanilla Space Age save was Sea Block with Angel's and Bob's, and answered a smelting question with ore processing that does not exist in that game | `only_the_mods_that_are_switched_on_are_reported` |

| 10 | `eframe` was declared `default-features = false`, and `default_fonts` is one of those defaults, so egui had no fonts at all. Every shape drew and there were no glyphs to paint. | "text is STILL black", three times, and it was never black and was never text | `there_are_fonts_and_they_produce_actual_glyphs`, and `both_font_families_are_present` |

## bug 10, in full

The Command Panel came up with the right background, the right accent, borders, status pips and
hairlines all correct, and not one letter anywhere on it. JJ reported it as black text on a
black background, which is exactly what it looks like, and it was neither black nor text.

Three wrong diagnoses before anybody read the manifest, and the wrong turns are the useful part.

**The desktop theme.** The machine is in light mode, egui follows the desktop and reapplies
that every frame, so setting visuals once loses a fraction of a second later. That is a real
bug and it was fixed and it was not this one. `theme::install` now sets `ThemePreference::Dark`
and runs every frame.

**The renderer.** Shapes drawing and glyphs not drawing is the signature of a font atlas that
never reached the GPU, so the OpenGL backend was blamed and the whole thing was rebuilt on
wgpu. It changed nothing, because the renderer was never involved.

**The colours.** A headless check walked the render output and confirmed the panel painted text
at `#D6DEE9` on `#06080B`. That check was correct and useless: it inspected the colour each
text shape asked for, and by construction it cannot see that there were no glyphs to paint in
it. A test that cannot fail for the real reason is worse than no test, because it is evidence
pointing the wrong way.

What solved it was a screenshot. Shapes present, glyphs absent, identical under two different
renderers, which is a font question and not a colour one and never was.

The guard measures glyphs rather than intent. It lays out four letters in every text role and
in both families, and asserts the laid out width, the height and the glyph count. Removing
`default_fonts` was tried again afterwards to be sure it bites: it fails with "laid out no
width, which means no font is loaded", and passes with the feature restored.

The general lesson is about `default-features = false`. It is reached for to keep a dependency
small, and it silently drops things that were never thought of as optional. Fonts are not a
feature of a graphical toolkit in any sense a person would recognise.

## bug 9, in full

The worst kind, because the fix made the thing worse than before and was reported as working.

Carl was giving vanilla advice to somebody whose mods directory held Sea Block, Angel's,
Bob's and Space Exploration. Reading that directory looked like the obvious fix, and the new
answer talked about crushing, floating and leaching, which is Angel's ore processing. It read
as a clear improvement and was shown as proof.

None of those mods were switched on. Factorio keeps what is downloaded and what is enabled in
different places, and `mods/mod-list.json` is the one the game reads. Eighty eight downloaded,
four enabled, and three of those are the official Space Age components.

So the advice went from being right about the wrong game to being wrong about a game nobody
was playing. Vanilla advice on a vanilla save was closer to correct than the confident,
detailed, entirely inapplicable answer that replaced it.

JJ caught it, not a test, and not the person who wrote it. The lesson is not about Factorio.
A file being on disk is not the same as it being in use, and the difference is exactly the
kind of thing that produces a confident answer about something that is not there.

| 10 | Any sentence containing "that", "this" or "here" was treated as pointing at the screen, so an ordinary conjunction took a screenshot. | "Remember that my mentor is called Hunter Zhang" flashed the screen, captured a black image, and spent vision tokens describing it | `a_conjunction_is_not_somebody_pointing_at_the_screen` |

| 11 | An interrupted append to the milestone file left it without a trailing newline, so the next append was glued onto the broken line and both were lost. | One crash cost two milestones, and the second was the one somebody had just recorded and believed was safe | `a_truncated_line_costs_only_itself_and_the_next_appends_survive_a_reopen` |
| 12 | `ask_streaming` piped the claude child's stderr and never read it until after the child had exited. A pipe holds about 64 KiB, and a child blocked writing stderr stops writing stdout too, so the answer never arrived and neither side ever moved. | Never fired in the wild. Found by reading, then reproduced with a stub `claude`: 1000 bytes of stderr answers fine, 65537 and 200000 both hang past a watchdog. | `a_chatty_stderr_does_not_deadlock_the_answer`, `a_quiet_stderr_still_answers` |

## bug 11, in full

Found by Process 3 while writing durability tests for the project store, not by anything failing.

Milestones are one JSON object per line. A write cut off part way through, by a crash or a full
disk, leaves a file that does not end in a newline. The next append then writes straight onto the
end of that broken line, and the reader sees one unparseable line where there were two records.

So an interruption cost **two** milestones rather than one, and the second was the worse loss: it
was written after the machine came back, by somebody who had every reason to believe it was
safely on disk.

The fix is to close the boundary before writing. If the file does not end cleanly, a newline is
appended first, then the new record. The damaged line stays damaged and stays counted by
`milestone_gaps`, which is honest and visible, but it no longer takes its successor with it.

The general shape is worth remembering: in an append only text format, a torn write is not a
local problem. It corrupts the *boundary*, and a boundary is shared with the record that comes
next. Anything that appends lines and does not check how the file ends has this bug.

The army journal appends the same way and does not have it, which is luck rather than design:
`Journal::append` writes through `writeln!` on a freshly opened handle and has never been
interrupted mid line in practice. Worth revisiting if it ever grows a buffered writer.

## bug 10, in full

Found by reading the conversation record rather than by anything failing.

`needs_screen` treated every pointer word as deictic. That is wrong about English by a wide
margin: "that" is a conjunction far more often than it is a pointer. "Remember that", "make
sure that", "I think that", "it turns out that".

So an ordinary sentence took a screenshot. It flashed the display, which GNOME gives no way to
suppress, captured a black image because the screen happened to be off, and then spent a few
thousand vision tokens having Carl describe the black image back.

The comment above the function already said it errs toward not looking, and the implementation
did the opposite. A comment describing behaviour the code does not have is worse than none,
because it stops anybody checking.

A pointer now counts only when it genuinely points: at the end of the sentence, after a
preposition aiming at it, or in a question short enough that there is nothing else it could
mean. "is this right" looks. "remember that my mentor is called Hunter Zhang" does not.

## bug 7, in full

The one worth reading, because every check said the audio was fine.

A canceller was started by hand early on, and later the same canceller was started again as a
systemd service. Both create a sink called `carl-speaker` and a source called `carl-mic`.

Naming a device by name then picks whichever the audio server feels like, and there is no
reason for the two choices to belong to the same canceller. Carl played into one and recorded
from the other. The one listening had no idea what the one playing was doing, so it
subtracted nothing, and Carl heard himself at full volume.

What made it hard to see is that every diagnostic said the right thing. `Devices::detect`
found a sink called `carl-speaker` and a source called `carl-mic`, exactly as it was written
to, and reported echo cancelled audio. The service was active. The nodes existed. The audio
was not cancelled.

It looked like this in the journal, each reply becoming the next question:

```
08:14:52  Carl says:  Good, thanks.
08:14:54  Carl hears: "Good, thanks."
08:14:59  Carl says:  Glad to hear it.
08:15:01  Carl hears: "Glad to hear it."
08:15:08  Carl says:  Are we doing echoes now?
```

The fix is to count rather than to look. Exactly one of each is the cancelled case. Zero is
refused, which it already was, and now two or more is refused as well, with a message naming
the spare process. Falling back to muting the microphone is worse audio and obviously correct
behaviour, which beats better audio that silently is not.

This is the third time the same shape has appeared in this project, after the microphone
hearing the speakers and Carl reading his own Slack messages. It is the first time it got
through a guard that was written specifically for it.

## bug 12, in full

Bug 1 again, in a different file, four months later.

Bug 1 was an agent's output going to a pipe nobody drained: the pipe filled at about 64 KiB,
the writer blocked, and the supervisor waited forever. The lesson was written down. Then
`ask_streaming` piped the claude child's **stderr** and read it only at the very end, after
`reader.join()` and `child.wait()`.

The deadlock is the same shape with one extra step. The child fills its stderr pipe and blocks.
A process blocked writing stderr is not writing stdout either. So the reader thread never sees
another line and never sees EOF, `child.wait()` is never reached because the loop above it
never ends, and the read of stderr that would have unblocked everything sits three lines below
code that can no longer run. Every part waits for a different part.

Nothing exotic is needed to fill it. `claude --verbose` produces node warnings and MCP server
chatter, and 64 KiB of that is a normal session.

Reproduced with a stub `claude` that writes N bytes to stderr and then a valid stream. 1000
bytes answers normally. 65537 and 200000 both hang. The break is exactly at the pipe capacity,
which is what confirms the mechanism rather than a coincidence.

Fix. The stderr pipe is taken before the read loop and drained on its own thread, the same way
stdout already was, and joined after `child.wait()` where EOF is guaranteed.

Not `Stdio::null()`, which was the other option and is one line shorter. The stderr text is the
only evidence of what happened when the child dies with no final envelope, and that error path
already existed and already used it. Fixing a deadlock by deleting the diagnostics would have
paid for it with every future failure being unexplainable.

`drain` reads to the end and keeps only the first 8 KiB. Reading to the end is the part that
matters, because stopping early refills the pipe and reintroduces the bug. Keeping only the
start is so a child that writes megabytes of warnings cannot trade the deadlock for unbounded
memory instead, and the first lines are the useful ones anyway.

Guard. `a_chatty_stderr_does_not_deadlock_the_answer` runs a real stub process with 200000
bytes of stderr and a 20 second deadline. It runs the turn on a thread and waits on a channel,
so a hang fails the test rather than hanging the whole suite. That matters here specifically:
bug 2 in this list was a test that hung a run with no output, and a guard against a deadlock
must not be able to become one.

`a_quiet_stderr_still_answers` is the control. Against the broken code it passes, which is the
point of it: it proves the chatty test is failing because of the volume rather than because the
stub is wrong.

Verified by writing both before the fix. The chatty one failed at its 20 second deadline and
the quiet one passed. After the fix both pass, and the chatty one takes 0.02 seconds.
