// SPDX-License-Identifier: Apache-2.0
//! Document-driven synthesis and retained-source admission.

use super::*;
use crate::test_support::plan_at;
use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use cadmpeg_ir::codec::write::{ArenaCoverage, ArenaDisposition};
use cadmpeg_ir::report::export::WritePath;

#[test]
fn unsupported_arena_is_dropped_while_neutral_geometry_is_written() {
    let decoded = IgesCodec.decode(&mut Cursor::new(point_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;"
    )), &DecodeOptions::default()).expect("a point fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.model.assets.push(cadmpeg_ir::assets::Asset {
        id: "test:model:asset#external"
            .try_into()
            .expect("valid authored test identity"),
        name: None,
        media_type: None,
        content: cadmpeg_ir::assets::AssetContent::External {
            uri: cadmpeg_core::text::NonBlankString::new("resource.png")
                .expect("external asset URI is nonblank"),
        },
        native_ref: None,
    });
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("unsupported assets are omitted");
    let ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("synthesis declares arena coverage");
    };
    assert_eq!(coverage.assets, ArenaDisposition::Dropped);
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::WriterArenaOmitted.kind()
            && loss.message.contains("test:model:asset#external")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("held geometry writes while unsupported assets are reported");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written geometry decodes");
    assert_eq!(output.ir().model.points.len(), ir.model.points.len());
}

#[test]
fn unrepresented_source_never_reaches_replay() {
    let bytes = explicit_tetrahedron_solid_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("explicit tetrahedron fixture decodes");
    let mut fidelity = decoded.source_fidelity().clone();
    let image = fidelity
        .retained_records()
        .iter()
        .find(|(_, record)| record.data() == Some(bytes.as_slice()))
        .map(|(id, _)| id.to_string())
        .expect("retained source image");
    fidelity
        .mark_untyped(&image, [cadmpeg_ir::ArenaClass::Design])
        .expect("source record exists");
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("an encodable target falls back to synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented { classes }) if classes.contains(&cadmpeg_ir::ArenaClass::Design)
    ));
}

#[test]
fn container_source_bytes_cannot_supply_undeclared_geometry() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .expect("container-level tetrahedron fixture decodes");
    assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::container());
    assert!(decoded
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Model));
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("a container can synthesize its held content");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("held container content writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written container content decodes");
    assert!(output.ir().model.bodies.is_empty());
    assert!(output.ir().model.points.is_empty());
    assert!(output.ir().model.faces.is_empty());
}

#[test]
fn encodable_free_surface_writes_beside_an_independent_solid() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("independent solid fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        native_ref: None,
        id: "test:model:surface#free"
            .try_into()
            .expect("valid authored test identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(10.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("authored sphere has a valid frame and radius"),
        )),
        source_object: None,
    });
    let plan = plan_at(IgesVersion::V5_3, &ir, None)
        .expect("independent solid receives a valid synthesis plan");
    assert!(
        plan.report().losses.iter().all(|loss| {
            loss.severity != cadmpeg_ir::report::Severity::Error
                || loss.code != IgesLossCode::WriterEntityOmitted.kind()
        }),
        "free surface writes beside a solid: {:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("supported solid writes beside a free surface");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written independent solid decodes");
    assert_eq!(
        output.ir().model.faces.len(),
        decoded.ir().model.faces.len()
    );
    assert_eq!(
        output.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    assert_eq!(
        output.ir().model.surfaces.len(),
        decoded.ir().model.surfaces.len() + 1
    );
}

#[test]
fn unsupported_uv_images_preserve_the_model_space_boundary() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::explicit_multi_pcurve_loop_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("multi-pcurve loop fixture decodes");
    let mut ir = decoded.ir().clone();
    assert!(!ir.model.pcurves.is_empty());
    for curve in &mut ir.model.pcurves {
        use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
        use cadmpeg_ir::math::Point2;
        let [start, end] = curve.parameter_range().unwrap().get();
        curve.geometry = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::new(
                1,
                vec![start, start, end, end],
                PcurveNurbsPoles::Polynomial {
                    points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
                },
                false,
            )
            .unwrap(),
        };
    }
    for coedge in &mut ir.model.coedges {
        for pcurve in &mut coedge.pcurves {
            pcurve.isoparametric = None;
        }
    }
    let plan = plan_at(IgesVersion::V5_3, &ir, None)
        .expect("model-space boundary receives a valid synthesis plan");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::WriterPcurvesOmitted.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
    }));
    for pcurve in &ir.model.pcurves {
        assert!(plan.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::WriterPcurvesOmitted.kind()
                && loss.message.contains(pcurve.id.as_str())
        }));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("supported model-space boundary writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written model-space boundary decodes");
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(output.ir().model.edges.len(), ir.model.edges.len());
    assert!(output.ir().model.pcurves.is_empty());
}

