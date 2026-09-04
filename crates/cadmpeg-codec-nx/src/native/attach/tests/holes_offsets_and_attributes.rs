// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::{
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{BodyId, SurfaceId};

use crate::test_support::*;

use super::*;

fn insert_test_procedural_surface(
    ir: &mut cadmpeg_ir::document::CadIr,
    owner: SurfaceId,
    procedural: ProceduralSurface,
) {
    ir.model.surfaces.push(Surface {
        id: owner.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.add_procedural_surface(owner, procedural).unwrap();
}

fn attach_test_body_procedural_surface(
    ir: &mut cadmpeg_ir::document::CadIr,
    body: &BodyId,
    owner: SurfaceId,
    procedural: ProceduralSurface,
) {
    attach_test_body_surface(ir, body, owner.clone());
    insert_test_procedural_surface(ir, owner, procedural);
}

fn attribute_field_name(
    topology_reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
    value_use: &str,
    class_uses: &[crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &[crate::native::parasolid::ParasolidAttributeDefinition],
    field_uses: &[crate::native::parasolid::ParasolidAttributeFieldUse],
    field_names: &[crate::native::parasolid::ParasolidAttributeFieldNames],
) -> Option<String> {
    super::ParasolidAttributeNameIndex::new(class_uses, definitions, field_uses, field_names)
        .field_name(topology_reference, value_use)
}

#[test]
fn nx_blind_hole_projection_requires_a_unique_cap_and_entry_direction() {
    use crate::native::features::{
        FeatureSimpleHoleTemplate, SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily,
        SimpleHoleForm,
    };
    use cadmpeg_ir::document::{CadIr, Model};
    use cadmpeg_ir::features::{
        FeatureDefinition, HoleKind, HolePlacement, Length, LinearTermination,
    };
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Region, Sense, Shell};

    let operation = "blind".to_string();
    let template = FeatureSimpleHoleTemplate {
        id: "template-blind".into(),
        operation_label: operation.clone(),
        payload_string: "payload-blind".into(),
        family: SimpleHoleFamily::GeneralHole,
        form: SimpleHoleForm::Simple,
        extent: SimpleHoleExtent::Blind,
        start_treatment: SimpleHoleEndTreatment::None,
        end_treatment: SimpleHoleEndTreatment::None,
    };
    let mut model = Model::default();
    let cylinder_surface = SurfaceId("blind-cylinder-surface".into());
    let cap_surface = SurfaceId("blind-cap-surface".into());
    model.surfaces.push(Surface {
        id: cylinder_surface.clone(),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    model.surfaces.push(Surface {
        id: cap_surface.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let (entry_loop, cylinder_cap_loop, cap_face_loop) = {
        let mut add_circle_loop =
            |loop_name: &str, edge_name: &str, center: Point3, radius: f64| {
                let loop_id = LoopId(loop_name.into());
                let edge_id = EdgeId(edge_name.into());
                let curve_id = CurveId(format!("{edge_name}-curve"));
                if !model.edges.iter().any(|edge| edge.id == edge_id) {
                    model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: CurveGeometry::Circle {
                            center,
                            axis: Vector3::new(0.0, 0.0, 1.0),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            radius,
                        },
                        source_object: None,
                    });
                    model.edges.push(Edge {
                        id: edge_id.clone(),
                        curve: Some(curve_id),
                        start: VertexId("vertex".into()),
                        end: VertexId("vertex".into()),
                        param_range: None,
                        tolerance: None,
                    });
                }
                let coedge_id = CoedgeId(format!("{loop_name}-coedge"));
                model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
                    next: coedge_id.clone(),
                    previous: coedge_id.clone(),
                    radial_next: coedge_id,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: None,
                });
                loop_id
            };
        (
            add_circle_loop(
                "blind-entry-loop",
                "blind-entry-edge",
                Point3::new(0.0, 0.0, 0.0),
                2.0,
            ),
            add_circle_loop(
                "blind-cylinder-cap-loop",
                "blind-cap-edge",
                Point3::new(0.0, 0.0, 3.0),
                2.0,
            ),
            add_circle_loop(
                "blind-cap-face-loop",
                "blind-cap-edge",
                Point3::new(0.0, 0.0, 3.0),
                2.0,
            ),
        )
    };
    let cylinder_face = FaceId("blind-cylinder-face".into());
    let cap_face = FaceId("blind-cap-face".into());
    model.faces.push(Face {
        id: cylinder_face.clone(),
        shell: ShellId("blind-shell".into()),
        surface: cylinder_surface,
        sense: Sense::Reversed,
        loops: vec![entry_loop, cylinder_cap_loop],
        name: None,
        color: None,
        tolerance: None,
    });
    model.faces.push(Face {
        id: cap_face.clone(),
        shell: ShellId("blind-shell".into()),
        surface: cap_surface,
        sense: Sense::Forward,
        loops: vec![cap_face_loop],
        name: None,
        color: None,
        tolerance: None,
    });
    let body = BodyId("blind-body".into());
    model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![RegionId("blind-region".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    model.regions.push(Region {
        id: RegionId("blind-region".into()),
        body: body.clone(),
        shells: vec![ShellId("blind-shell".into())],
    });
    model.shells.push(Shell {
        id: ShellId("blind-shell".into()),
        region: RegionId("blind-region".into()),
        faces: vec![cylinder_face, cap_face],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    let mut ir = CadIr::empty();
    ir.model = model;
    let operation_positions = BTreeMap::from([("blind", 0usize)]);
    assert_eq!(
        super::blind_hole_operations(std::slice::from_ref(&template), &operation_positions),
        Some(vec![operation.clone()]),
    );
    let outputs = BTreeMap::from([(operation.clone(), vec![body.clone()])]);
    let projection =
        super::blind_hole_body_projection(&ir, std::slice::from_ref(&operation), &outputs)
            .expect("complete blind-bore witness");
    assert_eq!(projection.outputs, outputs);
    assert_eq!(
        projection.diameters,
        BTreeMap::from([(operation.clone(), Length(4.0))])
    );
    assert_eq!(
        projection.blind_depths,
        BTreeMap::from([(operation.clone(), Length(3.0))])
    );
    assert_eq!(
        super::blind_hole_axis_placements_for_operations(
            &ir,
            std::slice::from_ref(&operation),
            &outputs,
        ),
        BTreeMap::from([(
            operation.clone(),
            HolePlacement::Directed {
                position: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            },
        )])
    );
    let definition = super::non_boolean_feature_definition_with_parameters(
        "SIMPLE HOLE",
        &["Hole_GeneralHole_Simple_Blind"],
        None,
        None,
        super::HoleProjection {
            placements: vec![HolePlacement::Directed {
                position: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            }],
            diameter: Some(Length(4.0)),
            extent: Some(LinearTermination::Blind {
                length: Length(3.0),
            }),
            ..super::HoleProjection::default()
        },
        BTreeMap::new(),
    );
    assert!(matches!(
        definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            },
            diameter: Some(Length(4.0)),
            extent: Some(LinearTermination::Blind { length: Length(3.0) }),
            placements,
            ..
        } if placements.as_deref() == Some(&[HolePlacement::Directed {
            position: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        }][..])
    ));

    let mut missing_cap = ir.clone();
    missing_cap.model.shells[0]
        .faces
        .retain(|face| face != &FaceId("blind-cap-face".into()));
    assert!(super::blind_hole_body_projection(
        &missing_cap,
        std::slice::from_ref(&operation),
        &outputs,
    )
    .is_none());
    let mut duplicate_cap = ir.clone();
    duplicate_cap.model.faces.push(Face {
        id: FaceId("blind-duplicate-cap-face".into()),
        shell: ShellId("blind-shell".into()),
        surface: SurfaceId("blind-cap-surface".into()),
        sense: Sense::Forward,
        loops: vec![LoopId("blind-cap-face-loop".into())],
        name: None,
        color: None,
        tolerance: None,
    });
    duplicate_cap.model.shells[0]
        .faces
        .push(FaceId("blind-duplicate-cap-face".into()));
    assert!(super::blind_hole_body_projection(
        &duplicate_cap,
        std::slice::from_ref(&operation),
        &outputs,
    )
    .is_none());
    let mut sheet = ir.clone();
    sheet.model.bodies[0].kind = BodyKind::Sheet;
    assert!(
        super::blind_hole_body_projection(&sheet, std::slice::from_ref(&operation), &outputs,)
            .is_none()
    );
    assert!(super::blind_hole_body_projection(
        &ir,
        &[operation.clone(), "second-operation".into()],
        &BTreeMap::from([
            (operation, vec![body.clone()]),
            ("second-operation".into(), vec![body]),
        ]),
    )
    .is_none());
}

#[test]
fn nx_counterbore_projection_requires_a_coaxial_pair_and_shoulder() {
    use crate::native::features::{
        FeatureSimpleHoleTemplate, SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily,
        SimpleHoleForm,
    };
    use cadmpeg_ir::document::{CadIr, Model};
    use cadmpeg_ir::features::{FeatureDefinition, HoleKind, HolePlacement, Length};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Region, Sense, Shell};

    let operation = "counterbore".to_string();
    let template = FeatureSimpleHoleTemplate {
        id: "template-counterbore".into(),
        operation_label: operation.clone(),
        payload_string: "payload-counterbore".into(),
        family: SimpleHoleFamily::GeneralHole,
        form: SimpleHoleForm::Counterbored,
        extent: SimpleHoleExtent::Through,
        start_treatment: SimpleHoleEndTreatment::None,
        end_treatment: SimpleHoleEndTreatment::None,
    };
    assert_eq!(
        super::counterbore_operations(
            std::slice::from_ref(&template),
            &BTreeMap::from([("counterbore", 0usize)]),
        ),
        Some(vec![operation.clone()]),
    );
    let competing_template = FeatureSimpleHoleTemplate {
        form: SimpleHoleForm::Simple,
        ..template.clone()
    };
    assert!(super::counterbore_operations(
        &[template.clone(), competing_template],
        &BTreeMap::from([("counterbore", 0usize)]),
    )
    .is_none());
    let mut model = Model::default();
    let mut add_circle_loop =
        |name: &str, shared_edge: Option<&str>, center: Point3, radius: f64| {
            let loop_id = LoopId(format!("{name}-loop"));
            let edge_name = shared_edge.unwrap_or(name);
            let curve_id = CurveId(format!("{edge_name}-curve"));
            let edge_id = EdgeId(format!("{edge_name}-edge"));
            let coedge_id = CoedgeId(format!("{name}-coedge"));
            if !model.edges.iter().any(|edge| edge.id == edge_id) {
                model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Circle {
                        center,
                        axis: Vector3::new(0.0, 0.0, 1.0),
                        ref_direction: Vector3::new(1.0, 0.0, 0.0),
                        radius,
                    },
                    source_object: None,
                });
                model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(curve_id),
                    start: VertexId("vertex".into()),
                    end: VertexId("vertex".into()),
                    param_range: None,
                    tolerance: None,
                });
            }
            model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            });
            loop_id
        };
    let mut add_face = |id: &str, surface: SurfaceId, sense: Sense, loops: Vec<LoopId>| {
        model.faces.push(Face {
            id: FaceId(id.into()),
            shell: ShellId("shell".into()),
            surface,
            sense,
            loops,
            name: None,
            color: None,
            tolerance: None,
        });
    };
    let bore_surface = SurfaceId("bore-surface".into());
    model.surfaces.push(Surface {
        id: bore_surface.clone(),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    let bore_loops = vec![
        add_circle_loop("bore-entry", None, Point3::new(0.0, 0.0, 0.0), 2.0),
        add_circle_loop(
            "bore-shoulder",
            Some("bore-shoulder"),
            Point3::new(0.0, 0.0, 10.0),
            2.0,
        ),
    ];
    add_face("bore-face", bore_surface, Sense::Reversed, bore_loops);

    let counterbore_surface = SurfaceId("counterbore-surface".into());
    model.surfaces.push(Surface {
        id: counterbore_surface.clone(),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        },
        source_object: None,
    });
    let counterbore_loops = vec![
        add_circle_loop(
            "counterbore-shoulder",
            Some("counterbore-shoulder"),
            Point3::new(0.0, 0.0, 10.0),
            4.0,
        ),
        add_circle_loop("counterbore-entry", None, Point3::new(0.0, 0.0, 12.0), 4.0),
    ];
    add_face(
        "counterbore-face",
        counterbore_surface,
        Sense::Reversed,
        counterbore_loops,
    );

    let shoulder_surface = SurfaceId("shoulder-surface".into());
    model.surfaces.push(Surface {
        id: shoulder_surface.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 10.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let shoulder_loops = vec![
        add_circle_loop(
            "shoulder-inner",
            Some("bore-shoulder"),
            Point3::new(0.0, 0.0, 10.0),
            2.0,
        ),
        add_circle_loop(
            "shoulder-outer",
            Some("counterbore-shoulder"),
            Point3::new(0.0, 0.0, 10.0),
            4.0,
        ),
    ];
    add_face(
        "shoulder-face",
        shoulder_surface,
        Sense::Forward,
        shoulder_loops,
    );

    let body = BodyId("body".into());
    model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![RegionId("region".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    model.regions.push(Region {
        id: RegionId("region".into()),
        body: body.clone(),
        shells: vec![ShellId("shell".into())],
    });
    model.shells.push(Shell {
        id: ShellId("shell".into()),
        region: RegionId("region".into()),
        faces: vec![
            FaceId("bore-face".into()),
            FaceId("counterbore-face".into()),
            FaceId("shoulder-face".into()),
        ],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    let mut ir = CadIr::empty();
    ir.model = model;
    let operations = vec![operation.clone()];
    let outputs = BTreeMap::from([(operation.clone(), vec![body.clone()])]);
    let body_faces = super::connected_solid_body_faces(&ir, &body).expect("solid body faces");
    assert_eq!(body_faces.len(), 3);
    let cylinders = super::cylindrical_face_witnesses(&ir, &body_faces).unwrap();
    assert_eq!(cylinders.len(), 2);
    assert!(super::plane_annulus_witness(
        &ir,
        &body_faces,
        &cylinders[0],
        1,
        &cylinders[1],
        0,
    ));
    assert!(super::counterbore_cylinders(&ir, &body_faces).is_some());
    let projection = super::counterbore_body_projection(&ir, &operations, &outputs)
        .expect("coaxial counterbore witness");
    assert_eq!(projection.outputs, outputs);
    assert_eq!(
        projection.diameters,
        BTreeMap::from([(operation.clone(), Length(4.0))])
    );
    assert_eq!(
        projection.counterbores,
        BTreeMap::from([(
            operation.clone(),
            super::CounterboreDimensions {
                diameter: Length(8.0),
                depth: Length(2.0),
            },
        )])
    );
    let inferred = super::counterbore_body_projection(&ir, &operations, &BTreeMap::new())
        .expect("unique connected solid counterbore witness");
    assert_eq!(inferred.outputs, outputs);
    assert_eq!(inferred.counterbores, projection.counterbores);
    assert_eq!(
        super::counterbore_axis_placements_for_operations(&ir, &operations, &outputs),
        BTreeMap::from([(
            operation.clone(),
            HolePlacement::Axis {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
        )])
    );
    let definition = super::non_boolean_feature_definition_with_parameters(
        "CBORE_HOLE",
        &["Hole_GeneralHole_Counterbored_Through"],
        None,
        None,
        super::HoleProjection {
            placements: vec![HolePlacement::Axis {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            }],
            diameter: Some(Length(4.0)),
            counterbore: projection.counterbores.get(&operation).copied(),
            ..super::HoleProjection::default()
        },
        BTreeMap::new(),
    );
    assert!(matches!(
        definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Counterbore {
                    diameter: Length(8.0),
                    depth: Length(2.0),
                },
                ..
            },
            diameter: Some(Length(4.0)),
            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll),
            placements,
            ..
        } if placements.as_deref() == Some(&[HolePlacement::Axis {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        }][..])
    ));

    let mut missing_shoulder = ir.clone();
    missing_shoulder.model.shells[0]
        .faces
        .retain(|face| face != &FaceId("shoulder-face".into()));
    assert!(super::counterbore_body_projection(&missing_shoulder, &operations, &outputs).is_none());
    let mut sheet = ir.clone();
    sheet.model.bodies[0].kind = BodyKind::Sheet;
    assert!(super::counterbore_body_projection(&sheet, &operations, &outputs).is_none());
    assert!(super::counterbore_body_projection(
        &ir,
        &[operation.clone(), "second-operation".into()],
        &BTreeMap::from([
            (operation, vec![body.clone()]),
            ("second-operation".into(), vec![body]),
        ]),
    )
    .is_none());
}

#[test]
fn nx_offset_feature_requires_one_output_image_and_one_exact_distance() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let output = BodyId("nx:s4:body#3".into());
    let make_offset = |ordinal: u32, distance: f64| {
        let owner = SurfaceId(format!("nx:s4:offset-surf#{ordinal}"));
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
            ProceduralSurfaceDefinition::Offset {
                support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
                distance,
                u_sense: Some(1),
                v_sense: Some(1),
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            None,
        );
        (owner, procedural)
    };
    for ordinal in 0..2 {
        let (owner, procedural) = make_offset(ordinal, 30.0);
        attach_test_body_procedural_surface(&mut ir, &output, owner, procedural);
    }

    let (definition, supports) =
        super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("unique offset distance");
    assert_eq!(supports.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Native(_),
            distance: None,
        }
    ));

    let input = BodyId("nx:s4:body#input".into());
    for ordinal in 0..2 {
        attach_test_body_surface(
            &mut ir,
            &input,
            SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
        );
    }
    let (definition, _) =
        super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniquely faced supports");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { faces, .. },
            distance: Some(cadmpeg_ir::features::Length(30.0)),
        } if faces.len() == 2
    ));

    for face in ir.model.faces.iter_mut().filter(|face| {
        face.surface.0 == "nx:s4:nurbs-surf#0" || face.surface.0 == "nx:s4:nurbs-surf#1"
    }) {
        face.sense = cadmpeg_ir::topology::Sense::Reversed;
    }
    let (definition, _) =
        super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniformly reversed support faces");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            distance: Some(cadmpeg_ir::features::Length(-30.0)),
            ..
        }
    ));

    ir.model
        .faces
        .iter_mut()
        .find(|face| face.surface == SurfaceId("nx:s4:nurbs-surf#0".into()))
        .expect("first support face")
        .sense = cadmpeg_ir::topology::Sense::Forward;
    let (definition, _) =
        super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("mixed support-face orientations retain offset family");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { .. },
            distance: None,
        }
    ));

    let mut ambiguous = ir.clone();
    attach_test_body_surface(
        &mut ambiguous,
        &BodyId("nx:s4:body#duplicate".into()),
        SurfaceId("nx:s4:nurbs-surf#0".into()),
    );
    let (definition, _) =
        super::offset_surface_feature_definition(&ambiguous, std::slice::from_ref(&output))
            .expect("offset semantics survive ambiguous face identity");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Native(_),
            distance: None,
        }
    ));

    let (unowned, procedural) = make_offset(99, -40.0);
    insert_test_procedural_surface(&mut ir, unowned, procedural);
    assert!(super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output)).is_some());
    ir.model.procedural_surfaces.pop();
    ir.model.surfaces.pop();

    let (owner, conflicting) = make_offset(2, -30.0);
    attach_test_body_procedural_surface(&mut ir, &output, owner, conflicting);
    assert!(super::offset_surface_feature_definition(&ir, &[output]).is_none());
}

