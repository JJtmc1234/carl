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

## slack

```sh
./target/debug/carl slack
```

Mention `@Carl` in a channel he has been invited to, or send him a direct message. He replies
in the thread, so a conversation stays together and he remembers the rest of it.

Socket Mode, which means no public URL and no hosting. Carl dials out to Slack over a
websocket rather than Slack calling in, so a laptop behind a router works as well as a server.

### setting it up

You have to do this part. It needs a browser and a workspace you can install apps into.

1. Go to https://api.slack.com/apps, choose **Create New App**, then **From an app manifest**.
   Pick your workspace and paste in `etc/slack-manifest.yaml`. That sets the name, the scopes
   and Socket Mode in one go.
2. **Install to Workspace**, then copy the **Bot User OAuth Token**. It starts `xoxb-`.
3. **Basic Information**, scroll to **App-Level Tokens**, **Generate Token and Scopes**. Give
   it the `connections:write` scope. Copy it. It starts `xapp-`.
4. Write both into `~/.carl/slack.json` and make it private:

```sh
cat > ~/.carl/slack.json <<'EOF'
{
  "bot": "xoxb-your-bot-token",
  "app": "xapp-your-app-token"
}
EOF
chmod 600 ~/.carl/slack.json
```

Carl refuses to start if that file is readable by other users, because a bot token lets
anyone post as him. `CARL_SLACK_BOT_TOKEN` and `CARL_SLACK_APP_TOKEN` work instead of the
file, which is what a service unit wants.

5. Invite him where you want him: `/invite @Carl`.

The two tokens look alike and Slack answers a swap with `invalid_auth`, which tells you
nothing. Carl checks the prefixes and says "the two are the wrong way round" instead.

### speaking first

```sh
./target/debug/carl say C0BNA6YQ16E "the smelters are backed up again"
./target/debug/carl greet C0BNA6YQ16E U0ALEX
./target/debug/carl greet C0BNA6YQ16E U0ALEX "how do you handle memory between conversations"
```

`say` posts an ordinary message. `greet` opens an exchange with another agent using the A2A
protocol, and with no message it sends a hello, which is how you find out whether the other
agent speaks it.

### talking to other agents

Carl can talk to Hunter's agent Alex. The protocol is written up in
[docs/a2a.md](docs/a2a.md), which is what Alex's side needs in order to be implemented.

The short version. Slack already says who sent a message, who it is for and which
conversation it belongs to, so the protocol only adds the two things Slack cannot: what kind
of message this is, and how many more hops the exchange may take.

```
<@U0ALEX> [a2a/1 ask ttl=5]
How do you handle memory between conversations?
```

Two agents left alone do not stop. Neither gets bored, neither runs out of things to say, and
every turn is a paid model call in a room other people are in. There are three guards and no
single one of them is enough.

| guard | stops | survives the other agent being broken |
|---|---|---|
| never answer yourself | Carl looping alone | yes |
| `ttl` in the protocol | a polite exchange running long | no, it is cooperative |
| six agent turns per thread, counted locally | everything else | yes |

A person speaking in the thread resets the count, because a person in the conversation is
what makes it a conversation rather than a loop.

### what he will not do

He never answers himself. He posts into the same channels he listens to, so his own messages
arrive back as events, and three separate checks catch that. Without them it is an infinite
loop that costs real money in a channel with real people in it.

He is not in ordinary channel conversation. Only a direct message or a message that mentions
him is a question. He does not hold the `channels:history` scope at all, so this is something
he cannot do rather than something he chooses not to.
