// SPDX-License-Identifier: Apache-2.0
//! Carriers, topology and history of another format that the semantic writer
//! writes exactly, or leaves out with the body that holds them.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, LoopId, PointId, ProceduralCurveId, ProceduralSurfaceId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{BodyKind, FaceLoops, Loop, LoopBoundary, Point, Vertex};

use crate::SldprtCodec;

fn cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry.bodies[0].name = None;
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    ir
}

fn plan_losses(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap()
        .report()
        .losses
        .clone()
}

fn loss<'a>(
    losses: &'a [cadmpeg_ir::report::loss::LossNote],
    code: &str,
) -> Option<&'a cadmpeg_ir::report::loss::LossNote> {
    losses.iter().find(|loss| loss.code.local_code() == code)
}

#[test]
fn procedural_carriers_are_written_from_their_solved_cache() {
    let mut ir = cube();
    let surface = &mut ir.geometry.surfaces[0];
    let SurfaceGeometry::Solved(solved) = surface.geometry.clone() else {
        panic!("cube surfaces are solved");
    };
    surface.geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint("other:model:surface-construction#0").unwrap(),
        cache: Some(solved),
    };
    let curve = &mut ir.geometry.curves[0];
    let CurveGeometry::Solved(solved) = curve.geometry.clone() else {
        panic!("cube curves are solved");
    };
    curve.geometry = CurveGeometry::Procedural {
        construction: ProceduralCurveId::mint("other:model:curve-construction#0").unwrap(),
        cache: Some(solved),
    };
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.bodies-omitted").is_none(),
        "{losses:#?}"
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.geometry.faces.len(), 6);
    assert_eq!(decoded.geometry.edges.len(), 12);
}

#[test]
fn a_vertex_loop_is_written_as_the_sole_fin_of_its_loop() {
    let mut ir = cube();
    let face = ir.geometry.faces[0].clone();
    let ring = face.loops.iter().next().unwrap().clone();
    let ring = ir.geometry.loops.iter().find(|row| row.id == ring).unwrap();
    let mut corners = Vec::new();
    for coedge in ring.coedges() {
        let coedge = ir
            .geometry
            .coedges
            .iter()
            .find(|row| &row.id == coedge)
            .unwrap();
        let edge = ir
            .geometry
            .edges
            .iter()
            .find(|row| row.id == coedge.edge)
            .unwrap();
        let vertex = ir
            .geometry
            .vertices
            .iter()
            .find(|row| Some(&row.id) == edge.vertices().next())
            .unwrap();
        let point = ir
            .geometry
            .points
            .iter()
            .find(|row| row.id == vertex.point)
            .unwrap();
        corners.push(point.position().get());
    }
    let count = f64::from(u32::try_from(corners.len()).unwrap());
    let centre = corners
        .iter()
        .fold(Point3::new(0.0, 0.0, 0.0), |sum, corner| {
            Point3::new(
                sum.x + corner.x / count,
                sum.y + corner.y / count,
                sum.z + corner.z / count,
            )
        });
    let point = PointId::mint("other:model:point#pole").unwrap();
    let vertex = VertexId::mint("other:model:vertex#pole").unwrap();
    let pole_loop = LoopId::mint("other:model:loop#pole").unwrap();
    ir.geometry.points.push(Point::new(
        point.clone(),
        FinitePoint3::new(centre).unwrap(),
        None,
    ));
    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.loops.push(Loop {
        id: pole_loop.clone(),
        face: face.id.clone(),
        boundary: LoopBoundary::Vertex {
            vertex,
            pcurves: Vec::new(),
        },
        native_ref: None,
    });
    let mut loops = face.loops.to_vec();
    loops.push(pole_loop);
    ir.geometry.faces[0].loops = FaceLoops::unspecified(loops);
    let decoded = crate::test_support::ir::encode_decode(&ir);
    let poles = decoded
        .geometry
        .loops
        .iter()
        .filter_map(|row| row.singular_vertex())
        .map(|(vertex, _)| vertex)
        .collect::<Vec<_>>();
    assert_eq!(poles.len(), 1, "{:#?}", decoded.geometry.loops);
    let vertex = decoded
        .geometry
        .vertices
        .iter()
        .find(|row| &row.id == poles[0])
        .unwrap();
    let position = decoded
        .geometry
        .points
        .iter()
        .find(|row| row.id == vertex.point)
        .unwrap()
        .position()
        .get();
    assert!(
        (position.x - centre.x).abs() < 1.0e-9
            && (position.y - centre.y).abs() < 1.0e-9
            && (position.z - centre.z).abs() < 1.0e-9,
        "{position:?} {centre:?}"
    );
    assert_eq!(decoded.geometry.faces.len(), 6);
}

