//! Round-tripping versioned data through rkyv.
//!
//! Run with: `cargo run --features rkyv-08 --example codec_rkyv`.
//!
//! The macro emits `parse_at_version_rkyv(version, &[u8]) -> AnyVersion` and
//! `AnyVersion::to_rkyv_bytes() -> Vec<u8>` when the family declares
//! `codec rkyv;` and the `polyvers/rkyv-08` (or `rkyv-07`) cargo feature is
//! enabled. The JSON dispatcher is emitted alongside, so the family also
//! derives serde traits.

#[cfg(not(any(feature = "rkyv-08", feature = "rkyv-07")))]
fn main() {
    eprintln!(
        "This example requires the `rkyv-08` (or `rkyv-07`) feature. Run with:\n  \
         cargo run --features rkyv-08 --example codec_rkyv"
    );
    std::process::exit(2);
}

#[cfg(any(feature = "rkyv-08", feature = "rkyv-07"))]
use polyvers::versioned;

#[cfg(any(feature = "rkyv-08", feature = "rkyv-07"))]
versioned! {
    family snapshot;
    derive(
        Debug,
        Clone,
        PartialEq,
        serde::Serialize,
        serde::Deserialize,
        rkyv::Archive,
        rkyv::Serialize,
        rkyv::Deserialize,
    );
    codec rkyv;

    version "0.1" {
        struct Main {
            tick: u64,
            label: String,
        }
    }

    version "0.2" extends "0.1" {
        struct Main {
            #[add] checksum: u64,
        }
    }
}

#[cfg(any(feature = "rkyv-08", feature = "rkyv-07"))]
fn main() -> Result<(), polyvers::Error> {
    let original = snapshot::AnyVersion::V0_2(snapshot::v0_2::Main {
        tick: 100,
        label: "release".into(),
        checksum: 0xCAFE_BABE,
    });

    let bytes = original.to_rkyv_bytes()?;
    println!("serialized {} bytes via rkyv", bytes.len());

    let parsed = snapshot::parse_at_version_rkyv("0.2", &bytes)?;
    println!("parsed at {}", parsed.version());

    let inner = parsed.into_v0_2().expect("v0_2 variant");
    println!("checksum: {:#x}", inner.checksum);

    // JSON dispatcher still works alongside the rkyv one — coexistence is the
    // current policy. Same family, same versions, two formats.
    let json = serde_json::to_string(&inner).expect("json");
    let any = snapshot::parse_at_version("0.2", &json)?;
    println!("re-parsed via JSON at {}", any.version());

    Ok(())
}
