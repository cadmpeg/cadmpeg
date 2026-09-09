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

#[test]
fn nx_blind_hole_projection_requires_a_unique_cap_and_entry_direction() {
    use crate::native::features::holes::FeatureSimpleHoleTemplate;
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;
    use cadmpeg_ir::document::{CadIr, Model};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{FeatureDefinition, HoleKind, HolePlacement, LinearTermination},
        scalar::Length,
    };

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
    let cylinder_surface =
        SurfaceId::mint("test:model:entity#blind-cylinder-surface").expect("identity grammar");
    let cap_surface =
        SurfaceId::mint("test:model:entity#blind-cap-surface").expect("identity grammar");
    model.surfaces.push(Surface {
        id: cylinder_surface.clone(),
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    model.surfaces.push(Surface {
        id: cap_surface.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let (entry_loop, cylinder_cap_loop, cap_face_loop) = {
        let mut add_circle_loop =
            |loop_name: &str, edge_name: &str, center: Point3, radius: f64| {
                let loop_id = LoopId::mint(format!("test:model:entity#{loop_name}"))
                    .expect("identity grammar");
                let edge_id = EdgeId::mint(format!("test:model:entity#{edge_name}"))
                    .expect("identity grammar");
                let curve_id = CurveId::mint(format!("test:model:entity#{edge_name}-curve"))
                    .expect("identity grammar");
                if !model.edges.iter().any(|edge| edge.id == edge_id) {
                    model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: CurveGeometry::Circle(
                            cadmpeg_ir::geometry::CircleCurve::try_new(
                                center,
                                Vector3::new(0.0, 0.0, 1.0),
                                Vector3::new(1.0, 0.0, 0.0),
                                radius,
                            )
                            .unwrap(),
                        ),
                        source_object: None,
                    });
                    model.edges.push(Edge {
                        id: edge_id.clone(),
                        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
                        start: VertexId::mint("test:model:entity#vertex")
                            .expect("identity grammar"),
                        end: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                        tolerance: None,
                    });
                }
                let coedge_id = CoedgeId::mint(format!("test:model:entity#{loop_name}-coedge"))
                    .expect("identity grammar");
                model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
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
    let cylinder_face =
        FaceId::mint("test:model:entity#blind-cylinder-face").expect("identity grammar");
    let cap_face = FaceId::mint("test:model:entity#blind-cap-face").expect("identity grammar");
    model.faces.push(Face {
        id: cylinder_face.clone(),
        shell: ShellId::mint("test:model:entity#blind-shell").expect("identity grammar"),
        surface: cylinder_surface,
        sense: Sense::Reversed,
        loops: vec![entry_loop, cylinder_cap_loop].into(),
        name: None,
        color: None,
        tolerance: None,
    });
    model.faces.push(Face {
        id: cap_face.clone(),
        shell: ShellId::mint("test:model:entity#blind-shell").expect("identity grammar"),
        surface: cap_surface,
        sense: Sense::Forward,
        loops: vec![cap_face_loop].into(),
        name: None,
        color: None,
        tolerance: None,
    });
    let body = BodyId::mint("test:model:entity#blind-body").expect("identity grammar");
    model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("test:model:entity#blind-region").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    model.regions.push(Region {
        id: RegionId::mint("test:model:entity#blind-region").expect("identity grammar"),
        body: body.clone(),
        shells: vec![ShellId::mint("test:model:entity#blind-shell").expect("identity grammar")],
    });
    model.shells.push(
        Shell::new(
            ShellId::mint("test:model:entity#blind-shell").expect("identity grammar"),
            RegionId::mint("test:model:entity#blind-region").expect("identity grammar"),
            vec![cylinder_face, cap_face],
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    );
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
        BTreeMap::from([(operation.clone(), Length::new(4.0).unwrap())])
    );
    assert_eq!(
        projection.blind_depths,
        BTreeMap::from([(
            operation.clone(),
            cadmpeg_ir::scalar::NonZeroLength::new(3.0).unwrap()
        )])
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
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    0.0, 0.0, 1.0
                ))
                .unwrap(),
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
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    0.0, 0.0, 1.0,
                ))
                .unwrap(),
            }],
            diameter: Some(Length::new(4.0).unwrap()),
            extent: Some(LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(3.0).unwrap(),
            }),
            ..super::HoleProjection::default()
        },
        BTreeMap::new(),
    )
    .unwrap();
    assert!(matches!(
        definition, FeatureDefinition::Hole {
            shape,

            extent: Some(LinearTermination::Blind { length: actual_length }),
            placements,
            ..
        } if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            }, Some(actual_diameter),) if (placements.as_deref() == Some(&[HolePlacement::Directed {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        }][..])) && actual_diameter.get() == 4.0 && actual_length.get() == 3.0)));

    let mut missing_cap = ir.clone();
    missing_cap.model.shells[0]
        .edit_topology(|faces, _, _| {
            faces.retain(|face| {
                face != &FaceId::mint("test:model:entity#blind-cap-face").expect("identity grammar")
            });
        })
        .unwrap();
    assert!(super::blind_hole_body_projection(
        &missing_cap,
        std::slice::from_ref(&operation),
        &outputs,
    )
    .is_none());
    let mut duplicate_cap = ir.clone();
    duplicate_cap.model.faces.push(Face {
        id: FaceId::mint("test:model:entity#blind-duplicate-cap-face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#blind-shell").expect("identity grammar"),
        surface: SurfaceId::mint("test:model:entity#blind-cap-surface").expect("identity grammar"),
        sense: Sense::Forward,
        loops: vec![
            LoopId::mint("test:model:entity#blind-cap-face-loop").expect("identity grammar")
        ]
        .into(),
        name: None,
        color: None,
        tolerance: None,
    });
    duplicate_cap.model.shells[0].add_face(
        FaceId::mint("test:model:entity#blind-duplicate-cap-face").expect("identity grammar"),
    );
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
    use crate::native::features::holes::FeatureSimpleHoleTemplate;
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;
    use cadmpeg_ir::document::{CadIr, Model};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{FeatureDefinition, HoleKind, HolePlacement},
        scalar::Length,
    };

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
            let loop_id =
                LoopId::mint(format!("test:model:entity#{name}-loop")).expect("identity grammar");
            let edge_name = shared_edge.unwrap_or(name);
            let curve_id = CurveId::mint(format!("test:model:entity#{edge_name}-curve"))
                .expect("identity grammar");
            let edge_id = EdgeId::mint(format!("test:model:entity#{edge_name}-edge"))
                .expect("identity grammar");
            let coedge_id = CoedgeId::mint(format!("test:model:entity#{name}-coedge"))
                .expect("identity grammar");
            if !model.edges.iter().any(|edge| edge.id == edge_id) {
                model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Circle(
                        cadmpeg_ir::geometry::CircleCurve::try_new(
                            center,
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                            radius,
                        )
                        .unwrap(),
                    ),
                    source_object: None,
                });
                model.edges.push(Edge {
                    id: edge_id.clone(),
                    carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
                    start: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                    end: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                    tolerance: None,
                });
            }
            model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                radial_next: coedge_id,
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            });
            loop_id
        };
    let mut add_face = |id: &str, surface: SurfaceId, sense: Sense, loops: Vec<LoopId>| {
        model.faces.push(Face {
            id: FaceId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
            shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
            surface,
            sense,
            loops: loops.into(),
            name: None,
            color: None,
            tolerance: None,
        });
    };
    let bore_surface = SurfaceId::mint("test:model:entity#bore-surface").expect("identity grammar");
    model.surfaces.push(Surface {
        id: bore_surface.clone(),
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ),
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

    let counterbore_surface =
        SurfaceId::mint("test:model:entity#counterbore-surface").expect("identity grammar");
    model.surfaces.push(Surface {
        id: counterbore_surface.clone(),
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 10.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .unwrap(),
        ),
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

    let shoulder_surface =
        SurfaceId::mint("test:model:entity#shoulder-surface").expect("identity grammar");
    model.surfaces.push(Surface {
        id: shoulder_surface.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 10.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
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

    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("test:model:entity#region").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    model.regions.push(Region {
        id: RegionId::mint("test:model:entity#region").expect("identity grammar"),
        body: body.clone(),
        shells: vec![ShellId::mint("test:model:entity#shell").expect("identity grammar")],
    });
    model.shells.push(
        Shell::new(
            ShellId::mint("test:model:entity#shell").expect("identity grammar"),
            RegionId::mint("test:model:entity#region").expect("identity grammar"),
            vec![
                FaceId::mint("test:model:entity#bore-face").expect("identity grammar"),
                FaceId::mint("test:model:entity#counterbore-face").expect("identity grammar"),
                FaceId::mint("test:model:entity#shoulder-face").expect("identity grammar"),
            ],
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    );
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
        BTreeMap::from([(operation.clone(), Length::new(4.0).unwrap())])
    );
    assert_eq!(
        projection.counterbores,
        BTreeMap::from([(
            operation.clone(),
            super::CounterboreDimensions {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
                depth: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
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
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                    .unwrap(),
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
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                    .unwrap(),
            }],
            diameter: Some(Length::new(4.0).unwrap()),
            counterbore: projection.counterbores.get(&operation).copied(),
            ..super::HoleProjection::default()
        },
        BTreeMap::new(),
    )
    .unwrap();
    assert!(matches!(
        definition, FeatureDefinition::Hole {
            shape,

            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll),
            placements,
            ..
        } if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Counterbore {
                    diameter: actual_diameter,
                    depth: actual_depth,
                },
                ..
            }, Some(actual_diameter_2),) if (placements.as_deref() == Some(&[HolePlacement::Axis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        }][..])) && actual_diameter.get() == 8.0 && actual_depth.get() == 2.0 && actual_diameter_2.get() == 4.0)));

    let mut missing_shoulder = ir.clone();
    missing_shoulder.model.shells[0]
        .edit_topology(|faces, _, _| {
            faces.retain(|face| {
                face != &FaceId::mint("test:model:entity#shoulder-face").expect("identity grammar")
            });
        })
        .unwrap();
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
    let output = BodyId::mint("nx:s4:body#3").expect("identity grammar");
    let make_offset = |ordinal: u32, distance: f64| {
        let owner =
            SurfaceId::mint(format!("nx:s4:offset-surf#{ordinal}")).expect("identity grammar");
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId::mint(format!("nx:s4:offset-construction#{ordinal}"))
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    SurfaceId::mint(format!("nx:s4:nurbs-surf#{ordinal}"))
                        .expect("identity grammar"),
                    distance,
                    Some(1),
                    Some(1),
                    None,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap();
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

    let input = BodyId::mint("nx:s4:body#input").expect("identity grammar");
    for ordinal in 0..2 {
        attach_test_body_surface(
            &mut ir,
            &input,
            SurfaceId::mint(format!("nx:s4:nurbs-surf#{ordinal}")).expect("identity grammar"),
        );
    }
    let (definition, _) =
        super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniquely faced supports");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { faces, .. },
            distance: Some(actual_distance),
        } if (faces.len() == 2) && actual_distance.get() == 30.0
    ));

    for face in ir.model.faces.iter_mut().filter(|face| {
        face.surface.as_str() == "nx:s4:nurbs-surf#0"
            || face.surface.as_str() == "nx:s4:nurbs-surf#1"
    }) {
        face.sense = cadmpeg_ir::topology::Sense::Reversed;
    }
    let (definition, _) =
        super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniformly reversed support faces");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            distance: Some(actual_distance),
            ..
        } if actual_distance.get() == -30.0
    ));

    ir.model
        .faces
        .iter_mut()
        .find(|face| {
            face.surface == SurfaceId::mint("nx:s4:nurbs-surf#0").expect("identity grammar")
        })
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
        &BodyId::mint("nx:s4:body#duplicate").expect("identity grammar"),
        SurfaceId::mint("nx:s4:nurbs-surf#0").expect("identity grammar"),
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
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, ThickenSide};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let output = BodyId::mint("nx:s4:body#3").expect("identity grammar");
    let make_offset = |ordinal: u32, distance: f64| {
        let owner =
            SurfaceId::mint(format!("nx:s4:offset-surf#{ordinal}")).expect("identity grammar");
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId::mint(format!("nx:s4:offset-construction#{ordinal}"))
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    SurfaceId::mint(format!("nx:s4:nurbs-surf#{ordinal}"))
                        .expect("identity grammar"),
                    distance,
                    Some(1),
                    Some(1),
                    None,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap();
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
            thickness: Some(actual_thickness),
            side: None,
        } if actual_thickness.get() == 12.5
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

    let input = BodyId::mint("nx:s4:body#input").expect("identity grammar");
    for ordinal in 0..2 {
        attach_test_body_surface(
            &mut ir,
            &input,
            SurfaceId::mint(format!("nx:s4:nurbs-surf#{ordinal}")).expect("identity grammar"),
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
        .find(|face| {
            face.surface == SurfaceId::mint("nx:s4:nurbs-surf#1").expect("identity grammar")
        })
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

    let zero_output = BodyId::mint("nx:s4:body#4").expect("identity grammar");
    let (owner, zero) = make_offset(3, 0.0);
    attach_test_body_procedural_surface(&mut ir, &zero_output, owner, zero);
    assert!(super::thicken_feature_definition(&ir, &[zero_output]).is_none());
}

#[test]
fn nx_thicken_symmetric_offsets_require_identical_support_sets() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, ThickenSide};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let output = BodyId::mint("nx:s4:body#symmetric").expect("identity grammar");
    let input = BodyId::mint("nx:s4:body#input").expect("identity grammar");
    let support = SurfaceId::mint("nx:s4:nurbs-surf#0").expect("identity grammar");
    attach_test_body_surface(&mut ir, &input, support.clone());
    let make_offset = |ordinal: u32, support: SurfaceId, distance: f64| {
        let owner =
            SurfaceId::mint(format!("nx:s4:offset-surf#{ordinal}")).expect("identity grammar");
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId::mint(format!("nx:s4:offset-construction#{ordinal}"))
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    support,
                    distance,
                    Some(1),
                    Some(1),
                    None,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap();
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
            thickness: Some(actual_thickness),
            side: Some(ThickenSide::Both),
        } if (faces.len() == 1) && actual_thickness.get() == 12.5
    ));

    let mut mismatched_support = ir.clone();
    mismatched_support
        .model
        .procedural_surfaces
        .last_mut()
        .expect("positive offset")
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Offset(definition_payload) = definition else {
                unreachable!()
            };
            definition_payload
                .set_support(SurfaceId::mint("nx:s4:nurbs-surf#other").expect("identity grammar"));
        })
        .unwrap();
    assert!(
        super::thicken_feature_definition(&mismatched_support, std::slice::from_ref(&output))
            .is_none()
    );

    ir.model
        .procedural_surfaces
        .last_mut()
        .expect("positive offset")
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Offset(definition_payload) = definition else {
                unreachable!()
            };
            definition_payload.try_set_distance(7.0).unwrap();
        })
        .unwrap();
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
    let output = BodyId::mint("nx:s4:body#3").expect("identity grammar");
    let support_a = SurfaceId::mint("test:model:entity#support-a").expect("identity grammar");
    let support_b = SurfaceId::mint("test:model:entity#support-b").expect("identity grammar");
    let support_c = SurfaceId::mint("test:model:entity#support-c").expect("identity grammar");
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
        [
            SurfaceId::mint("test:model:entity#a").expect("identity grammar"),
            SurfaceId::mint("test:model:entity#b").expect("identity grammar")
        ],
        [
            SurfaceId::mint("test:model:entity#c").expect("identity grammar"),
            SurfaceId::mint("test:model:entity#d").expect("identity grammar")
        ],
    ])
    .is_none());
    let make_blend = |ordinal: u32, radius: BlendRadiusLaw| {
        let owner =
            SurfaceId::mint(format!("nx:s4:blend-surf#{ordinal}")).expect("identity grammar");
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId::mint(format!("nx:s4:blend-construction#{ordinal}"))
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::Blend {
                supports: [None, None],
                spine: None,
                radius,
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            None,
        )
        .unwrap();
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
            radius: RadiusSpec::Constant { radius: actual_radius },
            ..
        }] if actual_radius.get() == 5.0)
    ));
    let (definition, _) = super::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        super::NxBlendFamily::Face,
    )
    .expect("face blend retains unresolved supports");
    assert!(matches!(
        definition, FeatureDefinition::FaceBlend {
            operands,

            radius: RadiusSpec::Constant { .. },
        } if matches!((operands.first_faces(), operands.second_faces(),), (FaceSelection::Unresolved, FaceSelection::Unresolved,))));

    let mut face_blend_ir = ir.clone();
    let first_support = SurfaceId::mint("nx:s4:blend-support#a").expect("identity grammar");
    let second_support = SurfaceId::mint("nx:s4:blend-support#b").expect("identity grammar");
    for procedural in &mut face_blend_ir.model.procedural_surfaces {
        procedural
            .edit_definition(|definition| {
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
            })
            .unwrap();
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
        definition, FeatureDefinition::FaceBlend {
            operands,

            radius: RadiusSpec::Constant { .. },
        } if matches!((operands.first_faces(), operands.second_faces(),), (FaceSelection::Resolved { ref faces, .. }, FaceSelection::Resolved {
                faces: ref second,
                ..
            },) if faces.len() == 1 && second.len() == 1 && faces != second)));

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
            radius: RadiusSpec::Constant { radius: actual_radius },
            ..
        }] if actual_radius.get() == 5.0)
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

    let conic_owner = SurfaceId::mint("nx:s4:blend-surf#3").expect("identity grammar");
    let conic = ProceduralSurface::new(
        ProceduralSurfaceId::mint("nx:s4:blend-construction#3").expect("identity grammar"),
        ProceduralSurfaceDefinition::Blend {
            supports: [None, None],
            spine: None,
            radius: BlendRadiusLaw::Constant { signed_radius: 7.0 },
            cross_section: BlendCrossSection::Conic,
            native: None,
        },
        None,
    )
    .unwrap();
    attach_test_body_procedural_surface(
        &mut ir,
        &BodyId::mint("nx:s4:body#3").expect("identity grammar"),
        conic_owner,
        conic,
    );
    assert!(super::blend_feature_definition(
        &ir,
        &[BodyId::mint("nx:s4:body#3").expect("identity grammar")],
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
        (
            "csys",
            FeatureId::mint("nx:test:feature#csys").expect("identity grammar"),
        ),
        (
            "consumer",
            FeatureId::mint("nx:test:feature#consumer").expect("identity grammar"),
        ),
    ]);

    assert_eq!(
        super::preceding_operation_dependency("csys", 2, &positions, &features),
        Some(FeatureId::mint("nx:test:feature#csys").expect("identity grammar"))
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

mod topology_attributes;
