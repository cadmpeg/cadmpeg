// SPDX-License-Identifier: Apache-2.0

use super::feature_output_bodies;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::units::Units;

#[test]
fn generated_edge_outputs_follow_producer_history_before_ir_feature_insertion() {
    let feature_row = |feature_id| crate::feature::FeatureRow {
        feature_id,
        header: [0xeb, 0x04],
        root_schema_class: None,
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    };
    let curve_row = |id, feature_id| crate::curve::CurveTopologyRow {
        id,
        type_byte: 8,
        feature_id,
        directions: [1, 0xf6],
        faces: [10, 11],
        next_edges: [id, id],
        offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .rows
        .extend([feature_row(50), feature_row(70)]);
    scan.features.affected_ids.extend([
        crate::feature::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: vec![45],
            offset: 0,
        },
        crate::feature::FeatureAffectedIds {
            feature_id: 50,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: vec![60],
            offset: 0,
        },
    ]);
    scan.curves
        .topology_rows
        .extend([curve_row(45, 50), curve_row(60, 70)]);

    let mut ir = CadIr::empty(Units::default());
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#70:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    assert_eq!(
        feature_output_bodies(&scan, &ir, 10),
        vec![BodyId("creo:feature:extrusion#70:body".to_string())]
    );
}