#[test]
fn periodic_nurbs_carriers_are_written_with_their_periodic_logicals() {
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::geometry::SolvedCurveGeometry;

    let poles = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let knots = vec![0.0, 0.0, 1.0, 2.0, 2.0];
    let curve = NurbsCurve::new(
        1,
        knots.clone(),
        NurbsPoles3::Polynomial {
            points: poles.clone(),
        },
        true,
    )
    .unwrap();
    let mut bytes = Vec::new();
    crate::writer::write_nurbs_curve(&mut bytes, 2, &curve, &mut 3, 1.0, "test:curve#periodic")
        .unwrap();
    let carrier = crate::brep::spline::scan_curve_carriers(&bytes, &mut Vec::new())
        .remove(&2)
        .expect("curve carrier");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(decoded)) = carrier.geometry else {
        panic!("expected a NURBS curve");
    };
    assert!(decoded.periodic());
    assert_eq!(decoded.knots().as_slice(), knots.as_slice());

    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, knots.clone(), true),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            poles
                .iter()
                .map(|pole| vec![*pole, Point3::new(pole.x, pole.y, 1.0)])
                .collect(),
            None,
        ),
        false,
    )
    .unwrap();
    let mut bytes = Vec::new();
    crate::writer::write_nurbs_surface(
        &mut bytes,
        2,
        &surface,
        &mut 3,
        1.0,
        "test:surface#periodic",
    )
    .unwrap();
    let carrier = crate::brep::spline::scan_surface_carriers(&bytes, &mut Vec::new())
        .remove(&2)
        .expect("surface carrier");
    let Some(SolvedSurfaceGeometry::Nurbs(decoded)) = carrier.geometry.solved() else {
        panic!("expected a NURBS surface");
    };
    assert!(decoded.u_periodic() && !decoded.v_periodic());
}

#[test]
fn display_mesh_groups_are_left_out_and_named() {
    use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh, TessellationTriangleGroup};
    let mut ir = cube();
    let mesh = Tessellation::new(
        "other:model:tessellation#mesh".try_into().unwrap(),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap()
    .with_triangle_groups(vec![TessellationTriangleGroup::new(None, vec![0])])
    .unwrap();
    ir.model.tessellations.push(mesh);
    let losses = plan_losses(&ir);
    assert!(
        losses.iter().any(
            |loss| loss.code.local_code() == "writer.tessellation-metadata-omitted"
                && loss
                    .message
                    .starts_with("1 tessellation triangle group record(s)")
        ),
        "{losses:#?}"
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.model.tessellations.len(), 1);
}

#[test]
fn a_body_the_encoder_cannot_write_is_left_out_and_the_others_are_written() {
    let mut ir = cube();
    let mut wire = ir.geometry.bodies[0].clone();
    wire.id = BodyId::mint("other:model:body#wire").unwrap();
    wire.kind = BodyKind::Wire;
    ir.geometry.bodies.push(wire);
    let losses = plan_losses(&ir);
    let bodies = loss(&losses, "writer.bodies-omitted").unwrap();
    assert!(
        bodies.message.contains("other:model:body#wire")
            && !bodies
                .message
                .contains(&format!("{} (", ir.geometry.bodies[0].id.as_str())),
        "{}",
        bodies.message
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.geometry.faces.len(), 6);
}

