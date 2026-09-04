// SPDX-License-Identifier: Apache-2.0

use super::{
    bodies_containing_edges, evaluated_sweep_body_kind, evaluated_sweep_output_bodies,
    feature_output_bodies,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FaceSelection, Feature, FeatureDefinition, GeneratedFaceRef};
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Face, Loop as IrLoop, Region, Sense, Shell};
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
        id: BodyId::mint("creo:feature:extrusion#70:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    assert_eq!(
        feature_output_bodies(&scan, &ir, 10),
        vec![BodyId::mint("creo:feature:extrusion#70:body".to_string()).expect("identity grammar")]
    );
}

#[test]
fn generated_face_outputs_follow_producer_history_after_feature_insertion() {
    let scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
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
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    super::super::dependencies::reconcile_feature_links(&scan, &mut ir, &BTreeMap::new());
    assert_eq!(
        ir.model.features[0].outputs,
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
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
        id: BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
        kind: BodyKind::Sheet,
        regions: vec![
            RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        body: BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
        shells: vec![
            ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar")
        ],
    });
    ir.model.shells.push(Shell {
        id: ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        region: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        faces: vec![FaceId::mint("creo:generated:face#7".to_string()).expect("identity grammar")],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: FaceId::mint("creo:generated:face#7".to_string()).expect("identity grammar"),
        shell: ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        surface: SurfaceId::mint("creo:visibgeom:surface#7".to_string()).expect("identity grammar"),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: vec![LoopId::mint("creo:generated:loop#7".to_string()).expect("identity grammar")]
            .into(),
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
            BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
            BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        ]
    );

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(Shell {
        id: ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        region: RegionId::mint("creo:ambiguous:region#10".to_string()).expect("identity grammar"),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    assert_eq!(
        feature_output_bodies(&scan, &duplicate_shell, 10),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        body: BodyId::mint("creo:ambiguous:body#10".to_string()).expect("identity grammar"),
        shells: Vec::new(),
    });
    assert_eq!(
        feature_output_bodies(&scan, &duplicate_region, 10),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    let mut duplicate_feature = ir.clone();
    let feature = duplicate_feature.model.features[0].clone();
    duplicate_feature.model.features.push(feature);
    assert_eq!(
        feature_output_bodies(&scan, &duplicate_feature, 10),
        vec![BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar")]
    );
}

#[test]
fn edge_output_joins_reject_duplicate_topology_owners() {
    let body_id = BodyId::mint("creo:test:body".to_string()).expect("identity grammar");
    let region_id = RegionId::mint("creo:test:region".to_string()).expect("identity grammar");
    let shell_id = ShellId::mint("creo:test:shell".to_string()).expect("identity grammar");
    let face_id = FaceId::mint("creo:test:face".to_string()).expect("identity grammar");
    let loop_id = LoopId::mint("creo:test:loop".to_string()).expect("identity grammar");
    let coedge_id = CoedgeId::mint("creo:test:coedge".to_string()).expect("identity grammar");
    let edge_id = EdgeId::mint("creo:test:edge".to_string()).expect("identity grammar");
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
        surface: SurfaceId::mint("creo:test:surface".to_string()).expect("identity grammar"),
        sense: Sense::Forward,
        loops: vec![loop_id.clone()].into(),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.loops.push(IrLoop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: vec![coedge_id.clone()],
            vertex_uses: Vec::new(),
        },
    });
    ir.model.coedges.push(Coedge {
        id: coedge_id.clone(),
        owner_loop: loop_id.clone(),
        edge: edge_id.clone(),
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
        face: FaceId::mint("creo:ambiguous:face".to_string()).expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        },
    });
    assert!(bodies_containing_edges(&duplicate_loop, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_face = ir.clone();
    duplicate_face.model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId::mint("creo:ambiguous:shell".to_string()).expect("identity grammar"),
        surface: SurfaceId::mint("creo:test:surface-2".to_string()).expect("identity grammar"),
        sense: Sense::Forward,
        loops: Vec::new().into(),
        name: None,
        color: None,
        tolerance: None,
    });
    assert!(bodies_containing_edges(&duplicate_face, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(Shell {
        id: shell_id.clone(),
        region: RegionId::mint("creo:ambiguous:region".to_string()).expect("identity grammar"),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    assert!(bodies_containing_edges(&duplicate_shell, std::slice::from_ref(&edge_id)).is_empty());

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: region_id.clone(),
        body: BodyId::mint("creo:ambiguous:body".to_string()).expect("identity grammar"),
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
        id: BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert_eq!(
        evaluated_sweep_output_bodies(&ir, 40),
        vec![BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar")]
    );
    assert_eq!(
        evaluated_sweep_body_kind(&ir, "extrusion", 40),
        Some(BodyKind::Solid)
    );

    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar"),
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
