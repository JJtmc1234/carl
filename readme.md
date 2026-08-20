# carl

Docs: [brainstorm.md](brainstorm.md) for how this was arrived at, [planning.md](planning.md) for the chunks and what is next, [infrastructure.md](infrastructure.md) for how it fits together, [progress-report.md](progress-report.md) for where it stands.

The army, which is most of the code: [docs/army-v1.md](docs/army-v1.md) for the shared types every part of it uses, [docs/chain-v1.md](docs/chain-v1.md) for the delegation chain, [docs/panel-v1.md](docs/panel-v1.md) for the Command Panel backend protocol, and [docs/a2a.md](docs/a2a.md) for the agent to agent protocol.

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

Carl writes his own notes. When something is worth keeping he puts a `[remember]` line in
the answer, the line is stripped before anybody sees it, and the note comes back in every
future conversation on every surface. It costs no extra model call, and he decides, since he
is the only one who knows whether something mattered.

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

## typing at him

```sh
./target/debug/carl chat
```

One conversation, held open, so the second question costs nothing to set up. Measured on three
short questions: 12.8 seconds as three separate `ask` invocations, 6.5 seconds in one chat.
Control d or `exit` to finish.

`ask` is still there and is still right for one question inside a script.

## talking to him

```sh
./target/debug/carl listen
```

Say **"hey carl"** to start, then just talk. Say **"end conversation"** to finish, and Carl
writes down whatever was worth keeping before going back to listening.

Anything you say that is not addressed to Carl is never transcribed past the wake check and
never written anywhere. Audio lives in RAM and in `/dev/shm`, never on the disk, so discarding
it is a real deletion rather than an unlink that leaves the bytes on the SSD.

He knows what you are playing without being told. If a game is running, or was in the last
six hours, he is given its version, its expansions, its overhaul mods and the name of the
most recent save. That matters more than anything in a screenshot: with Sea Block, Angel's,
Bob's and Space Exploration all stacked, almost no vanilla advice is correct, and none of
that is visible in a picture.

He also keeps track of where you are up to. That is a separate thing from memory, because
where you are in a game is true for an hour and who your mentor is stays true. States go in
one file per game and get replaced, facts go in `memory/` and stay.

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

## the chain of command

```sh
./target/debug/carl chain "add a health endpoint" --workdir ~/code/thing
```

One request goes down a chain of four agents and comes back up. JJ to Carl to Adrian to Mason
to Nora. Each one is a real `claude` process held open for the whole run, so the conversation
is the state: Mason reviews against the task he wrote rather than a description of it, and Nora
fixes her own first attempt rather than being briefed from cold.

```
JJ
 -> carl      objective + DONE WHEN
     -> adrian    objective + DONE WHEN
         -> mason     one task + DONE WHEN
             -> nora      works, verifies, submits
             <- mason     reruns the verification, ACCEPT or REJECT
         <- mason     reports up
     <- adrian    decides and reports up
 <- carl      tells JJ what happened
```

The interesting part is that the shape is enforced rather than requested. Carl cannot skip
Adrian because `Task::assign` refuses it, not because the driver remembers not to. Nothing is
marked done by whoever did it, because going up is `Submitted` followed by the creator
accepting or abandoning.

Three rules hold whatever an agent decides to type.

**Rank picks the tool list the process starts with.** A chief gets an empty list, a lead gets
read tools and `Bash`, a worker gets an editor too. A chief who asks to implement is refused,
and a chief who never asks still cannot write a file because it was never given the tool. Two
guards, because either alone is one forgotten call away from nothing.

**A task cannot exist without something checkable on it.** An agent handing work down is asked
for a `DONE WHEN` section, and asked once more if it forgets. Nothing invents a fallback
condition, because a generic "it works" satisfies the check while defeating the reason it is
there.

**A review that does not clearly say ACCEPT is not an acceptance.** The verdict reader defaults
to reject, because the other default lets a rambling answer with no decision in it pass work
nobody approved, which is the exact failure a review exists to prevent.

`--workdir` is where Nora works and the only place she can change files. `--journal` says where
to write what happened, and defaults to `events.jsonl` inside the workdir. That journal is the
point: twenty agents producing work with no record is twenty opinions and a story about where
they came from.

Mason having `Bash` and no editor is deliberate and imperfect, and worth knowing before you
rely on it. A reviewer has to rerun a verification rather than believe a summary, and a shell
can write a file. It is the smallest grant that still lets a reviewer check rather than trust.

## the command panel

```sh
./target/debug/carl panel
```

