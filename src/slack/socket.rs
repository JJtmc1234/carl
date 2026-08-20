//! The websocket half of Socket Mode.
//!
//! Dial, read envelopes, acknowledge each one straight away, reconnect when told to.
//!
//! The acknowledgement is the part with a trap in it. Slack wants one within three seconds
//! and resends the event if it does not get one. Claude takes five seconds at best, so
//! answering before acknowledging means Slack decides the event was dropped and sends it
//! again, and Carl answers the same question two or three times. That does not show up in
//! testing with one message and is extremely obvious in a channel with people in it.

use std::collections::VecDeque;
use std::time::Duration;

use std::net::TcpStream;

use tungstenite::Message;
use tungstenite::stream::MaybeTlsStream;

use super::{Api, Ask, Engaged, Me, ask_from};
use crate::Result;

/// How many recent events to remember for spotting a resend.
///
/// Slack resends on any doubt, including its own timeouts, and the copy carries the same
/// `event_id`. Small because a resend follows within seconds if it comes at all.
const REMEMBERED: usize = 64;

/// How long a single read waits before giving the loop a turn.
///
/// Short, because its only job is to stop `read` blocking forever. Slack has nothing to say
/// most of the time, so almost every tick is empty and that is normal rather than a signal.
const TICK: Duration = Duration::from_secs(10);

/// Silence after which Carl prods the connection rather than assuming it is fine.
const PING_AFTER: Duration = Duration::from_secs(45);

/// Silence after which the connection is treated as dead and rebuilt.
///
/// Comfortably longer than Slack's own ping interval and longer again than `PING_AFTER`, so a
/// live but quiet connection has had two chances to prove itself before this fires. Rebuilding
/// a healthy connection costs a reconnect; failing to rebuild a dead one costs every message
/// anybody sends until a human notices, so the two mistakes are not the same size.
const DEAD_AFTER: Duration = Duration::from_secs(120);

