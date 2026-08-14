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
| streaming answers | done | 6 |
| sentences from a stream | done | 10 |
| the listen loop | wake path confirmed on this machine, the rest needs a real conversation | |
| slack, socket mode | done | 15 |
| a2a, agent to agent | done, untested against Alex | 15 |
| answering to his own name | done | 8 |
| staying in a thread once addressed | done | 4 |
| names, and calling JJ JJ | done | 3 |
| python | done | 2 |

248 tests, clippy clean at deny warnings.

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

## saying it before it is finished

Claude is the slow part by a wide margin and nothing here changes that. What can change is
how much of it is spent in silence.

`--output-format stream-json` emits one JSON object per line as the answer is written. Carl
reads the text deltas, collects them into sentences, and speaks each one as it lands.

Measured on one four sentence answer.

| moment | time |
|---|---|
| first token | 5.8s |
| first sentence, which is when Carl now starts talking | 7.1s |
| whole answer, which is when he used to start | 9.4s |

So it saves the tail, 2.3s here and more on a longer answer. It does nothing about the wait
before the first word, because that is Claude thinking.

Two things about this were not obvious.

Splitting a stream into sentences is not the same job as tidying a finished answer. A code
fence opens in one chunk and closes four seconds later, and the decision about whether to
read a line aloud has to be made before the closing fence exists. So the buffer holds text
until it is certain what that text is: a newline for anything that might be markup, a full
stop for ordinary prose. There is a test that feeds the same answer one character at a time
and demands the identical result, because chunk boundaries come off the network and must not
change a single word.

The callback that speaks a sentence blocks for as long as the sentence takes. That would
stall Claude if the same thread were reading its output, so a reader thread drains it into a
channel and Claude writes at full speed while Carl talks. Third time that pattern has been
the answer in this project.

## Carl is not a coding assistant

He runs on the `claude` command line, whose own system prompt says it is a CLI for software
engineering tasks. That is right for Claude Code and wrong for Carl, and left alone it leaks
into the answers: a question about a video game gets turned down as out of scope.

`brief.rs` holds two pieces. `IDENTITY` says who Carl is and goes on every turn, typed or
spoken. `SPOKEN` is added on top only when the answer is going through a speaker.

Giving the model a name and a job works. Telling it that it is wrong does not.

## the instruction that mattered more than the code

Everything above shaves tenths of a second. Telling Carl that his answer is going to be
spoken out loud took a hundred seconds down to sixteen.

A screen answer can be skimmed, scrolled back over and abandoned halfway. A spoken answer
arrives one word at a time and you cannot skip the part you already know, so length is not a
matter of taste. It is latency, twice over: Claude spends longer writing it and Carl spends
longer saying it.

Same question, twice each, interleaved so load could not flatter either side.

| | words | claude | spoken aloud | total |
|---|---|---|---|---|
| before | about 200 | 22s | 78s | 100s |
| after | about 29 | 4.4s | 12s | 16s |

First token dropped from 15s to 2.8s as well, which was not expected. Claude thinks less hard
about an answer that has to fit in two sentences.

The brief lives in `voice_brief.rs` and is only added to spoken turns. `carl ask` in a
terminal deliberately does not get it, because two sentences is the wrong shape for something
you are reading.

## the other second and a half

| change | saved |
|---|---|
| transcribing with `base.en` instead of `small.en` | 2.0s |
| waiting 0.4s for you to stop talking instead of 0.9s | 0.5s |

Both were measured rather than assumed. On a real spoken question lasting 1.65 seconds,
`tiny.en` took 0.49s, `base.en` 0.89s and `small.en` 2.84s, and all three returned the
identical transcript word for word. The echo canceller suppresses room noise on the way
through, so the audio reaching whisper is cleaner than it was when `small.en` was chosen.
`--accurate` puts the bigger model back for a noisy room.

A faster model was the obvious thing to try and it did not help, but the reason first given
here was wrong and is worth writing down properly.

Haiku did refuse the question, twice in three runs, saying it was there to help with coding.
The conclusion drawn from that, that the smaller model is unusable for Carl, was wrong. With
Carl's real identity brief it answered three times out of three. What it had been given at
the time was a thin instruction saying "you are a voice assistant", which sits on top of
Claude Code's own description of itself as a tool for software engineering and does not
displace it. A name and a job displaces it. A correction does not.