#[test]
fn nx_thicken_feature_uses_the_magnitude_of_one_owned_offset_distance() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let output = BodyId("nx:s4:body#3".into());
    let make_offset = |ordinal: u32, distance: f64| {
        let owner = SurfaceId(format!("nx:s4:offset-surf#{ordinal}"));
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
            ProceduralSurfaceDefinition::Offset {
                support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
                distance,
                u_sense: Some(1),
                v_sense: Some(1),
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            None,
        );
        (owner, procedural)
    };
    for ordinal in 0..2 {
        let (owner, procedural) = make_offset(ordinal, -12.5);
        attach_test_body_procedural_surface(&mut ir, &output, owner, procedural);
    }

    let (definition, supports) =
        super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("unique nonzero offset distance");
    assert_eq!(supports.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Native(_),
            thickness: Some(Length(12.5)),
            side: None,
        }
    ));

    let mut sheet_output = ir.clone();
    sheet_output
        .model
        .bodies
        .iter_mut()
        .find(|body| body.id == output)
        .expect("output body")
        .kind = cadmpeg_ir::topology::BodyKind::Sheet;
    assert!(
        super::thicken_feature_definition(&sheet_output, std::slice::from_ref(&output)).is_none()
    );

    let input = BodyId("nx:s4:body#input".into());
    for ordinal in 0..2 {
        attach_test_body_surface(
            &mut ir,
            &input,
            SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
        );
    }
    let (definition, _) = super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
        .expect("uniquely faced supports");
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Resolved { faces, .. },
            side: Some(ThickenSide::Reverse),
            ..
        } if faces.len() == 2
    ));

    ir.model
        .faces
        .iter_mut()
        .find(|face| face.surface == SurfaceId("nx:s4:nurbs-surf#1".into()))
        .expect("second support face")
        .sense = cadmpeg_ir::topology::Sense::Reversed;
    let (definition, _) = super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
        .expect("mixed support senses preserve thicken semantics");
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Resolved { .. },
            side: None,
            ..
        }
    ));

    let (unowned, procedural) = make_offset(99, 40.0);
    insert_test_procedural_surface(&mut ir, unowned, procedural);
    assert!(super::thicken_feature_definition(&ir, std::slice::from_ref(&output)).is_some());
    ir.model.procedural_surfaces.pop();
    ir.model.surfaces.pop();

    let (owner, conflicting) = make_offset(2, 12.5);
    attach_test_body_procedural_surface(&mut ir, &output, owner, conflicting);
    assert!(super::thicken_feature_definition(&ir, &[output]).is_none());

    let zero_output = BodyId("nx:s4:body#4".into());
    let (owner, zero) = make_offset(3, 0.0);
    attach_test_body_procedural_surface(&mut ir, &zero_output, owner, zero);
    assert!(super::thicken_feature_definition(&ir, &[zero_output]).is_none());
}