#[test]
fn native_arena_declarations_cover_the_decoded_input() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("explicit tetrahedron fixture decodes");
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).expect("tetrahedron plans an export");
    cadmpeg_test_support::contract::assert_declarations_cover_input(
        "iges",
        decoded.ir(),
        plan.report(),
    );
}

#[test]
fn face_with_missing_carrier_reports_each_removed_boundary_record() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::trimmed_plane_with_inner_loop_file(
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("trimmed sheet decodes");
    let mut ir = decoded.ir().clone();
    ir.model.edges[0].carrier = cadmpeg_ir::topology::EdgeCarrier::new(None, None).unwrap();
    let (stripped, losses) = super::super::admission::strip_unusable_face_topology(&ir);
    let stripped = stripped.expect("face with missing carrier is removed");
    assert!(!ir.model.loops.is_empty() && !ir.model.coedges.is_empty());
    assert!(stripped.model.loops.len() < ir.model.loops.len());
    assert!(stripped.model.coedges.len() < ir.model.coedges.len());
    for id in ir
        .model
        .loops
        .iter()
        .filter(|record| !stripped.model.loops.iter().any(|kept| kept.id == record.id))
        .map(|record| record.id.as_str())
        .chain(
            ir.model
                .coedges
                .iter()
                .filter(|record| {
                    !stripped
                        .model
                        .coedges
                        .iter()
                        .any(|kept| kept.id == record.id)
                })
                .map(|record| record.id.as_str()),
        )
    {
        assert!(
            losses.iter().any(|loss| loss.message.contains(id)),
            "missing loss for {id}"
        );
    }
}

#[test]
fn unsupported_solid_reports_its_omitted_topology() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("solid fixture decodes");
    let ir = decoded.ir();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let graph = super::super::degrade::graph(&ctx, ir, IgesVersion::V4_0, "unsupported solid")
        .expect("degradation reports unsupported topology");
    for id in ir
        .model
        .faces
        .iter()
        .map(|record| record.id.as_str())
        .chain(ir.model.loops.iter().map(|record| record.id.as_str()))
        .chain(ir.model.coedges.iter().map(|record| record.id.as_str()))
    {
        assert!(
            graph.losses.iter().any(|loss| loss.message.contains(id)),
            "missing loss for {id}: {:#?}",
            graph.losses
        );
    }
}

#[test]
fn dangling_coedge_keeps_other_faces_and_reports_opened_solid() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("solid fixture decodes");
    let mut ir = decoded.ir().clone();
    let coedge_id = ir.model.coedges[0].id.to_string();
    let body_id = ir.model.bodies[0].id.to_string();
    ir.model.coedges[0].edge = "test:model:edge#missing".try_into().unwrap();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("invalid face degrades");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&coedge_id)));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&body_id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("remaining faces write");
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("remaining faces decode");
    assert!(!after.ir().model.faces.is_empty());
    assert!(after.ir().model.faces.len() < ir.model.faces.len());
    assert!(after
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.kind != cadmpeg_ir::topology::BodyKind::Solid));
}

