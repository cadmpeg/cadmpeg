use super::super::*;

#[test]
fn nx_simple_hole_feature_owns_its_exact_native_constructions() {
    use crate::native::features::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleRepeatedScalarLane,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences, FeatureSimpleHoleTemplate,
        SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
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
        values: vec![508.0, 38.1],
        raw_values: vec![[0x30; 8], [0x31; 8]],
        first_witness_offsets: vec![10, 18],
        second_witness_offsets: vec![30, 38],
    };
    let blocks = FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
        id: "blocks".to_string(),
        operation_label: operation.to_string(),
        first_data_blocks: ["block#231".to_string(), "block#232".to_string()],
        second_data_blocks: ["block#233".to_string(), "block#234".to_string()],
        first_reference_prefix: None,
        second_reference_prefix: None,
        first_reference_offsets: [20, 22],
        second_reference_offsets: [40, 42],
    };
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: blocks.first_data_blocks.clone(),
        second_data_blocks: blocks.second_data_blocks.clone(),
        operation_labels: vec![operation.into(), "other-operation".into()],
        scalar_lanes: vec!["lane".into(), "other-lane".into()],
        block_references: vec!["blocks".into(), "other-blocks".into()],
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
    use crate::native::features::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleTemplate, SimpleHoleEndTreatment,
        SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };
    use cadmpeg_ir::document::{CadIr, Model};
    use cadmpeg_ir::features::HolePlacement;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Region, Sense, Shell};
    use cadmpeg_ir::units::Units;
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
        operation_labels: operations.to_vec(),
        scalar_lanes: vec!["lane-a".into(), "lane-b".into()],
        block_references: vec!["refs-a".into(), "refs-b".into()],
    };
    let mut model = Model::default();
    for ordinal in 0..2 {
        let surface = SurfaceId(format!("surface-{ordinal}"));
        model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(ordinal as f64, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.55,
            },
            source_object: None::<SourceObjectAssociation>,
        });
        model.faces.push(Face {
            id: FaceId(format!("face-{ordinal}")),
            shell: ShellId("shell".into()),
            surface,
            sense: Sense::Reversed,
            loops: vec![
                LoopId(format!("loop-{ordinal}-0")),
                LoopId(format!("loop-{ordinal}-1")),
            ],
            name: None,
            color: None,
            tolerance: None,
        });
        for boundary in 0..2 {
            let loop_id = LoopId(format!("loop-{ordinal}-{boundary}"));
            let curve = CurveId(format!("bore-curve-{ordinal}-{boundary}"));
            let edge = EdgeId(format!("bore-edge-{ordinal}-{boundary}"));
            let coedge = CoedgeId(format!("bore-coedge-{ordinal}-{boundary}"));
            model.curves.push(Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(ordinal as f64, boundary as f64, 0.0),
                    axis: Vector3::new(0.0, 1.0, 0.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 2.55,
                },
                source_object: None,
            });
            model.edges.push(Edge {
                id: edge.clone(),
                curve: Some(curve),
                start: VertexId("vertex".into()),
                end: VertexId("vertex".into()),
                param_range: None,
                tolerance: None,
            });
            model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id,
                edge,
                next: coedge.clone(),
                previous: coedge.clone(),
                radial_next: coedge,
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
    }
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
        faces: vec![FaceId("face-0".into()), FaceId("face-1".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    let mut ir = CadIr::empty(Units::default());
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
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert_eq!(
        simple_hole_diameters(&ir, &templates, &[], &outputs),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert_eq!(
        hole_diameters_for_operations(&ir, &operations, &outputs),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
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
    single_hole.model.shells[0].faces = vec![FaceId("face-1".into())];
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
                origin: Point3::new(1.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
        )])
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut single_hole.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    origin.y = 91.0;
    assert_eq!(
        super::super::hole_axis_placements_for_operations(
            &single_hole,
            &single_operation,
            &single_output,
        ),
        std::collections::BTreeMap::from([(
            operations[1].clone(),
            HolePlacement::Axis {
                origin: Point3::new(1.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
        )])
    );
    let mut opposite_axis = single_hole.clone();
    let SurfaceGeometry::Cylinder { axis, .. } = &mut opposite_axis.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *axis = Vector3::new(0.0, -1.0, 0.0);
    for curve in opposite_axis
        .model
        .curves
        .iter_mut()
        .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
    {
        let CurveGeometry::Circle { axis, .. } = &mut curve.geometry else {
            unreachable!()
        };
        *axis = Vector3::new(0.0, -1.0, 0.0);
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
                origin: Point3::new(1.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
        )])
    );
    let mut different_radii = ir.clone();
    let SurfaceGeometry::Cylinder { radius, .. } = &mut different_radii.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *radius = 3.1;
    for curve in different_radii
        .model
        .curves
        .iter_mut()
        .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
    {
        let CurveGeometry::Circle { radius, .. } = &mut curve.geometry else {
            unreachable!()
        };
        *radius = 3.1;
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
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert!(hole_diameters_for_operations(
        &ir,
        &[operations[0].clone(), operations[0].clone()],
        &outputs,
    )
    .is_empty());
    let mut invalid_boundary = ir.clone();
    let CurveGeometry::Circle { radius, .. } = &mut invalid_boundary.model.curves[0].geometry
    else {
        unreachable!()
    };
    *radius += 0.1;
    assert!(hole_diameters_for_operations(&invalid_boundary, &operations, &outputs,).is_empty());
    let mut coincident_boundaries = ir.clone();
    let CurveGeometry::Circle { center, .. } = &mut coincident_boundaries.model.curves[1].geometry
    else {
        unreachable!()
    };
    center.y = 0.0;
    assert!(
        hole_diameters_for_operations(&coincident_boundaries, &operations, &outputs,).is_empty()
    );
    let mut nonparallel = single_hole.clone();
    let SurfaceGeometry::Cylinder { axis, .. } = &mut nonparallel.model.surfaces[1].geometry else {
        unreachable!()
    };
    *axis = Vector3::new(0.0, 0.0, 1.0);
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
        .push(RegionId("second-region".into()));
    assert!(hole_diameters_for_operations(&disconnected, &operations, &outputs).is_empty());
    let mut shared_carrier = ir.clone();
    shared_carrier.model.faces.push(Face {
        id: FaceId("unowned-shared-cylinder-face".into()),
        shell: ShellId("unowned-shell".into()),
        surface: SurfaceId("surface-0".into()),
        sense: Sense::Reversed,
        loops: vec![
            LoopId("unowned-loop-a".into()),
            LoopId("unowned-loop-b".into()),
        ],
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
    distinct.model.shells[0].faces.pop();
    distinct.model.bodies.push(Body {
        id: BodyId("second-body".into()),
        kind: BodyKind::Solid,
        regions: vec![RegionId("second-region".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    distinct.model.regions.push(Region {
        id: RegionId("second-region".into()),
        body: BodyId("second-body".into()),
        shells: vec![ShellId("second-shell".into())],
    });
    distinct.model.shells.push(Shell {
        id: ShellId("second-shell".into()),
        region: RegionId("second-region".into()),
        faces: vec![FaceId("face-1".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    distinct.model.faces[1].shell = ShellId("second-shell".into());
    let SurfaceGeometry::Cylinder { radius, .. } = &mut distinct.model.surfaces[1].geometry else {
        unreachable!()
    };
    *radius = 3.0;
    for curve in distinct
        .model
        .curves
        .iter_mut()
        .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
    {
        let CurveGeometry::Circle { radius, .. } = &mut curve.geometry else {
            unreachable!()
        };
        *radius = 3.0;
    }
    let distinct_outputs = std::collections::BTreeMap::from([
        ("hole-a".to_string(), vec![BodyId("body".into())]),
        ("hole-b".to_string(), vec![BodyId("second-body".into())]),
    ]);
    assert_eq!(
        simple_hole_diameters(
            &distinct,
            &templates,
            std::slice::from_ref(&group),
            &distinct_outputs,
        ),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(6.0)),
        ])
    );
    assert_eq!(
        hole_diameters_for_operations(&distinct, &operations, &distinct_outputs,),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(6.0)),
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
        &std::collections::BTreeMap::from([("hole-a".to_string(), vec![BodyId("body".into())],)]),
    )
    .is_empty());

    let mut chamfered = ir.clone();
    for bore in 0..2 {
        for end in 0..2 {
            let surface = SurfaceId(format!("cone-{bore}-{end}"));
            let face = FaceId(format!("cone-face-{bore}-{end}"));
            let loops = [
                LoopId(format!("cone-loop-{bore}-{end}-inner")),
                LoopId(format!("cone-loop-{bore}-{end}-outer")),
            ];
            chamfered.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Cone {
                    origin: Point3::new(bore as f64, end as f64, 0.0),
                    axis: Vector3::new(0.0, if end == 0 { 1.0 } else { -1.0 }, 0.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 0.0,
                    ratio: 1.0,
                    half_angle: std::f64::consts::FRAC_PI_4,
                },
                source_object: None,
            });
            chamfered.model.shells[0].faces.push(face.clone());
            chamfered.model.faces.push(Face {
                id: face,
                shell: ShellId("shell".into()),
                surface,
                sense: Sense::Reversed,
                loops: loops.to_vec(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary, (loop_id, radius)) in loops.into_iter().zip([2.55, 3.55]).enumerate() {
                let curve = CurveId(format!("cone-curve-{bore}-{end}-{boundary}"));
                let edge = EdgeId(format!("cone-edge-{bore}-{end}-{boundary}"));
                let coedge = CoedgeId(format!("cone-coedge-{bore}-{end}-{boundary}"));
                chamfered.model.curves.push(Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Circle {
                        center: Point3::new(bore as f64, end as f64, 0.0),
                        axis: Vector3::new(0.0, 1.0, 0.0),
                        ref_direction: Vector3::new(1.0, 0.0, 0.0),
                        radius,
                    },
                    source_object: None,
                });
                chamfered.model.edges.push(Edge {
                    id: edge.clone(),
                    curve: Some(curve),
                    start: VertexId("vertex".into()),
                    end: VertexId("vertex".into()),
                    param_range: None,
                    tolerance: None,
                });
                chamfered.model.coedges.push(Coedge {
                    id: coedge.clone(),
                    owner_loop: loop_id,
                    edge,
                    next: coedge.clone(),
                    previous: coedge.clone(),
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
                    diameter: cadmpeg_ir::features::Length(7.1),
                    angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                },
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::features::HoleKind::Chamfer {
                    diameter: cadmpeg_ir::features::Length(7.1),
                    angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
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
        id: SurfaceId("unrelated-cone".into()),
        geometry: SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.0,
            ratio: 0.0,
            half_angle: 0.0,
        },
        source_object: None,
    });
    unrelated.model.faces.push(Face {
        id: FaceId("unrelated-cone-face".into()),
        shell: ShellId("unrelated-shell".into()),
        surface: SurfaceId("unrelated-cone".into()),
        sense: Sense::Reversed,
        loops: vec![LoopId("unrelated-a".into()), LoopId("unrelated-b".into())],
        name: None,
        color: None,
        tolerance: None,
    });
    assert_eq!(
        super::super::simple_hole_chamfers(&unrelated, &templates, &outputs),
        super::super::simple_hole_chamfers(&chamfered, &templates, &outputs)
    );
    let mut unequal_chamfers = chamfered;
    let CurveGeometry::Circle { radius, .. } = &mut unequal_chamfers
        .model
        .curves
        .last_mut()
        .expect("required invariant")
        .geometry
    else {
        unreachable!()
    };
    *radius += 0.1;
    assert!(super::super::simple_hole_chamfers(&unequal_chamfers, &templates, &outputs).is_empty());

    let mut mismatched = ir;
    let SurfaceGeometry::Cylinder { radius, .. } = &mut mismatched.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *radius = 3.0;
    assert!(simple_hole_diameters(&mismatched, &templates, &[group], &outputs,).is_empty());
}
