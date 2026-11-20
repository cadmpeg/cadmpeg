// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::dialect::Admission;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::features::{SheetMetalFlangeWidth, TrimRegion, WrapMode};

/// Each unit arm of an enum tagged by `kind` alone reads its bare tag and
/// refuses every key beside it, whatever the key's name or value.
fn reads_its_tag_alone<T>(kind: &str)
where
    T: Serialize + DeserializeOwned + std::fmt::Debug,
{
    let bare = serde_json::json!({ "kind": kind });
    let read: T = serde_json::from_value(bare.clone()).expect("the bare tag reads");
    assert_eq!(serde_json::to_value(read).unwrap(), bare);

    for (key, value) in [
        ("zz_bogus", serde_json::json!(1)),
        ("value", serde_json::Value::Null),
        ("value", serde_json::json!({})),
    ] {
        let mut stated = bare.clone();
        stated[key] = value;
        let error = serde_json::from_value::<T>(stated.clone())
            .expect_err("a key beside the tag is refused")
            .to_string();
        assert!(
            error.contains(&format!("unknown field `{key}`")),
            "{stated}: {error}"
        );
    }
}

#[test]
fn every_tag_only_unit_arm_refuses_a_key_beside_its_tag() {
    for kind in ["admitted", "residual", "refused"] {
        reads_its_tag_alone::<Admission>(kind);
    }
    reads_its_tag_alone::<WrapMode>("scribe");
    for kind in ["unresolved", "inside", "outside"] {
        reads_its_tag_alone::<TrimRegion>(kind);
    }
    reads_its_tag_alone::<SheetMetalFlangeWidth>("full_edge");
}
