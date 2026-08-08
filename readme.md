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

## not built yet

Slack, screen capture, and voice. See `planning.md` for why the order is what it is.
