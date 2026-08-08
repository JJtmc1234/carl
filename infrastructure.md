# infrastructure

Holistic view of Carl. Components, how they talk, and what moves between them.

## what Carl is

A helper that remembers, written in Rust. It does not talk to the Claude API directly. It
runs the `claude` command line as a child process and reads its JSON.

That is a deliberate choice rather than a shortcut. There is no Rust binding for the Claude
Agent SDK, and the SDK is Claude Code packaged as a library, so driving the binary gets the
same harness, the same built in tools and the same session handling with no second language
runtime. It also makes Carl a process, which is exactly the thing the AOS runtime is being
built to supervise.

## the three layers of memory

Remembering forever is two different things, and conflating them is where this normally goes
wrong. Carl keeps them apart.

| layer | holds | lasts | where |
|---|---|---|---|
| the record | every message ever said, both directions | forever | `conversations.jsonl` |
| the thread | one live conversation | until Claude Code compacts it | a Claude Code session |
| memory | notes Carl chose to keep | forever, and small | `memory/*.md` |

The record is append only. Nothing is edited and nothing is deleted. It is cheap because it
is only a text file, one JSON object per line, readable with `cat` when whatever wrote it is
the thing that is broken.

The context sent to the model cannot be forever. A window is finite and cost scales with it,
so a long thread gets compacted. That is fine, because the record still holds the full text.

Memory is the layer people forget. Resuming a thread gives back and forth inside that thread.
It does nothing for something said in a different conversation last week. Only the notes
cross that gap, and because they ride along on every single message they have a size budget,
and a note dropped for space is named rather than silently skipped.

## the ordering that matters

Record the question, then ask. Never the other way round.

A crash after recording loses the answer, and an unanswered question can be asked again. A
crash before recording loses the question, and nothing can recover it. Only one of those is
survivable, so the code only ever makes that one.

The AOS event log follows the same rule. Arriving at it twice, from two directions, is a good
sign it is a real rule and not a preference.

## component map

```
  microphone
      |
      |  16 kHz wav, written to /dev/shm so discarded audio never touches the disk
      v
  audio.rs                            one long lived arecord, sliding window
    calibrate    ->  measure the empty room, so "quiet" is this room and not a guess
    level gate   ->  is anyone speaking at all, before whisper runs
    whisper tiny ->  what did they say, 0.7s per 11s of audio
      |
      v
  heard.rs                            the rules that decide what is kept
    idle    + no wake word  ->  destroy the recording, keep nothing
    idle    + "hey carl"    ->  wake, and keep the question on the same breath
    awake   + anything      ->  send it to Carl
    awake   + "end conv"    ->  summarise into memory, back to idle
      |
      v
  turn.rs                             done
    1. record what was said                  <- before anything can fail
    2. find or mint the session for a thread
    3. ask claude, resuming that session
    4. record the answer, or record the failure
      |
      +---> capture.rs   gnome-screenshot, only when the question needs a look
      +---> memory.rs    notes injected into every conversation
      +---> log.rs       the permanent record
      |
      v
  claude CLI  --print --output-format json --resume <session>
      |
      v
  speech.rs                           piper into aplay, 0.21s for 5s of speech
    and the mic is deaf for all of it, or Carl hears himself and replies to that
```

`ear.rs` drives that loop and is the only place the two states live.

## the privacy promise, and how it is kept

The mic is always on, so the rule has to be enforced rather than intended.

Audio is written to `/dev/shm`, which is RAM. Deleting a file from a disk unlinks it and
leaves the bytes on the SSD until something overwrites them. In RAM there is nothing left to
recover, so "insta-deleted" is literally true rather than an accounting entry.

`heard.rs` decides what is kept, and it is pure text in and a decision out. No audio and no
model, so every rule that governs deletion is testable without a microphone. That is why it
is a separate module.

| said while idle | what happens |
|---|---|
| anything without the wake word | destroyed, never transcribed further, never recorded |
| "hey carl ..." | kept from that point, and the question on the same breath is kept too |

## data that moves between components

| from | to | transport | payload |
|---|---|---|---|
| microphone | ear | `arecord` into `/dev/shm` | 16 kHz mono wav |
| ear | heard | in process | a transcript, as text |
| turn | claude | argv plus stdin | the prompt on stdin, session flags on argv |
| claude | turn | stdout | one JSON envelope |
| turn | disk | `conversations.jsonl` | one entry per message, appended |
| capture | claude | a file path in the working directory | a PNG, read by Claude's own file tools |

