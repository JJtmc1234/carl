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
| 27 | Service uptime subtracted systemd's `CLOCK_MONOTONIC` start stamp from `/proc/uptime`, which is `CLOCK_BOOTTIME`. Those are different clocks and boottime counts suspend, so every unit's uptime was inflated by the total time the laptop had ever been asleep. | Never fired, found by reading, then measured: `carl-aec` reported 959085 seconds up against a monotonic clock of 318912, so 11 days for a service at most 3.7 days old. | `a_real_units_uptime_never_exceeds_the_clock_it_is_measured_against`, `the_monotonic_clock_never_runs_ahead_of_boottime` |
| 28 | The property list never asked for `LoadState`, and `systemctl show` answers for a unit that does not exist with `ActiveState=inactive` and exits 0. So a unit systemd had never heard of was reported with the same health and the same word as one somebody deliberately stopped. | Never fired here, since all three units are installed. Verified by running the exact command against a made up unit name. | `a_unit_systemd_has_never_heard_of_is_unknown_rather_than_stopped`, `a_loaded_unit_that_is_stopped_still_reads_as_stopped`, `a_masked_unit_is_a_decision_rather_than_an_absence` |

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

## bug 27, in full

Two clocks that look like one number.

`Unit::uptime_secs` subtracted systemd's `ExecMainStartTimestampMonotonic`, which is on
`CLOCK_MONOTONIC`, from the first field of `/proc/uptime`, which is `CLOCK_BOOTTIME`. Boottime
counts time the machine spent suspended and monotonic does not, so the difference between them
is every second this laptop has ever been asleep, and that was being added to every service's
uptime.

Measured here while fixing it: `carl-aec` reported 959085 seconds up, eleven days, against a
monotonic clock that has only been running for 318912 seconds. A service cannot have been up
longer than the clock it is measured against has existed.

The `(up >= 0.0)` guard was dead as well. It is meant to reject a start stamp in the future, and
boottime is always ahead of monotonic, so it could never fire. That guard now works.

Fix. `monotonic_secs` reads `CLOCK_MONOTONIC` directly through `clock_gettime`, and
`diagnostics` uses it. `/proc/uptime` is not consulted on this path at all, because it answers a
different question.

The existing test could not have caught this, and that is worth recording rather than just
replacing it. `uptime_is_machine_uptime_less_the_start_stamp` supplied both numbers itself, so
it picked the same scale for both by construction. The arithmetic it checks was always right.
What was wrong was which clock the caller read, and no test that invents both values can see
that.

So the guard compares the two real clocks. `the_monotonic_clock_never_runs_ahead_of_boottime`
pins the relationship. `a_real_units_uptime_never_exceeds_the_clock_it_is_measured_against`
takes the actual units on this machine and asserts each reported uptime is within the monotonic
clock, which is an invariant rather than a number, so it does not go stale.

Verified by putting `/proc/uptime` back as the source. It failed with
"army.service.carl-aec reports 959085 seconds up against a monotonic clock of 318911", which is
the bug stated in one line.

## bug 28, in full

A missing unit and a stopped unit are different problems with different remedies, and the panel
had one word for both.

`systemctl --user show` exits 0 for a unit that does not exist and prints
`ActiveState=inactive`, `SubState=dead`. `read_with` only rejects a non zero status, so that
came back as a perfectly ordinary answer, `health()` returned `Blocked`, and `summary()` said
"stopped".

`Blocked` means somebody stopped a service on purpose, which the comment on that arm says in as
many words. So the panel told you a service was deliberately stopped, and the remedy that
implies, start it, cannot work for something that was never installed.

The cause is narrow. The `-p` list never asked for `LoadState`, which is the one field that
distinguishes the two, so the provider had no way to tell them apart even in principle.

Fix. `LoadState` joins the list, and `not-found`, `bad-setting` or `error` make the unit
`Unknown` with a summary naming it as not installed and pointing at `etc/systemd/install.sh`.

`masked` is deliberately not in that set. A masked unit does exist and somebody masked it, which
is a decision rather than an absence, so it keeps reading as stopped, which is what it is. There
is a test saying so, because the obvious list of "bad" load states includes it.

Worth recording that this never fired on this machine and could not have. All three units are
installed here, so the bug only appears on a machine where `etc/systemd/install.sh` has not run,
which is exactly a fresh checkout: the case where somebody most needs the panel to say what is
wrong. The issue was careful about this too, narrowing its own claim rather than asserting the
module doc was broken.

Guard. Three unit tests, one for each of the three load states that matter, plus one against
this machine asserting the installed units do not start reading as missing. Verified by removing
the `missing()` check, where the first fails and the neighbouring cases all still pass.