That is why the identity now goes on every turn rather than only spoken ones. `carl ask` in
a terminal inherited exactly the same problem and nobody had noticed.

With that fixed, the models were measured again on equal terms. Haiku takes 5.2 to 6.1
seconds to its first word, Opus 2.9 to 4.1. Opus stays the default because it is faster,
which is not the reason originally given.

## slack, and the two loops that were the same loop

Socket Mode, not the Events API. The Events API needs Slack to reach a public URL and this
machine is a laptop behind a router. Socket Mode dials out over a websocket, so nothing is
exposed and nothing is hosted.

```
  apps.connections.open  ->  a single use websocket url
  dial it
  for each envelope:
    acknowledge it immediately
    is it a question, and is it from me      <- event.rs, pure
    hand it to the worker                    <- so reading never stops
  worker: ask claude, post the answer
```

Three things about this were not obvious.

**Acknowledge before answering.** Slack wants an acknowledgement within three seconds and
resends the event if it does not get one. Claude takes five at the very best. Answering first
means Slack decides the event was dropped and sends it again, and Carl answers the same
question two or three times. That does not show up when you test with one message and is
extremely obvious in a channel. Envelopes are acknowledged before Carl has even decided
whether the message was for him, because an ignored envelope still has to be acknowledged.

**Carl posts into the room he is listening to.** His own message comes straight back as an
event. That is the microphone hearing the speakers again, exactly, and it is worse here,
because the room only had Carl in it and a channel has an audience and a bill. Three separate
checks catch it: a bot id on the event, the sender being Carl's own user id, and a subtype
that means this was not a person typing. Any one of them missing is an infinite loop.

**Resends carry the same event id.** Slack resends on any doubt including its own timeouts,
so the last sixty four event ids are remembered and a repeat is dropped.

The answer itself goes through the same `turn` machinery as the terminal and the microphone,
so a Slack thread gets the same record, the same memory and the same record before acting
ordering. A Slack thread id is `slack-<channel>-<parent timestamp>`, which is why `ThreadId`
has allowed a dot in its character set since the day it was written.

No spoken brief in Slack. It is read, not heard, and two sentences is the wrong shape for
something you can scroll back over.

## the third time the same loop showed up

The microphone heard the speakers. Carl saw his own Slack messages. Now two agents feed each
other. Same shape every time, and each one worse than the last: the room only had Carl in it,
the channel has an audience, and the agent exchange has an audience and a bill.

The first two were fixed by refusing to listen to yourself. That does not work here, because
Alex genuinely is somebody else and hearing him is the entire point.

So the rule stopped being about who is talking and became about how long it has been since a
person was involved. `patience.rs` counts consecutive agent messages per thread and stops at
six. A human message resets it. It is per thread, so Hunter and Alex arguing in one channel
cannot silence Carl answering JJ in another.

That local count is the guard that matters, because it is the only one that survives the
other agent being broken or hostile. The protocol `ttl` is cooperative, which means it is an
agreement rather than a safety mechanism.

## why the protocol is so small

Slack already carries most of what a protocol would normally invent. The sender is the
message author, the recipient is the mention, the conversation is the thread. Adding fields
for those would mean two sources of truth that can disagree, and Slack's would win.

That leaves two things Slack cannot express.

`kind` is what the other agent should do with it. Without it, "thanks, that helps" and "and
what about X" are both just text, so an agent answers both and the politeness itself becomes
the loop. `done` and `decline` are unanswerable by rule, and that is what gives an exchange
an ending.

`ttl` is how many hops are left. Every reply decrements it. At zero the only legal move is to
stop, and stopping means saying `done` rather than going quiet, because silence looks like a
crash and invites a retry.

The body underneath is ordinary prose, on purpose. People are reading the channel, and a
protocol they cannot follow over somebody's shoulder is one that gets switched off.

## Carl did not know he had a mouth

Asked in Slack to message another user, he replied that he could not, because the claude.ai
Slack connector on his session was not authorised. That reply was itself posted to Slack.

He runs on the claude command line, which has its own idea of what Slack access means, and
nothing had told him that his answer is posted automatically. `slack::CONTEXT` now does, on
every Slack turn. It is the same class of problem as him thinking he was a coding assistant:
the underlying tool has an identity and it leaks through unless something displaces it.

## a name is not a mention

