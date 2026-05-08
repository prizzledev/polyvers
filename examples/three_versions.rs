//! Three-version family showing every mutation kind, plus a sub-struct
//! introduced mid-evolution. Run with: `cargo run --example three_versions`.

use polyvers::versioned;

versioned! {
    family api_request;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);

    version "0.1" {
        struct Request {
            user: String,
            payload: Payload,
            legacy_token: String,
        }
        struct Payload {
            kind: String,
            data: Vec<u8>,
        }
    }

    version "0.2" extends "0.1" {
        struct Request {
            #[delete] legacy_token,
            #[add]    idempotency_key: String,
        }
        struct Payload {
            #[edit] kind: PayloadKind,
        }
        struct PayloadKind {
            tag: String,
            optional_arg: Option<String>,
        }
    }

    version "0.3" extends "0.2" {
        struct Request {
            #[add] timing_us: Option<u64>,
        }
        struct Payload {
            #[edit] data: String,
        }
    }
}

fn main() -> Result<(), polyvers::Error> {
    let v01_input = r#"{
        "user": "alice",
        "payload": {"kind": "ping", "data": [1, 2, 3]},
        "legacy_token": "abc"
    }"#;
    let v01 = api_request::parse_at_version("0.1", v01_input)?
        .into_v0_1()
        .expect("v0.1");
    println!("v0.1 = {v01:?}");

    let v02_input = r#"{
        "user": "bob",
        "payload": {
            "kind": {"tag": "echo", "optional_arg": "x"},
            "data": [9, 9, 9]
        },
        "idempotency_key": "req-1"
    }"#;
    let v02 = api_request::parse_at_version("0.2", v02_input)?
        .into_v0_2()
        .expect("v0.2");
    println!("\nv0.2 = {v02:?}");

    let v03_input = r#"{
        "user": "carol",
        "payload": {
            "kind": {"tag": "echo", "optional_arg": null},
            "data": "AAEC"
        },
        "idempotency_key": "req-42",
        "timing_us": 1234
    }"#;
    let v03 = api_request::parse_at_version("0.3", v03_input)?
        .into_v0_3()
        .expect("v0.3");
    println!("\nv0.3 = {v03:?}");

    println!("\nfamily knows: {:?}", api_request::VERSIONS);
    Ok(())
}
