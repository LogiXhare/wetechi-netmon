//! `wetechinetmonctl`: the incident CLI (Milestone 5E, ADR 0040).
//!
//! An API client and nothing more. Every command is one or two API
//! requests; the CLI holds no business logic and never touches the
//! database, so authorization and audit cannot be bypassed by using it.

pub mod args;
pub mod client;
pub mod config;
pub mod exit;
pub mod output;

use std::io::Write;

use hyper::Method;
use serde_json::Value;

use crate::args::{Command, IncidentRef, Invocation, Output};
use crate::client::{Call, Client, Reply};
use crate::config::Environment;

/// Where output goes, and what the terminal is.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Microseconds since the epoch, for relative ages.
    pub now_micros: i64,
}

/// Why a command stopped, already carrying its exit code.
#[derive(Debug)]
struct Stop {
    code: i32,
    /// For stderr; empty when the output already said it.
    message: String,
}

impl Stop {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Stop {
            code,
            message: message.into(),
        }
    }
}

/// Runs one command line and returns the exit code.
pub async fn run(args: &[String], env: &dyn Environment, io: &mut Io<'_>) -> i32 {
    run_with(args, env, io, |client| client).await
}

/// [`run`], with a hook to adjust the client (tests drop the backoff).
pub async fn run_with(
    args: &[String],
    env: &dyn Environment,
    io: &mut Io<'_>,
    adjust: impl FnOnce(Client) -> Client,
) -> i32 {
    let invocation = match args::parse(args) {
        Ok(invocation) => invocation,
        Err(error) => {
            let _ = writeln!(io.err, "Error: {}\n\n{}", error.0, args::USAGE);
            return exit::USAGE;
        }
    };
    match invocation.command {
        Command::Help => {
            let _ = write!(io.out, "{}", args::USAGE);
            return exit::SUCCESS;
        }
        Command::Version => {
            let _ = writeln!(io.out, "wetechinetmonctl {}", env!("CARGO_PKG_VERSION"));
            return exit::SUCCESS;
        }
        _ => {}
    }
    let endpoint = match config::resolve(env, invocation.global.profile.as_deref()) {
        Ok(endpoint) => endpoint,
        Err(message) => {
            let _ = writeln!(io.err, "Error: {message}");
            return exit::USAGE;
        }
    };
    let client = match Client::new(endpoint) {
        Ok(client) => adjust(client),
        Err(error) => {
            let _ = writeln!(io.err, "Error: {error}");
            return exit::USAGE;
        }
    };
    match execute(&client, &invocation, io).await {
        Ok(()) => exit::SUCCESS,
        Err(stop) => {
            if !stop.message.is_empty() {
                let _ = write!(io.err, "{}", stop.message);
                if !stop.message.ends_with('\n') {
                    let _ = writeln!(io.err);
                }
            }
            stop.code
        }
    }
}