An at sign is unambiguous. A name is not, and the same five letters do completely different
jobs in a sentence.

```
carl what should I research next     to him
what do you think, Carl?             to him
I asked Carl yesterday               about him
Carl's memory design is good         about him
ask carl when he gets back           about him, to somebody else
```

The two failures are not equal. Missing one aimed at him costs an at sign. Answering one that
was not is a bot interrupting two people discussing it, which is how a bot gets removed from
a channel. So the rule errs towards silence.

The rule is position, not keywords. His name has to be the first word, allowing a greeting in
front of it, or the last word. That is where a name sits when you are speaking to somebody
and almost never where it sits when you are speaking about them. Possessive is refused
outright, because "Carl's" is never a question for Carl.

`named.rs` is pure text in and a decision out, so all of it is testable without a workspace,
which matters because this is the code that decides when Carl speaks in front of other people.

The cost is `channels:history`, so Carl now receives every message in every channel he is in.
Nothing is recorded unless it was addressed to him, and the scope can be removed without
breaking mentions or direct messages.

## the error that described the wrong problem

Carl said he could not work out who was talking to him, and Slack agreed: `users.info` came
back `user_not_found` for `U0BP4HA8Z88`. Then `users.list` returned that exact id, attached
to JJ_tmc Multiversal, from the same token a second later.

Both were telling the truth. Slack has two calling conventions. Methods that write take a
JSON body. Methods that read take form encoded parameters, and sending one of those a JSON
body does not fail: the parameters are dropped and the method runs as though none were
passed. So `users.info` was asked to look up nobody, and reported that nobody was found.

It reads as a missing person and it is a missing parameter, which is why it pointed at the
scopes, at the reinstall and at the workspace before it pointed at the request.

There are now two functions with names that say which convention they use, and the hint
attached to `user_not_found` names the trap, because the next person to hit it will start
from the same wrong end.

## the memory that was never actually written

Carl is described as a helper that remembers. His memory directory was empty, and had been
the whole time, because the only thing that ever wrote a note was a summary at the end of a
spoken conversation and that path had never once completed. The headline feature had never
happened.

The obvious fix, asking Claude after every turn whether anything was worth keeping, costs a
second model call on every message to answer no almost every time.

So Carl writes the note himself, inside the answer he was already giving.

```
Go for logistics science next.
[remember] JJ is playing Factorio, currently on red and green science.
```

The line is taken out before the answer is shown, spoken or recorded, and what is left is
written to `memory/`. It costs nothing extra, it works on every surface at once, and Carl
decides, which is the only participant who knows whether something mattered.

Two details that were not obvious. The note is named from its own text rather than from the
clock, so writing the same fact twice replaces it instead of accumulating near duplicates
that each cost context on every future turn. And a line starting with a square bracket now
waits for its newline in the sentence splitter, because half a `[remember]` spoken out loud
before the rest of the line arrived would be the worst of both.

## three times the underlying tool leaked through

Carl runs on the `claude` command line, which has its own identity and its own features, and
they keep surfacing as Carl's.

| what leaked | what it looked like |
|---|---|
| Claude Code is for software engineering | refused a question about a video game |
| Claude Code has Slack connectors | said he could not send a message, in a message posted to Slack |
| Claude Code has its own memory system | said he had saved something and saved nothing |

The last one is the clearest. Asked to quote the instruction telling him how to remember, he
quoted Claude Code's, complete with frontmatter and a MEMORY.md index. The brief was
arriving, and losing.

Every one of the three was fixed the same way, and it is the same lesson each time. A
correction does not displace a built in identity. Only a replacement does. So the memory
instruction now opens by saying he has no memory directory, no MEMORY.md and no files to
write, that those instructions belong to the program he is running inside, and that the only
memory he has is the line.

## streaming into a place that cannot stream

The voice never suffered from Claude taking twenty five seconds, because a voice arriving
late is obviously still on its way. Slack showed nothing at all until the whole answer
landed, so a question sat there looking ignored and then was suddenly answered, and in
between the only honest reading was that Carl had broken.

Slack has no streaming. It has `chat.update`, so the message is posted before Claude is even
asked and rewritten as the words arrive. The placeholder says which wait this is, thinking or
looking at your screen, because those are different waits and a spinner would say neither.