#[test]
fn only_a_proven_spherical_pole_becomes_a_vertex_use() {
    use cadmpeg_ir::topology::{EdgeCarrier, LoopBoundary, LoopRing, Sense};
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("solid fixture decodes");
    let mut ir = decoded.ir().clone();
    let boundary = &ir.model.loops[0];
    let LoopBoundary::Ring(ring) = &boundary.boundary else {
        panic!("coedge ring");
    };
    let mut ordered = ring.coedges().to_vec();
    let vertex_uses = ring.vertex_uses().to_vec();
    let first = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == ordered[0])
        .unwrap()
        .clone();
    let first_edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == first.edge)
        .unwrap();
    let vertex = match first.sense {
        Sense::Forward => first_edge.end.clone(),
        Sense::Reversed => first_edge.start.clone(),
    };
    let mut pole_edge = first_edge.clone();
    pole_edge.id = "test:model:edge#pole".try_into().unwrap();
    pole_edge.native_ref = None;
    pole_edge.start = vertex.clone();
    pole_edge.end = vertex;
    pole_edge.carrier = EdgeCarrier::new(None, None).unwrap();
    let mut pole_use = first;
    pole_use.id = "test:model:coedge#pole".try_into().unwrap();
    pole_use.native_ref = None;
    pole_use.edge = pole_edge.id.clone();
    pole_use.radial_next = pole_use.id.clone();
    pole_use.pcurves.clear();
    ordered.insert(1, pole_use.id.clone());
    ir.model.loops[0].boundary = LoopBoundary::Ring(LoopRing::new(ordered, vertex_uses).unwrap());
    ir.model.edges.push(pole_edge);
    ir.model.coedges.push(pole_use);
    let (unchanged, losses) = crate::writer::admission::pole_edges(&ir).unwrap();
    assert!(losses.is_empty());
    assert_eq!(unchanged.model.edges.len(), ir.model.edges.len());

    use cadmpeg_ir::geometry::analytic::SphereSurface;
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::PcurveUse;
    let vertex = ir
        .model
        .vertices
        .iter()
        .find(|v| v.id == ir.model.edges.last().unwrap().start)
        .unwrap();
    let point = ir
        .model
        .points
        .iter()
        .find(|p| p.id == vertex.point)
        .unwrap()
        .position()
        .get();
    let surface_id = ir
        .model
        .faces
        .iter()
        .find(|face| face.id == ir.model.loops[0].face)
        .unwrap()
        .surface
        .clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(point.x, point.y, point.z - 1.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ));
    let id: cadmpeg_ir::ids::PcurveId = "test:model:pcurve#pole".try_into().unwrap();
    ir.model.pcurves.push(Pcurve {
        native_ref: None,
        id: id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(
                Point2::new(0.0, std::f64::consts::FRAC_PI_2),
                Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        metadata: PcurveMetadata::default(),
    });
    ir.model
        .coedges
        .last_mut()
        .unwrap()
        .pcurves
        .push(PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        });
    let (lowered, losses) = crate::writer::admission::pole_edges(&ir).unwrap();
    for id in ["test:model:edge#pole", "test:model:coedge#pole"] {
        assert!(
            losses.iter().any(|loss| loss.message.contains(id)),
            "missing loss for {id}"
        );
    }
    assert_eq!(lowered.model.edges.len() + 1, ir.model.edges.len());
    assert_eq!(lowered.model.coedges.len() + 1, ir.model.coedges.len());
    let LoopBoundary::Ring(ring) = &lowered.model.loops[0].boundary else {
        panic!("coedge ring")
    };
    assert!(ring
        .vertex_uses()
        .iter()
        .any(|use_| use_.vertex == ir.model.edges.last().unwrap().start));
}

#[test]
fn unsupported_free_surface_names_its_procedural_definition() {
    use cadmpeg_ir::geometry::analytic::TorusSurface;
    use cadmpeg_ir::geometry::{
        ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id: cadmpeg_ir::ids::SurfaceId = "test:model:surface#torus".try_into().unwrap();
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: id.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
                2.0,
            )
            .unwrap(),
        )),
    });
    ir.model
        .add_procedural_surface(
            &id,
            ProceduralSurface::new(
                "test:model:procedure#degenerate-torus".try_into().unwrap(),
                ProceduralSurfaceDefinition::DegenerateTorus { select_outer: true },
                None,
            ),
        )
        .unwrap();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    for id in [
        "test:model:surface#torus",
        "test:model:procedure#degenerate-torus",
    ] {
        assert!(
            plan.report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(id)),
            "missing loss for {id}: {:#?}",
            plan.report().losses
        );
    }
}

#[test]
fn unsupported_solid_names_each_owned_topology_record() {
    use cadmpeg_ir::geometry::analytic::TorusSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            2.0,
        )
        .unwrap(),
    ));
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let owned = ir
        .model
        .regions
        .iter()
        .map(|record| record.id.as_str())
        .chain(ir.model.shells.iter().map(|record| record.id.as_str()))
        .chain(ir.model.faces.iter().map(|record| record.id.as_str()))
        .chain(ir.model.loops.iter().map(|record| record.id.as_str()))
        .chain(ir.model.coedges.iter().map(|record| record.id.as_str()));
    for id in owned {
        assert!(
            plan.report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(id)),
            "missing loss for {id}: {:#?}",
            plan.report().losses
        );
    }
}