#[test]
fn a_procedural_surface_without_a_cache_leaves_its_body_out() {
    let mut ir = cube();
    let surface = ir.geometry.surfaces[0].id.as_str().to_owned();
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint("other:model:surface-construction#0").unwrap(),
        cache: None,
    };
    let losses = plan_losses(&ir);
    let bodies = loss(&losses, "writer.bodies-omitted").unwrap();
    assert!(
        bodies.message.contains(ir.geometry.bodies[0].id.as_str())
            && bodies.message.contains(&surface),
        "{}",
        bodies.message
    );
    assert!(loss(&losses, "output.empty").is_some(), "{losses:#?}");
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert!(decoded.geometry.faces.is_empty());
}

/// The cube with a triangle sketch and an extrude of it, as another format
/// states them: each feature carries that format's own native reference.
fn cube_with_foreign_history() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId,
    };

    let mut ir = cube();
    let sketch_id = SketchId::mint("other:model:sketch#profile").unwrap();
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 10.0),
    ];
    let entities = (0..3)
        .map(|index| SketchEntityId::mint(format!("other:model:sketch-entity#{index}")).unwrap())
        .collect::<Vec<_>>();
    for index in 0..3 {
        ir.model.sketch_entities.push(SketchEntity::new(
            entities[index].clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: points[index],
                end: points[(index + 1) % 3],
            })
            .unwrap(),
        ));
    }
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![entities
            .iter()
            .cloned()
            .map(|entity| SketchEntityUse {
                entity,
                reversed: false,
            })
            .collect()])
        .unwrap(),
        native_ref: None,
    });
    let feature = |id: &str, ordinal: u64, name: &str, definition| Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(format!("other:native#{name}")),
    };
    let sketch_feature = feature(
        "other:model:feature#profile",
        0,
        "Profile",
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
        }),
    );
    let extrude = feature(
        "other:model:feature#boss",
        1,
        "Boss",
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)),
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                    .unwrap(),
                source: None,
            },
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
                    },
                    draft: None,
                },
            },
            op: BooleanOp::Join,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }),
    );
    let (sketch_feature_id, extrude_id) = (sketch_feature.id.clone(), extrude.id.clone());
    ir.model.features.extend([sketch_feature, extrude]);
    ir.model
        .set_feature_regeneration_parent(&extrude_id, &sketch_feature_id)
        .unwrap();
    ir
}

#[test]
fn history_with_another_formats_native_references_is_written() {
    let ir = cube_with_foreign_history();
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.model.sketches.len(), 1);
}

/// Push a feature of another format named `name`, with the input topology
/// `other:history-input:state#{key}` holding one face and one edge, and
/// return its id. `definition` builds the operation from that state, face and
/// edge.
fn push_foreign_feature(
    ir: &mut cadmpeg_ir::CadIr,
    key: &str,
    name: &str,
    definition: impl FnOnce(
        cadmpeg_ir::ids::FeatureInputTopologyId,
        cadmpeg_ir::ids::HistoricalFaceId,
        cadmpeg_ir::ids::HistoricalEdgeId,
    ) -> cadmpeg_ir::features::FeatureDefinition,
) -> cadmpeg_ir::features::FeatureId {
    use cadmpeg_ir::features::{DistinctMembers, Feature, FeatureId, FeatureInputTopology};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalEdgeId, HistoricalFaceId};

    let id = FeatureId::mint(format!("other:model:feature#{key}")).unwrap();
    let state = FeatureInputTopologyId::mint(format!("other:history-input:state#{key}")).unwrap();
    let face = HistoricalFaceId::mint(format!("other:history-input:face#{key}:1")).unwrap();
    let edge = HistoricalEdgeId::mint(format!("other:history-input:edge#{key}:1")).unwrap();
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            id: state.clone(),
            input_of: id.clone(),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::try_from(vec![face.clone()]).unwrap(),
            edges: DistinctMembers::try_from(vec![edge.clone()]).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: Vec::new(),
            native_ref: None,
        });
    let ordinal = u64::try_from(ir.model.features.len()).unwrap();
    ir.model.features.push(Feature {
        product_definition: None,
        id: id.clone(),
        ordinal,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition(
            state, face, edge,
        )),
        native_ref: None,
    });
    id
}

