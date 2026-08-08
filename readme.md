# carl

A helper that remembers. Rust, driving the `claude` command line as a child process.

## how memory actually works

"Remember everything forever" is two different things, and keeping them apart is the whole
design. Conflating them is where this normally goes wrong.

| layer | what it holds | how long it lasts | where |
|---|---|---|---|
| the record | every message ever said, either way | forever | `conversations.jsonl` |
| the thread | one live conversation | until Claude Code compacts it | a Claude Code session |
| memory | notes Carl chose to keep | forever, and small | `memory/*.md` |

**The record is forever.** Append only, one JSON object per line, nothing deleted or edited.
It is cheap because it is only a file.

**The context cannot be forever.** What gets sent to the model each turn is bounded by the
context window, and cost scales with it. A conversation running for months cannot resend
every message. Claude Code compacts a long thread automatically, and that is fine, because
the record still has the full text.

**Memory is the part people forget.** Resuming a thread gives back and forth *inside* that
thread. It does nothing for something said in a different Slack thread last week. Only the
notes in `memory/` cross that gap, and they ride along on every single message, which is why
they have a size budget and why a dropped note is named rather than silently skipped.

## the ordering that matters

Record the question, then ask. Never the other way round.

A crash after recording loses the answer, and an unanswered question can simply be asked
again. A crash before recording loses the question, and nothing can recover it. Only one of
those two is survivable, so the code is arranged to only ever make that one.

The same rule runs the AOS event log. It is the second time it has come up, which is a good
sign it is a real rule and not a preference.

## try it

```sh
cargo build
cargo test

./target/debug/carl memory write jj "JJ is 11 and writes Rust. No dashes, no semicolons."
./target/debug/carl ask "what do you remember about me"
./target/debug/carl ask --thread kitchen "different conversation, you know nothing here"
./target/debug/carl history --all
./target/debug/carl threads
```

Everything lives under `~/.carl` by default. Point `--home` somewhere else to keep a
separate brain.

## talking to him

```sh
./target/debug/carl listen
```

Say **"hey carl"** to start, then just talk. Say **"end conversation"** to finish, and Carl
writes down whatever was worth keeping before going back to listening.

Anything you say that is not addressed to Carl is never transcribed past the wake check and
never written anywhere. Audio lives in RAM and in `/dev/shm`, never on the disk, so discarding
it is a real deletion rather than an unlink that leaves the bytes on the SSD.

He takes a picture of the screen only when the question needs one. "What should I research
next" does not look. "Should I put it here" does, because *here* has no meaning in the
sentence and can only mean the screen. This matters because GNOME flashes the display on every
screenshot and gives no way to turn it off.

You can talk over him. Three tenths of a second of you speaking stops him mid word, and what
you said to interrupt is kept as the start of your next sentence. That needs the echo
canceller running, see below. Without it he mutes the microphone while he speaks and has to
finish.

### what it costs, end to end

Carl is told on every spoken turn that his answer is going to be read out loud. That one
instruction does more for the wait than everything else put together, because a spoken answer
cannot be skimmed. You hear it one word at a time and you cannot skip the part you know.

Same question, with and without it, measured twice each and interleaved so load could not
flatter either one.

| | words | claude | spoken aloud | total |
|---|---|---|---|---|
| before | about 200 | 22s | 78s | 100s |
| after | about 29 | 4.4s | 12s | 16s |

Claude got faster too, not just shorter, because it stops thinking hard about an answer it
knows has to fit in two sentences. First token went from 15s to 2.8s.

The rest of the chain:

| step | time |
|---|---|
| you speak | as long as you take |
| quiet before he decides you are done | 0.4s, `--hush` to change it |
| whisper `base.en` | 0.9s, or 2.8s with `--accurate` |
| screenshot, only when needed | ~0.3s, plus a white flash |
| **claude, to its first word** | **~2.8s** |
| piper and playback | starts immediately, then real time |

He speaks each sentence as it is written rather than waiting for the last word, so the number
that matters is time to first word, not to the whole answer.

### the echo canceller

```sh
pipewire -c etc/carl-aec.conf &
```

Run it before `carl listen`. It creates a speaker Carl plays into and a microphone with his
own voice already subtracted, which is what lets him listen while he talks. Measured with
Carl speaking, his voice reaches the plain microphone at rms 0.048 and the cancelled one at
0.0017. It also suppresses room noise on the way through, which took this room from 0.24
to 0.001.

It is a separate process, so it can be stopped at any time and nothing else on the machine
notices. Carl checks for it at startup and says which mode he is in.

## the exact privacy promise

Worth being precise, because a reasonable person would assume otherwise.

**Audio is never kept.** Not addressed to Carl, not stored, not even transcribed past the
two-word wake check.

**Conversations you start are kept forever**, in `conversations.jsonl`, by design.

So the mic being always on does not mean the room is being recorded. It means two words are
being watched for, in memory, and everything else falls out of a three second window.

## not built yet

Slack. Everything it needs already exists, so it is a transport and a thread id.
