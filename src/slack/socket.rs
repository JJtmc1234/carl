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

use tungstenite::Message;

use super::{Api, Ask, Engaged, Me, ask_from};
use crate::Result;

/// How many recent events to remember for spotting a resend.
///
/// Slack resends on any doubt, including its own timeouts, and the copy carries the same
/// `event_id`. Small because a resend follows within seconds if it comes at all.
const REMEMBERED: usize = 64;

/// Reads envelopes until the process is killed, handing questions to `on_ask`.
/// How long a silent socket is given before it is treated as dead.
///
/// Slack pings roughly every thirty seconds, so three minutes of complete silence is not a
/// quiet channel, it is a link that is no longer there.
const SILENCE_MEANS_DEAD: Duration = Duration::from_secs(180);

/// Whether this error is the read timeout rather than a real failure.
///
/// Named rather than matched inline, because the two read identically in a log and only one of
/// them means something is wrong.
fn silent_too_long(e: &tungstenite::Error) -> bool {
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
/// tungstenite hands back either a plain stream or a TLS one, and the timeout lives on the
/// `TcpStream` under whichever it is. Slack is always TLS in practice, and the plain arm is
/// there so the timeout is not quietly lost if that ever changes.
fn set_read_timeout(
    ws: &tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    how_long: Duration,
) -> std::io::Result<()> {
    match ws.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(tcp) => tcp.set_read_timeout(Some(how_long)),
        // Which TLS variant exists depends on the feature this was built with, so both are
        // matched and the catch all keeps it compiling either way rather than only on the
        // machine it was written on.
        tungstenite::stream::MaybeTlsStream::Rustls(tls) => {
            tls.get_ref().set_read_timeout(Some(how_long))
        }
        other => {
            let _ = other;
            Ok(())
        }
    }
}

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

        // A read timeout, so a link that has quietly died is noticed.
        //
        // Without one, `read` blocks for ever. A connection that goes away without a close
        // frame, which is what a dropped route or a sleeping laptop leaves behind, sits there
        // looking established while nothing arrives. Carl went deaf for two days that way: the
        // socket showed ESTAB the whole time, no error was ever raised, and there was nothing
        // in the log because there was nothing to log.
        //
        // Slack sends a ping every half minute or so, and tungstenite answers those itself and
        // hands the loop an `Ok` it skips. So on a healthy link something arrives well inside
        // this, and a timeout genuinely means the link is gone rather than that Slack is quiet.
        if let Err(e) = set_read_timeout(&ws, SILENCE_MEANS_DEAD) {
            eprintln!("could not set a read timeout on the slack socket: {e}");
        }

        eprintln!("connected to slack.");
        backoff = Duration::from_secs(1);

        loop {
            let text = match ws.read() {
                Ok(Message::Text(t)) => t.to_string(),
                // Ping and pong are answered by the library. Close means go round again.
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Ok(Message::Binary(_)) => continue,
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    // A timeout here is the whole point of having one: the link went away
                    // without saying so, and going round again is the only way to notice.
                    if silent_too_long(&e) {
                        eprintln!(
                            "nothing from slack in {}s, treating the link as dead and dialling again.",
                            SILENCE_MEANS_DEAD.as_secs()
                        );
                    } else {
                        eprintln!("slack connection dropped: {e}");
                    }
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
