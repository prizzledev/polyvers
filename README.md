# polyvers

[![crates.io](https://img.shields.io/crates/v/polyvers.svg?label=crates.io&color=fc8d62)](https://crates.io/crates/polyvers)
[![docs.rs](https://img.shields.io/docsrs/polyvers?label=docs)](https://docs.rs/polyvers)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/github/actions/workflow/status/prizzledev/polyvers/rust.yml?label=Rust&logo=github)](https://github.com/prizzledev/polyvers/actions/workflows/rust.yml)

Single-macro schema versioning for Rust.

Declare a struct family once, with `#[add]` / `#[edit]` / `#[delete]` mutations across versions. The macro emits one fully-typed module per version so `serde` deserializes each version in a single pass — **no `serde(flatten)`**, which is known to slow deserialization by 50%+ ([serde-rs/serde#2186](https://github.com/serde-rs/serde/issues/2186), [#2363](https://github.com/serde-rs/serde/issues/2363)).

## Example

```rust
polyvers::versioned! {
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
    // Compile-time-known version: call serde directly.
    let json_v02 = r#"{"sub":{"prop":42},"another":1,"added":"hi"}"#;
    let cfg: the_config::v0_2::Main = serde_json::from_str(json_v02).unwrap();
    assert_eq!(cfg.sub.prop, 42);

    // Runtime-known version: dispatch via parse_at_version.
    let json_v01 = r#"{"some_prop":"x","sub":{"prop":"y"},"another":1}"#;
    let any = the_config::parse_at_version("0.1", json_v01)?;
    assert_eq!(any.version(), "0.1");

    // Latest is a type alias for the most recent version's main struct.
    let _: the_config::Latest = the_config::v0_2::Main {
        sub: the_config::v0_2::JustAStruct { prop: 0 },
        another: None,
        added: String::new(),
    };
    Ok(())
}
```

## Generated API

For `family the_config`, the macro emits:

| Item                              | Description                                                  |
|-----------------------------------|--------------------------------------------------------------|
| `the_config::v0_1::Main` etc.     | One module per version, with all family structs.             |
| `the_config::Latest`              | Type alias for the last version's first struct.              |
| `the_config::AnyVersion`          | Enum with one variant per version wrapping that version's main. |
| `the_config::VERSIONS`            | `&[&str]` of declared version strings.                       |
| `the_config::LATEST_VERSION`      | The last version string.                                     |
| `the_config::parse_at_version(v, json)` | Runtime version dispatch into `AnyVersion` from a JSON string. |

`AnyVersion` exposes `.version()`, `.into_v0_X()`, `.as_v0_X()` per version.

## Binary codecs (rkyv, bincode, postcard)

JSON is the default runtime format. For binary formats, add a `codec` clause and enable the matching cargo feature on `polyvers`. The macro then emits a `parse_at_version_<codec>(version, bytes)` free function and a `to_<codec>_bytes()` method on `AnyVersion`. The JSON dispatcher coexists.

```rust
polyvers::versioned! {
    family payload;
    derive(
        Debug, Clone, serde::Serialize, serde::Deserialize,
        rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
    );
    codec rkyv, bincode, postcard;   // any subset; comma-separated

    version "0.1" { struct Main { id: u32, data: Vec<u8> } }
    version "0.2" extends "0.1" { struct Main { #[edit] id: String } }
}

// Serialize/parse via each enabled codec:
let any = payload::AnyVersion::V0_1(payload::v0_1::Main { id: 7, data: vec![] });
let b = any.to_bincode_bytes()?;
let p = any.to_postcard_bytes()?;
let r = any.to_rkyv_bytes()?;
let same = payload::parse_at_version_rkyv("0.1", &r)?;
```

Cargo features:

| Feature      | Pulls in the user's `…` dependency | Generated fns                                  |
|--------------|------------------------------------|------------------------------------------------|
| `rkyv-08`    | `rkyv = "0.8"`                     | `parse_at_version_rkyv` / `to_rkyv_bytes`      |
| `rkyv-07`    | `rkyv = "0.7"` (legacy API)        | `parse_at_version_rkyv` / `to_rkyv_bytes`      |
| `bincode-2`  | `bincode = "2"` (`serde` feature)  | `parse_at_version_bincode` / `to_bincode_bytes`|
| `postcard-1` | `postcard = "1"` (`alloc` feature) | `parse_at_version_postcard` / `to_postcard_bytes` |

Enable `rkyv-08` xor `rkyv-07` — not both. Using `codec rkyv;` without either feature produces a clear compile-time error pointing at the missing feature.

Because the JSON dispatcher is always emitted, structs in a family that declares any codec must also derive `serde::Serialize` + `serde::Deserialize`. The future plan is to make the JSON dispatcher opt-out via `codec json;` for binary-only families.

## Per-version metadata

Each version can carry its own metadata (release date, author, breaking-change notes, anything you like) without putting those fields on the data struct. Declare a `meta TypeName;` at the top, then provide a `meta { … }` initializer in every version block. The macro emits `pub fn meta() -> TypeName` per version module, plus `meta_at_version(&str)` and `AnyVersion::meta()` at the family level.

```rust
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub released_iso: String,
    pub author: &'static str,
    pub breaking: bool,
}

polyvers::versioned! {
    family api;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize);
    meta crate::ReleaseInfo;

    version "0.1" {
        meta {
            released_iso: "2024-01-15T00:00:00Z".to_string(),
            author: "alice",
            breaking: false,
        }
        struct Main { id: u32 }
    }

    version "0.2" extends "0.1" {
        meta {
            released_iso: "2024-06-10T00:00:00Z".to_string(),
            author: "bob",
            breaking: true,
        }
        struct Main { #[edit] id: String }
    }
}

let info = api::v0_2::meta();          // from the version module
let info = api::meta_at_version("0.1") // by version string at runtime
    .expect("known version");
let any = api::parse_at_version("0.2", json)?;
let info = any.meta();                 // from a parsed AnyVersion
```

The metadata type is yours — any struct, any fields, any derives, including types like `chrono::DateTime<Utc>` that aren't `const`-constructible (the generated `meta()` is a regular function, so non-const expressions work).

If you declare `meta TypeName;` at the top, every version must provide a `meta { … }` block; if you don't, no version may. Both are caught at compile time.

## Mutation grammar

Inside a non-base version block, every field line must use one of:

- `#[add] name: Type` — introduce a new field. Errors if the field already exists in the parent.
- `#[edit] name: NewType` — change an existing field's type. Errors if the field doesn't exist.
- `#[delete] name` — remove a field (bare identifier, no type). Errors if the field doesn't exist.

Field-level attributes that aren't `#[add]/#[edit]/#[delete]` (e.g., `#[serde(rename = "...")]`) are forwarded onto generated fields and inherited across versions.

## Version-id mangling

Version literals become module/method names by replacing `.` with `_` and prefixing `v`:

| Literal      | Module     | Variant     |
|--------------|------------|-------------|
| `"0.1"`      | `v0_1`     | `V0_1`      |
| `"0.1.0"`    | `v0_1_0`   | `V0_1_0`    |
| `"1.0-rc1"`  | `v1_0__rc1`| `V1_0__rc1` |

Two literals that mangle to the same identifier (e.g., `"0.1"` and `"0_1"`) are rejected at compile time.

## Limitations (v0.1.1)

- JSON is always emitted; binary codecs are additive via the `codec` clause + cargo feature. TOML/YAML may come later.
- No automatic migrations between versions. (`From<v0_1::Main> for v0_2::Main` is on the v0.2.0 roadmap with `#[add(default = ...)]` and `#[edit(from = path)]` hints.)
- No generic type parameters on family structs.
- No visibility modifiers on fields — all generated fields are `pub`.
- Family struct types must be referenced by bare name inside the macro. Path-qualified references (`v0_1::JustAStruct`) are not rewritten across versions.
- Do not declare `type` aliases inside the macro body.

## Examples

Runnable examples live in [`examples/`](examples). Run any of them with `cargo run --example <name>`:

- [`basic`](examples/basic.rs) — the canonical 2-version family with both compile-time and runtime parsing.
- [`three_versions`](examples/three_versions.rs) — three versions showing every mutation kind, including a sub-struct that evolves alongside.
- [`manual_migration`](examples/manual_migration.rs) — implementing `From<v0_1::Main> for v0_2::Main` by hand (auto-derived migrations land in v0.2.0).
- [`runtime_dispatch`](examples/runtime_dispatch.rs) — reading the version out of a JSON file and dispatching across `AnyVersion`.
- [`version_meta`](examples/version_meta.rs) — attaching per-version metadata (release date, author, notes) via the `meta` clause.
- [`codec_rkyv`](examples/codec_rkyv.rs) — round-tripping versioned data through rkyv. Run with `cargo run --features rkyv-08 --example codec_rkyv`.

## License

Licensed under the [MIT License](LICENSE-MIT).