/// Push a length parameter `name` = `value` mm owned by `owner`.
fn push_length_parameter(
    ir: &mut cadmpeg_ir::CadIr,
    owner: &cadmpeg_ir::features::FeatureId,
    name: &str,
    value: f64,
) {
    use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};

    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: ParameterId::mint(format!("other:model:parameter#{name}")).unwrap(),
        owner: Some(owner.clone()),
        ordinal: u32::try_from(
            ir.model
                .parameters
                .iter()
                .filter(|parameter| parameter.owner.as_ref() == Some(owner))
                .count(),
        )
        .unwrap(),
        name: name.into(),
        expression: format!("{value} mm"),
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(value).unwrap(),
        )),
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
}

/// `cube_with_foreign_history` plus a fillet whose edges are stated in its
/// input topology, with a tangency weight and an owned radius parameter `d7`.
fn cube_with_foreign_fillet() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
    use cadmpeg_ir::features::{
        EdgeSelection, FeatureDefinition, FeatureOperation, NonEmptyMembers,
    };

    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "round", "Round", |state, _, edge| {
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::historical(
                    state,
                    vec![edge],
                    "other:design:edge-operand#round".into(),
                )
                .unwrap(),
                radius: RadiusSpec::Constant {
                    radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
                },
                tangency_weight: cadmpeg_ir::scalar::FiniteReal::new(1.0),
            }),
        })
    });
    push_length_parameter(&mut ir, &id, "d7", 2.0);
    ir
}

#[test]
fn a_fillet_on_input_topology_edges_is_written_with_its_radius_and_named() {
    let ir = cube_with_foreign_fillet();
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
    );
    for (code, named) in [
        (
            "writer.selection-identities-omitted",
            "other:model:feature#round",
        ),
        ("writer.feature-fields-omitted", "other:model:feature#round"),
        (
            "writer.parameters-renamed",
            "other:model:parameter#d7 (d7 as D1)",
        ),
    ] {
        let note = loss(&losses, code).unwrap_or_else(|| panic!("{code}: {losses:#?}"));
        assert!(note.message.contains(named), "{}", note.message);
    }

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let fillet = decoded
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round"))
        .expect("the fillet decodes back");
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Fillet { groups },
    ) = fillet.evaluation.definition()
    else {
        panic!("{:?}", fillet.evaluation.definition());
    };
    let [group] = &**groups else {
        panic!("{groups:?}");
    };
    assert_eq!(
        group.edges,
        cadmpeg_ir::features::EdgeSelection::Native("other:design:edge-operand#round".into())
    );
    assert!(
        matches!(
            group.radius,
            cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant { radius }
                if radius.get() == 2.0
        ),
        "{:?}",
        group.radius
    );
    let radius = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&fillet.id))
        .expect("the radius parameter decodes back");
    assert_eq!(radius.name, "D1");
    assert_eq!(
        radius.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0).unwrap()
        ))
    );
}

#[test]
fn a_fillet_the_history_drop_removes_is_named_by_that_drop_alone() {
    let mut ir = cube_with_foreign_fillet();
    let fillet = ir.model.features.last_mut().unwrap();
    fillet.evaluation.edit(|definition, _| {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Fillet { groups },
        ) = definition
        else {
            panic!("fillet");
        };
        let group = groups[0].clone();
        *groups =
            cadmpeg_ir::features::NonEmptyMembers::try_from(vec![group.clone(), group]).unwrap();
    });
    let losses = plan_losses(&ir);
    let dropped = loss(&losses, "writer.features-omitted").expect("named drop");
    assert!(
        dropped.message.contains("other:model:feature#round")
            && dropped.message.contains("other:model:parameter#d7"),
        "{}",
        dropped.message
    );
    for code in [
        "writer.selection-identities-omitted",
        "writer.feature-fields-omitted",
        "writer.parameters-renamed",
    ] {
        assert!(loss(&losses, code).is_none(), "{code}: {losses:#?}");
    }
}

