# Changelog

## 0.1.1

### Added

- `codec` clause in the `versioned!` DSL. Accepts any subset of `rkyv`,
  `bincode`, `postcard` (e.g. `codec rkyv, bincode;`).
- Per-codec dispatcher functions emitted at the family level:
  `parse_at_version_rkyv(version, &[u8])`,
  `parse_at_version_bincode(version, &[u8])`,
  `parse_at_version_postcard(version, &[u8])`.
- Per-codec serializer methods on `AnyVersion`: `to_rkyv_bytes`,
  `to_bincode_bytes`, `to_postcard_bytes`.
- Cargo features `rkyv-08`, `rkyv-07`, `bincode-2`, `postcard-1` on the
  `polyvers` crate. They pass through to `polyvers-macros` and gate the
  codegen for each codec. Using `codec X;` without the matching feature
  produces a clear compile-time error.
- `polyvers::Error::format_str(impl Into<String>)` for wrapping codec errors
  whose type does not implement `std::error::Error` (some `rkyv` 0.7 paths).
- `examples/codec_rkyv.rs` runnable example.

### Notes

- The JSON `parse_at_version` dispatcher is still always emitted. A family
  that declares any codec must therefore also derive
  `serde::{Serialize, Deserialize}`. Future work: opt-out via `codec json;`
  so binary-only families can drop serde derives.
- `rkyv-08` and `rkyv-07` are mutually exclusive — enable at most one.

## 0.1.0

Initial release. Single-macro schema versioning via `polyvers::versioned!`:

- `#[add]` / `#[edit]` / `#[delete]` field mutations across `version "…"`
  blocks, with implicit struct inheritance.
- One self-contained module emitted per declared version (`v0_1`, `v0_2`, …),
  so sub-struct types resolve to the correct version automatically — no
  `serde(flatten)` overhead.
- `Latest` type alias, `VERSIONS`/`LATEST_VERSION` constants, `AnyVersion`
  enum with `version()`/`into_vX_Y()`/`as_vX_Y()` helpers.
- `parse_at_version(version, &str) -> Result<AnyVersion, Error>` for runtime
  JSON dispatch.
- Per-version metadata via a `meta TypeName;` clause + `meta { … }` blocks,
  exposed as `vX_Y::meta()`, `meta_at_version(&str)`, and `AnyVersion::meta()`.
- Field-level attributes (e.g. `#[serde(rename = "…")]`) forwarded and
  inherited across versions.