#[test]
fn network_occurrence_survives_synthesis_with_its_placement() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
    let transform = decoded.ir().model.occurrences[0].transform;
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.occurrences.len(), 1);
    assert_eq!(after.ir().model.occurrences[0].transform, transform);
    let original = &decoded.ir().native.namespace("iges").unwrap().arenas()["entities"];
    let rewritten = &after.ir().native.namespace("iges").unwrap().arenas()["entities"];
    assert_eq!(
        original
            .iter()
            .map(|entity| entity.field("label"))
            .collect::<Vec<_>>(),
        rewritten
            .iter()
            .map(|entity| entity.field("label"))
            .collect::<Vec<_>>()
    );
    for (original, rewritten) in original.iter().zip(rewritten) {
        assert_eq!(original.field("parameters"), rewritten.field("parameters"));
    }

    assert!(!after
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("connect-point")
            || loss.message.contains("network instance")));
}

#[test]
fn edited_network_occurrence_placement_is_written_from_the_document() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let edited = cadmpeg_ir::transform::Transform::affine([
        [0.0, -1.0, 0.0, 12.0],
        [1.0, 0.0, 0.0, 23.0],
        [0.0, 0.0, 1.0, 34.0],
    ])
    .unwrap();
    ir.model.occurrences[0].transform = Some(edited);
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let id = ir.model.occurrences[0].id.to_string();
    assert!(
        !plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(&id)),
        "{:#?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.occurrences.len(), 1);
    assert_eq!(after.ir().model.occurrences[0].transform, Some(edited));
    let before_points = &ir.native.namespace("iges").unwrap().arenas()["connect_points"];
    let after_points = &after.ir().native.namespace("iges").unwrap().arenas()["connect_points"];
    assert_eq!(before_points.len(), after_points.len());
    for (before, after) in before_points.iter().zip(after_points) {
        assert_eq!(before.field("owner"), after.field("owner"));
    }
    assert_eq!(
        after.ir().model.occurrences[0].id,
        ir.model.occurrences[0].id
    );
}

#[test]
fn edited_network_translation_and_scale_use_instance_parameters() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let edited = cadmpeg_ir::transform::Transform::affine([
        [2.0, 0.0, 0.0, 12.0],
        [0.0, 3.0, 0.0, 23.0],
        [0.0, 0.0, 4.0, 34.0],
    ])
    .unwrap();
    ir.model.occurrences[0].transform = Some(edited);
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.occurrences[0].transform, Some(edited));
    assert_eq!(
        after.ir().model.occurrences[0].id,
        ir.model.occurrences[0].id
    );
    let namespace = after.ir().native.namespace("iges").unwrap();
    assert!(namespace
        .arenas()
        .get("transformations")
        .is_none_or(Vec::is_empty));
}

#[test]
fn missing_network_placement_does_not_replay_the_old_placement() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.occurrences[0].transform = None;
    let id = ir.model.occurrences[0].id.to_string();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        after.ir().model.occurrences[0].transform,
        Some(cadmpeg_ir::transform::Transform::identity())
    );
}

#[test]
fn edited_network_definition_name_is_written_from_the_document() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.product_definitions[0].label = Some("current, network".into());
    let id = ir.model.product_definitions[0].id.to_string();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(
        !plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(&id)),
        "{:#?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        after.ir().model.product_definitions[0].label.as_deref(),
        Some("current, network")
    );
    assert_eq!(after.ir().model.occurrences.len(), 1);
}

#[test]
fn edited_nested_occurrence_placement_is_written_from_the_document() {
    let bytes = crate::test_support::test_drawing_and_trimming::nested_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.occurrences.len(), 2);
    let edited = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 8.0],
        [0.0, 0.0, 1.0, 9.0],
    ])
    .unwrap();
    ir.model.occurrences[1].transform = Some(edited);
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.occurrences.len(), 2);
    assert_eq!(after.ir().model.occurrences[1].transform, Some(edited));
}

