//! Tests for the `parameters` module.

use super::native_scalar_matches_discrete_parameter;
use std::collections::BTreeMap;

#[test]
fn native_scalar_must_match_an_existing_discrete_parameter() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Pattern".into(),
        kind: "Pattern".into(),
        input_class: Some("moLPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    assert!(native_scalar_matches_discrete_parameter(
        &feature, "D1", "15", 15.0
    ));
    assert!(!native_scalar_matches_discrete_parameter(
        &feature,
        "D1",
        "15",
        8.371_160_993_642_741e298
    ));
}
