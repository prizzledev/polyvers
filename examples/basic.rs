//! Canonical 2-version example. Run with: `cargo run --example basic`.

use polyvers::versioned;

versioned! {
    family the_config;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);

    version "0.1" {
        struct Main {
            some_prop: String,
            sub: JustAStruct,
            another: Option<usize>,
        }
        struct JustAStruct {
            prop: String,
        }
    }

    version "0.2" extends "0.1" {
        struct Main {
            #[add]    added: String,
            #[edit]   sub: JustAStruct,
            #[delete] some_prop,
        }
        struct JustAStruct {
            #[edit] prop: u64,
        }
    }
}

fn main() -> Result<(), polyvers::Error> {
    let json_v02 = r#"{"sub":{"prop":42},"another":1,"added":"hi"}"#;
    let cfg: the_config::v0_2::Main = serde_json::from_str(json_v02).expect("parse v0.2");
    println!("v0.2 parsed (compile-time-known): {cfg:?}");
    println!(
        "  sub.prop = {} (was String in v0.1, now u64)",
        cfg.sub.prop
    );
    println!("  added    = {:?} (introduced in v0.2)", cfg.added);

    let json_v01 = r#"{"some_prop":"x","sub":{"prop":"y"},"another":1}"#;
    let any = the_config::parse_at_version("0.1", json_v01)?;
    println!("\nruntime-dispatched: version = {}", any.version());
    if let Some(v01) = any.as_v0_1() {
        println!(
            "  some_prop = {:?} (no longer present in v0.2)",
            v01.some_prop
        );
    }

    println!("\nVERSIONS       = {:?}", the_config::VERSIONS);
    println!("LATEST_VERSION = {:?}", the_config::LATEST_VERSION);

    let _: the_config::Latest = the_config::v0_2::Main {
        sub: the_config::v0_2::JustAStruct { prop: 0 },
        another: None,
        added: String::new(),
    };
    Ok(())
}
