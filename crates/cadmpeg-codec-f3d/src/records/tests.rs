// SPDX-License-Identifier: Apache-2.0

use super::{DesignFeatureKind, DesignParameterScope};

fn empty_scope(kind: DesignFeatureKind) -> serde_json::Value {
    serde_json::to_value(DesignParameterScope::empty("scope", kind, 1)).expect("serialize scope")
}

#[test]
fn flattened_scope_payloads_propagate_invalid_field_errors() {
    for (kind, field) in [
        (DesignFeatureKind::Extrude, "extrude_prologue"),
        (DesignFeatureKind::CoilPrimitive, "coil_extent"),
        (DesignFeatureKind::BaseFlange, "base_flange_operation"),
        (DesignFeatureKind::Loft, "path_feature_construction"),
    ] {
        let mut wire = empty_scope(kind);
        wire[field] = serde_json::json!(17);
        assert!(serde_json::from_value::<DesignParameterScope>(wire).is_err());
    }
}

#[test]
fn flattened_scope_frames_reject_partial_value_offset_pairs() {
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for (kind, prefix) in [
        (DesignFeatureKind::WorkPlane, "work_plane"),
        (DesignFeatureKind::JointOrigin, "joint_origin"),
    ] {
        for (suffix, value) in [
            ("transform", transform.clone()),
            ("transform_offset", serde_json::json!(10)),
            ("reference", serde_json::json!(2)),
            ("reference_offset", serde_json::json!(20)),
        ] {
            let field = format!("{prefix}_{suffix}");
            let mut wire = empty_scope(kind.clone());
            wire[&field] = value;
            let error = serde_json::from_value::<DesignParameterScope>(wire)
                .expect_err("partial frame must fail");
            assert!(error.to_string().contains(prefix));
        }
        let mut wire = empty_scope(kind);
        wire[format!("{prefix}_transform")] = transform.clone();
        wire[format!("{prefix}_transform_offset")] = serde_json::json!(10);
        wire[format!("{prefix}_reference")] = serde_json::json!(2);
        wire[format!("{prefix}_reference_offset")] = serde_json::json!(20);
        let decoded: DesignParameterScope = serde_json::from_value(wire.clone()).expect("complete frame");
        assert_eq!(serde_json::to_value(decoded).expect("serialize frame"), wire);
    }
}

#[test]
fn flattened_sketch_entity_requires_all_identity_fields() {
    for (field, value) in [
        ("entity_id", serde_json::json!("entity:2")),
        ("entity_suffix", serde_json::json!(2)),
        ("entity_reference_offset", serde_json::json!(20)),
    ] {
        let mut wire = empty_scope(DesignFeatureKind::Sketch);
        wire[field] = value;
        let error = serde_json::from_value::<DesignParameterScope>(wire)
            .expect_err("partial identity must fail");
        assert!(error.to_string().contains("entity_id"));
    }
}

#[test]
fn absent_flattened_scope_payloads_preserve_the_wire() {
    for kind in [
        DesignFeatureKind::Extrude,
        DesignFeatureKind::CoilPrimitive,
        DesignFeatureKind::BaseFlange,
        DesignFeatureKind::Loft,
        DesignFeatureKind::Sweep,
        DesignFeatureKind::WorkPlane,
        DesignFeatureKind::JointOrigin,
        DesignFeatureKind::Sketch,
    ] {
        let scope = DesignParameterScope::empty("scope", kind, 1);
        let wire = serde_json::to_string(&scope).expect("serialize scope");
        let decoded: DesignParameterScope = serde_json::from_str(&wire).expect("empty payload");
        assert_eq!(serde_json::to_string(&decoded).expect("serialize scope"), wire);
    }
}