#[test]
fn nx_thicken_symmetric_offsets_require_identical_support_sets() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let output = BodyId("nx:s4:body#symmetric".into());
    let input = BodyId("nx:s4:body#input".into());
    let support = SurfaceId("nx:s4:nurbs-surf#0".into());
    attach_test_body_surface(&mut ir, &input, support.clone());
    let make_offset = |ordinal: u32, support: SurfaceId, distance: f64| {
        let owner = SurfaceId(format!("nx:s4:offset-surf#{ordinal}"));
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
            ProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense: Some(1),
                v_sense: Some(1),
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            None,
        );
        (owner, procedural)
    };
    for (ordinal, distance) in [(0, -6.25), (1, 6.25)] {
        let (owner, procedural) = make_offset(ordinal, support.clone(), distance);
        attach_test_body_procedural_surface(&mut ir, &output, owner, procedural);
    }

    let (definition, supports) =
        super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("matched symmetric offsets");
    assert_eq!(supports, std::slice::from_ref(&support));
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Resolved { faces, .. },
            thickness: Some(Length(12.5)),
            side: Some(ThickenSide::Both),
        } if faces.len() == 1
    ));

    let mut mismatched_support = ir.clone();
    mismatched_support
        .model
        .procedural_surfaces
        .last_mut()
        .expect("positive offset")
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Offset { support, .. } = definition else {
                unreachable!()
            };
            *support = SurfaceId("nx:s4:nurbs-surf#other".into());
        });
    assert!(
        super::thicken_feature_definition(&mismatched_support, std::slice::from_ref(&output))
            .is_none()
    );

    ir.model
        .procedural_surfaces
        .last_mut()
        .expect("positive offset")
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Offset { distance, .. } = definition else {
                unreachable!()
            };
            *distance = 7.0;
        });
    assert!(super::thicken_feature_definition(&ir, std::slice::from_ref(&output)).is_none());
}

