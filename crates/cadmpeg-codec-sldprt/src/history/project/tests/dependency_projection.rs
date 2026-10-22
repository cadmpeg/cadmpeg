// SPDX-License-Identifier: Apache-2.0
//! Native authored dependency text does not replace actual construction operands.
#![allow(clippy::unwrap_used)]

use crate::history::tests::feature;
use crate::records::FeatureHistory;
use std::collections::BTreeMap;

#[test]
fn actual_profile_reference_wins_over_stale_authored_dependency_edges() {
    let mut first = feature("first", Some("1"), 0);
    first.xml_tag = "Sketch".into();
    first.kind = "Sketch".into();
    let mut second = feature("second", Some("2"), 1);
    second.xml_tag = "Sketch".into();
    second.kind = "Sketch".into();
    let mut extrusion = feature("extrusion", Some("3"), 2);
    extrusion.xml_tag = "Extrusion".into();
    extrusion.kind = "BossExtrude".into();
    extrusion
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Depth"), "4mm".into());
    extrusion.properties = BTreeMap::from([
        (cadmpeg_core::nonblank_literal!("Profile"), "1".into()),
        (
            cadmpeg_core::nonblank_literal!("EndCondition"),
            "Blind".into(),
        ),
        (
            cadmpeg_core::nonblank_literal!("Dependencies"),
            "2,2".into(),
        ),
    ]);
    let mut fillet = feature("round", Some("4"), 3);
    fillet.xml_tag = "Fillet".into();
    fillet.kind = "Fillet".into();
    fillet
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Radius"), "2mm".into());
    fillet.properties.insert(
        cadmpeg_core::nonblank_literal!("Dependencies"),
        "1,2,1".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second, extrusion, fillet],
    };
    let projected = super::super::project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history),
    )
    .unwrap();
    assert_eq!(
        projected[2].dependencies.as_slice(),
        &[projected[0].id.clone()]
    );
    assert!(projected[3].dependencies.is_empty());
    assert_eq!(history.features[2].properties["Dependencies"], "2,2");
    assert_eq!(history.features[3].properties["Dependencies"], "1,2,1");
}

#[test]
fn dependency_projection_preserves_the_callers_recursion_refusal() {
    let mut sketch = feature("sketch", Some("1"), 0);
    sketch.xml_tag = "Sketch".into();
    sketch.kind = "Sketch".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![sketch],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::project_features(&ctx, &[history]),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}
