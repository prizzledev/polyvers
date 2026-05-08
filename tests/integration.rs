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

#[test]
fn smoke_unknown_version_error() {
    let err = polyvers::Error::unknown_version("9.9", &["0.1", "0.2"]);
    assert!(err.to_string().contains("9.9"));
}

#[test]
fn parses_v0_1_via_runtime_dispatch() {
    let json = r#"{"some_prop":"x","sub":{"prop":"y"},"another":1}"#;
    let any = the_config::parse_at_version("0.1", json).expect("parses 0.1");
    assert_eq!(any.version(), "0.1");
    let v01 = any.into_v0_1().expect("variant V0_1");
    assert_eq!(v01.some_prop, "x");
    assert_eq!(v01.sub.prop, "y");
    assert_eq!(v01.another, Some(1));
}

#[test]
fn parses_v0_2_via_runtime_dispatch() {
    let json = r#"{"sub":{"prop":42},"another":1,"added":"z"}"#;
    let any = the_config::parse_at_version("0.2", json).expect("parses 0.2");
    assert_eq!(any.version(), "0.2");
    let v02 = any.into_v0_2().expect("variant V0_2");
    assert_eq!(v02.sub.prop, 42);
    assert_eq!(v02.another, Some(1));
    assert_eq!(v02.added, "z");
}

#[test]
fn parses_v0_2_directly_via_serde_json() {
    let json = r#"{"sub":{"prop":7},"another":null,"added":"hi"}"#;
    let v02: the_config::v0_2::Main = serde_json::from_str(json).expect("direct parse");
    assert_eq!(v02.sub.prop, 7);
    assert_eq!(v02.another, None);
    assert_eq!(v02.added, "hi");
}

#[test]
fn round_trip_v0_2() {
    let original = the_config::v0_2::Main {
        sub: the_config::v0_2::JustAStruct { prop: 100 },
        another: Some(5),
        added: "round-trip".into(),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let parsed: the_config::v0_2::Main = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, original);
}

#[test]
fn unknown_version_returns_error() {
    let err = the_config::parse_at_version("9.9", "{}").expect_err("rejected");
    match err {
        polyvers::Error::UnknownVersion { requested, known } => {
            assert_eq!(requested, "9.9");
            assert_eq!(known, &["0.1", "0.2"]);
        }
        _ => panic!("expected UnknownVersion, got {err}"),
    }
}

#[test]
fn malformed_json_returns_format_error() {
    let err = the_config::parse_at_version("0.1", "{not json").expect_err("rejected");
    assert!(matches!(err, polyvers::Error::Format(_)));
}

#[test]
fn latest_alias_points_at_v0_2() {
    let value: the_config::Latest = the_config::v0_2::Main {
        sub: the_config::v0_2::JustAStruct { prop: 0 },
        another: None,
        added: String::new(),
    };
    assert_eq!(value.added, "");
}

#[test]
fn version_constants() {
    assert_eq!(the_config::VERSIONS, &["0.1", "0.2"]);
    assert_eq!(the_config::LATEST_VERSION, "0.2");
}

#[test]
fn sub_struct_resolves_to_correct_version() {
    let v01 = the_config::v0_1::Main {
        some_prop: "a".into(),
        sub: the_config::v0_1::JustAStruct {
            prop: "string".into(),
        },
        another: None,
    };
    let _v02 = the_config::v0_2::Main {
        sub: the_config::v0_2::JustAStruct { prop: 999u64 },
        another: None,
        added: "b".into(),
    };
    assert_eq!(v01.sub.prop, "string");
}

#[test]
fn as_v0_1_returns_some_for_matching_variant() {
    let any = the_config::parse_at_version(
        "0.1",
        r#"{"some_prop":"x","sub":{"prop":"y"},"another":null}"#,
    )
    .expect("ok");
    assert!(any.as_v0_1().is_some());
    assert!(any.as_v0_2().is_none());
}