#[test]
fn nx_blend_feature_requires_one_output_image_and_circular_result_carriers() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, RadiusSpec};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, BlendSupport, ProceduralSurface,
        ProceduralSurfaceDefinition,
    };
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let output = BodyId("nx:s4:body#3".into());
    let support_a = SurfaceId("support-a".into());
    let support_b = SurfaceId("support-b".into());
    let support_c = SurfaceId("support-c".into());
    assert_eq!(
        super::blend_support_bipartition(vec![
            [support_a.clone(), support_b.clone()],
            [support_b.clone(), support_c.clone()],
        ]),
        Some((
            vec![support_a.clone(), support_c.clone()],
            vec![support_b.clone()],
        ))
    );
    assert!(super::blend_support_bipartition(vec![
        [support_a.clone(), support_b.clone()],
        [support_b.clone(), support_c.clone()],
        [support_c, support_a],
    ])
    .is_none());
    assert!(super::blend_support_bipartition(vec![
        [SurfaceId("a".into()), SurfaceId("b".into())],
        [SurfaceId("c".into()), SurfaceId("d".into())],
    ])
    .is_none());
    let make_blend = |ordinal: u32, radius: BlendRadiusLaw| {
        let owner = SurfaceId(format!("nx:s4:blend-surf#{ordinal}"));
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId(format!("nx:s4:blend-construction#{ordinal}")),
            ProceduralSurfaceDefinition::Blend {
                supports: [None, None],
                spine: None,
                radius,
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            None,
        );
        (owner, procedural)
    };
    let (first_owner, first) = make_blend(0, BlendRadiusLaw::Constant { signed_radius: 5.0 });
    attach_test_body_procedural_surface(&mut ir, &output, first_owner, first);
    let (second_owner, second) = make_blend(
        1,
        BlendRadiusLaw::Constant {
            signed_radius: -5.0,
        },
    );
    attach_test_body_procedural_surface(&mut ir, &output, second_owner, second);

    let (definition, surfaces) = super::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        super::NxBlendFamily::Edge,
    )
    .expect("one circular constant-radius blend result");
    assert_eq!(surfaces.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            groups
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant { radius: cadmpeg_ir::features::Length(5.0) },
            ..
        }])
    ));
    let (definition, _) = super::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        super::NxBlendFamily::Face,
    )
    .expect("face blend retains unresolved supports");
    assert!(matches!(
        definition,
        FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Unresolved,
            second_faces: FaceSelection::Unresolved,
            radius: RadiusSpec::Constant { .. },
        }
    ));

    let mut face_blend_ir = ir.clone();
    let first_support = SurfaceId("nx:s4:blend-support#a".into());
    let second_support = SurfaceId("nx:s4:blend-support#b".into());
    for procedural in &mut face_blend_ir.model.procedural_surfaces {
        procedural.edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend { supports, .. } = definition else {
                unreachable!()
            };
            *supports = [
                Some(BlendSupport {
                    surface: first_support.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second_support.clone(),
                    reversed: true,
                }),
            ];
        });
    }
    attach_test_body_surface(&mut face_blend_ir, &output, first_support);
    attach_test_body_surface(&mut face_blend_ir, &output, second_support);
    let (definition, _) = super::blend_feature_definition(
        &face_blend_ir,
        std::slice::from_ref(&output),
        super::NxBlendFamily::Edge,
    )
    .expect("complete edge-blend supports");
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet { groups }
            if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Constant { .. },
                ..
            }])
    ));
    let (definition, _) = super::blend_feature_definition(
        &face_blend_ir,
        std::slice::from_ref(&output),
        super::NxBlendFamily::Face,
    )
    .expect("complete face-blend supports");
    assert!(matches!(
        definition,
        FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Resolved { ref faces, .. },
            second_faces: FaceSelection::Resolved {
                faces: ref second,
                ..
            },
            radius: RadiusSpec::Constant { .. },
        } if faces.len() == 1 && second.len() == 1 && faces != second
    ));

    let (unowned, procedural) = make_blend(
        99,
        BlendRadiusLaw::Constant {
            signed_radius: 17.0,
        },
    );
    insert_test_procedural_surface(&mut ir, unowned, procedural);
    let (definition, _) = super::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        super::NxBlendFamily::Edge,
    )
    .expect("required invariant");
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            groups
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant { radius: cadmpeg_ir::features::Length(5.0) },
            ..
        }])
    ));
    ir.model.procedural_surfaces.pop();
    ir.model.surfaces.pop();

    let (owner, conflicting) = make_blend(2, BlendRadiusLaw::Constant { signed_radius: 7.0 });
    attach_test_body_procedural_surface(&mut ir, &output, owner, conflicting);
    let (definition, _) =
        super::blend_feature_definition(&ir, &[output], super::NxBlendFamily::Edge)
            .expect("required invariant");
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            groups
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
        radius: RadiusSpec::UnresolvedConstant,
            ..
        }])
    ));
    assert!(super::blend_feature_definition(&ir, &[], super::NxBlendFamily::Edge,).is_none());

    let conic_owner = SurfaceId("nx:s4:blend-surf#3".into());
    let conic = ProceduralSurface::new(
        ProceduralSurfaceId("nx:s4:blend-construction#3".into()),
        ProceduralSurfaceDefinition::Blend {
            supports: [None, None],
            spine: None,
            radius: BlendRadiusLaw::Constant { signed_radius: 7.0 },
            cross_section: BlendCrossSection::Conic,
            native: None,
        },
        None,
    );
    attach_test_body_procedural_surface(
        &mut ir,
        &BodyId("nx:s4:body#3".into()),
        conic_owner,
        conic,
    );
    assert!(super::blend_feature_definition(
        &ir,
        &[BodyId("nx:s4:body#3".into())],
        super::NxBlendFamily::Edge,
    )
    .is_none());
}