#[test]
fn a_delete_face_on_input_topology_faces_decodes_back_and_is_named() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "remove", "Remove", |state, face, _| {
        FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::historical(
                state,
                vec![face],
                "other:design:face-operand#remove".into(),
            )
            .unwrap(),
            heal: true,
        })
    });
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
    );
    let note = loss(&losses, "writer.selection-identities-omitted").expect("named selection");
    assert!(note.message.contains(id.as_str()), "{}", note.message);

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let delete = decoded
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Remove"))
        .expect("the delete-face feature decodes back");
    assert_eq!(
        delete.evaluation.definition(),
        &FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::Native("other:design:face-operand#remove".into()),
            heal: true,
        })
    );
}

#[test]
fn a_sketch_curve_outside_every_profile_is_left_out_and_its_sketch_is_written() {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    };
    let mut ir = cube_with_foreign_history();
    let sketch = ir.model.sketches[0].id.clone();
    ir.model.sketch_entities.push(SketchEntity::new(
        SketchEntityId::mint("other:model:sketch-entity#construction").unwrap(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(5.0, 5.0),
        })
        .unwrap(),
    ));
    let losses = plan_losses(&ir);
    let notes = losses
        .iter()
        .filter(|loss| loss.code.local_code() == "writer.features-omitted")
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0]
            .message
            .contains("without 1 such curve(s): other:model:sketch-entity#construction"),
        "{}",
        notes[0].message
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.model.sketches.len(), 1);
}

#[test]
fn every_shell_of_a_region_is_written_in_its_shell_chain() {
    use cadmpeg_ir::ids::ShellId;
    use cadmpeg_ir::topology::Shell;
    let mut ir = cube();
    let region = ir.geometry.regions[0].id.clone();
    let second = ShellId::mint("other:model:shell#second").unwrap();
    ir.geometry.shells.push(Shell::new(second.clone(), region));
    ir.geometry.regions[0].shells.push(second.clone());
    for face in ir.geometry.faces.iter_mut().skip(3) {
        face.shell = second.clone();
    }
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.geometry.faces.len(), 6);
    assert_eq!(decoded.geometry.shells.len(), 2);
    assert_eq!(decoded.geometry.regions[0].shells.len(), 2);
}