versioned! {
    family with_attrs;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);

    version "0.1" {
        struct Item {
            #[serde(rename = "kind")]
            type_: String,
            count: u32,
        }
    }

    version "0.2" extends "0.1" {
        struct Item {
            #[edit] count: i64,
        }
    }
}

#[test]
fn serde_rename_attribute_is_forwarded_to_v0_1() {
    let json = r#"{"kind":"foo","count":3}"#;
    let item: with_attrs::v0_1::Item = serde_json::from_str(json).expect("parses");
    assert_eq!(item.type_, "foo");
    assert_eq!(item.count, 3);
}

#[test]
fn serde_rename_attribute_inherits_into_v0_2() {
    let json = r#"{"kind":"bar","count":-7}"#;
    let item: with_attrs::v0_2::Item = serde_json::from_str(json).expect("parses");
    assert_eq!(item.type_, "bar");
    assert_eq!(item.count, -7);
}

versioned! {
    family with_generics;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);

    version "0.1" {
        struct Outer {
            items: Vec<Inner>,
            maybe: Option<Inner>,
        }
        struct Inner {
            id: u32,
        }
    }

    version "0.2" extends "0.1" {
        struct Inner {
            #[add] label: String,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    pub released_iso: String,
    pub author: &'static str,
    pub breaking: bool,
    pub notes: Option<&'static str>,
}

versioned! {
    family release_log;
    derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq);
    meta crate::ReleaseInfo;

    version "0.1" {
        meta {
            released_iso: "2024-01-15T00:00:00Z".to_string(),
            author: "alice",
            breaking: false,
            notes: None,
        }
        struct Main {
            id: u32,
        }
    }

    version "0.2" extends "0.1" {
        meta {
            released_iso: "2024-06-10T00:00:00Z".to_string(),
            author: "bob",
            breaking: true,
            notes: Some("id is now a String UUID"),
        }
        struct Main {
            #[edit] id: String,
        }
    }
}

#[test]
fn meta_per_version_module_function() {
    let m1 = release_log::v0_1::meta();
    assert_eq!(m1.released_iso, "2024-01-15T00:00:00Z");
    assert_eq!(m1.author, "alice");
    assert!(!m1.breaking);

    let m2 = release_log::v0_2::meta();
    assert!(m2.breaking);
    assert_eq!(m2.notes, Some("id is now a String UUID"));
}

#[test]
fn meta_at_version_dispatches_at_runtime() {
    let m = release_log::meta_at_version("0.2").expect("known version");
    assert_eq!(m.author, "bob");
    assert!(release_log::meta_at_version("9.9").is_none());
}

#[test]
fn any_version_meta_method() {
    let any = release_log::parse_at_version("0.1", r#"{"id":7}"#).expect("ok");
    assert_eq!(any.meta().author, "alice");

    let any2 = release_log::parse_at_version("0.2", r#"{"id":"u-1"}"#).expect("ok");
    assert!(any2.meta().breaking);
}

#[test]
fn nested_generic_resolves_to_correct_version() {
    let json_v01 = r#"{"items":[{"id":1},{"id":2}],"maybe":null}"#;
    let outer_v01: with_generics::v0_1::Outer = serde_json::from_str(json_v01).expect("v01");
    assert_eq!(outer_v01.items.len(), 2);
    assert_eq!(outer_v01.items[0].id, 1);
    assert!(outer_v01.maybe.is_none());

    let json_v02 = r#"{"items":[{"id":7,"label":"x"}],"maybe":{"id":9,"label":"y"}}"#;
    let outer_v02: with_generics::v0_2::Outer = serde_json::from_str(json_v02).expect("v02");
    assert_eq!(outer_v02.items[0].id, 7);
    assert_eq!(outer_v02.items[0].label, "x");
    assert_eq!(outer_v02.maybe.unwrap().label, "y");
}