#[test]
fn explicit_curve_bounded_surface_names_its_reduced_construction() {
    use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition};
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_surface_fixtures::bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let owner = ir.model.faces[0].surface.clone();
    let mut support = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == owner)
        .unwrap()
        .clone();
    support.id = "test:model:surface#support".try_into().unwrap();
    support.source_object = None;
    let support_id = support.id.clone();
    ir.model.surfaces.push(support);
    ir.model
        .add_procedural_surface(
            &owner,
            ProceduralSurface::new(
                "test:model:procedure#explicit-boundary".try_into().unwrap(),
                ProceduralSurfaceDefinition::CurveBounded {
                    support: support_id,
                    boundaries: Vec::new(),
                    boundary_pcurves: Vec::new(),
                    implicit_outer: false,
                },
                None,
            ),
        )
        .unwrap();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(
        plan.report().losses.iter().any(|loss| loss
            .message
            .contains("test:model:procedure#explicit-boundary")),
        "{:#?}",
        plan.report().losses
    );
}

#[test]
fn source_less_product_and_root_occurrence_write_as_subfigures() {
    use cadmpeg_ir::products::{Occurrence, ProductDefinition};
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.product_definitions.push(
        serde_json::from_value::<ProductDefinition>(serde_json::json!({
            "id":"test:model:product#one", "kind":"part", "label":"Assembly part"
        }))
        .unwrap(),
    );
    ir.model.occurrences.push(serde_json::from_value::<Occurrence>(serde_json::json!({
        "id":"test:model:occurrence#one", "prototype":{"scope":"local","definition":"test:model:product#one"},
        "parent":{"kind":"root"}, "ordinal":0,
        "transform":[[0.0,-1.0,0.0,4.0],[1.0,0.0,0.0,5.0],[0.0,0.0,1.0,6.0]]
    })).unwrap());
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.product_definitions.len(), 1);
    assert_eq!(
        after.ir().model.product_definitions[0].label.as_deref(),
        Some("Assembly part")
    );
    assert_eq!(after.ir().model.occurrences.len(), 1);
    assert_eq!(
        after.ir().model.occurrences[0].transform,
        ir.model.occurrences[0].transform
    );
}

#[test]
fn source_less_nested_products_keep_occurrence_hierarchy_and_placement() {
    use cadmpeg_ir::products::{Occurrence, ProductDefinition};
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (id, label) in [("parent", "Parent"), ("child", "Child")] {
        ir.model.product_definitions.push(
            serde_json::from_value::<ProductDefinition>(serde_json::json!({
                "id": format!("test:model:product#{id}"), "kind":"part", "label":label
            }))
            .unwrap(),
        );
    }
    ir.model.occurrences.push(serde_json::from_value::<Occurrence>(serde_json::json!({
        "id":"test:model:occurrence#parent", "prototype":{"scope":"local","definition":"test:model:product#parent"},
        "parent":{"kind":"root"}, "ordinal":0, "transform":[[1,0,0,0],[0,1,0,0],[0,0,1,0]]
    })).unwrap());
    ir.model.occurrences.push(serde_json::from_value::<Occurrence>(serde_json::json!({
        "id":"test:model:occurrence#child", "prototype":{"scope":"local","definition":"test:model:product#child"},
        "parent":{"kind":"occurrence","occurrence":"test:model:occurrence#parent"}, "ordinal":1,
        "transform":[[1,0,0,4],[0,1,0,5],[0,0,1,6]]
    })).unwrap());
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.product_definitions.len(), 2);
    assert_eq!(after.ir().model.occurrences.len(), 2);
    assert_eq!(
        after.ir().model.occurrences[1].transform,
        ir.model.occurrences[1].transform
    );
    assert!(
        matches!(&after.ir().model.occurrences[1].parent, cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } if *occurrence == after.ir().model.occurrences[0].id)
    );
}

