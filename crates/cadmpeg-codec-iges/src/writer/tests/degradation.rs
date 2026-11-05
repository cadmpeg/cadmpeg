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
    ir.model.product_definitions[0].label = Some("current network".into());
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
        Some("current network")
    );
    assert_eq!(after.ir().model.occurrences.len(), 1);
}