#[test]
fn two_coincident_circles_are_written_without_a_shared_seam_vertex() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId,
    };
    let mut ir = cube();
    let sketch = SketchId::mint("other:model:sketch#circles").unwrap();
    let circles = (0..2)
        .map(|index| {
            SketchEntityId::mint(format!("other:model:sketch-entity#circle-{index}")).unwrap()
        })
        .collect::<Vec<_>>();
    for circle in &circles {
        ir.model.sketch_entities.push(SketchEntity::new(
            circle.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
            })
            .unwrap(),
        ));
    }
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: Some("Circles".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(
            circles
                .iter()
                .map(|entity| {
                    vec![SketchEntityUse {
                        entity: entity.clone(),
                        reversed: false,
                    }]
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("other:model:feature#circles").unwrap(),
        ordinal: 0,
        name: Some("Circles".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
        native_ref: Some("other:native#circles".into()),
    });
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.model.sketches.len(), 1);
    let report = cadmpeg_ir::validate::validate_neutral(&decoded, Vec::new()).unwrap();
    let incompatible = report
        .findings
        .iter()
        .filter(|finding| finding.message.contains("locus is incompatible"))
        .collect::<Vec<_>>();
    assert!(incompatible.is_empty(), "{incompatible:#?}");
}

#[test]
fn a_signed_sphere_is_written_reversed_and_a_signed_or_spindle_torus_keeps_its_sense() {
    use cadmpeg_ir::geometry::analytic::{SphereSurface, TorusSurface};
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::topology::Sense;
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let cases = [
        (
            SolvedSurfaceGeometry::Sphere(
                SphereSurface::try_new(origin, axis, reference, -2.0).unwrap(),
            ),
            true,
        ),
        (
            SolvedSurfaceGeometry::Torus(
                TorusSurface::try_new(origin, axis, reference, 2.0, -0.5).unwrap(),
            ),
            false,
        ),
        (
            SolvedSurfaceGeometry::Torus(
                TorusSurface::try_new(origin, axis, reference, 2.0, 3.0).unwrap(),
            ),
            false,
        ),
    ];
    for (geometry, reversed) in cases {
        let mut ir = cube();
        let surface = ir.geometry.surfaces[0].id.clone();
        ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(geometry.clone());
        let sense = ir
            .geometry
            .faces
            .iter()
            .find(|face| face.surface == surface)
            .unwrap()
            .sense;
        let losses = plan_losses(&ir);
        assert!(
            loss(&losses, "writer.bodies-omitted").is_none(),
            "{losses:#?}"
        );
        let decoded = crate::test_support::ir::encode_decode(&ir);
        let (written, face) =
            decoded
                .geometry
                .faces
                .iter()
                .find_map(|face| {
                    let surface = decoded
                        .geometry
                        .surfaces
                        .iter()
                        .find(|row| row.id == face.surface)?;
                    match surface.geometry.solved()? {
                        found @ (SolvedSurfaceGeometry::Sphere(_)
                        | SolvedSurfaceGeometry::Torus(_)) => Some((found.clone(), face)),
                        _ => None,
                    }
                })
                .unwrap();
        match (&geometry, &written) {
            (SolvedSurfaceGeometry::Sphere(stated), SolvedSurfaceGeometry::Sphere(read)) => {
                assert!((read.radius().get() - stated.radius().get().abs()).abs() < 1.0e-9);
            }
            (SolvedSurfaceGeometry::Torus(stated), SolvedSurfaceGeometry::Torus(read)) => {
                assert!(
                    (read.minor_radius().get() - stated.minor_radius().get().abs()).abs() < 1.0e-9
                );
                assert!((read.major_radius().get() - stated.major_radius().get()).abs() < 1.0e-9);
            }
            other => panic!("{other:?}"),
        }
        let expected = if reversed {
            match sense {
                Sense::Forward => Sense::Reversed,
                Sense::Reversed => Sense::Forward,
            }
        } else {
            sense
        };
        assert_eq!(face.sense, expected);
    }
}

#[test]
fn a_face_binding_whose_colour_differs_from_the_face_colour_is_named_and_the_body_written() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::topology::Color;
    let mut ir = cube();
    let face = ir.geometry.faces[0].id.clone();
    ir.geometry.faces[0].color = Color::new(1.0, 0.0, 0.0, 1.0);
    let appearance = Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint("other:model:appearance#blue").unwrap(),
        name: Some("Blue".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Color::new(0.0, 0.0, 1.0, 1.0),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
        native_ref: None,
    };
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "other:model:appearance-binding#blue".try_into().unwrap(),
        target: AppearanceTarget::Face(face).into(),
        appearance: appearance.id.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
        native_ref: None,
    });
    ir.model.appearances.push(appearance);
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.bodies-omitted").is_none(),
        "{losses:#?}"
    );
    assert!(
        losses.iter().any(
            |loss| loss.code.local_code() == "writer.appearance-bindings-omitted"
                && loss.message.contains("other:model:appearance-binding#blue")
        ),
        "{losses:#?}"
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.geometry.faces.len(), 6);
}

#[test]
fn an_equation_is_written_as_its_displayed_value_and_named() {
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    let mut ir = cube_with_foreign_history();
    let owner = ir.model.features[1].id.clone();
    let base = ParameterId::mint("other:model:parameter#d5").unwrap();
    let parameter =
        |id: ParameterId, ordinal, name: &str, expression: &str, dependencies| DesignParameter {
            product_definition: None,
            id,
            owner: Some(owner.clone()),
            ordinal,
            name: name.into(),
            expression: expression.into(),
            display: None,
            value: Some(ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(12.7).unwrap(),
            )),
            dependencies,
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(format!("other:native#{name}")),
        };
    ir.model.parameters.push(parameter(
        base.clone(),
        0,
        "d5",
        "12.7 mm",
        cadmpeg_ir::features::DistinctMembers::default(),
    ));
    ir.model.parameters.push(parameter(
        ParameterId::mint("other:model:parameter#d6").unwrap(),
        1,
        "d6",
        "d5",
        std::iter::once(base).collect(),
    ));
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
    );
    let note = loss(&losses, "writer.parameter-expressions-omitted").expect("named equation");
    assert!(
        note.message.contains("other:model:parameter#d6 (d6 = d5)"),
        "{}",
        note.message
    );
    assert!(!note.message.contains("parameter#d5 "), "{}", note.message);
    let decoded = crate::test_support::ir::encode_decode(&ir);
    let written = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d6")
        .expect("d6 decodes back");
    assert_eq!(
        written.value,
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(12.7).unwrap()
        ))
    );
    assert!(
        crate::history::literals::parse_parameter_literal(&written.expression).is_some(),
        "{}",
        written.expression
    );
    let report = cadmpeg_ir::validate::validate_neutral(&decoded, Vec::new()).unwrap();
    let textless = report
        .findings
        .iter()
        .filter(|finding| finding.message.contains("holds no text token"))
        .collect::<Vec<_>>();
    assert!(textless.is_empty(), "{textless:#?}");
}