#[test]
fn shared_source_less_definition_names_an_unrepresentable_child() {
    use cadmpeg_ir::products::{Occurrence, ProductDefinition};
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (id, label) in [("parent", "Parent"), ("child", "Child")] {
        ir.model.product_definitions.push(
            serde_json::from_value::<ProductDefinition>(serde_json::json!({
                "id": format!("test:model:product#{id}"), "kind":"part", "label":label
            }))
            .unwrap(),
        );
    }
    for (id, ordinal) in [("first", 0), ("second", 1)] {
        ir.model.occurrences.push(serde_json::from_value::<Occurrence>(serde_json::json!({
            "id":format!("test:model:occurrence#{id}"), "prototype":{"scope":"local","definition":"test:model:product#parent"},
            "parent":{"kind":"root"}, "ordinal":ordinal, "transform":[[1,0,0,0],[0,1,0,0],[0,0,1,0]]
        })).unwrap());
    }
    ir.model.occurrences.push(serde_json::from_value::<Occurrence>(serde_json::json!({
        "id":"test:model:occurrence#child", "prototype":{"scope":"local","definition":"test:model:product#child"},
        "parent":{"kind":"occurrence","occurrence":"test:model:occurrence#first"}, "ordinal":2,
        "transform":[[1,0,0,4],[0,1,0,5],[0,0,1,6]]
    })).unwrap());
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:occurrence#child")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(after.ir().model.occurrences.len(), 2);
}

#[test]
fn compound_curve_keeps_its_components_and_construction() {
    let bytes = crate::test_support::test_curves_and_surfaces::composite_curve_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.procedural_curves.len(), 1);
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).unwrap();
    let construction = decoded.ir().model.procedural_curves[0].id.to_string();
    assert!(
        !plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(&construction)),
        "{:#?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        after.ir().model.curves.len(),
        decoded.ir().model.curves.len()
    );
    assert_eq!(after.ir().model.procedural_curves.len(), 1);
    assert_eq!(after.ir().model.edges.len(), decoded.ir().model.edges.len());
}

#[test]
fn compound_curve_uses_edited_components_instead_of_its_solved_cache() {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let bytes = crate::test_support::test_curves_and_surfaces::composite_curve_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let ProceduralCurveDefinition::Compound(compound) = ir.model.procedural_curves[0].definition()
    else {
        panic!("compound")
    };
    let ids = compound
        .components()
        .iter()
        .map(|component| component.component.clone())
        .collect::<Vec<_>>();
    for (id, origin, direction) in [
        (
            &ids[0],
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
        (
            &ids[1],
            Point3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
    ] {
        ir.model
            .curves
            .iter_mut()
            .find(|curve| &curve.id == id)
            .unwrap()
            .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(origin, direction).unwrap(),
        ));
    }
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let after = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ProceduralCurveDefinition::Compound(compound) =
        after.ir().model.procedural_curves[0].definition()
    else {
        panic!("compound")
    };
    let first = after
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == compound.components()[0].component)
        .unwrap();
    let Some(SolvedCurveGeometry::Line(line)) = first.geometry.solved() else {
        panic!("line")
    };
    assert_eq!(*line.direction().as_raw(), Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn finite_straight_pcurve_with_unspecified_flag_keeps_its_boundary() {
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use cadmpeg_ir::math::Point2;

    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::explicit_multi_pcurve_loop_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("multi-pcurve loop decodes");
    let mut ir = decoded.ir().clone();
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|edge| !edge.pcurves.is_empty())
        .unwrap();
    let coedge_id = coedge.id.clone();
    let pcurve_id = coedge.pcurves[0].pcurve.clone();
    coedge.pcurves[0].isoparametric = None;
    let pcurve = ir
        .model
        .pcurves
        .iter_mut()
        .find(|curve| curve.id == pcurve_id)
        .unwrap();
    pcurve.geometry = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(0.5, 0.0)).unwrap(),
    );
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("line pcurve plans");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(coedge_id.as_str())
            && loss.message.contains(pcurve_id.as_str())
            && loss.message.contains("isoparametric")));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(pcurve_id.as_str())
            && loss.message.contains("line representation")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written boundary decodes");
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(output.ir().model.pcurves.len(), ir.model.pcurves.len());
    assert!(output
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|edge| &edge.pcurves)
        .any(|item| item.isoparametric == Some(true)));
}

#[test]
fn regenerated_annotation_records_keep_their_semantic_projection() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::bounded_associativity_forms_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("associativity fixture decodes");
    assert!(!decoded.ir().model.semantic_annotations.is_empty());
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).expect("annotations synthesize");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("annotations re-decode");
    assert_eq!(
        output.ir().model.semantic_annotations.len(),
        decoded.ir().model.semantic_annotations.len()
    );
}

