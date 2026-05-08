//! Per-version metadata.
//!
//! `meta MetaType;` at the top of the macro tells polyvers which user-defined
//! struct holds per-version metadata (release date, author, notes — anything).
//! Each `version` block then provides a `meta { … }` initializer. The macro
//! emits `pub fn meta() -> MetaType` per version module, plus
//! `meta_at_version(&str) -> Option<MetaType>` and `AnyVersion::meta()` at the
//! family level.
//!
//! Run with: `cargo run --example version_meta`.

use polyvers::versioned;

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub released_iso: String,
    pub author: &'static str,
    pub breaking: bool,
    pub notes: Option<&'static str>,
}

versioned! {
    family api;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize);
    meta crate::ReleaseInfo;

    version "0.1" {
        meta {
            released_iso: "2024-01-15T00:00:00Z".to_string(),
            author: "alice",
            breaking: false,
            notes: None,
        }
        struct Request {
            id: u32,
            payload: String,
        }
    }

    version "0.2" extends "0.1" {
        meta {
            released_iso: "2024-06-10T00:00:00Z".to_string(),
            author: "bob",
            breaking: true,
            notes: Some("id is now a UUID string"),
        }
        struct Request {
            #[edit] id: String,
            #[add] retries: u32,
        }
    }

    version "0.3" extends "0.2" {
        meta {
            released_iso: "2024-12-01T00:00:00Z".to_string(),
            author: "carol",
            breaking: false,
            notes: Some("payload field is no longer required"),
        }
        struct Request {
            #[edit] payload: Option<String>,
        }
    }
}

fn main() -> Result<(), polyvers::Error> {
    println!("== meta() per version module ==");
    for v in api::VERSIONS {
        let info = api::meta_at_version(v).expect("declared version");
        println!(
            "  {v}: {}{} by {} on {}{}",
            if info.breaking { "BREAKING " } else { "" },
            v,
            info.author,
            info.released_iso,
            info.notes.map(|n| format!(" — {n}")).unwrap_or_default(),
        );
    }

    println!("\n== AnyVersion::meta() during runtime dispatch ==");
    let json = r#"{"id":"u-42","payload":"hi","retries":3}"#;
    let any = api::parse_at_version("0.2", json)?;
    let m = any.meta();
    println!(
        "parsed at {}, released by {} ({})",
        any.version(),
        m.author,
        if m.breaking {
            "breaking change"
        } else {
            "compatible"
        },
    );

    Ok(())
}
