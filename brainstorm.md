# brainstorm

How Carl was arrived at, and what lost.

## the want

An assistant that is actually around. Not a tab you open, decide to consult, and type into.
Something you can talk to while doing something else, that knows who you are without being
told again, and that is reachable from wherever you already are.

Three things follow from that and they are all awkward.

**It has to be there without being asked.** A helper you have to go and start is a tool. That
means a microphone that is always on, which means a promise about what is kept.

**It has to remember.** Not within one conversation, which is free, but across all of them
and across surfaces. Something said out loud on Tuesday should be known in Slack on Friday.

**It has to be reachable from where you already are.** Which is a terminal, a game, and a
Slack workspace, and those are three different shapes.

## how to talk to a model, three options

| | |
|---|---|
| the HTTP API directly | total control, and everything is yours to build: tools, sessions, compaction, retries |
| the Claude Agent SDK | the Claude Code harness as a library, in Python or TypeScript |
| driving the `claude` binary | the same harness, as a child process |

The API lost because the interesting problems here are not model plumbing. Tool use, session
resumption and context compaction are solved, and rebuilding them is a month spent not
building the thing.

The SDK lost narrowly and for one reason. There is no Rust binding, so taking it meant a
Python or TypeScript runtime sitting next to a Rust program for the rest of the project.

Driving the binary won. It is the same harness the SDK wraps, with no second language, and it
makes Carl a supervisor of a child process, which is exactly the shape AOS is being built to
handle. The two projects inform each other rather than competing.

The cost, which was underestimated, is that the binary has its own identity and its own
features and they leak through as Carl. That has now happened three times: he refused a
question about a video game because Claude Code is for software engineering, he said he could
not send a Slack message in a message posted to Slack, and he claimed to save a memory using
a system that is not his. Each one was fixable and none was predictable from the outside.

## why not just Python

Everything here would be shorter in Python. It is a worse fit for two reasons.

The microphone needs a reader thread draining a pipe while a model call blocks for twenty
seconds. That is a place where being wrong is a bug you can hear, and it was in fact the
first serious bug in the project.

And AOS is Rust. Carl being Rust means the pieces move between them, and `ThreadId`,
`Root` and the risk tiers are the same ideas in both.

## how to listen

**Push to talk** was the safe answer and lost because it needs a hand, and the whole point is
being usable while doing something else.

**Always on and always transcribing** is the honest wrong answer. Everything said in the room
would go to a model, and no promise about that is worth anything.

**A wake word** won. The mic is always open, the last three seconds live in RAM, and nothing
is transcribed past a two word check unless those two words appear. Audio goes to `/dev/shm`,
which is memory, so discarding it is a real deletion and not an unlink that leaves the bytes
on the disk.

## how to remember

The mistake to avoid is treating "remember everything" as one thing. It is three.

| layer | holds | lasts |
|---|---|---|
| the record | every message either way | forever |
| the thread | one live conversation | until it is compacted |
| memory | notes worth carrying | forever, and small |

Conflating the record with what gets sent is the usual failure. A conversation running for
months cannot resend every message, and a context window is finite. Keeping them apart means
the record can be complete and cheap while the context stays bounded.

Memory is the layer people forget entirely. Resuming a thread gives back and forth inside
that thread and does nothing for something said in a different one last week. Only notes
cross that gap.

How the notes get written was harder than expected. Asking the model after every turn costs
a second call on every message to answer no. Summarising at the end of a conversation needs
an end, which Slack does not have and which the voice path never actually reached, so the
memory directory stayed empty for the whole first day. Letting Carl write a note inside the
answer he was already giving costs nothing and works everywhere at once.

## the ordering, arrived at twice

Record the question, then ask. Never the other way round.

A crash after recording loses the answer, and an unanswered question can be asked again. A
crash before recording loses the question, and nothing recovers it. Only one of those is
survivable.

The AOS event log reached the same rule from a completely different direction. Arriving at
something twice, independently, is the strongest evidence available that it is a real rule
and not a preference.

## what was deliberately left out

**A sandbox.** Carl can run python, which is shell access wearing a hat. That is a real
decision and it is written down as one rather than described as safe. AOS took the opposite
position for its capability server, and the two projects now disagree on purpose.

**A GUI.** A voice, a terminal and Slack cover being reachable. A window is a fourth thing to
maintain and a fourth place for state to disagree.

**Wake word training.** A model that recognises one specific voice would be better and is
weeks of work. Whisper on two words is good enough, and being good enough on day one beats
being right in a month.
