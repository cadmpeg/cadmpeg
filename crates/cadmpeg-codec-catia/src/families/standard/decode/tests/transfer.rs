// SPDX-License-Identifier: Apache-2.0
//! Standard-family dump tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};

use cadmpeg_ir::math::{Point3, Vector3};

use crate::test_support::*;
use crate::variant::Variant;
use crate::CatiaCodec;

#[test]
fn standard_decode_retains_native_surface_carrier_tags() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart()),
            &DecodeOptions::default(),
        )
        .expect("standard decode");
    let identities = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            surface
                .source_object
                .as_ref()
                .map(|source| (source.format.as_str(), source.object_id.as_str()))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        [
            ("catia", "cgm-carrier:ccbbaa"),
            ("catia", "cgm-carrier:332211"),
        ]
    );
}

#[test]
fn standard_decode_distinguishes_consolidated_surface_frames() {
    let mut payload = a5_surface_stream();
    payload.extend_from_slice(&a5_surface_stream());
    let mut file = standard_catpart();
    file.splice(16..16, payload);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let identities = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .filter_map(|surface| surface.source_object.as_ref())
        .map(|source| source.object_id.as_str())
        .collect::<Vec<_>>();
    let frame_identities = identities
        .iter()
        .copied()
        .filter(|identity| identity.starts_with("cgm-a5-surface-frame:"))
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(frame_identities.len(), 2);
    assert!(!identities.contains(&"cgm-surface:000000"));
}

#[test]
fn standard_decode_retains_vertex_allocation_tags() {
    let mut surf = surf_stream();
    for identity in [0x01_0203u32, 0x01_0206, 0x01_0209] {
        surf.push(0x54);
        surf.extend_from_slice(&identity.to_le_bytes()[..3]);
        surf.extend_from_slice(&[0, 0, 0]);
    }
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_from_streams(&main_stream(), &surf)),
            &DecodeOptions::default(),
        )
        .expect("standard decode");
    let identities = decoded
        .ir()
        .model
        .points
        .iter()
        .map(|point| {
            point
                .source_object
                .as_ref()
                .map(|source| (source.format.as_str(), source.object_id.as_str()))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        [
            Some(("catia", "cgm-vertex:010203")),
            Some(("catia", "cgm-vertex:010206")),
            Some(("catia", "cgm-vertex:010209")),
        ]
    );
}

