//! The command line, parsed by hand (ADR 0040).
//!
//! `wetechinetmonctl [GLOBAL FLAGS] incidents <verb> [INCIDENT] [FLAGS]`.
//! Flags may sit anywhere after the program name. An unknown flag, a
//! missing value or a stray argument is a usage error, never ignored: a
//! misspelled flag must fail loudly, not run a different command than the
//! operator believes.

/// How results are printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Aligned columns for people.
    Table,
    /// `Table` with more columns.
    Wide,
    /// The API's response body, verbatim.
    Json,
}

/// Flags that apply to every command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    pub output: Output,
    pub profile: Option<String>,
}

/// An incident as the operator named it: its UUID or its number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentRef(pub String);

/// `incidents list` filters, passed to the API as query parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListArgs {
    /// `(name, value)` query pairs, in the order given.
    pub query: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    List(ListArgs),
    Show(IncidentRef),
    /// A paged history, with `--limit` and `--cursor` passed through.
    History {
        kind: History,
        incident: IncidentRef,
        query: Vec<(String, String)>,
    },
    NoteList(IncidentRef),
}

/// The paged histories of one incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    Timeline,
    Detections,
    Audit,
}

impl History {
    pub fn path(self) -> &'static str {
        match self {
            History::Timeline => "timeline",
            History::Detections => "detections",
            History::Audit => "audit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub global: Global,
    pub command: Command,
}

/// Why the command line was refused. The text is for the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

fn usage(text: impl Into<String>) -> UsageError {
    UsageError(text.into())
}

pub const USAGE: &str = "\
Usage: wetechinetmonctl [--output table|wide|json] [--profile NAME] incidents <verb> ...

Reading:
  incidents list [--state S] [--severity S] [--priority P] [--direction D]
                 [--target-type T] [--sort opened_at|last_detected_at]
                 [--order desc|asc] [--limit N] [--cursor C]
                 [--opened-from TIME --opened-to TIME]
  incidents show INCIDENT
  incidents timeline INCIDENT [--limit N] [--cursor C]
  incidents detections INCIDENT [--limit N] [--cursor C]
  incidents audit INCIDENT [--limit N] [--cursor C]
  incidents note list INCIDENT

INCIDENT is an incident id or number, such as WNM-2026-000123.
--state, --severity and --priority repeat, or take a comma-separated list.

Credentials are never flags. Set WETECHINETMON_API_TOKEN, or a profile in
the config file (WETECHINETMON_CONFIG). See apps/cli/README.md.
";

/// Flags that take a value, and the `list` query parameter each becomes.
const LIST_FLAGS: &[(&str, &str, bool)] = &[
    // (flag, query parameter, may repeat as a comma-separated list)
    ("--state", "state", true),
    ("--severity", "severity", true),
    ("--priority", "priority", true),
    ("--direction", "direction", false),
    ("--target-type", "target_type", false),
    ("--sort", "sort", false),
    ("--order", "order", false),
    ("--limit", "limit", false),
    ("--cursor", "cursor", false),
    ("--opened-from", "opened_from", false),
    ("--opened-to", "opened_to", false),
];

/// Names that look like credentials. Refused with a pointer to the
/// environment, because flags land in shell history.
const CREDENTIAL_FLAGS: &[&str] = &["--token", "--api-token", "--password", "--secret"];

pub fn parse(args: &[String]) -> Result<Invocation, UsageError> {
    let mut global = Global {
        output: Output::Table,
        profile: None,
    };
    let mut words: Vec<String> = Vec::new();
    let mut flags: Vec<(String, String)> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        if CREDENTIAL_FLAGS.contains(&name) {
            return Err(usage(format!(
                "{name} is not accepted: credentials on the command line land in shell \
                 history. Set WETECHINETMON_API_TOKEN or use a profile."
            )));
        }
        let mut value = |name: &str| -> Result<String, UsageError> {
            match &inline {
                Some(value) => Ok(value.clone()),
                None => rest
                    .next()
                    .cloned()
                    .ok_or_else(|| usage(format!("{name} needs a value"))),
            }
        };
        match name {
            "-h" | "--help" => {
                return Ok(Invocation {
                    global,
                    command: Command::Help,
                })
            }
            "--version" => {
                return Ok(Invocation {
                    global,
                    command: Command::Version,
                })
            }
            "-o" | "--output" => {
                global.output = match value(name)?.as_str() {
                    "table" => Output::Table,
                    "wide" => Output::Wide,
                    "json" => Output::Json,
                    other => {
                        return Err(usage(format!(
                            "--output is table, wide or json, not {other:?}"
                        )))
                    }
                }
            }
            "--profile" => global.profile = Some(value(name)?),
            flag if flag.starts_with('-') && flag != "-" => {
                let Some((_, query, _)) = LIST_FLAGS.iter().find(|(f, _, _)| *f == flag) else {
                    return Err(usage(format!("unknown flag {flag}")));
                };
                flags.push(((*query).to_string(), value(name)?));
            }
            word => {
                if inline.is_some() {
                    return Err(usage(format!("unexpected argument {arg}")));
                }
                words.push(word.to_string());
            }
        }
    }

    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let command = match words.as_slice() {
        [] => Command::Help,
        ["incidents", "list"] => {
            let mut query = Vec::new();
            for (name, value) in flags.drain(..) {
                let repeats = LIST_FLAGS.iter().any(|(_, q, r)| *q == name && *r);
                if repeats {
                    for one in value.split(',').filter(|v| !v.is_empty()) {
                        query.push((name.clone(), one.to_string()));
                    }
                } else {
                    query.push((name, value));
                }
            }
            Command::List(ListArgs { query })
        }
        ["incidents", "show", id] => Command::Show(IncidentRef((*id).to_string())),
        ["incidents", verb @ ("timeline" | "detections" | "audit"), id] => {
            let kind = match *verb {
                "timeline" => History::Timeline,
                "detections" => History::Detections,
                _ => History::Audit,
            };
            let mut query = Vec::new();
            for (name, value) in std::mem::take(&mut flags) {
                if name != "limit" && name != "cursor" {
                    return Err(usage(format!(
                        "--{} does not apply to incidents {verb}",
                        name.replace('_', "-")
                    )));
                }
                query.push((name, value));
            }
            Command::History {
                kind,
                incident: IncidentRef((*id).to_string()),
                query,
            }
        }
        ["incidents", "note", "list", id] => Command::NoteList(IncidentRef((*id).to_string())),
        ["incidents"] => return Err(usage("incidents needs a verb; see --help")),
        ["incidents", verb, ..] => {
            return Err(usage(format!(
                "incidents {verb}: unknown verb, or the wrong number of arguments; see --help"
            )))
        }
        [other, ..] => return Err(usage(format!("unknown command {other}; see --help"))),
    };
    if let Some((name, _)) = flags.first() {
        return Err(usage(format!(
            "--{} only applies to incidents list",
            name.replace('_', "-")
        )));
    }
    Ok(Invocation { global, command })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(line: &str) -> Result<Invocation, UsageError> {
        let args: Vec<String> = line.split_whitespace().map(String::from).collect();
        parse(&args)
    }