The real problem is pacing, which is why it is its own type with its own tests. Slack allows
roughly one edit a second per channel and going over returns `ratelimited`, at which point
the answer stops updating, which is worse than never having streamed. So rewrites wait 1.5
seconds, and a trickle of new text waits longer than a flood, because rewriting a whole
message to add three words flickers for no benefit.

`[remember]` lines are stripped before each rewrite rather than after. A half written note
appearing in a channel and then vanishing again would be worse than not streaming at all.

## knowing what game is being played

Every screenshot was a fresh stranger. Carl could see a factory and had no idea which game
it was, which version, or whether the tech tree in front of him was the one he knows about.
He gave vanilla advice to somebody playing Sea Block with Angel's, Bob's and Space
Exploration stacked on top, where there are no ores on the map at all and almost nothing
about the base game applies.

None of that needed a screenshot. It is all in the process list and in the game's own files.

| where | what it says |
|---|---|
| `ps` | whether the game is running now |
| `factorio-current.log`, first line | version and expansions |
| the log's timestamp | when it was last played |
| `mods/mod-list.json` | which mods are switched on, which is not the same as which are downloaded |
| `saves/` | the save name, which is often the best clue about progress |

Window titles would be the obvious signal and are not available. This machine runs Wayland,
where a program cannot ask what another window is called, and asking GNOME Shell directly
returns false.

### only the overhauls get named

There are eighty four mods installed here. Listing them costs more context than the whole
rest of the brief and buries the four that matter under sixty that do not. Nobody needs to be
told about `squeak through`. Everybody needs to be told about Sea Block, because in Sea Block
there is no ore on the map and every answer about mining is wrong.

So known overhaul families are recognised and named, and the rest is a count.

### three wrong turns getting there

**A keyword list decided when to include it.** It failed on the first real question. "I need
more iron plates" contains none of the words, so Carl answered about stone furnaces. Recency
replaced it: the game is up, or it was up in the last six hours, or it is not mentioned.
Recency is a fact and a keyword list is a guess.

**Space Exploration was silently missed.** Family names are matched with spaces removed, and
the pattern still had a space in it, so it could never match. A test now asserts no pattern
contains a space, because that failure is invisible: it does not error, it just quietly finds
nothing.

**Downloaded is not enabled.** The first version read the mods directory. Eighty eight mods
were downloaded and four were switched on, so a vanilla Space Age save was described as Sea
Block with Angel's and Bob's, and the answer that came out was further from correct than the
vanilla answer it replaced. `mod-list.json` is what the game itself reads. JJ caught this, no
test did, and it had been shown to him as proof the feature worked.

**The binary was stale, twice.** `cargo clippy` and `cargo test` do not relink
`target/debug/carl`, so a real run was exercising old code and the conclusion drawn from it
was wrong. Third time in this project.

## a fact and a state are different things

Carl has two kinds of memory now and keeping them apart is the point.

| | example | how long it is true | where it goes |
|---|---|---|---|
| a fact | JJ's mentor is Hunter | next month as well | `memory/`, forever, one file each |
| a state | JJ is on blue science | about an hour | `game/<name>.md`, one file, replaced |

Putting a state in permanent memory fills it with things that are quietly false, and a wrong
note is worse than a missing one because it comes back on every turn and gets stated with
confidence. So a game picture is one file per game, replaced rather than appended. There is
exactly one current picture and the newest wins.

It is also what makes a second look worth taking. Without it every screenshot was a fresh
stranger, and "is that better than before" could not be answered at all.

The first version only worked when Carl looked at the screen, because that is what the
instruction said, and JJ told him the state in words instead so nothing was recorded. The
trigger is now learning something, however he learned it, and the marker is explained
alongside the other two rather than buried in the game section.

Proven across conversations. Told about red and green science and no oil in one thread, he
answered correctly in a different thread that shares nothing but the picture, and when told
oil was running he rewrote the whole thing rather than appending a contradiction.

## one voice per answer, not one per sentence

Piper takes 0.29 seconds to start, measured, and that is per process. Carl started a fresh
piper and a fresh player for every sentence, so a three sentence answer paid it three times
and left an audible gap between each one.

Now one stream is opened on the first sentence and stays open, and each sentence is a line
written to something already running. Measured on three sentences, 1.015 seconds became
0.719. The number matters less than the gaps disappearing, because a pause between every
sentence is the thing that makes a voice sound like a machine reading a list.