The prompt goes on stdin rather than the argument vector, because a spoken sentence or a
Slack message can be longer than the argument limit and can contain anything at all.

## what is built

| part | state | tests |
|---|---|---|
| the record | done | 4 |
| threads and sessions | done | 6 |
| memory | done | 6 |
| the claude driver | done | 6 |
| thread ids | done | 4 |
| screen capture | done, captured this machine at 3840x2400 | 6 |
| wake word logic | done | 12 |
| the microphone | done | 6 |
| whisper, two tier | done | 6 |
| Carl's voice | done | 7 |
| the listen loop | wake path confirmed on this machine, the awake path is being fixed | |
| Slack | not built | |

69 tests, clippy clean at deny warnings.

## measured, not estimated

Everything below was timed on this machine rather than guessed.

| thing | number |
|---|---|
| screen capture | 3840x2400 |
| whisper `tiny.en`, 11s of speech | 0.7s |
| whisper `small.en`, 11s of speech | 3.4s |
| whisper `large-v3-turbo`, 11s of speech | 13.9s, unusable in a loop |
| piper, 5.2s of speech | 0.21s, real time factor 0.04 |
| a whole spoken exchange | 6 to 27s, almost all of it Claude |

## measuring loudness, and why peak was wrong

Carl decides you have stopped talking by watching how loud the last moment was. The first
version used peak, the largest single sample in the window. That is the wrong measure.

One key press sends peak to 0.24 in a room that is otherwise silent. The threshold was 0.02,
so the tail never counted as quiet, so the recording never stopped early and ran to its cap
of thirty seconds on every single turn. Carl looked deaf. He was listening the whole time.

Two changes. Loudness is now RMS, which averages over the window and ignores a lone click.
And the threshold is measured from the room at startup rather than written into the source,
because a laptop fan and a quiet study are not the same number and no constant fits both.

## why Carl can hear you while he talks

A speaker and a microphone in one room are a feedback loop. Carl says an answer, the mic picks
it up, whisper transcribes it, and he answers his own answer.

The fix that needs no help from anyone is to mute the microphone whenever he speaks, and that
is still what happens when nothing better is available. It costs you the ability to interrupt
him, which is most of what separates a conversation from a machine reading at you.

The better fix is to subtract his voice instead of muting. `etc/carl-aec.conf` starts a small
second PipeWire process that creates two virtual devices. Carl plays into a sink the canceller
owns, so the canceller knows exactly what sound is leaving the machine and when. It removes
that from the microphone signal and offers the result as a separate source, which is what Carl
listens to.

Measured on this machine, with Carl talking:

| microphone | rms | peak |
|---|---|---|
| plain | 0.0476 | 0.509 |
| echo cancelled | 0.0017 | 0.019 |

Twenty eight times quieter, which puts his own voice below the noise floor of a quiet room.
The room itself measured 0.001 afterwards, against 0.24 before, because the canceller runs
noise suppression on the way through.

So Carl listens while he talks, and three tenths of a second of you speaking over him stops
him mid word. Not one tenth: a door or a chair leg clears the threshold for a single check,
and being cut off by a chair is worse than not being interruptible.

The canceller is a separate process and can be stopped at any time. Carl checks for both
halves of the pair at startup and falls back to muting if either is missing. Half a canceller
is worse than none, because he would believe he could not hear himself while hearing himself
at full volume.

## the older half duplex fallback

A speaker and a microphone in one room are a feedback loop. Carl says an answer, the mic
picks it up, whisper transcribes it, and Carl answers his own answer. Nothing downstream can
tell his voice from yours, because by the time it is a transcript both are just text.

So the mic goes deaf for the length of every spoken line, plus 350 milliseconds for the room
to stop ringing and the sound card to finish its own buffer. The reader thread keeps draining
the pipe throughout and throws the bytes away, because a pipe nobody drains backs up and the
next thing heard would be however far behind real time the silence lasted. That was already
one bug and it is not worth having twice.

The cost is that Carl is half duplex. You cannot interrupt him, and anything said while he is
speaking is gone. That is a real limitation, not a detail.

Lifting it takes echo cancellation, which subtracts what is playing from what the mic hears
so the two can run at once. PipeWire ships a WebRTC canceller and this machine already has
the module installed. It belongs in the audio server rather than in Carl, because the audio
server is the only thing that knows what the speakers are playing right now.

The voice is reachable only through a type that demands the microphone as an argument. It
does not use it for anything except switching it off. A spoken line that forgets to mute
would fail silently and sound like Carl talking to himself, so the wrong version is made
impossible to write rather than left to memory.