    fn reference(id: &str) -> IncidentRef {
        IncidentRef(id.to_string())
    }

    #[test]
    fn every_read_verb_parses() {
        let cases = [
            (
                "incidents show WNM-2026-000123",
                Command::Show(reference("WNM-2026-000123")),
            ),
            (
                "incidents timeline X --limit 5",
                Command::History {
                    kind: History::Timeline,
                    incident: reference("X"),
                    query: vec![("limit".into(), "5".into())],
                },
            ),
            (
                "incidents detections X",
                Command::History {
                    kind: History::Detections,
                    incident: reference("X"),
                    query: vec![],
                },
            ),
            (
                "incidents audit X --cursor c1",
                Command::History {
                    kind: History::Audit,
                    incident: reference("X"),
                    query: vec![("cursor".into(), "c1".into())],
                },
            ),
            ("incidents note list X", Command::NoteList(reference("X"))),
            ("", Command::Help),
            ("--help", Command::Help),
            ("--version", Command::Version),
        ];
        for (line, command) in cases {
            assert_eq!(run(line).unwrap().command, command, "{line}");
        }
    }

    #[test]
    fn list_flags_become_query_pairs_and_lists_split() {
        let parsed = run("incidents list --state open,acknowledged --state resolved --severity=critical --limit 5")
            .unwrap();
        let Command::List(list) = parsed.command else {
            panic!("not a list")
        };
        let pairs: Vec<(&str, &str)> = list
            .query
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("state", "open"),
                ("state", "acknowledged"),
                ("state", "resolved"),
                ("severity", "critical"),
                ("limit", "5")
            ]
        );
    }

    #[test]
    fn global_flags_may_sit_anywhere() {
        let parsed = run("incidents show X -o json --profile prod").unwrap();
        assert_eq!(parsed.global.output, Output::Json);
        assert_eq!(parsed.global.profile.as_deref(), Some("prod"));
        assert_eq!(
            run("--output=wide incidents list").unwrap().global.output,
            Output::Wide
        );
    }

    #[test]
    fn mistakes_are_refused_not_ignored() {
        for line in [
            "incidents list --stat open",
            "incidents show",
            "incidents show A B",
            "incidents frobnicate X",
            "incidents",
            "alerts list",
            "incidents show X --state open",
            "incidents timeline X --state open",
            "incidents list --limit",
            "-o yaml incidents list",
        ] {
            assert!(run(line).is_err(), "{line} should be refused");
        }
    }

    #[test]
    fn credentials_are_never_flags() {
        for line in [
            "incidents list --token wnm_x",
            "--api-token=wnm_x incidents list",
        ] {
            let error = run(line).unwrap_err();
            assert!(error.0.contains("WETECHINETMON_API_TOKEN"), "{}", error.0);
            assert!(!error.0.contains("wnm_x"), "the secret is never echoed");
        }
    }
}