#[test]
fn edited_semantic_note_text_reaches_regenerated_parameters() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::bounded_associativity_forms_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("associativity fixture decodes");
    let mut ir = decoded.ir().clone();
    let note = ir
        .model
        .semantic_annotations
        .iter_mut()
        .find(|item| !item.text.is_empty())
        .expect("text note");
    note.text[0].push('x');
    let expected = note.text.clone();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("edited note synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("edited note re-decodes");
    assert!(output
        .ir()
        .model
        .semantic_annotations
        .iter()
        .any(|item| item.text == expected));
}

#[test]
fn regenerated_associativities_retain_required_back_pointers() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::bounded_associativity_forms_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("associativity fixture decodes");
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).expect("associativities synthesize");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("associativities re-decode");
    assert!(
        output
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("predefined associativity")),
        "{:?}",
        output.report().losses
    );
}

#[test]
fn edited_boolean_operation_reaches_the_regenerated_tree() {
    use cadmpeg_ir::features::{BooleanKind, FeatureDefinition, FeatureOperation};
    let decoded = IgesCodec.decode(
        &mut Cursor::new(crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_with_boolean_file()),
        &DecodeOptions::default(),
    ).expect("Boolean fixture decodes");
    let mut ir = decoded.ir().clone();
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Combine { .. })
            )
        })
        .expect("Boolean feature");
    let mut definition = feature.evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::Combine { op, .. }) = &mut definition else {
        unreachable!()
    };
    *op = BooleanKind::Cut;
    feature.evaluation.set_definition(definition);
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("edited Boolean synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("Boolean re-decodes");
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Combine {
            op: BooleanKind::Cut,
            ..
        })
    )));
}

#[test]
fn reconstructed_default_display_record_is_declared_regenerated() {
    use cadmpeg_ir::codec::write::NativeDisposition;
    let decoded = IgesCodec.decode(&mut Cursor::new(point_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;"
    )), &DecodeOptions::default()).expect("point fixture decodes");
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).expect("point synthesizes");
    assert_eq!(
        plan.report()
            .native_dispositions
            .disposition("iges", "display_attributes"),
        NativeDisposition::Regenerated
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("output decodes");
    let before = &decoded.ir().native.namespace("iges").unwrap().arenas()["display_attributes"][0];
    let after = &output.ir().native.namespace("iges").unwrap().arenas()["display_attributes"][0];
    for field in [
        "color_number",
        "level_number",
        "line_font_number",
        "line_weight_number",
        "view",
        "visible",
    ] {
        assert_eq!(before.field(field), after.field(field), "{field}");
    }
}

#[test]
fn empty_note_text_is_reported_without_invalid_hollerith() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::bounded_associativity_forms_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("associativity fixture decodes");
    let mut ir = decoded.ir().clone();
    let note = ir
        .model
        .semantic_annotations
        .iter_mut()
        .find(|item| !item.text.is_empty())
        .expect("text note");
    note.text[0].clear();
    let note_id = note.id.to_string();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("empty note synthesizes");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&note_id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("output re-decodes");
    assert!(output
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("hollerith-count-zero")));
}

#[test]
fn implicit_nurbs_domain_retains_its_loopless_face() {
    let decoded = IgesCodec.decode(
        &mut Cursor::new(crate::test_support::test_drawing_and_trimming::parameter_domain_trimmed_surface_file("144,1,0,0,0;")),
        &DecodeOptions::default(),
    ).expect("natural-domain face decodes");
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded.ir().model.faces[0].loops.is_empty());
    let plan =
        plan_at(IgesVersion::V5_3, decoded.ir(), None).expect("natural-domain face synthesizes");
    let face_id = decoded.ir().model.faces[0].id.as_str();
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains(face_id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("natural-domain face re-decodes");
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), 1);
    assert!(output.ir().model.faces[0].loops.is_empty());
}

#[test]
fn empty_model_emits_its_canonical_length_unit_definition() {
    let ir = CadIr::empty();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("empty document synthesizes");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::NoExportableSolids.kind()));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("units definition re-decodes");
    assert!(output.ir().model.bodies.is_empty());
    assert!(output.ir().model.faces.is_empty());
    let units = &output.ir().native.namespace("iges").unwrap().arenas()["units_data"];
    assert_eq!(units.len(), 1);
    assert!(output
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("units")));
}