#[test]
fn decode_standard_transfers_vertices_and_cylinder() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred());
    // Three vertex records → three points and three vertices.
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    // A vertex coordinate is transferred verbatim in millimetres (no scaling).
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|p| (p.position.x - 10.0).abs() < 1.0e-6));

    // Cylinder and tag-bridged plane carriers are decoded from their stored
    // parameters.
    assert_eq!(result.ir().model.surfaces.len(), 2);
    assert_eq!(result.ir().model.curves.len(), 1);
    let unknowns = result.ir().native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].id.as_str(), "catia:payload:unknown#brep-stream");
    assert!(unknowns[0]
        .links
        .contains(&"catia:standard:circle#0".to_string()));
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Cylinder { radius, axis, .. } => {
            assert!((radius - 5.0).abs() < 1.0e-6);
            assert!((axis.z - 1.0).abs() < 1.0e-6);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }
            if (origin.x - 1.0).abs() < 1.0e-6
                && (origin.y - 2.0).abs() < 1.0e-6
                && (origin.z - 3.0).abs() < 1.0e-6
                && normal.x.abs() < 1.0e-6
                && normal.y.abs() < 1.0e-6
                && (normal.z.abs() - 1.0).abs() < 1.0e-6
                && (u_axis.x * u_axis.x + u_axis.y * u_axis.y + u_axis.z * u_axis.z - 1.0).abs() < 1.0e-6
                && (u_axis.x * normal.x + u_axis.y * normal.y + u_axis.z * normal.z).abs() < 1.0e-6
    )));

    // Stored face/carrier rows do not establish a B-rep without a complete
    // trim and edge graph. Carriers remain free and vertices receive only the
    // neutral ownership required for a disconnected point set.
    assert!(result.ir().model.faces.is_empty());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir().model.shells[0].free_vertices.len(), 3);
    assert!(result.ir().model.edges.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.code.category() == cadmpeg_ir::report::LossCategory::Topology));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage()
            .iter()
            .filter(|(key, _)| key.starts_with("standard_topology_failure_"))
            .map(|(_, count)| count)
            .sum::<usize>(),
        1
    );
    assert_eq!(
        [
            "standard_topology_mesh_ambiguity_coordinate_root_closure_count",
            "standard_topology_mesh_ambiguity_endpoint_resolution_count",
            "standard_topology_mesh_ambiguity_distinct_topology_solutions_count",
        ]
        .into_iter()
        .map(|key| result.report().coverage().get(key).copied().unwrap_or(0))
        .sum::<usize>(),
        result
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_FAILURE_AMBIGUOUS_SOLUTION_COUNT)
    );
    assert_eq!(
        [
            "standard_topology_mesh_exhaustion_quotient_preparation_count",
            "standard_topology_mesh_exhaustion_incidence_enumeration_count",
            "standard_topology_mesh_exhaustion_endpoint_resolution_count",
        ]
        .into_iter()
        .map(|key| result.report().coverage().get(key).copied().unwrap_or(0))
        .sum::<usize>(),
        result
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_FAILURE_SEARCH_EXHAUSTED_COUNT)
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT)
            + result
                .report()
                .coverage_count(crate::coverage::STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT)
            + result
                .report()
                .coverage_count(crate::coverage::STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT),
        result
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT)
    );
    assert!(
        result
            .report()
            .coverage()
            .iter()
            .filter(|(key, _)| {
                key.starts_with("standard_topology_mesh_rejection_")
                    && !key.starts_with("standard_topology_mesh_rejection_incidence_")
                    && (!key.contains("endpoint_incidence_")
                        || key.ends_with("endpoint_incidence_count"))
            })
            .map(|(_, count)| count)
            .sum::<usize>()
            <= 1
    );
    assert_eq!(
        result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_COUNT),
        result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT)
            + result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_BOUNDARY_RECONSTRUCTION_COUNT)
    );
    assert_eq!(
        result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT),
        result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_INPUT_SHAPE_COUNT)
            + result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_CHOICE_PRUNING_COUNT)
            + result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_FIXED_ASSIGNMENT_COUNT)
            + result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_DOMAIN_COUNT)
            + result.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_COMPOSITION_COUNT)
    );

    // The produced IR validates (free carriers, no dangling references).
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_standard_retains_unresolved_roster_carrier_without_fabricating_a_face() {
    let mut surf = surf_stream();
    let bridge = [0xff, 0x11, 0x22, 0x33, 0x00, 0x02, 0x00, 0x33, 0x32];
    let bridge_start = surf
        .windows(bridge.len())
        .position(|bytes| bytes == bridge)
        .expect("plane parameter bridge");
    surf.drain(bridge_start..bridge_start + bridge.len() + 40);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_from_streams(&main_stream(), &surf)),
            &DecodeOptions::default(),
        )
        .expect("decode unresolved roster carrier");

    assert_eq!(decoded.ir().model.surfaces.len(), 2);
    assert!(decoded.ir().model.faces.is_empty());
    assert!(matches!(
        decoded.ir().model.surfaces[1].geometry,
        SurfaceGeometry::Unknown { record: Some(_) }
    ));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 unresolved surface carriers")
    }));
}

