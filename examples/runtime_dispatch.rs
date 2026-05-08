//! Read the version out of an input string before parsing into the right
//! type. v0.1.0 does not auto-detect a `version` field — auto-detection lands
//! in a later release behind `#[polyvers(version_field = "version")]`. Until
//! then, two lines of serde do the job.
//!
//! Run with: `cargo run --example runtime_dispatch`.

use polyvers::versioned;
use serde::Deserialize;

versioned! {
    family event;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize);

    version "1.0" {
        struct Main {
            id: u64,
            name: String,
        }
    }

    version "1.1" extends "1.0" {
        struct Main {
            #[add] tags: Vec<String>,
        }
    }

    version "2.0" extends "1.1" {
        struct Main {
            #[edit] id: String,
        }
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    version: String,
}

fn parse_auto(input: &str) -> Result<event::AnyVersion, polyvers::Error> {
    let probe: VersionProbe = serde_json::from_str(input).map_err(polyvers::Error::format)?;
    event::parse_at_version(&probe.version, input)
}

fn main() -> Result<(), polyvers::Error> {
    let inputs = [
        r#"{"version":"1.0","id":7,"name":"hello"}"#,
        r#"{"version":"1.1","id":8,"name":"with-tags","tags":["a","b"]}"#,
        r#"{"version":"2.0","id":"550e8400-e29b-41d4-a716-446655440000","name":"uuid","tags":[]}"#,
    ];

    for input in inputs {
        let any = parse_auto(input)?;
        match any {
            event::AnyVersion::V1_0(e) => {
                println!("[1.0] id={} name={}", e.id, e.name);
            }
            event::AnyVersion::V1_1(e) => {
                println!("[1.1] id={} name={} tags={:?}", e.id, e.name, e.tags);
            }
            event::AnyVersion::V2_0(e) => {
                println!("[2.0] id={:?} name={} tags={:?}", e.id, e.name, e.tags);
            }
        }
    }

    let bogus = r#"{"version":"99.9","id":0,"name":"nope"}"#;
    match parse_auto(bogus) {
        Err(polyvers::Error::UnknownVersion { requested, known }) => {
            println!("\nas expected, unknown version `{requested}`: known = {known:?}");
        }
        other => panic!("expected UnknownVersion, got {other:?}"),
    }
    Ok(())
}