#[test]
fn empty_v4_model_emits_the_identity_frame() {
    let ir = CadIr::empty();
    let plan = plan_at(IgesVersion::V4_0, &ir, None).expect("empty V4 document synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("identity frame re-decodes");
    assert!(output.ir().model.bodies.is_empty());
    assert!(output.ir().model.faces.is_empty());
    let transforms = &output.ir().native.namespace("iges").unwrap().arenas()["transformations"];
    assert_eq!(transforms.len(), 1);
    assert!(output
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("transformation")));
}

#[test]
fn face_custom_color_keeps_its_appearance_name() {
    let harness =
        cadmpeg_test_support::golden::Harness::new(env!("CARGO_MANIFEST_DIR"), "igs", "unused");
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "colored_explicit_vertex_loop")
        .unwrap();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None).unwrap();
    for record in [
        "iges:appearance:color#D13",
        "iges:model:appearance-binding#face-D9",
    ] {
        assert!(plan
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains(record)));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let binding = output
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| {
            matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Face(_)
            )
        })
        .unwrap();
    let appearance = output
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap();
    assert_eq!(appearance.name.as_deref(), Some("custom"));
}

#[test]
fn constant_coordinate_nurbs_keeps_unspecified_isoparametric_use() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::explicit_multi_pcurve_loop_file(),
            ),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|edge| !edge.pcurves.is_empty())
        .unwrap();
    let coedge_id = coedge.id.clone();
    let id = coedge.pcurves[0].pcurve.clone();
    coedge.pcurves[0].isoparametric = None;
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(coedge_id.as_str())
            && loss.message.contains(id.as_str())
            && loss.message.contains("isoparametric")));
    assert!(!plan.report().losses.iter().any(|loss| loss.code
        == IgesLossCode::WriterPcurvesOmitted.kind()
        && loss.message.contains(id.as_str())));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.pcurves.len(), ir.model.pcurves.len());
    assert!(output
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|edge| &edge.pcurves)
        .any(|use_| use_.isoparametric == Some(true)));
}

#[test]
fn open_solid_keeps_its_face_as_a_reported_sheet() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::trimmed_plane_with_inner_loop_file(
                ),
            ),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
    let id = ir.model.bodies[0].id.clone();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str())
            && loss.message.contains("open shell")
            && loss.message.contains("sheet")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert!(output
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Sheet));
}

#[test]
fn directory_disposition_requires_exact_generated_record() {
    use cadmpeg_ir::codec::write::NativeDisposition;

    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file(),
            ),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let transform = ir.model.occurrences[0].transform.unwrap();
    let mut rows = transform.affine_rows();
    rows[0][3] += 1.0;
    ir.model.occurrences[0].transform =
        Some(cadmpeg_ir::transform::Transform::affine(rows).unwrap());
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let dispositions = &plan.report().native_dispositions;
    assert_eq!(
        dispositions.record_disposition("iges", "cards", "iges:physical:card#1"),
        NativeDisposition::Written
    );
    assert_eq!(
        dispositions.record_disposition("iges", "cards", "iges:physical:card#12"),
        NativeDisposition::Dropped
    );
    assert_eq!(
        dispositions.record_disposition("iges", "entities", "iges:entity:directory#5"),
        NativeDisposition::Written
    );
    assert_eq!(
        dispositions.record_disposition("iges", "entities", "iges:entity:directory#7"),
        NativeDisposition::Dropped
    );
}

#[test]
fn nonorthogonal_occurrence_placement_is_reported_without_invalid_matrix() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let mut rows = ir.model.occurrences[0].transform.unwrap().affine_rows();
    rows[0][1] = 1.0;
    ir.model.occurrences[0].transform =
        Some(cadmpeg_ir::transform::Transform::affine(rows).unwrap());
    let id = ir.model.occurrences[0].id.clone();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str())
            && loss.message.contains("not an orthonormal")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.occurrences.len(), 1);
}

#[test]
fn reflected_occurrence_uses_left_handed_matrix() {
    let bytes = crate::test_support::test_drawing_and_trimming::connected_network_subfigure_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let reflected = cadmpeg_ir::transform::Transform::affine([
        [-1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 30.0],
    ])
    .unwrap();
    ir.model.occurrences[0].transform = Some(reflected);
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.occurrences[0].transform, Some(reflected));
}
