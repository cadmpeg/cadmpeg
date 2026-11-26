// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use super::{normalise, object_at_mut, probe_key, sweep, Probed, Step, UNKNOWN_KEY};
use cadmpeg_core::json;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(rename = "items")]
    _items: Vec<Branch>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Branch {
    Strict {
        #[serde(rename = "inner")]
        _inner: StrictValue,
    },
    Loose {
        #[serde(rename = "inner")]
        _inner: LooseValue,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictValue {
    #[serde(rename = "value")]
    _value: u32,
}

#[derive(Deserialize)]
struct LooseValue {
    #[serde(rename = "value")]
    _value: u32,
}

#[test]
fn equal_child_keys_under_different_parent_variants_are_both_probed() {
    for kinds in [["strict", "loose"], ["loose", "strict"]] {
        let document = json!({"items": [
            {"kind": kinds[0], "inner": {"value": 1}},
            {"kind": kinds[1], "inner": {"value": 2}},
        ]});
        let swept = sweep(
            &document,
            &json!({"items": []}),
            |probe| Document::deserialize(probe).is_ok(),
            &mut Probed::default(),
        )
        .expect("every path comes from the document");
        assert_eq!(
            swept.accepting.into_iter().collect::<Vec<_>>(),
            ["/items/#/inner"],
        );
        assert_eq!(swept.swept, 5);
    }
}

#[test]
fn a_graft_an_earlier_document_produced_is_not_read_again() {
    let first = json!({"items": [{"kind": "strict", "inner": {"value": 1}}]});
    let second = json!({"items": [
        {"kind": "strict", "inner": {"value": 1}},
        {"kind": "loose", "inner": {"value": 2}},
    ]});
    let reads = Cell::new(0_usize);
    let accepts = |probe: &Value| {
        reads.set(reads.get() + 1);
        Document::deserialize(probe).is_ok()
    };
    let empty = json!({"items": []});
    let fresh = sweep(&second, &empty, accepts, &mut Probed::default())
        .expect("every path comes from the document");
    let fresh_reads = reads.replace(0);

    let mut probed = Probed::default();
    sweep(&first, &empty, accepts, &mut probed).expect("every path comes from the document");
    reads.set(0);
    let shared =
        sweep(&second, &empty, accepts, &mut probed).expect("every path comes from the document");

    assert_eq!(shared.accepting, fresh.accepting);
    assert_eq!(
        shared.accepting.into_iter().collect::<Vec<_>>(),
        ["/items/#/inner"],
    );
    assert!(
        reads.get() < fresh_reads,
        "the strict element was read again: {} reads, {fresh_reads} without the earlier document",
        reads.get()
    );
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NestedDocument {
    #[serde(rename = "meta")]
    _meta: Option<Meta>,
    #[serde(rename = "items")]
    _items: Vec<StrictValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "extra")]
    _extra: Vec<LooseValue>,
}

#[test]
fn an_element_the_empty_document_cannot_hold_is_probed_in_its_member() {
    let document = json!({
        "meta": {"name": "a", "extra": [{"value": 1}]},
        "items": [{"value": 2}],
    });
    let swept = sweep(
        &document,
        &json!({"meta": null, "items": []}),
        |probe| NestedDocument::deserialize(probe).is_ok(),
        &mut Probed::default(),
    )
    .expect("every path comes from the document");
    assert_eq!(
        swept.accepting.into_iter().collect::<Vec<_>>(),
        ["/meta/extra/#"],
    );
    assert_eq!(swept.swept, 4);
    assert_eq!(swept.fallbacks, 0);
}

#[test]
fn missing_probe_target_is_an_error_instead_of_a_skipped_probe() {
    let mut document = json!({"items": [null]});
    for path in [
        vec![Step::Key("missing".into())],
        vec![Step::Key("items".into()), Step::Index(1)],
        vec![Step::Key("items".into()), Step::Index(0)],
    ] {
        assert!(object_at_mut(&mut document, &path).is_err());
    }
}

#[test]
fn source_member_names_cannot_alias_path_separators() {
    assert_eq!(
        normalise(&[Step::Key("a/b~c".into()), Step::Index(0)]),
        "/a~1b~0c/#",
    );
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingSentinelDocument {
    #[serde(rename = "items")]
    _items: Vec<ExistingSentinelItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingSentinelItem {
    #[serde(rename = "value")]
    _value: u32,
    #[serde(rename = "zz_bogus")]
    _sentinel: u32,
}

#[test]
fn an_existing_sentinel_member_is_preserved_while_probing() {
    let document = json!({"items": [{"value": 1, "zz_bogus": 7}]});
    assert!(
        ExistingSentinelDocument::deserialize(&document).is_ok(),
        "the sentinel fixture must be admitted before the sweep runs"
    );
    let swept = sweep(
        &document,
        &json!({"items": []}),
        |probe| ExistingSentinelDocument::deserialize(probe).is_ok(),
        &mut Probed::default(),
    )
    .expect("every path comes from the document");

    assert!(
        swept.accepting.is_empty(),
        "the extra suffixed key is denied"
    );
}

#[test]
fn probe_key_uses_the_first_absent_suffix() {
    let fields = Map::from_iter([
        (UNKNOWN_KEY.to_owned(), Value::Null),
        (format!("{UNKNOWN_KEY}_"), Value::Null),
    ]);
    assert_eq!(probe_key(&fields), format!("{UNKNOWN_KEY}__"));
}
