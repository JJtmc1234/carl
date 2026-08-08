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

### what it costs, end to end

| step | time |
|---|---|
| you speak | as long as you take |
| whisper `small.en` | ~0.9s |
| screenshot, only when needed | ~0.3s, plus a white flash |
| **claude** | **5 to 25s** |
| piper and playback | ~0.3s |

Claude dominates and nothing else is close. This is good for "what should I research next"
and not a conversation you can interrupt.

## the exact privacy promise

Worth being precise, because a reasonable person would assume otherwise.

**Audio is never kept.** Not addressed to Carl, not stored, not even transcribed past the
two-word wake check.

**Conversations you start are kept forever**, in `conversations.jsonl`, by design.

So the mic being always on does not mean the room is being recorded. It means two words are
being watched for, in memory, and everything else falls out of a three second window.

## not built yet

Slack. Everything it needs already exists, so it is a transport and a thread id.
