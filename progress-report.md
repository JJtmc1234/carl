# progress report

Where Carl stands. Updated 2026 08 08.

## summary

Carl is a general purpose assistant written in Rust that drives the `claude` command line as
a child process. He is reachable three ways: a terminal, a microphone, and Slack. He runs as
three systemd user services on JJ's machine.

Everything below was built in one day, in 22 commits, from an empty directory.

| part | status | tests |
|---|---|---|
| the permanent record | done | 4 |
| threads and sessions | done | 6 |
| memory notes | done | 6 |
| the claude driver | done | 8 |
| screen capture | done | 6 |
| wake word logic | done | 12 |
| microphone | done | 6 |
| whisper, two tier | done | 6 |
| voice | done | 10 |
| streaming answers | done | 6 |
| sentences from a stream | done | 10 |
| echo cancellation | done, measured | |
| slack, socket mode | done, live | 15 |
| answering to his own name | done | 8 |
| A2A protocol | built and specified, unproven | 15 |
| running 24/7 | services installed and running | |

152 tests, clippy clean at deny warnings.

## what he can do

Ask him something in a terminal and the answer streams back. Say "hey carl" and he answers out
loud, looks at the screen if the question needs it, and can be interrupted mid sentence.
Mention him in Slack, message him directly, or just use his name, and he replies in the
thread. He remembers every conversation forever, and carries notes across them.

## the three numbers that matter

Everything here was measured on JJ's machine rather than estimated.

| | before | after |
|---|---|---|
| a spoken exchange, end to end | 100s | 16s |
| Carl's own voice in his microphone | rms 0.048 | rms 0.0017 |
| transcribing what you said | 2.84s | 0.89s |

The first is the one to look at, and the reason for it is not code. Carl is told on every
spoken turn that his answer will be read out loud, so it has to be one or two sentences. That
alone took roughly 200 words down to 29, and the model got faster as well as shorter: first
token went from 15s to 2.8s, because it stops thinking hard about an answer that has to fit
in two sentences.

## the same bug, three times

The most useful thing this project taught is that one mistake kept coming back wearing
different clothes.

**The microphone heard the speakers.** Carl spoke, the mic picked it up, whisper transcribed
it, and he answered his own answer. Nothing downstream could tell his voice from anyone
else's, because by then both were just text.

**Carl saw his own Slack messages.** He posts into the channel he listens to, so his own
replies arrived back as events. Same shape, worse consequences: the room only had Carl in it,
a channel has other people and a bill.

**Two agents feed each other.** Carl replying to Alex replying to Carl. Neither gets bored,
neither runs out of things to say, and every turn costs money in front of an audience.

The first two were fixed the same way, by refusing to listen to yourself. The third could not
be, because Alex genuinely is somebody else and hearing him is the entire point. So the rule
had to change from *who is talking* to *how long since a person was involved*. That is what
`patience.rs` counts, and it is the only guard that survives the other agent being broken.

## the mistakes worth writing down

**A threshold measured on the wrong thing.** Carl went deaf after waking for a whole evening.
The silence test used peak loudness against a fixed floor, and one key press puts peak at 0.24
in a silent room, so the recording never ended early and ran its full 30 second cap every
time. RMS fixed it, and the threshold is now measured from the room at startup rather than
written into the source.

**An error that described the wrong problem.** Slack said `user_not_found` for a user id that
`users.list` returned a second later from the same token. Both were true. Slack's read methods
take form encoded parameters and silently drop a JSON body, so the lookup was asked to find
nobody and reported finding nobody. It reads as a missing person and it is a missing
parameter, and it pointed at scopes, reinstalls and the workspace before it pointed at the
request.

**A claim made without measuring it.** Carl was told a faster model would help. Haiku refused
the question outright and was written down as unusable. That conclusion was wrong: given
Carl's real identity brief it answered every time, and the earlier refusal came from a thin
instruction that failed to displace Claude Code's own description of itself as a coding tool.
Measured again on equal terms, Opus was faster anyway. The correction is in the git history
rather than quietly edited out.

**Rate limiting that did nothing.** The systemd units had `StartLimitIntervalSec` in
`[Service]`, where systemd ignores it without complaining. `systemd-analyze verify` caught it.

## what is not done

**A2A is unproven.** The protocol is specified in [docs/a2a.md](docs/a2a.md) and implemented,
and Carl has sent Alex a hello in the shared workspace. Alex does not speak it yet, so
nothing has come back. Hunter has the spec and can implement the other half without reading
any of Carl's code.

**24/7 is partial.** All three services run and restart on failure. The voice and the echo
canceller stop at logout, which is correct rather than a limitation, since they are PipeWire
clients and there is no microphone to listen to when nobody is logged in. Slack needs no
session but user services stop at logout by default, so surviving that needs
`sudo loginctl enable-linger`, which is JJ's to run.

**Python is shell access.** Carl can run `python3`, which can call `os.system`, write files
and open sockets. Anyone who can message him in Slack can ask him to run something.
`~/.carl/workspace` is a working directory and not a sandbox, and it is not described as one
anywhere.

## next

1. Hunter implements Alex's half of A2A and the two agents actually talk.
2. Lingering, so Slack survives a logout.
3. Whatever the first week of real use turns up. Every bug of consequence so far came from
   running the thing, not from reading it.