#[test]
fn decode_standard_builds_surface_bound_topology_graph() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(tetrahedron_topology_catpart()),
            &DecodeOptions::default(),
        )
        .expect("decode generated topology part");

    assert_eq!(decoded.ir().model.faces.len(), 4);
    assert_eq!(decoded.ir().model.loops.len(), 4);
    assert_eq!(decoded.ir().model.edges.len(), 6);
    assert_eq!(decoded.ir().model.coedges.len(), 12);
    assert!(decoded
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.loops.len() == 1));
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.radial_next != coedge.id));
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some()));
    assert_eq!(
        decoded
            .ir()
            .model
            .curves
            .iter()
            .map(|curve| curve
                .source_object
                .as_ref()
                .map(|source| source.object_id.as_str()))
            .collect::<Vec<_>>(),
        (1..=6)
            .map(|tag| format!("cgm-edge-support:{tag:06x}"))
            .collect::<Vec<_>>()
            .iter()
            .map(|object_id| Some(object_id.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage()
            .iter()
            .filter(|(key, _)| key.starts_with("standard_topology_failure_"))
            .map(|(_, count)| count)
            .sum::<usize>(),
        0
    );
    assert_eq!(
        [
            "standard_topology_mesh_ambiguity_coordinate_root_closure_count",
            "standard_topology_mesh_ambiguity_endpoint_resolution_count",
            "standard_topology_mesh_ambiguity_distinct_topology_solutions_count",
        ]
        .into_iter()
        .map(|key| decoded.report().coverage().get(key).copied().unwrap_or(0))
        .sum::<usize>(),
        decoded
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_FAILURE_AMBIGUOUS_SOLUTION_COUNT)
    );
    assert_eq!(
        [
            "standard_topology_mesh_exhaustion_quotient_preparation_count",
            "standard_topology_mesh_exhaustion_incidence_enumeration_count",
            "standard_topology_mesh_exhaustion_endpoint_resolution_count",
        ]
        .into_iter()
        .map(|key| decoded.report().coverage().get(key).copied().unwrap_or(0))
        .sum::<usize>(),
        decoded
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_FAILURE_SEARCH_EXHAUSTED_COUNT)
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT)
            + decoded
                .report()
                .coverage_count(crate::coverage::STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT)
            + decoded
                .report()
                .coverage_count(crate::coverage::STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT),
        decoded
            .report()
            .coverage_count(crate::coverage::STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT)
    );
    assert_eq!(
        decoded
            .report()
            .coverage()
            .iter()
            .filter(|(key, _)| {
                key.starts_with("standard_topology_mesh_rejection_")
                    && !key.starts_with("standard_topology_mesh_rejection_incidence_")
                    && (!key.contains("endpoint_incidence_")
                        || key.ends_with("endpoint_incidence_count"))
            })
            .map(|(_, count)| count)
            .sum::<usize>(),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_COUNT),
        decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT)
            + decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_BOUNDARY_RECONSTRUCTION_COUNT)
    );
    assert_eq!(
        decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT),
        decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_INPUT_SHAPE_COUNT)
            + decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_CHOICE_PRUNING_COUNT)
            + decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_FIXED_ASSIGNMENT_COUNT)
            + decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_DOMAIN_COUNT)
            + decoded.report().coverage_count(crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_COMPOSITION_COUNT)
    );
}

#[test]
fn decode_fbb_only_without_parseable_counted_table_transfers_only_carriers() {
    assert_eq!(
        crate::container::scan_bytes(fbb_only_catpart()).variant,
        Variant::FbbOnly
    );
    let mut cur = Cursor::new(fbb_only_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.points.is_empty());
    assert_eq!(result.ir().model.surfaces.len(), 2);
}

#[test]
fn decode_standard_does_not_promote_unbound_consolidated_pcurve() {
    let mut file = standard_catpart();
    file.splice(16..16, a5_pcurve_stream());
    let file_len = u32::try_from(file.len()).expect("pcurve fixture length");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated pcurve");
    assert!(decoded.ir().model.pcurves.is_empty());
    assert!(!decoded.ir().native_unknowns("catia").unwrap().is_empty());
}

#[test]
fn standard_decode_refines_a_unique_quantized_analytic_carrier() {
    let exact_x = 1.000_000_01_f64;
    let mut surf = surf_stream();
    for (index, value) in [exact_x as f32, 2.0_f32, 3.0_f32, 1.0_f32, 0.0_f32, 2.0_f32]
        .into_iter()
        .enumerate()
    {
        surf[8 + 4 * index..12 + 4 * index].copy_from_slice(&be_f32(value));
    }
    let mut consolidated = b2_cylinder_stream();
    consolidated[5..13].copy_from_slice(&le_f64(exact_x));
    let mut file = standard_catpart_from_streams(&main_stream(), &surf);
    file.splice(16..16, consolidated);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode exact consolidated analytic refinement");
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "catia:standard:surf#0")
        .expect("refined standard cylinder");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder { origin, axis, .. }
            if origin.x == exact_x
                && axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::REFINED_CONSOLIDATED_ANALYTIC_SURFACE_COUNT),
        1
    );
}

