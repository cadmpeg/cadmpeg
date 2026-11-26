use super::{DesignEdgeFlangeScope, DesignReferenceEdgeFlange, DesignReferenceFlangeEdge};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use serde_json::json;

#[test]
fn reference_edge_wire_retains_independent_controls() {
    let wire = json!({
        "bend_position": {"value": 2, "offset": 20},
        "radius_control": {"value": 0.25, "offset": 80},
        "edges": [{"group": {"value": 11, "offset": 100}}]
    });
    let construction: DesignReferenceEdgeFlange =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).expect("partial source controls");
    assert!(construction.height_owner.is_none());
    assert_eq!(
        construction
            .radius_control
            .as_ref()
            .expect("positive control")
            .value
            .get(),
        0.25
    );
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(construction).expect("source controls wire"),
        wire
    );
    for key in [
        "bend_position",
        "edge_count",
        "height_datum",
        "radius_control",
        "angle_owner",
        "height_owner",
        "settings",
        "aggregate_group",
        "result_count",
    ] {
        let mut invalid = json!({});
        invalid[key] = serde_json::Value::Null;
        let error =
            cadmpeg_core::serde_erase::json::from_value::<DesignReferenceEdgeFlange>(invalid)
                .expect_err("null source control")
                .to_string();
        assert!(error.contains(key), "{error}");
    }
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<DesignReferenceFlangeEdge>(
            json!({"first_owner":null})
        )
        .is_err()
    );
}

#[test]
fn reference_edge_scope_wire_keeps_one_source_form() {
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#0", DesignFeatureKind::EdgeFlange, 7);
    if let DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
        *slot = DesignEdgeFlangeScope::ReferenceFramed(DesignReferenceEdgeFlange::default());
    }
    let wire =
        cadmpeg_core::serde_erase::json::to_value(&scope).expect("reference edge scope wire");
    assert_eq!(wire["edge_flange_construction"], json!({}));
    assert!(wire.get("edge_flange_operation").is_none());
    let decoded: DesignParameterScope =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).expect("reference scope");
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(decoded).expect("reference scope wire"),
        wire
    );
    let mut both = wire;
    both["edge_flange_operation"] = json!({
        "edge_wrapper_record_indices":[11],
        "edge_group_record_indices":[12],
        "edge_operand_record_indices":[15],
        "aggregate_group_record_index":20,
        "aggregate_operand_record_indices":[23],
        "height_owner_record_index":30,
        "height_extent":{"kind":"distance"},
        "angle_owner_record_index":33,
        "width_mode":"full_edge",
        "width_distance_owner_record_indices":[],
        "width_parameter_source":"edge_width",
        "settings_record_index":40,
        "bend_radius":0.25,
        "bend_radius_offset":50,
        "reference_side_code":4,
        "height_datum":"inner_faces",
        "bend_position":"inside"
    });
    let error = cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(both)
        .expect_err("two source forms")
        .to_string();
    assert!(error.contains("more than one payload family"), "{error}");
}
