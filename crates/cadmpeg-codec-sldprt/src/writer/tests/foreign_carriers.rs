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

#[test]
fn history_with_another_formats_native_references_is_written() {
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
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
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