Closing the input is what ends it. Piper reads until end of file, so a stream with its input
still open is not finished, it is waiting, and anything asking whether it is done waits
forever.

The half duplex path still speaks one sentence at a time. Without a canceller the microphone
has to be deaf while Carl talks, and a stream held open would keep it deaf between sentences
as well, which is worse than the gap.

## where the remaining delay actually is

| step | time |
|---|---|
| quiet before he decides you stopped | 0.4s |
| whisper `base.en` | 0.9s |
| claude, starting the CLI | 0.8s |
| claude, to its first word | 2.0s |
| piper, first sound | 0.3s, now once per answer rather than per sentence |

The CLI startup is the next real target. Every turn starts a new `claude` process, which
reads its config and connects its servers before a single token is asked for. A session held
open across turns would remove it outright.

## one process per task, alive for the whole run

Every turn used to start a new `claude`. Measured, that costs about 0.8 seconds before a
single token is asked for, spent reading config and connecting servers, and then the model
starts cold. Held open, the same question reaches its first token in 0.97 seconds where a
fresh process takes 2.8.

A task here is a thread. The voice is one, so it keeps one process for the whole run and
never pays startup again. Every Slack conversation is one, and the pool keeps four open and
closes the least recently used, because each process peaked around 195MB and holding one per
thread forever would eventually be every thread anybody opened. The voice is used constantly,
so least recently used means it is never the one closed.

### what this forced

A system prompt is fixed when a process starts and cannot be changed afterwards. Carl's
memory grows, his picture of the game is rewritten, and the game itself starts and stops, so
none of that could stay where it was.

So a turn is now split by how long each piece stays true. Who Carl is goes in the system
prompt, once, for the life of the process. Everything that varies arrives in front of the
question. A one shot process could have kept it all together and used to, and keeping the two
apart costs nothing there.

### and what it has to survive

A process that lives for hours has time to be killed, run out of memory, or be restarted
underneath. A dead one is reopened and the question retried once, and the reopen resumes the
conversation rather than starting a second one, which is the difference between a hiccup and
losing everything said so far.

## the noise a person makes while thinking

Carl takes a second or two to answer. That is fast for a model and long for a silence, and
silence after a question reads as not having been heard.

A person asked something does not go quiet. They say "mm" and then answer, and the sound
carries no information at all. Knowing you were heard is the whole of it.

Two things decide whether this helps or makes it worse.

It has to be instant, so it cannot be synthesised when it is wanted. Piper takes 0.29 seconds
to start, which is most of the gap being covered. The phrases are rendered once at startup
into `/dev/shm` and playing one afterwards costs only starting `aplay`.

And it must not fire when the answer is quick, because a noise followed immediately by the
answer sounds like a stutter and is worse than the silence it replaced. So it is armed rather
than played: a thread holds a 450 millisecond timer and the first word of the answer cancels
it. Dropping it cancels it too, so no path can leave one to fire late.

None of the phrases promise anything. "Let me check" is a claim about what Carl is doing, and
he is not doing it, he is waiting. "Mm" is only an acknowledgement, which is all this is for,
and there is a test that keeps it that way.

## interrupting him while he thinks

Talking over Carl mid sentence already worked. Changing your mind before he had said anything
did not, so the question you abandoned still got answered at you.

The reason was structural. A turn was read by blocking on the channel, and the only chance to
give up was inside the callback that receives words, which does not run while there are no
words. So the read now waits in tenth of a second steps and asks a second callback whether to
carry on. The microphone is watched during exactly the gap where changing your mind happens.

### the trap underneath it

The model does not stop because the listener walked away. The rest of that answer is still
being written and still arriving, so the next question would be handed the tail of the one
before it.

So an abandoned turn is marked, and the next question clears the leftovers before sending
anything. The clearing is bounded at twenty seconds: a session that will never produce its
ending must not hang the next question forever, and one stale fragment is survivable where
Carl never speaking again is not.

### the test that proved nothing

The first version of `tests/session.rs` passed with the clearing deleted.

The stand in for `claude` gave every answer the same words, so an answer left over from an
abandoned turn read exactly like a fresh one and the assertion could not tell them apart.
Numbering the turns fixed it, and now removing the clearing fails exactly that test and
nothing else.

Worth remembering as a rule rather than a Factorio style one off. A test that cannot fail is
worse than no test, because it is counted.