#[test]
fn standard_decode_transfers_resolved_consolidated_cylinder_surface_curve() {
    let mut records = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        records.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            records.extend_from_slice(&value.to_le_bytes());
        }
    }
    records.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));
    let mut file = standard_catpart();
    file.splice(16..16, records);
    let file_len = u32::try_from(file.len()).expect("consolidated fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved consolidated edge");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| {
            curve
                .id
                .as_str()
                .starts_with("catia:consolidated:construction#")
        })
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("two resolved support sides form an intersection");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let pcurve = context.sides[0].pcurve.as_ref().expect("cylinder pcurve");
    let start = cadmpeg_ir::eval::pcurve_uv(pcurve, 0.0).expect("pcurve start");
    let end = cadmpeg_ir::eval::pcurve_uv(pcurve, 1.0).expect("pcurve end");
    assert_eq!([start.u, start.v], [0.0, 0.0]);
    assert_eq!([end.u, end.v], [0.5, 1.0]);
}

#[test]
fn standard_decode_transfers_resolved_consolidated_cone_surface_curve() {
    let u = [0.0f64, 1.0];
    let v = [2.0f64, 3.0];
    let mut records = a5_pcurve_stream_with_uv(u, v);
    records.extend_from_slice(&a5_pcurve_stream_with_uv(u, v));
    records.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    records.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    records.extend_from_slice(&b2_cone_stream());
    for (u, v) in u.into_iter().zip(v) {
        let phi = u / 3.0;
        let point = [
            1.0 + v * 0.25f64.sin() * phi.cos(),
            2.0 + v * 0.25f64.sin() * phi.sin(),
            3.0 + v * 0.25f64.cos(),
        ];
        records.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            records.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }
    let mut file = standard_catpart();
    file.splice(16..16, records);
    let file_len = u32::try_from(file.len()).expect("consolidated fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved consolidated cone edge");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| {
            curve
                .id
                .as_str()
                .starts_with("catia:consolidated:construction#")
        })
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("two resolved support sides form an intersection");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let pcurve = context.sides[0].pcurve.as_ref().expect("cone pcurve");
    let start = cadmpeg_ir::eval::pcurve_uv(pcurve, 0.0).expect("pcurve start");
    let end = cadmpeg_ir::eval::pcurve_uv(pcurve, 1.0).expect("pcurve end");
    assert_eq!([start.u, start.v], [0.0, 0.0]);
    assert_eq!([end.u, end.v], [1.0 / 3.0, 0.25f64.cos()]);
}

#[test]
fn standard_decode_transfers_resolved_consolidated_nurbs_surface_curves() {
    for offset in [0.0, 1.25] {
        let mut file = standard_catpart();
        file.splice(16..16, a5_nurbs_bound_edge_stream(offset));
        let file_len = u32::try_from(file.len()).expect("consolidated fixture length");
        file[8..12].copy_from_slice(&be32(file_len));

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("decode resolved consolidated NURBS edge");
        let procedural = decoded
            .ir()
            .model
            .procedural_curves
            .iter()
            .find(|curve| {
                curve
                    .id
                    .as_str()
                    .starts_with("catia:consolidated:construction#")
            })
            .expect("resolved consolidated construction");
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            panic!("two resolved support sides form an intersection");
        };
        let surface_id = context.sides[1]
            .surface
            .as_ref()
            .expect("resolved NURBS support");
        let pcurve = context.sides[1].pcurve.as_ref().expect("NURBS pcurve");
        let start = cadmpeg_ir::eval::pcurve_uv(pcurve, 0.0).expect("pcurve start");
        let end = cadmpeg_ir::eval::pcurve_uv(pcurve, 1.0).expect("pcurve end");
        assert_eq!([start.u, start.v], [0.0, 0.0]);
        assert_eq!([end.u, end.v], [1.0, 0.0]);

        if offset == 0.0 {
            let surface = decoded
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == surface_id)
                .expect("direct NURBS carrier");
            assert!(matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
        } else {
            let construction = decoded
                .ir()
                .model
                .procedural_surfaces
                .iter()
                .find(|surface| {
                    decoded.ir().model.procedural_surface_owner(&surface.id) == Some(surface_id)
                })
                .expect("offset NURBS construction");
            let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
                support, distance, ..
            } = construction.definition()
            else {
                panic!("resolved normal offset is retained as an offset construction");
            };
            assert!((*distance - offset).abs() < 1.0e-12);
            assert!(decoded.ir().model.surfaces.iter().any(|surface| {
                surface.id == *support && matches!(surface.geometry, SurfaceGeometry::Nurbs(_))
            }));
        }
    }
}