#[test]
fn nx_construction_dependency_requires_a_preceding_projected_operation() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::FeatureId;

    let positions = BTreeMap::from([("csys", 1), ("consumer", 2), ("later", 3)]);
    let features = BTreeMap::from([
        ("csys", FeatureId("nx:test:feature#csys".into())),
        ("consumer", FeatureId("nx:test:feature#consumer".into())),
    ]);

    assert_eq!(
        super::preceding_operation_dependency("csys", 2, &positions, &features),
        Some(FeatureId("nx:test:feature#csys".into()))
    );
    assert_eq!(
        super::preceding_operation_dependency("consumer", 2, &positions, &features),
        None
    );
    assert_eq!(
        super::preceding_operation_dependency("later", 2, &positions, &features),
        None
    );
    assert_eq!(
        super::preceding_operation_dependency("missing", 2, &positions, &features),
        None
    );
}

#[test]
fn topology_numeric_attribute_values_transfer_in_native_lane_order() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId};
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidEntity51NumericKind, ParasolidEntity51NumericUse,
        ParasolidEntity52IntegerRecord, ParasolidEntity53DoubleRecord,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.shells[0].id = ShellId("nx:s3:shell#58".into());
    ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
    ir.model.loops[0].id = LoopId("nx:s3:loop#59".into());
    let references = [(13, 58), (14, 60), (15, 59)].map(|(topology_type, topology_xmt)| {
        ParasolidTopologyAttributeListReference {
            id: format!("topology-reference-{topology_type}"),
            stream_ordinal: 3,
            topology_type,
            topology_xmt,
            attribute_list_xmt: 50,
            attribute_list_record: Some("entity".into()),
            inflated_offset: 300,
        }
    });
    let integer = ParasolidEntity52IntegerRecord {
        id: "integers".into(),
        stream_ordinal: 3,
        xmt: 70,
        values: vec![4, u32::MAX],
        byte_len: 18,
        inflated_offset: 400,
    };
    let double = ParasolidEntity53DoubleRecord {
        id: "doubles".into(),
        stream_ordinal: 3,
        xmt: 71,
        values: vec![0.25, 7.5],
        byte_len: 26,
        inflated_offset: 500,
    };
    let uses = [
        ParasolidEntity51NumericUse {
            id: "double-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: 4,
            referenced_xmt: 71,
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: double.id.clone(),
            inflated_offset: 200,
        },
        ParasolidEntity51NumericUse {
            id: "integer-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: 3,
            referenced_xmt: 70,
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: integer.id.clone(),
            inflated_offset: 200,
        },
    ];
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 34,
        next_definition_xmt: 1,
        identifier_xmt: 35,
        identifier_inflated_offset: 90,
        name: "SDL/TYSA_DENSITY".into(),
        type_id: 8004,
        action_codes: [0; 8],
        field_names_xmt: 1,
        legal_owner_flags: [0; 16],
        legal_owner_flag_count: 16,
        field_count: 1,
        field_codes: vec![2],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        id: "class-use".into(),
        topology_attribute_reference: references[2].id.clone(),
        entity_51_record: "entity".into(),
        attribute_class_use: "attribute-class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let class_uses = [class_use];
    let definitions = [definition];
    let sources = super::ParasolidNumericAttributeSources {
        numeric_uses: &uses,
        integers: &[integer],
        doubles: &[double],
    };
    let topology_attribute_index = super::ParasolidTopologyAttributeIndex::new(
        &ir,
        &references,
        &class_uses,
        &definitions,
        &[],
        &[],
    );
    let mut annotations = AnnotationBuilder::new();

    super::attach_parasolid_topology_numeric_attributes(
        &mut ir,
        &sources,
        &topology_attribute_index,
        &mut annotations,
    );

    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.0.contains("topology-numeric-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 6);
    assert_eq!(
        attributes[0].target,
        AttributeTarget::Shell(ShellId("nx:s3:shell#58".into()))
    );
    assert_eq!(attributes[0].name, "parasolid_type_integer_reference_3");
    assert_eq!(
        attributes[4].name,
        "SDL/TYSA_DENSITY.parasolid_type_integer_reference_3"
    );
    assert_eq!(
        attributes[0].values,
        [
            AttributeValue::Integer(4),
            AttributeValue::Integer(i64::from(u32::MAX))
        ]
    );
    for (attributes, target) in [
        (
            &attributes[0..2],
            AttributeTarget::Shell(ShellId("nx:s3:shell#58".into())),
        ),
        (
            &attributes[2..4],
            AttributeTarget::Face(FaceId("nx:s3:face#60".into())),
        ),
        (
            &attributes[4..6],
            AttributeTarget::Loop(LoopId("nx:s3:loop#59".into())),
        ),
    ] {
        assert!(attributes
            .iter()
            .all(|attribute| attribute.target == target));
        assert_eq!(
            attributes[1].values,
            [AttributeValue::Float(0.25), AttributeValue::Float(7.5)]
        );
    }
}