## the thread count was worth more than the resident model

Keeping whisper loaded looked like the obvious next saving, and measuring it first was worth
doing. Of 913 milliseconds for a short question, only 151 is loading the model. The other 591
is the encoding, which happens either way. So a resident whisper is worth about 200
milliseconds, not the half second it was billed as.

The thread count turned out to matter more, and in the opposite direction to the guess in the
code.

| threads | time |
|---|---|
| 4 | 845ms |
| 8 | 709ms |
| 12 | 636ms |
| 16 | 685ms |

Carl was using every core but one, on the reasoning that leaving one free is polite to
whatever else is running. That is slower, by ninety milliseconds, and the reason is that this
machine does not have sixteen of the same core. It has fast ones and slow ones, whisper
splits its work into equal pieces, and every piece finishes at the speed of the core it landed
on. Giving work to the slow cores makes the whole batch wait for them.

Three quarters of the machine, as a rule rather than the number twelve, because the next
machine will have a different count and the same shape of problem.

## noticing that an answer did not land

Carl had no idea when he got something wrong. He answered, the answer was useless, and the
next question arrived as though the exchange had gone fine.

The person who knows it went wrong is the one who has to say so, and they usually already did,
immediately, in the next thing they said. Two signals, both ordinary conversation rather than
anything anybody has to learn.

Saying no. "No, I meant the other one" only gets said when an answer missed.

Saying it again. Asking the same thing twice in a row means the first answer did not count,
whatever the words were. Compared as sets of words rather than as strings, because nobody
repeats themselves exactly.

The trap is that "no" is also the ordinary answer to a question, and Carl asks plenty of
them. So a bare no only counts when the last thing Carl said did not end in a question mark.
Without that, every "no, I have not unlocked steel" would read as being told off.

What it does with it is the cheap part. The next turn is told the last one missed, and told
what to do about it: do not repeat it, do not defend it, work out what was actually meant.
Being told only that you were wrong invites saying the same thing more firmly.

### and a regression it turned up

Passing the note meant putting something in front of the question, which is how it was
noticed that the pool work had started passing Carl's own scaffolding as what somebody said.
The Slack context was going straight into the permanent record.

`said` and `sent` have been different fields since the first week for exactly this reason, and
eight positional arguments in a row is how they got swapped. They are a struct now.

## every note says who said it

Memory is one pile and everybody who can reach Carl writes into it. Hunter is in the
workspace, so something he mentioned about his own base would come back a week later as a
fact about JJ's, stated with exactly the confidence of anything JJ said himself.

So a note carries its source, as a line in the note rather than in the filename or a separate
index. The note is handed to a model as text, so the attribution has to survive as text, and a
second file to keep in step is a second file to get out of step.

```
JJ prefers short answers.

(said by JJ)
```

Carl is told to weigh it. A preference one person stated is theirs and not everybody's, and
if a note might not apply to whoever is talking now, saying so beats acting on it.

Wiring it turned up that the Slack path had stopped recording an author at all when it moved
to the held open process, and that the terminal never had one. Both are fixed, and the
terminal is attributed to JJ because only one person has that terminal and it is the same
person who has the microphone.

## python, with the rest of the machine taken away

Carl needs to work something out rather than guess, because arithmetic is what a model is
most confidently wrong about. Granting `python3` did that and granted a great deal besides:
it could read any file the user could read, write any file the user could write, and open a
socket to anywhere. Anybody who could message Carl in Slack could ask for all of it.

`etc/carl-python` is the same interpreter under bubblewrap. The system is bound read only,
one directory is writable, and the network, the process table and the home directory are
simply not there.

| tried | result |
|---|---|
| `2**64 - 1` | works |
| write in the workspace | works |
| read `~/.carl/slack.json` | `FileNotFoundError` |
| list the home directory | `FileNotFoundError` |
| open a socket | `OSError` |
| count visible processes | 2 |

The first attempt bound all of `/` read only with one writable directory inside it. That
stops python changing anything and does nothing whatever about reading, so it could still
open the Slack tokens and print them. Read only is not the same as not there, and the fix was
to name what python may see rather than what it may write.

Carl and AOS now agree. AOS refuses to have a shell because a shell is every tier at once and
cannot be described to a policy. Carl has an interpreter, which is the same problem, and the
answer is the same: bound what it can reach rather than trusting what it will do.
