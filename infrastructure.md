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
| the microphone | done | 3 |
| whisper, two tier | done | 6 |
| Carl's voice | done | 7 |
| the listen loop | written, every part it calls is proven, the loop itself is not | |
| Slack | not built | |

65 tests, clippy clean at deny warnings.

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