#[test]
fn topology_attribute_field_names_use_unique_declared_assignments() {
    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeFieldNames, ParasolidAttributeFieldUse,
        ParasolidAttributeFieldValueKind, ParasolidTopologyAttributeClassUse,
        ParasolidTopologyAttributeListReference,
    };

    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: 14,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("entity".into()),
        inflated_offset: 300,
    };
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 34,
        next_definition_xmt: 1,
        identifier_xmt: 35,
        identifier_inflated_offset: 90,
        name: "SDL/TYSA_DENSITY".into(),
        type_id: 8004,
        action_codes: [0; 8],
        field_names_xmt: 1,
        legal_owner_flags: [0; 16],
        legal_owner_flag_count: 16,
        field_count: 2,
        field_codes: vec![2, 3],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        id: "topology-class-use".into(),
        topology_attribute_reference: reference.id.clone(),
        entity_51_record: "entity".into(),
        attribute_class_use: "attribute-class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let field_use = ParasolidAttributeFieldUse {
        id: "field-use".into(),
        stream_ordinal: 3,
        attribute_class_use: "attribute-class-use".into(),
        entity_51_record: "entity".into(),
        attribute_definition: definition.id.clone(),
        field_ordinal: 0,
        field_code: 2,
        reference_ordinal: 5,
        value_kind: ParasolidAttributeFieldValueKind::Doubles,
        value_use: "double-use".into(),
        value_record: "double-record".into(),
        inflated_offset: 200,
    };

    assert_eq!(
        attribute_field_name(
            &reference,
            "double-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&field_use),
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_DENSITY.density")
    );

    let units = ParasolidAttributeFieldUse {
        field_ordinal: 1,
        field_code: 3,
        reference_ordinal: 6,
        value_kind: ParasolidAttributeFieldValueKind::String,
        value_use: "string-use".into(),
        value_record: "string-record".into(),
        ..field_use.clone()
    };
    assert_eq!(
        attribute_field_name(
            &reference,
            "string-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[units],
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_DENSITY.units")
    );

    let generic_definition = ParasolidAttributeDefinition {
        name: "CLASS".into(),
        field_names_xmt: 25,
        ..definition.clone()
    };
    assert_eq!(
        attribute_field_name(
            &reference,
            "double-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&generic_definition),
            std::slice::from_ref(&field_use),
            &[],
        )
        .as_deref(),
        Some("CLASS.field_0.parasolid_type_2")
    );

    let named_definition = ParasolidAttributeDefinition {
        name: "PVM/25_1".into(),
        field_names_xmt: 25,
        ..definition.clone()
    };
    let field_names = ParasolidAttributeFieldNames {
        id: "field-names-relation".into(),
        stream_ordinal: 3,
        attribute_definition: named_definition.id.clone(),
        field_names_record: "field-names-record".into(),
        value_records: vec!["name-1".into(), "name-2".into()],
        names: vec!["width".into(), "units".into()],
    };
    assert_eq!(
        attribute_field_name(
            &reference,
            "double-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&named_definition),
            std::slice::from_ref(&field_use),
            std::slice::from_ref(&field_names),
        )
        .as_deref(),
        Some("PVM/25_1.width")
    );

    let duplicate_class = ParasolidTopologyAttributeClassUse {
        id: "duplicate-class-use".into(),
        ..class_use.clone()
    };
    assert!(attribute_field_name(
        &reference,
        "double-use",
        &[class_use, duplicate_class],
        &[definition],
        &[field_use],
        &[],
    )
    .is_none());
}

#[test]
fn topology_attribute_fields_use_declared_ordinal_and_type_for_every_class() {
    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeFieldUse, ParasolidAttributeFieldValueKind,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: 14,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("entity".into()),
        inflated_offset: 300,
    };
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 34,
        next_definition_xmt: 1,
        identifier_xmt: 35,
        identifier_inflated_offset: 90,
        name: "SDL/TYSA_BLEND_ID".into(),
        type_id: 8004,
        action_codes: [0; 8],
        field_names_xmt: 1,
        legal_owner_flags: [0; 16],
        legal_owner_flag_count: 16,
        field_count: 2,
        field_codes: vec![3, 2],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        id: "topology-class-use".into(),
        topology_attribute_reference: reference.id.clone(),
        entity_51_record: "entity".into(),
        attribute_class_use: "attribute-class-use".into(),
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let text_field = ParasolidAttributeFieldUse {
        id: "text-field-use".into(),
        stream_ordinal: 3,
        attribute_class_use: class_use.attribute_class_use.clone(),
        entity_51_record: class_use.entity_51_record.clone(),
        attribute_definition: definition.id.clone(),
        field_ordinal: 0,
        field_code: 3,
        reference_ordinal: 5,
        value_kind: ParasolidAttributeFieldValueKind::String,
        value_use: "text-use".into(),
        value_record: "text-record".into(),
        inflated_offset: 200,
    };
    let numeric_field = ParasolidAttributeFieldUse {
        id: "numeric-field-use".into(),
        field_ordinal: 1,
        field_code: 2,
        reference_ordinal: 6,
        value_kind: ParasolidAttributeFieldValueKind::Doubles,
        value_use: "numeric-use".into(),
        value_record: "numeric-record".into(),
        ..text_field.clone()
    };

    assert_eq!(
        attribute_field_name(
            &reference,
            "text-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&text_field),
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_BLEND_ID.field_0.parasolid_type_3")
    );
    assert_eq!(
        attribute_field_name(
            &reference,
            "numeric-use",
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_field),
            &[],
        )
        .as_deref(),
        Some("SDL/TYSA_BLEND_ID.field_1.parasolid_type_2")
    );
}

