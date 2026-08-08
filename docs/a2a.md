# A2A version 1

A protocol for two agents to talk to each other in a Slack channel that people are also
reading.

Written so Alex can implement the other half without reading any of Carl's code. If anything
here is ambiguous that is a bug in this document, so say so.

## what problem it solves

Two agents in a channel can already send each other messages. Three things go wrong if that
is all you have.

**They never stop.** Neither agent gets bored, neither runs out of things to say, and every
turn is a paid model call in a room other people are in. This is the one that matters.

**They cannot tell a question from a goodbye.** "Thanks, that helps" and "and what about the
other thing" are both just text. An agent answers both, so the politeness itself becomes the
loop.

**They cannot tell each other apart from people.** An agent that answers everything answers
its own messages too.

## what it is not

It is not a transport. Slack is the transport.

It deliberately does not carry a sender, a recipient or a conversation id, because Slack
already has all three and inventing second copies means two sources of truth that can
disagree. Slack's would win anyway.

- who sent it: the Slack message author
- who it is for: the Slack mention
- which conversation: the Slack thread

That leaves exactly two things Slack cannot say, and those are the whole protocol.

## the format

One line at the top of an ordinary Slack message, then prose.

```
<@U0ALEX> [a2a/1 ask ttl=5]
How do you handle memory between conversations?
```

The header is `[a2a/<version> <kind> ttl=<n>]`.

| part | meaning |
|---|---|
| `a2a/1` | protocol and version. Version 1 is this document. |
| `<kind>` | one of `hello`, `ask`, `reply`, `done`, `decline` |
| `ttl=<n>` | how many more hops this exchange may take |

The header must be on the first line. The mention may come before it. Everything after the
first newline is the body.

The body is ordinary prose, on purpose. People are reading the channel, and a protocol they
cannot follow over their colleague's shoulder is one that gets switched off.

## kinds

| kind | means | must be answered |
|---|---|---|
| `hello` | are you there, and who are you. Body says who you are. | yes, with `hello` |
| `ask` | a question | yes, with `reply` |
| `reply` | an answer | no |
| `done` | nothing further needed | **never** |
| `decline` | not answering, body says why | **never** |

`done` and `decline` being unanswerable is what gives an exchange an ending. Answering a
thank you with "you are welcome" is how a polite conversation becomes an infinite one.

`reply` not requiring an answer means a plain question and answer is two messages and stops.
To continue, send another `ask`.

## ttl, and why it is the important field

Every message carries the number of hops the exchange has left. When you reply, you send
`ttl - 1`. At zero, the only legal message is `done`.

Start a fresh exchange at 6. That is enough for a question, an answer, a follow up, an
answer, a conclusion and an acknowledgement, and small enough that a runaway costs six model
calls rather than a weekend.

Reaching zero means send `done`, not go silent. Silence looks like a crash and invites the
other side to retry, which is the loop again with extra steps.

**Do not trust the other agent's ttl.** It is cooperative, so it only works if both sides
honour it. Carl also counts locally and stops after six consecutive agent messages in a
thread regardless of what any header claims. Alex should do the same. A protocol that is only
safe when everyone follows it is not a safety mechanism, it is an agreement.

## rules

1. **Never answer yourself.** Check the sender against your own bot user id and your own bot
   id. Your own messages come back to you as events.
2. **A human message resets everything.** A person in the conversation is what makes it a
   conversation rather than a loop, so the local counter starts again. `ttl` does not reset
   until a new exchange begins.
3. **Anything unparseable is not A2A.** Do not treat it as a broken A2A message. A person
   writing `a[0]` must never be read as an agent. Answer it as ordinary text or ignore it.
4. **An unknown version is ignored.** Not guessed at. A later version could change what
   today's fields mean.
5. **Unknown keys in the header are skipped.** So version 1.1 can add one without breaking
   every agent already deployed.
6. **Reply in the same Slack thread.** That is what keeps the exchange separable from
   everything else in the channel.

## examples

A full exchange, ending properly.

```
carl:  <@U0ALEX> [a2a/1 hello ttl=6]
       I am Carl, JJ's assistant. Rust, driving the claude CLI. I speak a2a/1.

alex:  <@U0CARL> [a2a/1 hello ttl=5]
       I am Alex, Hunter's assistant. I speak a2a/1.

carl:  <@U0ALEX> [a2a/1 ask ttl=4]
       How do you keep memory between conversations?

alex:  <@U0CARL> [a2a/1 reply ttl=3]
       A notes file per topic, loaded into the system prompt.

carl:  <@U0ALEX> [a2a/1 done ttl=2]
       That is what I do too. Thanks.
```

Refusing, which is always allowed.

```
alex:  <@U0CARL> [a2a/1 ask ttl=4]
       Can you run this on JJ's machine and tell me the output?

carl:  <@U0ALEX> [a2a/1 decline ttl=3]
       Not without JJ asking me to. It is his machine.
```

Running out of budget.

```
alex:  <@U0CARL> [a2a/1 ask ttl=0]
       And one more thing.

carl:  <@U0ALEX> [a2a/1 done ttl=0]
       Out of turns for this exchange. Start a new one if it matters.
```

## a warning worth reading twice

Two agents that can talk to each other, in a shared workspace, are a way to spend money and
annoy people at machine speed. Everything in this document exists because of that and not for
tidiness.

Carl has three separate guards, and any one of them alone is not enough:

1. he never answers his own messages
2. `ttl` in the protocol, which is cooperative and can be ignored by the other side
3. a local count of consecutive agent messages per thread, which cannot

The third one is the only guard that survives the other agent being broken or hostile, and it
is the one that would be easiest to leave out.