#[test]
fn decode_standard_transfers_exact_offset_construction() {
    let surface_bytes = a5_surface_stream();
    let carriers = crate::families::a5a8::records::a5_surfaces(&surface_bytes);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let domain = [
        surface.u_knots()[0],
        surface.v_knots()[0],
        *surface.u_knots().last().unwrap(),
        *surface.v_knots().last().unwrap(),
    ];
    let mut payload = surface_bytes;
    payload.extend_from_slice(&b2_offset_support_stream_for(domain));
    let mut file = standard_catpart();
    file.splice(16..16, payload);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one offset construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
        support,
        distance,
        u_sense,
        v_sense,
        extension,
        ..
    } = procedural.definition()
    else {
        panic!("offset construction");
    };
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id == *support));
    assert_eq!(*distance, 2.5);
    assert_eq!([*u_sense, *v_sense], [None, None]);
    assert_eq!(
        *extension,
        cadmpeg_ir::geometry::OffsetExtension::Legacy(
            cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
        )
    );
    let Some(bounds) = procedural.record_bounds else {
        panic!("offset parameter bounds");
    };
    for (actual, expected) in bounds.into_iter().zip(domain) {
        let Some(actual) = actual else {
            panic!("offset parameter bound");
        };
        assert!((actual - expected).abs() < 1.0e-12);
    }
}

#[test]
fn decode_standard_transfers_construction_use_offset() {
    let surface_bytes = a5_surface_stream();
    let carriers = crate::families::a5a8::records::a5_surfaces(&surface_bytes);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let domain = [
        surface.u_knots()[0],
        surface.v_knots()[0],
        *surface.u_knots().last().unwrap(),
        *surface.v_knots().last().unwrap(),
    ];
    let mut payload = surface_bytes;
    payload.extend_from_slice(&b2_construction_use_stream_for(domain));
    let mut file = standard_catpart();
    file.splice(16..16, payload);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one offset construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { distance, .. } =
        procedural.definition()
    else {
        panic!("offset construction");
    };
    assert_eq!(*distance, -2.0);
    let Some(bounds) = procedural.record_bounds else {
        panic!("offset parameter bounds");
    };
    for (actual, expected) in bounds.into_iter().zip(domain) {
        let Some(actual) = actual else {
            panic!("offset parameter bound");
        };
        assert!((actual - expected).abs() < 1.0e-12);
    }
}

#[test]
fn decode_standard_transfers_exact_rolling_ball_jet() {
    let mut file = standard_catpart();
    file.splice(16..16, a5_freeform_curve_stream());
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            decoded.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
        })
        .expect("rolling-ball surface");
    assert!(matches!(
        &surface.geometry,
        SurfaceGeometry::Procedural { construction, .. } if construction == &procedural.id
    ));
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = procedural.definition()
    else {
        panic!("rolling-ball jet");
    };
    assert_eq!(*degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].first_limit, Point3::new(1.0, 0.0, 0.0));
    assert_eq!(sites[1].second_limit, Point3::new(0.0, 2.0, 0.0));
    assert_eq!(sites[0].angle, std::f64::consts::FRAC_PI_2);
    assert_eq!(
        sites[0].first_derivative.center,
        Vector3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn standard_decode_transfers_consolidated_guide_curve() {
    let mut bytes = standard_catpart();
    bytes.splice(16..16, a5_guide_curve_stream());
    let file_len = u32::try_from(bytes.len()).expect("guide fixture length");
    bytes[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode guide fixture");
    let guide = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str().starts_with("catia:guide:curve#"))
        .expect("typed guide curve");
    let CurveGeometry::Nurbs(nurbs) = &guide.geometry else {
        panic!("guide curve must be NURBS");
    };
    assert_eq!(nurbs.degree(), 5);
    assert_eq!(nurbs.control_points().first().unwrap().x, 0.0);
    assert_eq!(nurbs.control_points().last().unwrap().z, 4.0);
}
