//! Per-codec round-trip tests. Each module is gated by its feature flag so the
//! suite compiles cleanly without any codec features enabled and exercises the
//! full dispatcher when one is.

#[cfg(feature = "bincode-2")]
mod bincode_codec {
    use polyvers::versioned;

    versioned! {
        family payload;
        derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);
        codec bincode;

        version "0.1" {
            struct Main {
                id: u32,
                name: String,
            }
        }

        version "0.2" extends "0.1" {
            struct Main {
                #[edit] id: String,
                #[add]  retries: u32,
            }
        }
    }

    #[test]
    fn round_trip_at_each_version() {
        let v01 = payload::AnyVersion::V0_1(payload::v0_1::Main {
            id: 1,
            name: "alice".into(),
        });
        let bytes = v01.to_bincode_bytes().expect("serialize v0.1");
        let parsed = payload::parse_at_version_bincode("0.1", &bytes).expect("parse v0.1");
        assert_eq!(parsed.version(), "0.1");
        let inner = parsed.into_v0_1().expect("v0_1 variant");
        assert_eq!(inner.id, 1);
        assert_eq!(inner.name, "alice");

        let v02 = payload::AnyVersion::V0_2(payload::v0_2::Main {
            name: "bob".into(),
            id: "abc".into(),
            retries: 9,
        });
        let bytes = v02.to_bincode_bytes().expect("serialize v0.2");
        let parsed = payload::parse_at_version_bincode("0.2", &bytes).expect("parse v0.2");
        assert_eq!(parsed.version(), "0.2");
        let inner = parsed.into_v0_2().expect("v0_2 variant");
        assert_eq!(inner.id, "abc");
        assert_eq!(inner.retries, 9);
    }

    #[test]
    fn unknown_version_via_bincode_dispatcher() {
        let err = payload::parse_at_version_bincode("9.9", &[]).expect_err("rejected");
        assert!(matches!(err, polyvers::Error::UnknownVersion { .. }));
    }

    #[test]
    fn json_dispatcher_still_works_alongside_bincode() {
        // codec bincode; must coexist with the JSON dispatcher.
        let json = r#"{"id":7,"name":"carol"}"#;
        let any = payload::parse_at_version("0.1", json).expect("parse JSON");
        assert_eq!(any.version(), "0.1");
    }
}

#[cfg(feature = "postcard-1")]
mod postcard_codec {
    use polyvers::versioned;

    versioned! {
        family msg;
        derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);
        codec postcard;

        version "0.1" {
            struct Main {
                kind: u8,
                payload: Vec<u8>,
            }
        }

        version "0.2" extends "0.1" {
            struct Main {
                #[add] flags: u32,
            }
        }
    }

    #[test]
    fn round_trip_at_each_version() {
        let v01 = msg::AnyVersion::V0_1(msg::v0_1::Main {
            kind: 1,
            payload: vec![1, 2, 3, 4, 5],
        });
        let bytes = v01.to_postcard_bytes().expect("serialize v0.1");
        let parsed = msg::parse_at_version_postcard("0.1", &bytes).expect("parse v0.1");
        let inner = parsed.into_v0_1().expect("v0_1 variant");
        assert_eq!(inner.kind, 1);
        assert_eq!(inner.payload, vec![1, 2, 3, 4, 5]);

        let v02 = msg::AnyVersion::V0_2(msg::v0_2::Main {
            kind: 2,
            payload: vec![],
            flags: 0xDEAD_BEEF,
        });
        let bytes = v02.to_postcard_bytes().expect("serialize v0.2");
        let parsed = msg::parse_at_version_postcard("0.2", &bytes).expect("parse v0.2");
        let inner = parsed.into_v0_2().expect("v0_2 variant");
        assert_eq!(inner.flags, 0xDEAD_BEEF);
    }

    #[test]
    fn unknown_version_via_postcard_dispatcher() {
        let err = msg::parse_at_version_postcard("9.9", &[]).expect_err("rejected");
        assert!(matches!(err, polyvers::Error::UnknownVersion { .. }));
    }
}

#[cfg(feature = "rkyv-08")]
mod rkyv_codec {
    use polyvers::versioned;

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

    #[test]
    fn round_trip_at_each_version() {
        let v01 = snapshot::AnyVersion::V0_1(snapshot::v0_1::Main {
            tick: 42,
            label: "first".into(),
        });
        let bytes = v01.to_rkyv_bytes().expect("serialize v0.1");
        let parsed = snapshot::parse_at_version_rkyv("0.1", &bytes).expect("parse v0.1");
        let inner = parsed.into_v0_1().expect("v0_1 variant");
        assert_eq!(inner.tick, 42);
        assert_eq!(inner.label, "first");

        let v02 = snapshot::AnyVersion::V0_2(snapshot::v0_2::Main {
            tick: 100,
            label: "second".into(),
            checksum: 0xCAFEBABE,
        });
        let bytes = v02.to_rkyv_bytes().expect("serialize v0.2");
        let parsed = snapshot::parse_at_version_rkyv("0.2", &bytes).expect("parse v0.2");
        let inner = parsed.into_v0_2().expect("v0_2 variant");
        assert_eq!(inner.checksum, 0xCAFEBABE);
    }

    #[test]
    fn unknown_version_via_rkyv_dispatcher() {
        let err = snapshot::parse_at_version_rkyv("9.9", &[]).expect_err("rejected");
        assert!(matches!(err, polyvers::Error::UnknownVersion { .. }));
    }

    #[test]
    fn corrupt_bytes_return_format_error() {
        let err = snapshot::parse_at_version_rkyv("0.1", &[0, 1, 2, 3]).expect_err("rejected");
        assert!(matches!(err, polyvers::Error::Format(_)));
    }
}

// Multi-codec on a single family.
#[cfg(all(feature = "bincode-2", feature = "postcard-1"))]
mod multi_codec {
    use polyvers::versioned;

    versioned! {
        family multi;
        derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);
        codec bincode, postcard;

        version "0.1" {
            struct Main { a: u32 }
        }
    }

    #[test]
    fn both_dispatchers_emitted() {
        let v = multi::AnyVersion::V0_1(multi::v0_1::Main { a: 7 });
        let b = v.to_bincode_bytes().unwrap();
        let p = v.to_postcard_bytes().unwrap();
        assert!(!b.is_empty());
        assert!(!p.is_empty());
        let parsed_b = multi::parse_at_version_bincode("0.1", &b).unwrap();
        let parsed_p = multi::parse_at_version_postcard("0.1", &p).unwrap();
        assert_eq!(parsed_b.into_v0_1().unwrap().a, 7);
        assert_eq!(parsed_p.into_v0_1().unwrap().a, 7);
    }
}
