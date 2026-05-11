# Changelog

## 0.1.2

### Added

- `pub const FIELD_COUNT: usize` emitted inside every per-version module,
  carrying the field count of the family's main struct. Downstream parsers
  can dispatch on the on-wire length without redeclaring the count
  alongside the macro invocation.
- Auto-`impl From<v_prev::Struct> for v_curr::Struct` for every shared
  struct on every `#[add]`-only hop. The codegen bridges family-internal
  types across the version boundary (`.into()` for direct references,
  `.map(Into::into)` for `Option<T>`, `.into_iter().map(Into::into).collect()`
  for `Vec<T>`). Hops with `#[edit]` / `#[delete]` are deliberately skipped;
  the user writes a manual `From` and the chain picks it up.
- `pub fn AnyVersion::into_latest(self) -> Latest` emitted when the main
  struct's chain is fully auto-emittable from end to end. Walks the
  auto-generated `From` impls one hop at a time, so v0.1 → v0.3 routes
  through the intermediate v0.2 step.
- `#[add(default = <expr>)]` syntax. The expression is used verbatim to
  populate the added field in the auto-`From` migration. Without the
  attribute, the codegen falls back to `::core::default::Default::default()`
  — so `Added` field types need `Default` (typically satisfied by adding
  `Default` to the family `derive(...)` list).

### Notes

- Auto-`From` for a struct is only emitted when every family struct it
  transitively references is also auto-emittable on the same hop (computed
  by a fixed-point pass). This guarantees the carried `.into()` calls
  never dangle.
- The `into_latest` emission is gated on the *main* struct's entire chain
  being clean. If any hop has a `#[edit]` / `#[delete]` on the main
  struct, `into_latest` is not emitted and the user writes their own.

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
