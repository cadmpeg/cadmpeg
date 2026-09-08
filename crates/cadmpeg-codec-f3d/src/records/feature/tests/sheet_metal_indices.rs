// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::{
    DesignEdgeFlangeEdge, DesignEdgeFlangeOperation, DesignEdgeFlangeSelection,
    DesignEdgeFlangeShape, DesignHemOperation,
};

#[test]
fn hem_operand_indices_derive_from_groups_and_reject_wire_disagreement() {
    let wire = serde_json::json!({
        "edge_wrapper_record_index":1, "edge_group_record_index":2, "edge_operand_record_index":5,
        "aggregate_group_record_index":6, "aggregate_operand_record_index":9,
        "parameter_owners":{"kind":"gap_length", "gap_owner_record_index":10, "length_owner_record_index":11},
        "settings_record_index":12, "bend_radius":0.25, "bend_radius_offset":100,
        "form_code":3, "direction_code":1, "direction_reversal_byte":0, "reference_side_code":4
    });
    let mut operation: DesignHemOperation = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&operation).unwrap(), wire);
    operation.edge_group_record_index = 20;
    operation.aggregate_group_record_index = 30;
    assert_eq!(operation.edge_operand_record_index(), 23);
    assert_eq!(operation.aggregate_operand_record_index(), 33);
    let changed = serde_json::to_value(operation).unwrap();
    assert_eq!(changed["edge_operand_record_index"], 23);
    assert_eq!(changed["aggregate_operand_record_index"], 33);
    for field in [
        "edge_operand_record_index",
        "aggregate_operand_record_index",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(99);
        assert!(serde_json::from_value::<DesignHemOperation>(invalid).is_err());
    }
}

#[test]
fn flange_selection_couples_single_edge_aggregate_and_preserves_multiple_operands() {
    let mut edge = DesignEdgeFlangeEdge {
        wrapper_record_index: 10,
        group_record_index: 20,
        aggregate_operand_record_index: 33,
    };
    assert_eq!(edge.operand_record_index(), 23);
    edge.group_record_index = 25;
    assert_eq!(edge.operand_record_index(), 28);
    assert!(DesignEdgeFlangeSelection::try_new(
        DesignEdgeFlangeShape::FullEdge {
            edges: vec![edge],
            height: crate::records::feature::DesignEdgeFlangeHeightExtent::Distance
        },
        30
    )
    .is_ok());
    edge.aggregate_operand_record_index = 34;
    assert!(DesignEdgeFlangeSelection::try_new(
        DesignEdgeFlangeShape::FullEdge {
            edges: vec![edge],
            height: crate::records::feature::DesignEdgeFlangeHeightExtent::Distance
        },
        30
    )
    .is_err());
    assert!(DesignEdgeFlangeSelection::try_new(
        DesignEdgeFlangeShape::FullEdge {
            edges: vec![
                edge,
                DesignEdgeFlangeEdge {
                    wrapper_record_index: 11,
                    group_record_index: 26,
                    aggregate_operand_record_index: 35
                }
            ],
            height: crate::records::feature::DesignEdgeFlangeHeightExtent::Distance
        },
        30
    )
    .is_ok());
    let wire = serde_json::json!({
        "edge_wrapper_record_indices":[10], "edge_group_record_indices":[20], "edge_operand_record_indices":[23],
        "aggregate_group_record_index":30, "aggregate_operand_record_indices":[33],
        "height_owner_record_index":40, "angle_owner_record_index":41, "width_mode":"full_edge",
        "width_distance_owner_record_indices":[], "settings_record_index":42, "bend_radius":0.25,
        "bend_radius_offset":50, "reference_side_code":4, "height_datum":"inner_faces", "bend_position":"adjacent"
    });
    assert!(serde_json::from_value::<DesignEdgeFlangeOperation>(wire.clone()).is_ok());
    for field in [
        "edge_operand_record_indices",
        "aggregate_operand_record_indices",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!([99]);
        assert!(serde_json::from_value::<DesignEdgeFlangeOperation>(invalid).is_err());
    }
    let mut multiple = wire;
    multiple["edge_wrapper_record_indices"] = serde_json::json!([10, 11]);
    multiple["edge_group_record_indices"] = serde_json::json!([20, 21]);
    multiple["edge_operand_record_indices"] = serde_json::json!([23, 24]);
    multiple["aggregate_operand_record_indices"] = serde_json::json!([34, 35]);
    assert!(serde_json::from_value::<DesignEdgeFlangeOperation>(multiple).is_ok());
}