The backend for watching the army while it runs. It binds a unix socket at
`~/.carl/panel/panel.sock` and serves line delimited JSON in both directions, so `nc -U` is a
working client on the day the thing that is broken is the backend.

**The socket file is the authentication.** It sits in a `0700` directory and is itself `0600`,
so only your own processes can reach it, and that is enforced by the kernel rather than by a
check in the code. Nothing listens on the network. A port on loopback would be reachable by
every process and every user on the machine.

Four calls: `ping`, `snapshot` for the whole world at once, `subscribe` to be sent events as
they happen, and `command` to intervene. Every frame carries a version, and a backend speaking
a different one refuses the frame rather than guessing at it.

Subscribing takes over the connection, because a subscribed panel is streaming and interleaving
replies on the same socket would need framing this protocol does not have. A panel that wants
both opens two connections.

Liveness is a tail of the journal rather than a rebuild on a timer. Rebuilding would be simpler
and wrong twice over: it would miss anything that happened and was undone between two ticks,
and it would give the panel no way to tell a change from a redraw.

The protocol is written down in full in [docs/panel-v1.md](docs/panel-v1.md), and every example
in it was copied from a real run rather than written by hand. It is frozen: see
[docs/command-panel-v1.md](docs/command-panel-v1.md) for what that means and what v1 does not
do.

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

He answers straight away with what he is doing, then rewrites that message as the answer
arrives. Slack has no way to stream into a message, so the message is edited in place, paced
at one rewrite every 1.5 seconds because Slack rate limits edits and going over freezes the
answer rather than failing politely. Twenty five seconds of nothing looks like being ignored,
and that was the whole problem.

Three ways to reach him. Mention `@Carl`, send him a direct message, or just use his name in
a channel he is in. He replies in the thread, so a conversation stays together and he
remembers the rest of it.

Using his name is deliberately strict about *where* the name is, because a name is ambiguous
in a way an at sign is not.

| | |
|---|---|
| `carl what should I research next` | answered |
| `what do you think, Carl?` | answered |
| `hey carl` | answered |
| `I asked Carl yesterday` | left alone |
| `Carl's memory design is the good bit` | left alone |
| `ask carl when he gets back` | left alone |

The rule is that his name has to be at the very start or the very end, which is where a name
goes when you are speaking *to* somebody and almost never where it goes when you are speaking
*about* them. Missing one aimed at him costs you an at sign. Answering one that was not is
Carl butting into other people's conversation, which is how a bot gets thrown out of a
channel, so it errs towards staying quiet.

**You only have to say his name once.** Once Carl has answered in a thread, that thread is
his, and follow up messages in it need no name and no mention. Saying his name is how a
conversation starts, not a toll on every sentence of it. The voice already worked this way,
where "hey carl" wakes him and then you just talk.

Being in one thread does not open the whole channel. A new top level message still has to
address him.

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

### names, and running python

Carl looks up who is speaking and calls them by name. Slack only ever sends a user id, so
without the lookup he would be greeting people as `U0BNSU5N96X`. Each id is looked up once
and remembered, because a name does not change between two sentences and every lookup is a
round trip in the way of a reply.

JJ's account says "JJ_tmc Multiversal" and he is called JJ. That is a one line table in
[src/slack/who.rs](src/slack/who.rs), so adding somebody else takes one line. Everyone else
is called whatever their account says.

Carl can also run python, and is told to use it rather than estimate anything with more than
two digits in it. Guessing at a sum and being confidently wrong is worse than taking a second
to work it out.

That python runs inside a sandbox, which it did not at first. `etc/carl-python` is the same
interpreter in a namespace where the home directory does not exist, the network is gone, and
one directory is writable.

Checked rather than assumed:

| tried | result |
|---|---|
| `2**64 - 1` | works |
| write in the workspace | works |
| read `~/.carl/slack.json` | `FileNotFoundError` |
| list the home directory | `FileNotFoundError` |
| open a socket | `OSError` |
| see other processes | 2, not the machine's |

The first version bound the whole filesystem read only, which stops python changing anything
and does nothing about reading. It could still print the Slack tokens. Read only is not the
same as not there.

To take it away entirely, `Runner::default().allowing(vec![])`.

### what he will not do

He never answers himself. He posts into the same channels he listens to, so his own messages
arrive back as events, and three separate checks catch that. Without them it is an infinite
loop that costs real money in a channel with real people in it.

He does receive every message in channels he is in, because that is the only way to notice
his own name without an at sign. Nothing is recorded or sent anywhere unless it was addressed
to him. If that is more than you want, take `channels:history` and `groups:history` out of the
manifest and reinstall. Mentions and direct messages carry on working without them.