/// `value` with every byte outside the RFC 3986 unreserved set escaped.
pub fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn query_string(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let joined: Vec<String> = pairs
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect();
    format!("?{}", joined.join("&"))
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Sends `call`; a non-success answer stops the command with its exit
/// code, printing the problem document (verbatim under `--output json`).
async fn fetch(
    client: &Client,
    call: Call,
    io: &mut Io<'_>,
    output: Output,
) -> Result<Reply, Stop> {
    let reply = client
        .send(&call)
        .await
        .map_err(|error| Stop::new(exit::UNAVAILABLE, format!("Error: {error}")))?;
    if reply.status.is_success() {
        return Ok(reply);
    }
    let code = exit::for_status(reply.status);
    if output == Output::Json {
        print_verbatim(io, &reply.body);
        return Err(Stop::new(code, ""));
    }
    let message = match serde_json::from_slice::<Value>(&reply.body) {
        Ok(document) => output::problem(&document),
        Err(_) => format!("Error: the API answered {}\n", reply.status),
    };
    Err(Stop::new(code, message))
}

fn get(path: String) -> Call {
    Call {
        method: Method::GET,
        path,
        body: None,
        idempotency_key: None,
    }
}

fn print_verbatim(io: &mut Io<'_>, body: &[u8]) {
    let _ = io.out.write_all(body);
    if !body.ends_with(b"\n") {
        let _ = writeln!(io.out);
    }
}

fn parse_json(body: &[u8]) -> Result<Value, Stop> {
    serde_json::from_slice(body).map_err(|_| {
        Stop::new(
            exit::FAILURE,
            "Error: the API answered with something other than JSON",
        )
    })
}

/// The incident's id: as given if it is one, else looked up by number in
/// the caller's tenant (ADR 0040).
async fn resolve_id(
    client: &Client,
    incident: &IncidentRef,
    io: &mut Io<'_>,
    output: Output,
) -> Result<String, Stop> {
    let text = incident.0.as_str();
    if is_uuid(text) {
        return Ok(text.to_ascii_lowercase());
    }
    let well_formed = !text.is_empty()
        && text.len() <= 32
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !well_formed {
        return Err(Stop::new(
            exit::USAGE,
            format!(
                "Error: {:?} is neither an incident id nor an incident number\n",
                output::clean(text)
            ),
        ));
    }
    let reply = fetch(
        client,
        get(format!(
            "/api/v1/incidents?incident_number={}&limit=1",
            encode(text)
        )),
        io,
        output,
    )
    .await?;
    let page = parse_json(&reply.body)?;
    page["items"][0]["incident_id"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| {
            Stop::new(
                exit::NOT_FOUND,
                format!("Error: no incident numbered {text}\n"),
            )
        })
}

async fn execute(client: &Client, invocation: &Invocation, io: &mut Io<'_>) -> Result<(), Stop> {
    let output = invocation.global.output;
    let wide = output == Output::Wide;
    match &invocation.command {
        Command::Help | Command::Version => Ok(()),
        Command::List(list) => {
            let reply = fetch(
                client,
                get(format!("/api/v1/incidents{}", query_string(&list.query))),
                io,
                output,
            )
            .await?;
            if output == Output::Json {
                print_verbatim(io, &reply.body);
                return Ok(());
            }
            let page = parse_json(&reply.body)?;
            let _ = write!(
                io.out,
                "{}",
                output::incident_list(&page, wide, io.now_micros)
            );
            if let Some(cursor) = page["next_cursor"].as_str() {
                let _ = writeln!(io.err, "More: add --cursor {cursor}");
            }
            Ok(())
        }
        Command::Show(incident) => {
            let id = resolve_id(client, incident, io, output).await?;
            let reply = fetch(client, get(format!("/api/v1/incidents/{id}")), io, output).await?;
            if output == Output::Json {
                print_verbatim(io, &reply.body);
                return Ok(());
            }
            let incident = parse_json(&reply.body)?;
            let _ = write!(io.out, "{}", output::incident(&incident, io.now_micros));
            Ok(())
        }
        Command::History {
            kind,
            incident,
            query,
        } => {
            let id = resolve_id(client, incident, io, output).await?;
            let path = format!(
                "/api/v1/incidents/{id}/{}{}",
                kind.path(),
                query_string(query)
            );
            let reply = fetch(client, get(path), io, output).await?;
            if output == Output::Json {
                print_verbatim(io, &reply.body);
                return Ok(());
            }
            let page = parse_json(&reply.body)?;
            let _ = write!(io.out, "{}", output::history(*kind, &page, wide));
            if let Some(cursor) = page["next_cursor"].as_str() {
                let _ = writeln!(io.err, "More: add --cursor {cursor}");
            }
            Ok(())
        }
        Command::NoteList(incident) => {
            let id = resolve_id(client, incident, io, output).await?;
            let reply = fetch(
                client,
                get(format!("/api/v1/incidents/{id}/notes")),
                io,
                output,
            )
            .await?;
            if output == Output::Json {
                print_verbatim(io, &reply.body);
                return Ok(());
            }
            let list = parse_json(&reply.body)?;
            let _ = write!(io.out, "{}", output::notes(&list));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(encode("WNM-2026-000123"), "WNM-2026-000123");
        assert_eq!(encode("a&b=c d"), "a%26b%3Dc%20d");
        assert_eq!(
            query_string(&[
                ("state".into(), "open".into()),
                ("cursor".into(), "x/y".into())
            ]),
            "?state=open&cursor=x%2Fy"
        );
    }

    #[test]
    fn ids_are_told_from_numbers() {
        assert!(is_uuid("0192f3c4-8a7b-7e1f-9c2d-3e4f5a6b7c8d"));
        assert!(!is_uuid("WNM-2026-000123"));
        assert!(!is_uuid("0192f3c4x8a7b-7e1f-9c2d-3e4f5a6b7c8d"));
    }
}
