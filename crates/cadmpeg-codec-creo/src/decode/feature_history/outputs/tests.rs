// SPDX-License-Identifier: Apache-2.0

use super::{
    bodies_containing_edges, evaluated_sweep_body_kind, evaluated_sweep_output_bodies,
    feature_output_bodies,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FaceSelection, Feature, FeatureDefinition, GeneratedFaceRef};
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Face, Loop as IrLoop, LoopBoundaryRole, Region, Sense, Shell,
};
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

    let mut ir = CadIr::empty();
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
    let mut ir = CadIr::empty();
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
    let mut ir = CadIr::empty();
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

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(Shell {
        id: ShellId("creo:generated:shell#10".to_string()),
        region: RegionId("creo:ambiguous:region#10".to_string()),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    assert_eq!(
        feature_output_bodies(&scan, &duplicate_shell, 10),
        vec![BodyId("creo:feature:extrusion#50:body".to_string())]
    );

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: RegionId("creo:generated:region#10".to_string()),
        body: BodyId("creo:ambiguous:body#10".to_string()),
        shells: Vec::new(),
    });
    assert_eq!(
        feature_output_bodies(&scan, &duplicate_region, 10),
        vec![BodyId("creo:feature:extrusion#50:body".to_string())]
    );

    let mut duplicate_feature = ir.clone();
    let feature = duplicate_feature.model.features[0].clone();
    duplicate_feature.model.features.push(feature);
    assert_eq!(
        feature_output_bodies(&scan, &duplicate_feature, 10),
        vec![BodyId("creo:generated:result#10".to_string())]
    );
}

#[test]
fn edge_output_joins_reject_duplicate_topology_owners() {
    let body_id = BodyId("creo:test:body".to_string());
    let region_id = RegionId("creo:test:region".to_string());
    let shell_id = ShellId("creo:test:shell".to_string());
    let face_id = FaceId("creo:test:face".to_string());
    let loop_id = LoopId("creo:test:loop".to_string());
    let coedge_id = CoedgeId("creo:test:coedge".to_string());
    let edge_id = EdgeId("creo:test:edge".to_string());
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Solid,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: vec![face_id.clone()],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: SurfaceId("creo:test:surface".to_string()),
        sense: Sense::Forward,
        loops: vec![loop_id.clone()],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.loops.push(IrLoop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary_role: LoopBoundaryRole::default(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: vec![coedge_id.clone()],
            vertex_uses: Vec::new(),
        },
    });
    ir.model.coedges.push(Coedge {
        id: coedge_id.clone(),
        owner_loop: loop_id.clone(),
        edge: edge_id.clone(),
        next: coedge_id.clone(),
        previous: coedge_id.clone(),
        radial_next: coedge_id,
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    assert_eq!(
        bodies_containing_edges(&ir, std::slice::from_ref(&edge_id)),
        vec![body_id.clone()]
    );

    let mut duplicate_loop = ir.clone();
    duplicate_loop.model.loops.push(IrLoop {
        id: loop_id.clone(),
        face: FaceId("creo:ambiguous:face".to_string()),
        boundary_role: LoopBoundaryRole::default(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        },
    });
    assert!(bodies_containing_edges(&duplicate_loop, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_face = ir.clone();
    duplicate_face.model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId("creo:ambiguous:shell".to_string()),
        surface: SurfaceId("creo:test:surface-2".to_string()),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    });
    assert!(bodies_containing_edges(&duplicate_face, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(Shell {
        id: shell_id.clone(),
        region: RegionId("creo:ambiguous:region".to_string()),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    assert!(bodies_containing_edges(&duplicate_shell, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: region_id.clone(),
        body: BodyId("creo:ambiguous:body".to_string()),
        shells: Vec::new(),
    });
    assert!(bodies_containing_edges(&duplicate_region, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_body = ir;
    duplicate_body.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert!(bodies_containing_edges(&duplicate_body, std::slice::from_ref(&edge_id)).is_empty());
}

#[test]
fn evaluated_sweep_body_joins_reject_duplicate_ids() {
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#40:body".to_string()),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert_eq!(
        evaluated_sweep_output_bodies(&ir, 40),
        vec![BodyId("creo:feature:extrusion#40:body".to_string())]
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "extrusion", 40),
        Some(BodyKind::Solid)
    );

    ir.model.bodies.push(Body {
        id: BodyId("creo:feature:extrusion#40:body".to_string()),
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert!(evaluated_sweep_output_bodies(&ir, 40).is_empty());
    assert_eq!(evaluated_sweep_body_kind(&ir, "extrusion", 40), None);
}
