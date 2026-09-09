use super::super::*;

#[test]
fn nx_simple_hole_feature_owns_its_exact_native_constructions() {
    use crate::native::features::holes::FeatureSimpleHoleConstructionGroup;
    use crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLane;
    use crate::native::features::holes::FeatureSimpleHoleTemplate;
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;
    use crate::native::features::holes::{
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences, SimpleHoleBlockReference,
        SimpleHoleReferencePair,
    };
    let operation = "nx:feature-history:operation-label#1-4";
    let template = FeatureSimpleHoleTemplate {
        id: "template".to_string(),
        operation_label: operation.to_string(),
        payload_string: "string".to_string(),
        family: SimpleHoleFamily::GeneralHole,
        form: SimpleHoleForm::Simple,
        extent: SimpleHoleExtent::Through,
        start_treatment: SimpleHoleEndTreatment::Chamfer,
        end_treatment: SimpleHoleEndTreatment::Chamfer,
    };
    let lane = FeatureSimpleHoleRepeatedScalarLane {
        id: "lane".to_string(),
        operation_label: operation.to_string(),
        values: crate::om::nonempty::NonEmpty::new(
            [(508.0_f64, [10, 30]), (38.1_f64, [18, 38])].map(|(value, witness_offsets)| {
                let mut raw = value.to_be_bytes();
                raw[0] -= 0x10;
                crate::om::scalar::RepeatedScalar {
                    scalar: crate::om::scalar::ShiftedBinary64::read(&raw).unwrap(),
                    witness_offsets,
                }
            }),
        )
        .unwrap(),
    };
    let blocks = FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
        id: "blocks".to_string(),
        operation_label: operation.to_string(),
        first: SimpleHoleReferencePair {
            references: [
                SimpleHoleBlockReference {
                    data_block: "block#231".into(),
                    source_offset: 20,
                },
                SimpleHoleBlockReference {
                    data_block: "block#232".into(),
                    source_offset: 22,
                },
            ],
            wrapped: false,
        },
        second: SimpleHoleReferencePair {
            references: [
                SimpleHoleBlockReference {
                    data_block: "block#233".into(),
                    source_offset: 40,
                },
                SimpleHoleBlockReference {
                    data_block: "block#234".into(),
                    source_offset: 42,
                },
            ],
            wrapped: false,
        },
    };
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: blocks
            .first
            .references
            .each_ref()
            .map(|reference| reference.data_block.clone()),
        second_data_blocks: blocks
            .second
            .references
            .each_ref()
            .map(|reference| reference.data_block.clone()),
        members: crate::native::features::holes::SimpleHoleConstructionMembers::new(vec![
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: operation.into(),
                scalar_lane: "lane".into(),
                block_reference: "blocks".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "other-operation".into(),
                scalar_lane: "other-lane".into(),
                block_reference: "other-blocks".into(),
            },
        ])
        .unwrap(),
    };
    let properties = super::super::simple_hole_native_properties(
        operation,
        &[template],
        &[lane],
        &[blocks],
        &[group],
    );
    assert_eq!(properties["simple_hole_template"], "template");
    assert_eq!(properties["simple_hole_repeated_scalar_lane"], "lane");
    assert_eq!(
        properties["simple_hole_repeated_scalar_lane_block_references"],
        "blocks"
    );
    assert_eq!(properties["simple_hole_construction_group"], "group");
    assert!(super::super::simple_hole_native_properties(
        "nx:feature-history:operation-label#1-5",
        &[],
        &[],
        &[],
        &[],
    )
    .is_empty());
}

