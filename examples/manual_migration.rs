//! polyvers v0.1.0 does not auto-generate `From` impls between versions.
//! Auto-derived migrations land in v0.2.0 with `#[add(default = ...)]` and
//! `#[edit(from = path)]` attribute extensions. Until then, write the
//! conversion by hand and put it next to the macro invocation.
//!
//! Run with: `cargo run --example manual_migration`.

use polyvers::versioned;

versioned! {
    family settings;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);

    version "0.1" {
        struct Main {
            theme: String,
            font_size_px: u32,
            beta_features: bool,
        }
    }

    version "0.2" extends "0.1" {
        struct Main {
            #[edit]   theme: Theme,
            #[edit]   font_size_px: u16,
            #[delete] beta_features,
            #[add]    color_blind_mode: ColorBlindMode,
        }
        struct Theme {
            name: String,
            dark: bool,
        }
        struct ColorBlindMode {
            enabled: bool,
        }
    }
}

impl From<settings::v0_1::Main> for settings::v0_2::Main {
    fn from(old: settings::v0_1::Main) -> Self {
        // beta_features is dropped; theme becomes a typed Theme; the new
        // color_blind_mode field is filled with a sensible default.
        Self {
            theme: settings::v0_2::Theme {
                name: old.theme,
                dark: false,
            },
            font_size_px: u16::try_from(old.font_size_px).unwrap_or(u16::MAX),
            color_blind_mode: settings::v0_2::ColorBlindMode { enabled: false },
        }
    }
}

fn main() -> Result<(), polyvers::Error> {
    let on_disk = r#"{
        "theme": "solarized-dark",
        "font_size_px": 14,
        "beta_features": true
    }"#;
    let old = settings::parse_at_version("0.1", on_disk)?
        .into_v0_1()
        .expect("v0.1");
    println!("loaded v0.1: {old:?}");

    let migrated: settings::v0_2::Main = old.into();
    println!("\nmigrated to v0.2: {migrated:#?}");

    let written = serde_json::to_string_pretty(&migrated).expect("serialize");
    println!("\nwritten back as v0.2 JSON:\n{written}");
    Ok(())
}