#[test]
fn topology_attribute_index_retains_linked_type_81_records() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeFieldUse, ParasolidAttributeFieldValueKind,
        ParasolidEntity51NumericKind, ParasolidEntity51NumericUse, ParasolidEntity53DoubleRecord,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: 14,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("head".into()),
        inflated_offset: 300,
    };
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 34,
        next_definition_xmt: 1,
        identifier_xmt: 35,
        identifier_inflated_offset: 90,
        name: "CLASS".into(),
        type_id: 8000,
        action_codes: [0; 8],
        field_names_xmt: 1,
        legal_owner_flags: [0; 16],
        legal_owner_flag_count: 16,
        field_count: 1,
        field_codes: vec![2],
        inflated_offset: 100,
    };
    let class_uses = [
        ParasolidTopologyAttributeClassUse {
            id: "head-class".into(),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: "head".into(),
            attribute_class_use: "head-class-use".into(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        },
        ParasolidTopologyAttributeClassUse {
            id: "child-class".into(),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: "child".into(),
            attribute_class_use: "child-class-use".into(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        },
    ];
    let field_uses = [
        ParasolidAttributeFieldUse {
            id: "head-field".into(),
            stream_ordinal: 3,
            attribute_class_use: "head-class-use".into(),
            entity_51_record: "head".into(),
            attribute_definition: definition.id.clone(),
            field_ordinal: 0,
            field_code: 2,
            reference_ordinal: 5,
            value_kind: ParasolidAttributeFieldValueKind::Doubles,
            value_use: "head-use".into(),
            value_record: "head-value".into(),
            inflated_offset: 200,
        },
        ParasolidAttributeFieldUse {
            id: "child-field".into(),
            stream_ordinal: 3,
            attribute_class_use: "child-class-use".into(),
            entity_51_record: "child".into(),
            attribute_definition: definition.id.clone(),
            field_ordinal: 0,
            field_code: 2,
            reference_ordinal: 5,
            value_kind: ParasolidAttributeFieldValueKind::Doubles,
            value_use: "child-use".into(),
            value_record: "child-value".into(),
            inflated_offset: 210,
        },
    ];
    let numeric_uses = [
        ParasolidEntity51NumericUse {
            id: "head-use".into(),
            stream_ordinal: 3,
            entity_51_record: "head".into(),
            reference_ordinal: 5,
            referenced_xmt: 70,
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "head-value".into(),
            inflated_offset: 200,
        },
        ParasolidEntity51NumericUse {
            id: "child-use".into(),
            stream_ordinal: 3,
            entity_51_record: "child".into(),
            reference_ordinal: 5,
            referenced_xmt: 71,
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "child-value".into(),
            inflated_offset: 210,
        },
    ];
    let doubles = [
        ParasolidEntity53DoubleRecord {
            id: "head-value".into(),
            stream_ordinal: 3,
            xmt: 70,
            values: vec![1.0],
            byte_len: 18,
            inflated_offset: 400,
        },
        ParasolidEntity53DoubleRecord {
            id: "child-value".into(),
            stream_ordinal: 3,
            xmt: 71,
            values: vec![2.0],
            byte_len: 18,
            inflated_offset: 410,
        },
    ];
    let index = super::ParasolidTopologyAttributeIndex::new(
        &ir,
        std::slice::from_ref(&reference),
        &class_uses,
        std::slice::from_ref(&definition),
        &field_uses,
        &[],
    );

    assert_eq!(index.contexts.len(), 2);
    assert_eq!(
        index.class_names.get(reference.id.as_str()).copied(),
        Some("CLASS")
    );
    assert_eq!(
        index
            .attribute_names
            .field_name(&reference, "head-use")
            .as_deref(),
        Some("CLASS.field_0.parasolid_type_2")
    );
    assert_eq!(
        index
            .attribute_names
            .field_name(&reference, "child-use")
            .as_deref(),
        Some("CLASS.field_0.parasolid_type_2")
    );

    let sources = super::ParasolidNumericAttributeSources {
        numeric_uses: &numeric_uses,
        integers: &[],
        doubles: &doubles,
    };
    let mut annotations = AnnotationBuilder::new();
    super::attach_parasolid_topology_numeric_attributes(
        &mut ir,
        &sources,
        &index,
        &mut annotations,
    );
    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.0.contains("topology-numeric-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 2);
    assert!(attributes.iter().all(|attribute| {
        attribute.target == AttributeTarget::Face(FaceId("nx:s3:face#60".into()))
            && attribute.name == "CLASS.field_0.parasolid_type_2"
    }));
    assert_ne!(attributes[0].id, attributes[1].id);
    assert!(attributes
        .iter()
        .any(|attribute| { attribute.values == [AttributeValue::Float(1.0)] }));
    assert!(attributes
        .iter()
        .any(|attribute| { attribute.values == [AttributeValue::Float(2.0)] }));
}

#[test]
fn topology_structured_attribute_values_preserve_serialized_lanes() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::parasolid::{
        ParasolidAttributeFieldValueKind as Kind, ParasolidEntity51StructuredUse,
        ParasolidEntity57AxisRecord, ParasolidEntity58TagRecord, ParasolidEntity62UnicodeRecord,
        ParasolidEntityVectorRecord, ParasolidTopologyAttributeListReference,
        ParasolidVectorValueKind,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: 14,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some("entity".into()),
        inflated_offset: 300,
    };
    let vectors = [
        (ParasolidVectorValueKind::Points, "point", [1.0, 2.0, 3.0]),
        (ParasolidVectorValueKind::Vectors, "vector", [4.0, 5.0, 6.0]),
        (
            ParasolidVectorValueKind::Directions,
            "direction",
            [7.0, 8.0, 9.0],
        ),
    ]
    .map(|(kind, id, value)| ParasolidEntityVectorRecord {
        id: id.into(),
        stream_ordinal: 3,
        kind,
        xmt: 70,
        values: vec![value],
        byte_len: 36,
        inflated_offset: 400,
    });
    let axis = ParasolidEntity57AxisRecord {
        id: "axis".into(),
        stream_ordinal: 3,
        xmt: 73,
        values: vec![[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        byte_len: 60,
        inflated_offset: 430,
    };
    let tag = ParasolidEntity58TagRecord {
        id: "tag".into(),
        stream_ordinal: 3,
        xmt: 74,
        values: vec![u32::MAX],
        byte_len: 16,
        inflated_offset: 440,
    };
    let unicode = ParasolidEntity62UnicodeRecord {
        id: "unicode".into(),
        stream_ordinal: 3,
        xmt: 75,
        code_units: vec![0x03bc],
        value: "μ".into(),
        byte_len: 14,
        inflated_offset: 450,
    };
    let uses = [
        (Kind::Points, "point"),
        (Kind::Vectors, "vector"),
        (Kind::Directions, "direction"),
        (Kind::Axes, "axis"),
        (Kind::Tags, "tag"),
        (Kind::Unicode, "unicode"),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (kind, record))| ParasolidEntity51StructuredUse {
        id: format!("use-{ordinal}"),
        stream_ordinal: 3,
        entity_51_record: "entity".into(),
        reference_ordinal: u32::try_from(ordinal).expect("test ordinal fits u32") + 5,
        referenced_xmt: u32::try_from(ordinal).expect("test ordinal fits u32") + 70,
        kind,
        value_record: record.into(),
        inflated_offset: 200,
    })
    .collect::<Vec<_>>();
    let mut annotations = AnnotationBuilder::new();
    let sources = super::ParasolidStructuredAttributeSources {
        structured_uses: &uses,
        vectors: &vectors,
        axes: &[axis],
        tags: &[tag],
        unicode: &[unicode],
    };
    let topology_attribute_index = super::ParasolidTopologyAttributeIndex::new(
        &ir,
        std::slice::from_ref(&reference),
        &[],
        &[],
        &[],
        &[],
    );
    super::attach_parasolid_topology_structured_attributes(
        &mut ir,
        &sources,
        &topology_attribute_index,
        &mut annotations,
    );

    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.0.contains("topology-structured-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 6);
    assert!(attributes.iter().all(|attribute| {
        attribute.target == AttributeTarget::Face(FaceId("nx:s3:face#60".into()))
    }));
    let values = attributes
        .iter()
        .map(|attribute| (attribute.name.as_str(), attribute.values.as_slice()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        values["parasolid_type_85_point_reference_5"],
        [AttributeValue::Vector(vec![1.0, 2.0, 3.0])]
    );
    assert_eq!(
        values["parasolid_type_87_axis_reference_8"],
        [AttributeValue::Vector(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0])]
    );
    assert_eq!(
        values["parasolid_type_88_tag_reference_9"],
        [AttributeValue::Integer(i64::from(u32::MAX))]
    );
    assert_eq!(
        values["parasolid_type_98_unicode_reference_10"],
        [AttributeValue::String("μ".into())]
    );
}