#[test]
fn a_displayed_value_parameter_the_history_drop_removes_is_named_by_that_drop_alone() {
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    let mut ir = cube_with_foreign_history();
    let owner = ir.model.features[1].id.clone();
    let parameter = |name: &str, owner| DesignParameter {
        product_definition: None,
        id: ParameterId::mint(format!("other:model:parameter#{name}")).unwrap(),
        owner,
        ordinal: 0,
        name: name.into(),
        expression: "width * 2".into(),
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(12.7).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    ir.model.parameters.push(parameter("kept", Some(owner)));
    ir.model.parameters.push(parameter("global", None));
    let losses = plan_losses(&ir);
    let dropped = loss(&losses, "writer.features-omitted").expect("named drop");
    assert!(
        dropped
            .message
            .contains("parameters [other:model:parameter#global]"),
        "{}",
        dropped.message
    );
    let note = loss(&losses, "writer.parameter-expressions-omitted").expect("named equation");
    assert!(
        note.message.contains("writes 1 parameter(s)"),
        "{}",
        note.message
    );
    assert!(
        note.message
            .contains("other:model:parameter#kept (kept = width * 2)"),
        "{}",
        note.message
    );
    assert!(
        !note.message.contains("parameter#global"),
        "{}",
        note.message
    );
}

#[test]
fn a_curve_shared_by_two_profiles_is_written_once() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId,
    };
    let mut ir = cube();
    let sketch = SketchId::mint("other:model:sketch#squares").unwrap();
    // Two squares side by side; the line at x = 10 bounds both.
    let lines = [
        ((0.0, 0.0), (10.0, 0.0)),
        ((10.0, 0.0), (10.0, 10.0)),
        ((10.0, 10.0), (0.0, 10.0)),
        ((0.0, 10.0), (0.0, 0.0)),
        ((10.0, 0.0), (20.0, 0.0)),
        ((20.0, 0.0), (20.0, 10.0)),
        ((20.0, 10.0), (10.0, 10.0)),
    ];
    let ids = (0..lines.len())
        .map(|index| {
            SketchEntityId::mint(format!("other:model:sketch-entity#line-{index}")).unwrap()
        })
        .collect::<Vec<_>>();
    for (id, ((x0, y0), (x1, y1))) in ids.iter().zip(lines) {
        ir.model.sketch_entities.push(SketchEntity::new(
            id.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(x0, y0),
                end: Point2::new(x1, y1),
            })
            .unwrap(),
        ));
    }
    let profile = |uses: &[(usize, bool)]| {
        uses.iter()
            .map(|&(index, reversed)| SketchEntityUse {
                entity: ids[index].clone(),
                reversed,
            })
            .collect::<Vec<_>>()
    };
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: Some("Squares".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![
            profile(&[(0, false), (1, false), (2, false), (3, false)]),
            profile(&[(1, true), (4, false), (5, false), (6, false)]),
        ])
        .unwrap(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("other:model:feature#squares").unwrap(),
        ordinal: 0,
        name: Some("Squares".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
        native_ref: Some("other:native#squares".into()),
    });
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
    );
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.model.sketches.len(), 1);
    let curves = decoded
        .model
        .sketch_entities
        .iter()
        .filter(|entity| {
            !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        })
        .count();
    assert_eq!(curves, lines.len());
    assert_eq!(decoded.model.sketches[0].profiles.len(), 2);
}