/// Reads envelopes until the process is killed, handing questions to `on_ask`.
pub fn serve(api: &Api, me: &Me, on_ask: &mut dyn FnMut(Ask)) -> Result<()> {
    let mut seen: VecDeque<String> = VecDeque::new();
    // Threads Carl has answered in. Kept here rather than in the worker because it has to be
    // updated in step with the decision to answer, and the worker is deliberately behind a
    // queue that can be several messages long.
    let mut engaged = Engaged::new();
    let mut backoff = Duration::from_secs(1);

    loop {
        // Fetched every time. The url is single use and expires within seconds, so a cached
        // one is only ever good for the reconnect that does not need it.
        let url = match api.open_socket() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("cannot open a slack socket: {e}. Retrying in {backoff:?}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };

        let mut ws = match tungstenite::connect(&url) {
            Ok((ws, _)) => ws,
            Err(e) => {
                eprintln!("cannot dial slack: {e}. Retrying in {backoff:?}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };

        // Without this `ws.read()` blocks forever on a connection that died without a FIN,
        // which is what a laptop suspending or a router handing out a new NAT mapping leaves
        // behind. The process then sits alive and silent, having printed "connected to slack."
        // and nothing since, so `Restart=always` cannot help because nothing has exited, and
        // the OS TCP keepalive default is two hours. See bug 18.
        if let Err(e) = set_read_timeout(&ws, TICK) {
            eprintln!(
                "cannot set a read timeout on the slack socket: {e}. Retrying in {backoff:?}"
            );
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(Duration::from_secs(60));
            continue;
        }

        eprintln!("connected to slack.");
        backoff = Duration::from_secs(1);
        let mut heard_at = std::time::Instant::now();

        loop {
            let text = match ws.read() {
                Ok(Message::Text(t)) => {
                    heard_at = std::time::Instant::now();
                    t.to_string()
                }
                // Ping and pong are answered by the library. Close means go round again.
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {
                    heard_at = std::time::Instant::now();
                    continue;
                }
                Ok(Message::Binary(_)) => {
                    heard_at = std::time::Instant::now();
                    continue;
                }
                Ok(Message::Close(_)) => break,

                // A tick with nothing on it, which is ordinary. Slack speaks only when it has
                // something to say, and its own pings are minutes apart.
                Err(e) if is_timeout(&e) => {
                    let quiet = heard_at.elapsed();
                    if quiet > DEAD_AFTER {
                        eprintln!(
                            "nothing from slack for {quiet:?}, so the connection is dead. \
                             Reconnecting."
                        );
                        break;
                    }
                    // Prodded rather than waited on. Slack answers a ping with a pong, which
                    // is traffic, so a live connection proves itself well before the deadline
                    // above. A dead one cannot, because the pong never comes back, and that
                    // is the whole detector: sending may well succeed on a half open socket.
                    if quiet > PING_AFTER {
                        let _ = ws.send(Message::Ping(Vec::new().into()));
                    }
                    continue;
                }
                Err(e) => {
                    eprintln!("slack connection dropped: {e}");
                    break;
                }
            };

            let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };

            match frame.get("type").and_then(|t| t.as_str()) {
                Some("hello") => continue,
                // Slack cycles connections on purpose, roughly hourly, and before deploys.
                // It is routine rather than a fault.
                Some("disconnect") => {
                    eprintln!("slack asked us to reconnect.");
                    break;
                }
                _ => {}
            }

            // Before anything else, including deciding whether it is even a question. An
            // envelope Carl ignores still has to be acknowledged or Slack keeps sending it.
            if let Some(id) = frame.get("envelope_id").and_then(|i| i.as_str())
                && let Err(e) = ws.send(Message::text(format!("{{\"envelope_id\":\"{id}\"}}")))
            {
                eprintln!("could not acknowledge an envelope: {e}");
                break;
            }

            let Some(payload) = frame.get("payload") else {
                continue;
            };

            // Printed for every envelope, not behind a flag. When nothing is happening you
            // need to know whether Slack is sending nothing or sending something Carl is
            // filtering out, and those two have completely different fixes. Same reason the
            // ear prints every transcript.
            eprintln!(
                "  <- {} / {}",
                frame.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
                payload
                    .get("event")
                    .and_then(|e| e.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("no event")
            );

            // A resend carries the same event id. Without this, a slow moment on Slack's side
            // turns into Carl answering the same question twice.
            if let Some(id) = payload.get("event_id").and_then(|i| i.as_str()) {
                if seen.contains(&id.to_string()) {
                    eprintln!("  ignoring a resend of {id}");
                    continue;
                }
                seen.push_back(id.to_string());
                if seen.len() > REMEMBERED {
                    seen.pop_front();
                }
            }

            match ask_from(payload, &me.user_id, &me.bot_id, &engaged) {
                Some(ask) => {
                    engaged.join(&ask.thread_ts);
                    on_ask(ask);
                }
                // Says why rather than going quiet. "Carl ignored me" and "Carl never heard
                // me" look identical from the outside and are not the same problem.
                None => eprintln!("     ignored, not a question addressed to Carl"),
            }
        }
    }
}

/// Whether this error is the read timeout expiring rather than a real failure.
///
/// Two kinds, because a timeout on a socket surfaces as `WouldBlock` on some platforms and
/// `TimedOut` on others, and tungstenite passes the io error through untouched.
fn is_timeout(e: &tungstenite::Error) -> bool {
    match e {
        tungstenite::Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
        _ => false,
    }
}

/// Puts a read timeout on the socket underneath the websocket.
///
/// tungstenite owns the stream, so the timeout has to be reached through it. Slack is `wss`,
/// so the Rustls arm is the live one, and the plain arm is there because which variant arrives
/// is decided by the url rather than by anything here.
fn set_read_timeout(
    ws: &tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    how_long: Duration,
) -> std::io::Result<()> {
    let tcp = match ws.get_ref() {
        MaybeTlsStream::Plain(tcp) => tcp,
        MaybeTlsStream::Rustls(tls) => &tls.sock,
        // `MaybeTlsStream` is non exhaustive, so a future variant lands here. Refusing is the
        // right answer: carrying on would leave the read unbounded again, which is the bug.
        other => {
            return Err(std::io::Error::other(format!(
                "unfamiliar socket kind behind the websocket, so no read timeout could be set: \
                 {other:?}"
            )));
        }
    };
    tcp.set_read_timeout(Some(how_long))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug. `ws.read()` on a connection that died without a FIN blocks forever, so the
    /// outer reconnect loop is never reached and the process sits alive and silent. A laptop
    /// suspending or a router handing out a new NAT mapping leaves exactly that.
    ///
    /// Proves both halves at once against a real socket: that the timeout reaches the stream
    /// tungstenite owns, and that what comes back is recognised as a timeout rather than as a
    /// dropped connection.
    #[test]
    fn a_peer_that_says_nothing_times_out_rather_than_blocking_forever() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // Accepts and then says nothing, which is what a half open connection looks like from
        // this end. It holds the socket open, so without a read timeout the read below has no
        // reason to ever return.
        let peer = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(5));
            drop(sock);
        });

        let tcp = TcpStream::connect(addr).unwrap();
        let mut ws = tungstenite::WebSocket::from_raw_socket(
            MaybeTlsStream::Plain(tcp),
            tungstenite::protocol::Role::Client,
            None,
        );

        set_read_timeout(&ws, Duration::from_millis(200)).expect("the timeout has to be set");

        let started = std::time::Instant::now();
        let err = ws
            .read()
            .expect_err("a silent peer cannot produce a message");
        let took = started.elapsed();

        assert!(
            took < Duration::from_secs(2),
            "the read took {took:?}, so it is still waiting on the peer rather than the clock"
        );
        assert!(
            is_timeout(&err),
            "the timeout has to be told apart from a real failure, got {err:?}"
        );

        let _ = peer.join();
    }

    /// A real failure must not be mistaken for a quiet tick, or a dropped connection would
    /// spin here until `DEAD_AFTER` instead of reconnecting at once.
    #[test]
    fn a_real_failure_is_not_a_timeout() {
        for kind in [
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::UnexpectedEof,
        ] {
            let e = tungstenite::Error::Io(std::io::Error::new(kind, "gone"));
            assert!(!is_timeout(&e), "{kind:?} is not a timeout");
        }
        assert!(!is_timeout(&tungstenite::Error::ConnectionClosed));
    }

    /// The three durations only work in this order. Ping after dead would mean the connection
    /// is torn down before it is ever prodded, so the prod could never save a live one, and a
    /// tick longer than either would step straight past both.
    #[test]
    fn the_timings_leave_room_to_prove_a_connection_is_alive() {
        assert!(
            TICK < PING_AFTER,
            "a tick has to be able to notice the ping point"
        );
        assert!(
            PING_AFTER < DEAD_AFTER,
            "prod before giving up, or the prod is pointless"
        );
        assert!(
            DEAD_AFTER - PING_AFTER > TICK * 2,
            "a live connection needs more than one tick to answer the ping before it is dropped"
        );
    }
}
