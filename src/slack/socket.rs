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

use super::{Api, Ask, ask_from};
use crate::Result;

/// How many recent events to remember for spotting a resend.
///
/// Slack resends on any doubt, including its own timeouts, and the copy carries the same
/// `event_id`. Small because a resend follows within seconds if it comes at all.
const REMEMBERED: usize = 64;

/// Reads envelopes until the process is killed, handing questions to `on_ask`.
pub fn serve(api: &Api, me: &str, on_ask: &mut dyn FnMut(Ask)) -> Result<()> {
    let mut seen: VecDeque<String> = VecDeque::new();
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

            if let Some(ask) = ask_from(payload, me) {
                on_ask(ask);
            }
        }
    }
}
