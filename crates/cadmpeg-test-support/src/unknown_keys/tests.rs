// SPDX-License-Identifier: Apache-2.0

use super::{normalise, object_at_mut, sweep, Step};
use serde::Deserialize;
use serde_json::json;

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
        let swept = sweep(&document, &json!({"items": []}), |probe| {
            Document::deserialize(probe).is_ok()
        })
        .expect("every path comes from the document");
        assert_eq!(
            swept.accepting.into_iter().collect::<Vec<_>>(),
            ["/items/#/inner"],
        );
        assert_eq!(swept.swept, 5);
    }
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
