// SPDX-License-Identifier: Apache-2.0

use super::feature_output_bodies;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FaceSelection, Feature, FeatureDefinition, GeneratedFaceRef};
use cadmpeg_ir::ids::{BodyId, FaceId, LoopId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Body, BodyKind, Face, Region, Shell};
use cadmpeg_ir::units::Units;
use std::collections::BTreeMap;

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

#[test]
fn generated_face_outputs_follow_producer_history_after_feature_insertion() {
    let scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty(Units::default());
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#50:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.features.push(Feature::new(
        "creo:model:feature#10".into(),
        0,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Generated {
                faces: vec![GeneratedFaceRef {
                    feature: "creo:model:feature#50".into(),
                    local_id: "surface#7".to_string(),
                }],
                native: "creo:generated-face#7".to_string(),
            },
            thickness: None,
            side: None,
        },
    ));

    assert_eq!(
        feature_output_bodies(&scan, &ir, 10),
        vec![BodyId("creo:feature:extrusion#50:body".to_string())]
    );

    super::super::dependencies::reconcile_feature_links(&scan, &mut ir, &BTreeMap::new());
    assert_eq!(
        ir.model.features[0].outputs,
        vec![BodyId("creo:feature:extrusion#50:body".to_string())]
    );
}

#[test]
fn generated_result_faces_are_outputs_alongside_generated_input_bodies() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 10,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty(Units::default());
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#50:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.bodies.push(Body {
        id: BodyId("creo:generated:result#10".to_string()),
        kind: BodyKind::Sheet,
        regions: vec![RegionId("creo:generated:region#10".to_string())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: RegionId("creo:generated:region#10".to_string()),
        body: BodyId("creo:generated:result#10".to_string()),
        shells: vec![ShellId("creo:generated:shell#10".to_string())],
    });
    ir.model.shells.push(Shell {
        id: ShellId("creo:generated:shell#10".to_string()),
        region: RegionId("creo:generated:region#10".to_string()),
        faces: vec![FaceId("creo:generated:face#7".to_string())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: FaceId("creo:generated:face#7".to_string()),
        shell: ShellId("creo:generated:shell#10".to_string()),
        surface: SurfaceId("creo:visibgeom:surface#7".to_string()),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: vec![LoopId("creo:generated:loop#7".to_string())],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.features.push(Feature::new(
        "creo:model:feature#10".into(),
        0,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Generated {
                faces: vec![GeneratedFaceRef {
                    feature: "creo:model:feature#50".into(),
                    local_id: "surface#7".to_string(),
                }],
                native: "creo:generated-face#7".to_string(),
            },
            thickness: None,
            side: None,
        },
    ));

    assert_eq!(
        feature_output_bodies(&scan, &ir, 10),
        vec![
            BodyId("creo:generated:result#10".to_string()),
            BodyId("creo:feature:extrusion#50:body".to_string()),
        ]
    );
}