#[test]
fn nx_hole_geometry_projection_requires_complete_through_bore_partitions() {
    use crate::native::features::holes::FeatureSimpleHoleConstructionGroup;
    use crate::native::features::holes::FeatureSimpleHoleTemplate;
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;
    use cadmpeg_ir::document::{CadIr, Model};
    use cadmpeg_ir::features::HolePlacement;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Region, Sense, Shell};
    use cadmpeg_ir::SourceObjectAssociation;

    let operations = ["hole-a".to_string(), "hole-b".to_string()];
    let templates = operations
        .iter()
        .map(|operation| FeatureSimpleHoleTemplate {
            id: format!("template-{operation}"),
            operation_label: operation.clone(),
            payload_string: format!("string-{operation}"),
            family: SimpleHoleFamily::GeneralHole,
            form: SimpleHoleForm::Simple,
            extent: SimpleHoleExtent::Through,
            start_treatment: SimpleHoleEndTreatment::Chamfer,
            end_treatment: SimpleHoleEndTreatment::Chamfer,
        })
        .collect::<Vec<_>>();
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: ["a".into(), "b".into()],
        second_data_blocks: ["c".into(), "d".into()],
        members: crate::native::features::holes::SimpleHoleConstructionMembers::new(vec![
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: operations[0].clone(),
                scalar_lane: "lane-a".into(),
                block_reference: "refs-a".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: operations[1].clone(),
                scalar_lane: "lane-b".into(),
                block_reference: "refs-b".into(),
            },
        ])
        .unwrap(),
    };
    let mut model = Model::default();
    for ordinal in 0..2 {
        let surface = SurfaceId::mint(format!("test:model:entity#surface-{ordinal}"))
            .expect("identity grammar");
        model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::CylinderSurface::try_new(
                    Point3::new(ordinal as f64, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.55,
                )
                .unwrap(),
            ),
            source_object: None::<SourceObjectAssociation>,
        });
        model.faces.push(Face {
            id: FaceId::mint(format!("test:model:entity#face-{ordinal}"))
                .expect("identity grammar"),
            shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
            surface,
            sense: Sense::Reversed,
            loops: vec![
                LoopId::mint(format!("test:model:entity#loop-{ordinal}-0"))
                    .expect("identity grammar"),
                LoopId::mint(format!("test:model:entity#loop-{ordinal}-1"))
                    .expect("identity grammar"),
            ]
            .into(),
            name: None,
            color: None,
            tolerance: None,
        });
        for boundary in 0..2 {
            let loop_id = LoopId::mint(format!("test:model:entity#loop-{ordinal}-{boundary}"))
                .expect("identity grammar");
            let curve = CurveId::mint(format!("test:model:entity#bore-curve-{ordinal}-{boundary}"))
                .expect("identity grammar");
            let edge = EdgeId::mint(format!("test:model:entity#bore-edge-{ordinal}-{boundary}"))
                .expect("identity grammar");
            let coedge = CoedgeId::mint(format!(
                "test:model:entity#bore-coedge-{ordinal}-{boundary}"
            ))
            .expect("identity grammar");
            model.curves.push(Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Circle(
                    cadmpeg_ir::geometry::CircleCurve::try_new(
                        Point3::new(ordinal as f64, boundary as f64, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        2.55,
                    )
                    .unwrap(),
                ),
                source_object: None,
            });
            model.edges.push(Edge {
                id: edge.clone(),
                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve)),
                start: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                end: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                tolerance: None,
            });
            model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id,
                edge,
                radial_next: coedge,
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
    }
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
                FaceId::mint("test:model:entity#face-0").expect("identity grammar"),
                FaceId::mint("test:model:entity#face-1").expect("identity grammar"),
            ],
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    );
    let mut ir = CadIr::empty();
    ir.model = model;
    let outputs = std::collections::BTreeMap::from([
        ("hole-a".to_string(), vec![body.clone()]),
        ("hole-b".to_string(), vec![body]),
    ]);
    let inferred =
        super::super::hole_body_projection(&ir, &operations, &std::collections::BTreeMap::new())
            .expect("complete bore bijection");
    assert_eq!(inferred.outputs, outputs);
    assert_eq!(
        simple_hole_diameters(&ir, &templates, std::slice::from_ref(&group), &outputs,),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
        ])
    );
    assert_eq!(
        simple_hole_diameters(&ir, &templates, &[], &outputs),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
        ])
    );
    assert_eq!(
        hole_diameters_for_operations(&ir, &operations, &outputs),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
        ])
    );
    assert!(
        super::super::hole_axis_placements_for_operations(&ir, &operations, &outputs).is_empty()
    );
    assert!(super::super::hole_axis_placements_for_operations(
        &ir,
        &operations,
        &std::collections::BTreeMap::new(),
    )
    .is_empty());
    let mut single_hole = ir.clone();
    {
        let members = vec![FaceId::mint("test:model:entity#face-1").expect("identity grammar")];
        single_hole.model.shells[0].edit_topology(|faces, _, _| *faces = members)
    }
    .unwrap();
    let single_operation = [operations[1].clone()];
    let single_output = std::collections::BTreeMap::from([(
        operations[1].clone(),
        outputs[&operations[1]].clone(),
    )]);
    assert_eq!(
        super::super::hole_axis_placements_for_operations(
            &single_hole,
            &single_operation,
            &single_output,
        ),
        std::collections::BTreeMap::from([(
            operations[1].clone(),
            HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0))
                    .unwrap(),
            },
        )])
    );
    let SurfaceGeometry::Cylinder(cylinder_surface) = &mut single_hole.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let axis = cylinder_surface.axis();
    let ref_direction = cylinder_surface.ref_direction();
    let radius = &cylinder_surface.radius();
    let mut origin = *origin;
    origin.y = 91.0;
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(origin, *axis, *ref_direction, *radius)
            .unwrap();
    assert_eq!(
        super::super::hole_axis_placements_for_operations(
            &single_hole,
            &single_operation,
            &single_output,
        ),
        std::collections::BTreeMap::from([(
            operations[1].clone(),
            HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0))
                    .unwrap(),
            },
        )])
    );
    let mut opposite_axis = single_hole.clone();
    let SurfaceGeometry::Cylinder(cylinder_surface) = &mut opposite_axis.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let ref_direction = cylinder_surface.ref_direction();
    let radius = &cylinder_surface.radius();

    let axis = Vector3::new(0.0, -1.0, 0.0);
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(*origin, axis, *ref_direction, *radius)
            .unwrap();
    for curve in opposite_axis.model.curves.iter_mut().filter(|curve| {
        curve
            .id
            .as_str()
            .starts_with("test:model:entity#bore-curve-1-")
    }) {
        let CurveGeometry::Circle(circle_curve) = &mut curve.geometry else {
            unreachable!()
        };
        let center = circle_curve.center();
        let ref_direction = circle_curve.ref_direction();
        let radius = &circle_curve.radius();

        let axis = Vector3::new(0.0, -1.0, 0.0);
        *circle_curve =
            cadmpeg_ir::geometry::CircleCurve::try_new(*center, axis, *ref_direction, *radius)
                .unwrap();
    }
    assert_eq!(
        super::super::hole_axis_placements_for_operations(
            &opposite_axis,
            &single_operation,
            &single_output,
        ),
        std::collections::BTreeMap::from([(
            operations[1].clone(),
            HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0))
                    .unwrap(),
            },
        )])
    );
    let mut different_radii = ir.clone();
    let SurfaceGeometry::Cylinder(cylinder_surface) =
        &mut different_radii.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let axis = cylinder_surface.axis();
    let ref_direction = cylinder_surface.ref_direction();

    let radius = 3.1;
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(*origin, *axis, *ref_direction, radius)
            .unwrap();
    for curve in different_radii.model.curves.iter_mut().filter(|curve| {
        curve
            .id
            .as_str()
            .starts_with("test:model:entity#bore-curve-1-")
    }) {
        let CurveGeometry::Circle(circle_curve) = &mut curve.geometry else {
            unreachable!()
        };
        let center = circle_curve.center();
        let axis = circle_curve.axis();
        let ref_direction = circle_curve.ref_direction();

        let radius = 3.1;
        *circle_curve =
            cadmpeg_ir::geometry::CircleCurve::try_new(*center, *axis, *ref_direction, radius)
                .unwrap();
    }
    assert!(hole_diameters_for_operations(&different_radii, &operations, &outputs,).is_empty());
    assert!(super::super::hole_body_projection(
        &different_radii,
        &operations,
        &std::collections::BTreeMap::new(),
    )
    .is_none());
    let unresolved_primary =
        std::collections::BTreeMap::from([(operations[0].clone(), Vec::<BodyId>::new())]);
    assert!(super::super::hole_body_projection(
        &ir,
        std::slice::from_ref(&operations[0]),
        &unresolved_primary,
    )
    .is_none());
    assert_eq!(
        simple_hole_diameters(
            &ir,
            &templates,
            std::slice::from_ref(&group),
            &std::collections::BTreeMap::new(),
        ),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
        ])
    );
    assert!(hole_diameters_for_operations(
        &ir,
        &[operations[0].clone(), operations[0].clone()],
        &outputs,
    )
    .is_empty());
    let mut invalid_boundary = ir.clone();
    let CurveGeometry::Circle(circle_curve) = &mut invalid_boundary.model.curves[0].geometry else {
        unreachable!()
    };
    let center = circle_curve.center();
    let axis = circle_curve.axis();
    let ref_direction = circle_curve.ref_direction();
    let radius = &circle_curve.radius();
    let mut radius = *radius;
    radius += 0.1;
    *circle_curve =
        cadmpeg_ir::geometry::CircleCurve::try_new(*center, *axis, *ref_direction, radius).unwrap();
    assert!(hole_diameters_for_operations(&invalid_boundary, &operations, &outputs,).is_empty());
    let mut coincident_boundaries = ir.clone();
    let CurveGeometry::Circle(circle_curve) = &mut coincident_boundaries.model.curves[1].geometry
    else {
        unreachable!()
    };
    let center = circle_curve.center();
    let axis = circle_curve.axis();
    let ref_direction = circle_curve.ref_direction();
    let radius = &circle_curve.radius();
    let mut center = *center;
    center.y = 0.0;
    *circle_curve =
        cadmpeg_ir::geometry::CircleCurve::try_new(center, *axis, *ref_direction, *radius).unwrap();
    assert!(
        hole_diameters_for_operations(&coincident_boundaries, &operations, &outputs,).is_empty()
    );
    let mut nonparallel = single_hole.clone();
    let SurfaceGeometry::Cylinder(cylinder_surface) = &mut nonparallel.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let ref_direction = cylinder_surface.ref_direction();
    let radius = &cylinder_surface.radius();

    let axis = Vector3::new(0.0, 0.0, 1.0);
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(*origin, axis, *ref_direction, *radius)
            .unwrap();
    assert!(super::super::hole_axis_placements_for_operations(
        &nonparallel,
        &single_operation,
        &single_output,
    )
    .is_empty());
    let mut sheet = ir.clone();
    sheet.model.bodies[0].kind = BodyKind::Sheet;
    assert!(hole_diameters_for_operations(&sheet, &operations, &outputs).is_empty());
    let mut disconnected = ir.clone();
    disconnected.model.bodies[0]
        .regions
        .push(RegionId::mint("test:model:entity#second-region").expect("identity grammar"));
    assert!(hole_diameters_for_operations(&disconnected, &operations, &outputs).is_empty());
    let mut shared_carrier = ir.clone();
    shared_carrier.model.faces.push(Face {
        id: FaceId::mint("test:model:entity#unowned-shared-cylinder-face")
            .expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#unowned-shell").expect("identity grammar"),
        surface: SurfaceId::mint("test:model:entity#surface-0").expect("identity grammar"),
        sense: Sense::Reversed,
        loops: vec![
            LoopId::mint("test:model:entity#unowned-loop-a").expect("identity grammar"),
            LoopId::mint("test:model:entity#unowned-loop-b").expect("identity grammar"),
        ]
        .into(),
        name: None,
        color: None,
        tolerance: None,
    });
    assert_eq!(
        simple_hole_diameters(
            &shared_carrier,
            &templates,
            std::slice::from_ref(&group),
            &outputs,
        ),
        simple_hole_diameters(&ir, &templates, std::slice::from_ref(&group), &outputs,)
    );

    let mut distinct = ir.clone();
    distinct.model.shells[0]
        .edit_topology(|faces, _, _| faces.pop())
        .unwrap();
    distinct.model.bodies.push(Body {
        id: BodyId::mint("test:model:entity#second-body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("test:model:entity#second-region").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    distinct.model.regions.push(Region {
        id: RegionId::mint("test:model:entity#second-region").expect("identity grammar"),
        body: BodyId::mint("test:model:entity#second-body").expect("identity grammar"),
        shells: vec![ShellId::mint("test:model:entity#second-shell").expect("identity grammar")],
    });
    distinct.model.shells.push(Shell::with_face(
        ShellId::mint("test:model:entity#second-shell").expect("identity grammar"),
        RegionId::mint("test:model:entity#second-region").expect("identity grammar"),
        FaceId::mint("test:model:entity#face-1").expect("identity grammar"),
    ));
    distinct.model.faces[1].shell =
        ShellId::mint("test:model:entity#second-shell").expect("identity grammar");
    let SurfaceGeometry::Cylinder(cylinder_surface) = &mut distinct.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let axis = cylinder_surface.axis();
    let ref_direction = cylinder_surface.ref_direction();

    let radius = 3.0;
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(*origin, *axis, *ref_direction, radius)
            .unwrap();
    for curve in distinct.model.curves.iter_mut().filter(|curve| {
        curve
            .id
            .as_str()
            .starts_with("test:model:entity#bore-curve-1-")
    }) {
        let CurveGeometry::Circle(circle_curve) = &mut curve.geometry else {
            unreachable!()
        };
        let center = circle_curve.center();
        let axis = circle_curve.axis();
        let ref_direction = circle_curve.ref_direction();

        let radius = 3.0;
        *circle_curve =
            cadmpeg_ir::geometry::CircleCurve::try_new(*center, *axis, *ref_direction, radius)
                .unwrap();
    }
    let distinct_outputs = std::collections::BTreeMap::from([
        (
            "hole-a".to_string(),
            vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
        ),
        (
            "hole-b".to_string(),
            vec![BodyId::mint("test:model:entity#second-body").expect("identity grammar")],
        ),
    ]);
    assert_eq!(
        simple_hole_diameters(
            &distinct,
            &templates,
            std::slice::from_ref(&group),
            &distinct_outputs,
        ),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::scalar::Length::new(6.0).unwrap()
            ),
        ])
    );
    assert_eq!(
        hole_diameters_for_operations(&distinct, &operations, &distinct_outputs,),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::scalar::Length::new(5.1).unwrap()
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::scalar::Length::new(6.0).unwrap()
            ),
        ])
    );
    assert!(hole_diameters_for_operations(
        &distinct,
        &operations,
        &std::collections::BTreeMap::new(),
    )
    .is_empty());
    assert!(hole_diameters_for_operations(
        &ir,
        &operations,
        &std::collections::BTreeMap::from([(
            "hole-a".to_string(),
            vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
        )]),
    )
    .is_empty());

    let mut chamfered = ir.clone();
    for bore in 0..2 {
        for end in 0..2 {
            let surface = SurfaceId::mint(format!("test:model:entity#cone-{bore}-{end}"))
                .expect("identity grammar");
            let face = FaceId::mint(format!("test:model:entity#cone-face-{bore}-{end}"))
                .expect("identity grammar");
            let loops = [
                LoopId::mint(format!("test:model:entity#cone-loop-{bore}-{end}-inner"))
                    .expect("identity grammar"),
                LoopId::mint(format!("test:model:entity#cone-loop-{bore}-{end}-outer"))
                    .expect("identity grammar"),
            ];
            chamfered.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Cone(
                    cadmpeg_ir::geometry::ConeSurface::try_new(
                        Point3::new(bore as f64, end as f64, 0.0),
                        Vector3::new(0.0, if end == 0 { 1.0 } else { -1.0 }, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        0.0,
                        1.0,
                        std::f64::consts::FRAC_PI_4,
                    )
                    .unwrap(),
                ),
                source_object: None,
            });
            chamfered.model.shells[0].add_face(face.clone());
            chamfered.model.faces.push(Face {
                id: face,
                shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
                surface,
                sense: Sense::Reversed,
                loops: loops.to_vec().into(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary, (loop_id, radius)) in loops.into_iter().zip([2.55, 3.55]).enumerate() {
                let curve = CurveId::mint(format!(
                    "test:model:entity#cone-curve-{bore}-{end}-{boundary}"
                ))
                .expect("identity grammar");
                let edge = EdgeId::mint(format!(
                    "test:model:entity#cone-edge-{bore}-{end}-{boundary}"
                ))
                .expect("identity grammar");
                let coedge = CoedgeId::mint(format!(
                    "test:model:entity#cone-coedge-{bore}-{end}-{boundary}"
                ))
                .expect("identity grammar");
                chamfered.model.curves.push(Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Circle(
                        cadmpeg_ir::geometry::CircleCurve::try_new(
                            Point3::new(bore as f64, end as f64, 0.0),
                            Vector3::new(0.0, 1.0, 0.0),
                            Vector3::new(1.0, 0.0, 0.0),
                            radius,
                        )
                        .unwrap(),
                    ),
                    source_object: None,
                });
                chamfered.model.edges.push(Edge {
                    id: edge.clone(),
                    carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve)),
                    start: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                    end: VertexId::mint("test:model:entity#vertex").expect("identity grammar"),
                    tolerance: None,
                });
                chamfered.model.coedges.push(Coedge {
                    id: coedge.clone(),
                    owner_loop: loop_id,
                    edge,
                    radial_next: coedge,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: None,
                });
            }
        }
    }
    assert_eq!(
        super::super::simple_hole_chamfers(&chamfered, &templates, &outputs),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::features::HoleKind::Chamfer {
                    diameter: cadmpeg_ir::scalar::PositiveLength::new(7.1).unwrap(),
                    angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2)
                        .unwrap(),
                },
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::features::HoleKind::Chamfer {
                    diameter: cadmpeg_ir::scalar::PositiveLength::new(7.1).unwrap(),
                    angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2)
                        .unwrap(),
                },
            ),
        ])
    );
    assert_eq!(
        super::super::simple_hole_chamfers(
            &chamfered,
            &templates,
            &std::collections::BTreeMap::new(),
        ),
        super::super::simple_hole_chamfers(&chamfered, &templates, &outputs)
    );
    let mut sheet = chamfered.clone();
    sheet.model.bodies[0].kind = BodyKind::Sheet;
    assert!(super::super::simple_hole_chamfers(&sheet, &templates, &outputs).is_empty());
    let mut unrelated = chamfered.clone();
    unrelated.model.surfaces.push(Surface {
        id: SurfaceId::mint("test:model:entity#unrelated-cone").expect("identity grammar"),
        geometry: SurfaceGeometry::Cone(
            cadmpeg_ir::geometry::ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                0.0,
                1.0,
                0.0,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    unrelated.model.faces.push(Face {
        id: FaceId::mint("test:model:entity#unrelated-cone-face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#unrelated-shell").expect("identity grammar"),
        surface: SurfaceId::mint("test:model:entity#unrelated-cone").expect("identity grammar"),
        sense: Sense::Reversed,
        loops: vec![
            LoopId::mint("test:model:entity#unrelated-a").expect("identity grammar"),
            LoopId::mint("test:model:entity#unrelated-b").expect("identity grammar"),
        ]
        .into(),
        name: None,
        color: None,
        tolerance: None,
    });
    assert_eq!(
        super::super::simple_hole_chamfers(&unrelated, &templates, &outputs),
        super::super::simple_hole_chamfers(&chamfered, &templates, &outputs)
    );
    let mut unequal_chamfers = chamfered;
    let CurveGeometry::Circle(circle_curve) = &mut unequal_chamfers
        .model
        .curves
        .last_mut()
        .expect("required invariant")
        .geometry
    else {
        unreachable!()
    };
    let center = circle_curve.center();
    let axis = circle_curve.axis();
    let ref_direction = circle_curve.ref_direction();
    let radius = &circle_curve.radius();
    let mut radius = *radius;
    radius += 0.1;
    *circle_curve =
        cadmpeg_ir::geometry::CircleCurve::try_new(*center, *axis, *ref_direction, radius).unwrap();
    assert!(super::super::simple_hole_chamfers(&unequal_chamfers, &templates, &outputs).is_empty());

    let mut mismatched = ir;
    let SurfaceGeometry::Cylinder(cylinder_surface) = &mut mismatched.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let axis = cylinder_surface.axis();
    let ref_direction = cylinder_surface.ref_direction();

    let radius = 3.0;
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(*origin, *axis, *ref_direction, radius)
            .unwrap();
    assert!(simple_hole_diameters(&mismatched, &templates, &[group], &outputs,).is_empty());
}