#[test]
fn a_carrier_that_states_a_fit_tolerance_is_written_and_the_tolerance_named() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::geometry::FitTolerance;
    let mut ir = cube();
    let mut nurbs = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    nurbs.set_cache_fit_tolerance(Some(FitTolerance::try_new(0.01).unwrap()));
    let surface = ir.geometry.surfaces[0].id.clone();
    ir.geometry.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.bodies-omitted").is_none(),
        "{losses:#?}"
    );
    let note = loss(&losses, "writer.cache-tolerances-omitted").expect("named tolerance");
    assert!(note.message.contains("largest 0.01 mm"), "{}", note.message);
    assert!(note.message.contains(surface.as_str()), "{}", note.message);
}

#[test]
fn a_nurbs_sketch_curve_used_reversed_decodes_back_with_its_profile() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId,
    };
    let mut ir = cube();
    let sketch = SketchId::mint("other:model:sketch#spline").unwrap();
    let ids = (0..3)
        .map(|index| {
            SketchEntityId::mint(format!("other:model:sketch-entity#spline-{index}")).unwrap()
        })
        .collect::<Vec<_>>();
    let line = |start: (f64, f64), end: (f64, f64)| {
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap()
    };
    // The spline runs from (0, 0) to (0, 10); the profile uses it from its end.
    let spline = PcurveNurbs::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        PcurveNurbsPoles::from_lanes(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(-3.0, 5.0),
                Point2::new(0.0, 10.0),
            ],
            None,
        )
        .unwrap(),
        false,
    )
    .unwrap();
    let geometries = [
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (0.0, 10.0)),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve: spline }).unwrap(),
    ];
    for (id, geometry) in ids.iter().zip(geometries) {
        ir.model
            .sketch_entities
            .push(SketchEntity::new(id.clone(), sketch.clone(), geometry));
    }
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: Some("Spline".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![ids
            .iter()
            .enumerate()
            .map(|(index, entity)| SketchEntityUse {
                entity: entity.clone(),
                reversed: index == 2,
            })
            .collect()])
        .unwrap(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("other:model:feature#spline").unwrap(),
        ordinal: 0,
        name: Some("Spline".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
        native_ref: Some("other:native#spline".into()),
    });
    let decoded = crate::test_support::ir::encode_decode(&ir);
    assert_eq!(decoded.model.sketches.len(), 1);
    let kinds = decoded
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match *entity.geometry.definition() {
            SketchGeometryDefinition::Point { .. } => None,
            SketchGeometryDefinition::Nurbs { .. } => Some("nurbs"),
            _ => Some("other"),
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    assert!(kinds.contains(&"nurbs"), "{kinds:?}");
    assert_eq!(decoded.model.sketches[0].profiles.len(), 1);
}

#[test]
fn a_face_loop_whose_coedges_do_not_meet_is_named() {
    use cadmpeg_ir::topology::Sense;
    let mut ir = cube();
    let face = ir.geometry.faces[0].clone();
    let first = ir
        .geometry
        .loops
        .iter()
        .find(|row| row.face == face.id)
        .unwrap()
        .coedges()[0]
        .clone();
    assert!(crate::writer::reduce::unchained_face_loops(&ir).is_none());
    for coedge in &mut ir.geometry.coedges {
        if coedge.id == first {
            coedge.sense = match coedge.sense {
                Sense::Forward => Sense::Reversed,
                Sense::Reversed => Sense::Forward,
            };
        }
    }
    let note = crate::writer::reduce::unchained_face_loops(&ir).expect("named face");
    assert_eq!(note.code.local_code(), "writer.face-loops-unchained");
    assert!(note.message.contains(face.id.as_str()), "{}", note.message);
    // The faces across the edges at the break are named with it.
    assert!(note.message.starts_with("4 face(s)"), "{}", note.message);
}
