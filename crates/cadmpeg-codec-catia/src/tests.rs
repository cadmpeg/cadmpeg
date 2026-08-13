// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.CATPart` byte image
//! whose bytes exercise the real container, variant-detection, and geometry
//! decode paths and fail if the code regresses.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};

use cadmpeg_ir::math::{Point3, Vector3};

use crate::variant::Variant;
use crate::CatiaCodec;

pub(crate) use crate::test_support::*;

#[test]
fn consolidated_record_sources_follow_physical_stream_extents() {
    let scan = crate::container::scan_bytes(standard_catpart());
    let inner = scan.inner.as_ref().expect("inner stream directory");
    let expected = inner
        .descriptors
        .iter()
        .flat_map(|descriptor| {
            descriptor.extents.iter().map(|extent| {
                let start = inner.inner + extent.phys_off as usize;
                start..start + extent.phys_len as usize
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        crate::container::consolidated_record_ranges(&scan),
        expected
    );
    assert!(crate::container::consolidated_record_ranges(&scan)
        .iter()
        .all(|range| !range.contains(&inner.inner)));
}

#[test]
fn consolidated_support_resolution_withholds_cross_family_matches() {
    let mut bytes = a5_cone_bound_edge_stream();
    let cylinder = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream())
        .into_iter()
        .next()
        .expect("one cylinder carrier");
    bytes.extend_from_slice(&b2_cylinder_stream());
    for uv in [[0.0, 2.0], [1.0, 3.0]] {
        let point = crate::families::b2::records::b2_cylinder_point(&cylinder, uv)
            .expect("cylinder endpoint");
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [point.x, point.y, point.z] {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }

    let resolved = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    let [edge] = resolved.as_slice() else {
        panic!("one consolidated edge block");
    };
    assert_eq!(edge.supports, [None, None]);
    assert!(edge.shared_loci.is_none());
}

#[test]
fn flagged_fbb_marker_is_structural() {
    assert!(crate::container::is_fbb_row(&[
        0xb0, 0x04, 0x04, 0xff, 0x99, 0x1f, 0x1a, 0xd1,
    ]));
    assert!(!crate::container::is_fbb_row(&[
        0x20, 0x04, 0x04, 0xff, 0xff, 0xc4, 0xb2, 0xaa,
    ]));
}

#[test]
fn fbb_census_separates_groups_from_face_rows() {
    let row = [0x30, 0x04, 0x04, 0xff, 0, 1, 2, 3];
    let mut body = row.to_vec();
    body.extend_from_slice(&row);
    body.extend_from_slice(&[0xaa; 8]);
    body.extend_from_slice(&row);

    assert_eq!(crate::container::fbb_run_ranges(&body), vec![0..16, 24..32]);
    let scan = crate::container::scan_bytes(standard_catpart());
    assert_eq!(scan.census.fbb_runs, 1);
    assert_eq!(scan.census.fbb_face_rows, 2);
}

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

    assert!(result.report().geometry_transferred);
    // Three vertex records → three points and three vertices.
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    // A vertex coordinate is transferred verbatim in millimetres (no scaling).
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|p| (p.position.x - 10.0).abs() < 1e-6));

    // Cylinder and tag-bridged plane carriers are decoded from their stored
    // parameters.
    assert_eq!(result.ir().model.surfaces.len(), 2);
    assert_eq!(result.ir().model.curves.len(), 1);
    let unknowns = result.ir().native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].id.0, "catia:payload:unknown#brep-stream");
    assert!(unknowns[0]
        .links
        .contains(&"catia:standard:circle#0".to_string()));
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Cylinder { radius, axis, .. } => {
            assert!((radius - 5.0).abs() < 1e-6);
            assert!((axis.z - 1.0).abs() < 1e-6);
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
            if (origin.x - 1.0).abs() < 1e-6
                && (origin.y - 2.0).abs() < 1e-6
                && (origin.z - 3.0).abs() < 1e-6
                && normal.x.abs() < 1e-6
                && normal.y.abs() < 1e-6
                && (normal.z.abs() - 1.0).abs() < 1e-6
                && (u_axis.x * u_axis.x + u_axis.y * u_axis.y + u_axis.z * u_axis.z - 1.0).abs() < 1e-6
                && (u_axis.x * normal.x + u_axis.y * normal.y + u_axis.z * normal.z).abs() < 1e-6
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
            .coverage
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
        .map(|key| result.report().coverage.get(key).copied().unwrap_or(0))
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
        .map(|key| result.report().coverage.get(key).copied().unwrap_or(0))
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
            .coverage
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
            .coverage
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
        .map(|key| decoded.report().coverage.get(key).copied().unwrap_or(0))
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
        .map(|key| decoded.report().coverage.get(key).copied().unwrap_or(0))
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
            .coverage
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
fn decode_zero_entity_falls_back_to_metadata() {
    let f = zero_entity_catpart();
    let scan = crate::container::scan_bytes(f.clone());
    assert_eq!(scan.variant, Variant::ZeroEntity);
    assert!(scan.inner.is_none());

    let mut cur = Cursor::new(f);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.report().geometry_transferred);
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(
        source.attributes.get("variant").map(String::as_str),
        Some("zero_entity")
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.message.contains("zero_entity")));
}

#[test]
fn zero_entity_directory_markers_stay_outside_the_record_stream() {
    let mut body = vec![0u8; 16];
    body[12..].copy_from_slice(&[0xa9, 0x03, 0x10, 0x08]);
    let directory = [0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let directory_offset = 16 + body.len();
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(
        u32::try_from(directory_offset).expect("bounded directory offset"),
    ));
    file.extend_from_slice(&be32(
        u32::try_from(directory.len()).expect("bounded directory length"),
    ));
    file.extend_from_slice(&body);
    file.extend_from_slice(&directory);

    let scan = crate::container::scan_bytes(file);
    assert_eq!(scan.census.a9_records, 0);
    assert_eq!(scan.variant, Variant::Unknown);
    let ranges = crate::container::consolidated_record_ranges(&scan);
    let native = crate::native::CatiaNative::decode_with_record_ranges(&scan.data, &ranges);
    assert!(native.zero_entity_records.is_empty());
    assert!(native.zero_entity_support_runs.is_empty());
}

#[test]
fn zero_entity_finjpl_records_stay_outside_the_record_stream() {
    let record = [0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut body = record.to_vec();
    body.extend_from_slice(b"FINJPL  ");
    body.extend_from_slice(&record);
    let directory = [0u8; 16];
    let directory_offset = 16 + body.len();
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(
        u32::try_from(directory_offset).expect("bounded directory offset"),
    ));
    file.extend_from_slice(&be32(
        u32::try_from(directory.len()).expect("bounded directory length"),
    ));
    file.extend_from_slice(&body);
    file.extend_from_slice(&directory);

    let scan = crate::container::scan_bytes(file);
    assert_eq!(scan.census.a9_records, 1);
    assert_eq!(scan.variant, Variant::ZeroEntity);
    let ranges = crate::container::consolidated_record_ranges(&scan);
    let native = crate::native::CatiaNative::decode_with_record_ranges(&scan.data, &ranges);
    assert_eq!(native.zero_entity_records.len(), 1);
}

#[test]
fn decode_accounts_for_unresolved_legacy_entity_runs() {
    let mut bytes = zero_entity_catpart();
    for (entity_id, lead) in [(1_u32, 0x81), (3, 0xe5), (8, 0xfd)] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([lead, 0xfd, 0x8c]);
    }
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode legacy identity run");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ENTITY_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ENTITY_IDENTITY_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_81_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_82_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_E5_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_FD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_SELECTOR_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.message.contains("legacy design run")
    }));
}

#[test]
fn decode_retains_compound_legacy_text_fields_and_relation_roles() {
    fn compound_field(bytes: &mut Vec<u8>, value: &str, role: &str, selector_low: u8) {
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend(value.as_bytes());
        bytes.push(u8::try_from(role.len() + 1).expect("short role"));
        bytes.extend(role.as_bytes());
        bytes.extend([0xe3, selector_low]);
    }

    fn selected_compound_field(
        bytes: &mut Vec<u8>,
        value: &str,
        role_selector: u8,
        selector_low: u8,
    ) {
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend(value.as_bytes());
        bytes.extend([role_selector, 0xe3, selector_low]);
    }

    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0xe5);
    compound_field(&mut bytes, "", "body", 0x53);
    compound_field(&mut bytes, "2 * #1_", "param", 0x52);
    compound_field(&mut bytes, "(#1_ : #In LENGTH) : LENGTH\n", "opened", 0x51);
    bytes.push(0xea);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0xfd);
    bytes.extend([0xa2, 0xe3, 0xa0]);
    selected_compound_field(&mut bytes, "", 0xcf, 0x9f);
    selected_compound_field(&mut bytes, "#1_ + #2_", 0xd1, 0x9e);
    selected_compound_field(
        &mut bytes,
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : LENGTH\n",
        0xd3,
        0x9d,
    );
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode compound legacy fields");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_TEXT_FIELD_COUNT),
        6
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_E3_ROLE_TAIL_TEXT_FIELD_COUNT),
        6
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_TEXT_FIELD_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SELECTED_ROLE_COUNT),
        4
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SCHEMA_FIELD_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_RELATION_COUNT),
        2
    );

    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load compound legacy fields");
    assert!(native.legacy_entity_runs[0]
        .text_fields
        .iter()
        .all(|field| {
            field.encoding == crate::native::CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
        }));
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].expression,
        "2 * #1_"
    );
    assert_eq!(
        native.legacy_entity_runs[0].relations[1].expression,
        "#1_ + #2_"
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[3]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&crate::native::CatiaLegacyRoleName::Selector(0xa2), 4769))
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[4]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&crate::native::CatiaLegacyRoleName::Selector(0xcf), 4768))
    );

    let mut invalid_relation_pair = native.clone();
    let prelude = invalid_relation_pair.legacy_entity_runs[0].text_fields[3].clone();
    invalid_relation_pair.legacy_entity_runs[0].relations[1].expression_offset =
        prelude.byte_offset;
    invalid_relation_pair.legacy_entity_runs[0].relations[1].expression = prelude.value;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_relation_pair
        .store(&mut namespace)
        .expect("store invalid selected relation pair");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].role_selectors[3].name =
        crate::native::CatiaLegacyRoleName::Selector(0);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid selected role");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn decode_retains_legacy_relation_synchronous_states() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0x81, 0xfd, 0x8c]);
    for (selector, state) in [(15108_u32, 0x81), (15109, 0x82)] {
        bytes.extend([
            10, b's', b'y', b'n', b'c', b'h', b'r', b'o', b'n', b'e', 0x80,
        ]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend([0xe8, 0x00, 0x1c, 0x01, state, 0xfe]);
    }
    bytes.extend([0xa3, 0xe3, 0x3c, 0xe8, 0x00, 0x1c, 0x01, 0x82]);
    bytes.extend([0xa4, 0xe3, 0x3d, 0xe8, 0x34, 0x17, 0x01, 0xfe]);
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode legacy relation update states");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SYNCHRONOUS_STATE_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SYNCHRONOUS_RELATION_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ASYNCHRONOUS_RELATION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_SCHEMA_FIELD_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT),
        4
    );
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load retained update states");
    assert_eq!(
        native.legacy_entity_runs[0]
            .synchronous_states
            .iter()
            .map(|state| (state.selector, state.synchronous))
            .collect::<Vec<_>>(),
        [(15108, false), (15109, true), (4669, true)]
    );
    assert_eq!(
        native.legacy_entity_runs[0]
            .schema_fields
            .iter()
            .map(|field| (field.field_code, field.payload.as_slice()))
            .collect::<Vec<_>>(),
        [
            (0x1c00, &[0x81, 0xfe][..]),
            (0x1c00, &[0x82, 0xfe][..]),
            (0x1c00, &[0x82][..]),
        ]
    );

    let mut missing_selected_successor = native.clone();
    missing_selected_successor.legacy_entity_runs[0]
        .role_selectors
        .pop();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    missing_selected_successor
        .store(&mut namespace)
        .expect("store selected state without successor role");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_field_boundary = native.clone();
    invalid_field_boundary.legacy_entity_runs[0].schema_fields[0].boundary_role_byte_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_field_boundary
        .store(&mut namespace)
        .expect("store invalid schema-field boundary");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut missing_bound_field_code = native.clone();
    missing_bound_field_code.legacy_entity_runs[0].role_selectors[0].field_code = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    missing_bound_field_code
        .store(&mut namespace)
        .expect("store schema field without its role binding");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].synchronous_states[0].selector += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid relation update state");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_parameter() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    bytes.extend(12.5_f64.to_bits().to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy parameter");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy parameter")
    };
    assert_eq!(parameter.name, "Width");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.5)
        ))
    );
    assert_eq!(parameter.expression, "12.5 mm");
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("catia:legacy:entity-run#")));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_string() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([
        12, b'R', b'e', b's', b'p', b'o', b'n', b's', b'i', b'b', b'l', b'e', 0xfe,
    ]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'S', b't', b'r', b'i', b'n', b'g', 0x83]);
    bytes.extend(b"\xfe\x85\x93\x82\xfe");
    bytes.extend([
        12, b'C', b'i', b'l', b'a', b's', b' ', b'E', b'v', b'a', b'n', b's',
    ]);
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy string");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy string")
    };
    assert_eq!(parameter.name, "Responsible");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "Cilas Evans".to_string()
        ))
    );
    assert_eq!(parameter.expression, "\"Cilas Evans\"");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_STRING_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_NAMED_STRING_VALUE_COUNT),
        1
    );
}

#[test]
fn decode_transfers_an_input_bound_legacy_string_formula() {
    fn named_string(bytes: &mut Vec<u8>, entity_id: u32, name: &str, value: &str) {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1]);
        bytes.push(u8::try_from(entity_id - 1).expect("small name selector"));
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(name.len() + 1).expect("short parameter name"));
        bytes.extend(name.as_bytes());
        bytes.push(0xfe);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.extend([7, b'S', b't', b'r', b'i', b'n', b'g', 0x83]);
        bytes.extend(b"\xfe\x85\x93\x82\xfe");
        bytes.push(u8::try_from(value.len() + 1).expect("short string value"));
        bytes.extend(value.as_bytes());
    }

    fn relation_field(bytes: &mut Vec<u8>, role: &str, selector: &[u8], value: &str) {
        bytes.push(u8::try_from(role.len() + 1).expect("short relation role"));
        bytes.extend(role.as_bytes());
        bytes.extend(selector);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short relation text"));
        bytes.extend(value.as_bytes());
        bytes.push(0xfe);
    }

    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0x81, 0xfd, 0x8c]);
    relation_field(
        &mut bytes,
        "body",
        &[0x80, 1, 0, 0, 0],
        "#3_ = #1_ + \"-\" + #2_",
    );
    relation_field(
        &mut bytes,
        "param",
        &[0xd1, 3],
        "(#1_ : #In String,#2_ : #In String,#3_ : #Out String) : VoidType\n",
    );
    named_string(&mut bytes, 2, "#1_", "left");
    named_string(&mut bytes, 3, "#2_", "right");
    named_string(&mut bytes, 4, "Result", "left-right");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode input-bound legacy string formula");
    let result = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .expect("legacy formula result parameter");
    assert_eq!(
        result.value,
        Some(cadmpeg_ir::ParameterValue::String("left-right".to_string()))
    );
    assert_eq!(result.expression, "#1_ + \"-\" + #2_");
    let dependency_names = result
        .dependencies
        .iter()
        .map(|dependency| {
            decoded
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *dependency)
                .expect("legacy formula dependency")
                .name
                .as_str()
        })
        .collect::<Vec<_>>();
    assert_eq!(dependency_names, ["#1_", "#2_"]);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_integer() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'C', b'o', b'u', b'n', b't', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([8, b'I', b'n', b't', b'e', b'g', b'e', b'r', 0x83]);
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy integer");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy integer")
    };
    assert_eq!(parameter.name, "Count");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(11))
    );
    assert_eq!(parameter.expression, "11");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_INTEGER_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEGACY_NAMED_INTEGER_VALUE_COUNT),
        1
    );
}

#[test]
fn decode_transfers_an_unset_typed_legacy_parameter() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe7");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode unset typed legacy parameter");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one transferred legacy parameter")
    };
    assert_eq!(parameter.name, "Width");
    assert_eq!(parameter.value, None);
    assert!(parameter.expression.is_empty());
    assert_eq!(parameter.properties["value_type"], "LENGTH");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_transfers_unset_non_numeric_legacy_parameters() {
    for parameter_type in ["Boolean", "String"] {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([6, b'V', b'a', b'l', b'u', b'e', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short parameter type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe\xe7");
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode unset non-numeric legacy parameter");
        let [parameter] = decoded.ir().model.parameters.as_slice() else {
            panic!("one transferred legacy parameter")
        };

        assert_eq!(parameter.name, "Value");
        assert_eq!(parameter.value, None);
        assert!(parameter.expression.is_empty());
        assert_eq!(parameter.properties["value_type"], parameter_type);
        assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn decode_transfers_intrinsically_typed_evaluated_string_and_integer_parameters() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([0x58, 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01\x09Revision\xfe");
    bytes.extend([0x5f, 0xd1, 9]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x93\x82\xfe\x0bRevision-1");
    bytes.push(0xea);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([0x58, 0xd1, 10]);
    bytes.extend(b"\xe8\x00\x12\x01\x07Search\xfe");
    bytes.extend([6, b'V', b'a', b'l', b'b', b'y', 0xd1, 11]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x80");
    bytes.extend((-7_i32).to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode intrinsically typed evaluated parameters");

    let [string, integer] = decoded.ir().model.parameters.as_slice() else {
        panic!("two transferred evaluated parameters")
    };
    assert_eq!(string.name, "Revision");
    assert_eq!(
        string.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "Revision-1".to_string()
        ))
    );
    assert_eq!(string.expression, "\"Revision-1\"");
    assert_eq!(string.properties["value_type"], "String");
    assert_eq!(integer.name, "Search");
    assert_eq!(
        integer.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(-7))
    );
    assert_eq!(integer.properties["value_type"], "Integer");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        2
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_does_not_override_a_string_value_type_descriptor() {
    for descriptor in [
        b"\xfe\x84\x92\x82\x08Integer\x83".as_slice(),
        b"\xfe\x84\x92\x82\x82\x83".as_slice(),
    ] {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([0x58, 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01\x06Value\xfe");
        bytes.extend([0x5f, 0xd1, 9]);
        bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
        bytes.extend(descriptor);
        bytes.extend(b"\xfe\x85\x93\x82\xfe\x05Text");
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode string with an incompatible or unresolved descriptor");

        assert!(decoded.ir().model.parameters.is_empty());
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
            0
        );
    }
}

#[test]
fn decode_rejects_a_legacy_parameter_with_multiple_type_descriptors() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    for value_type in [b"LENGTH".as_slice(), b"Real".as_slice()] {
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(value_type.len() + 1).expect("short type"));
        bytes.extend(value_type);
        bytes.push(0x83);
    }
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    bytes.extend(12.5_f64.to_bits().to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode ambiguous legacy parameter");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        0
    );
}

#[test]
fn decode_resolves_only_an_acyclic_unique_legacy_type_selector_chain() {
    fn selected_type(terminal: Option<&str>) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82\x84\x83");
        bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
        bytes.extend(8.0_f64.to_bits().to_le_bytes());
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend(b"\xfe\x84\x92\x82");
        if let Some(value_type) = terminal {
            bytes.push(u8::try_from(value_type.len() + 1).expect("short type"));
            bytes.extend(value_type.as_bytes());
            bytes.push(0x83);
        } else {
            bytes.extend([0x81, 0x83]);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(selected_type(Some("LENGTH"))),
            &DecodeOptions::default(),
        )
        .expect("decode selected legacy type");
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(8.0)
        ))
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT),
        1
    );

    let cyclic = CatiaCodec
        .decode(
            &mut Cursor::new(selected_type(None)),
            &DecodeOptions::default(),
        )
        .expect("decode cyclic legacy type");
    assert!(cyclic.ir().model.parameters.is_empty());
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT),
        0
    );
}

#[test]
fn decode_transfers_only_an_agreeing_closed_legacy_formula() {
    fn legacy_constant(
        expression: &str,
        stored: Option<f64>,
        parameter_type: &str,
        relation_type: &str,
    ) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        let signature = format!("() : {relation_type}\n");
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, signature.as_str()),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([7, b'R', b'e', b's', b'u', b'l', b't', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe");
        if let Some(stored) = stored {
            bytes.push(0xe6);
            bytes.extend(stored.to_bits().to_le_bytes());
        } else {
            bytes.push(0xe7);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", Some(5.0), "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy formula");
    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one legacy formula parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);

    let mismatched = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", Some(6.0), "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched legacy formula");
    assert_eq!(mismatched.ir().model.parameters[0].expression, "6");
    assert_eq!(
        mismatched
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", None, "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode unset closed legacy formula");
    let [parameter] = unset.ir().model.parameters.as_slice() else {
        panic!("one unset legacy formula parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(parameter.value, None);
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );

    let mismatched_unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", None, "LENGTH", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode type-mismatched unset legacy formula");
    let [parameter] = mismatched_unset.ir().model.parameters.as_slice() else {
        panic!("one unset legacy parameter")
    };
    assert!(parameter.expression.is_empty());
    assert_eq!(parameter.value, None);
    assert_eq!(
        mismatched_unset
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let boolean = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("not false", None, "Boolean", "Boolean")),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean negation formula");
    let [parameter] = boolean.ir().model.parameters.as_slice() else {
        panic!("one Boolean formula parameter")
    };
    assert_eq!(parameter.expression, "not false");
    assert_eq!(parameter.value, None);
    assert_eq!(
        parameter.properties.get("value_type").map(String::as_str),
        Some("Boolean")
    );
    assert_eq!(
        boolean
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );

    let conditional = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant(
                "true ? 5 ; 1 / 0",
                Some(5.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode lazy conditional formula");
    let [parameter] = conditional.ir().model.parameters.as_slice() else {
        panic!("one conditional formula parameter")
    };
    assert_eq!(parameter.expression, "true ? 5 ; 1 / 0");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(
        conditional
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
}

#[test]
fn decode_transfers_a_zero_input_legacy_output_assignment() {
    fn legacy_output_assignment(
        expression: &str,
        stored: Option<f64>,
        parameter_type: &str,
        output_type: &str,
    ) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        let signature = format!("(#1_ : #Out {output_type}) : VoidType\n");
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, signature.as_str()),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([7, b'R', b'e', b's', b'u', b'l', b't', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe");
        if let Some(stored) = stored {
            bytes.push(0xe6);
            bytes.extend(stored.to_bits().to_le_bytes());
        } else {
            bytes.push(0xe7);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let transferred = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment(
                "#1_ = 2+3",
                Some(5.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode output assignment");
    let [parameter] = transferred.ir().model.parameters.as_slice() else {
        panic!("one legacy output parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(
        transferred
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment("#1_ = 2+3", None, "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode unset output assignment");
    assert_eq!(unset.ir().model.parameters[0].expression, "2+3");
    assert_eq!(unset.ir().model.parameters[0].value, None);

    let mismatched_value = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment(
                "#1_ = 2+3",
                Some(6.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched output assignment");
    assert_eq!(mismatched_value.ir().model.parameters[0].expression, "6");
    assert_eq!(
        mismatched_value
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let mismatched_type = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_output_assignment(
                "#1_ = 2+3",
                None,
                "LENGTH",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode type-mismatched output assignment");
    assert!(mismatched_type.ir().model.parameters[0]
        .expression
        .is_empty());
    assert_eq!(
        mismatched_type
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );
}

#[test]
fn decode_transfers_an_agreeing_closed_legacy_string_formula() {
    fn legacy_string_constant(expression: &str, stored: &str) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, "() : String\n"),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([0x58, 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01\x0fNewResponsible\xfe");
        bytes.extend([0x5f, 0xd1, 9]);
        bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
        bytes.extend(b"\xfe\x85\x93\x82\xfe");
        bytes.push(u8::try_from(stored.len() + 1).expect("short stored string"));
        bytes.extend(stored.as_bytes());
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")",
                "Easy Evans",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy string formula");
    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("one legacy string formula parameter")
    };
    assert_eq!(
        parameter.expression,
        "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")"
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::String("Easy Evans".to_string()))
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());

    let mismatched = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")",
                "Cilas Evans",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched legacy string formula");
    assert_eq!(
        mismatched.ir().model.parameters[0].expression,
        "\"Cilas Evans\""
    );
    assert_eq!(
        mismatched
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        0
    );

    let methods = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ToLower(\"MIXED\").Extract(1,4) - \"x\"",
                "ied",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy string-method formula");
    let [parameter] = methods.ir().model.parameters.as_slice() else {
        panic!("one legacy string-method formula parameter")
    };
    assert_eq!(
        parameter.expression,
        "ToLower(\"MIXED\").Extract(1,4) - \"x\""
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::String("ied".to_string()))
    );
    assert_eq!(
        methods
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT),
        1
    );
}

#[test]
fn decode_zero_entity_transfers_framed_cylinder() {
    let mut cur = Cursor::new(zero_entity_cylinder_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(result.ir().model.points.is_empty());
    assert!(result.ir().model.vertices.is_empty());
    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.shells.is_empty());
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
            assert_eq!(*radius, 4.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_parametric_surface_curve_without_a_cache() {
    let result = CatiaCodec
        .decode(
            &mut Cursor::new(zero_entity_cylinder_parametric_support_catpart()),
            &DecodeOptions::default(),
        )
        .expect("decode zero-entity parametric support");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_ZERO_ENTITY_SUPPORT_CURVE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT
        ),
        1
    );
    let [curve] = result.ir().model.curves.as_slice() else {
        panic!("one transferred support curve")
    };
    let [construction] = result.ir().model.procedural_curves.as_slice() else {
        panic!("one cacheless support construction")
    };
    assert!(matches!(
        &curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Procedural {
            construction: id
        } if id == &construction.id
    ));
    assert_eq!(construction.curve, curve.id);
    assert_eq!(construction.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail: None,
    } = &construction.definition
    else {
        panic!("parametric surface-curve construction")
    };
    assert_eq!(
        *family,
        cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric
    );
    assert_eq!(context.parameter_range, [0.0, 1.0]);
    assert_eq!(
        context.sides[0].surface.as_ref(),
        Some(&result.ir().model.surfaces[0].id)
    );
    assert!(context.sides[0].pcurve.is_some());
    assert_eq!(context.sides[1].surface, None);
    assert_eq!(context.sides[1].pcurve, None);

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_exact_model_curve_directly() {
    let mut file = vec![0u8; 16];
    file[..8].copy_from_slice(OUTER_MAGIC);
    file.extend(zero_entity_support_stream());
    let result = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity exact support");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_ZERO_ENTITY_SUPPORT_CURVE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT
        ),
        0
    );
    assert!(matches!(
        result.ir().model.curves.as_slice(),
        [cadmpeg_ir::geometry::Curve {
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(_),
            ..
        }]
    ));
    assert!(result.ir().model.procedural_curves.is_empty());

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_inline_nurbs_surface() {
    let mut cur = Cursor::new(zero_entity_nurbs_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (3, 3));
            assert_eq!((surface.u_count, surface.v_count), (7, 7));
            assert_eq!(
                surface.u_knots,
                vec![0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0]
            );
            assert_eq!(surface.control_points.len(), 49);
            assert_eq!(surface.control_points[48].x, 48.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn decode_geometry_fallback_transfers_an_external_a8_pole_grid() {
    let file = object_main_catpart(&a8_elided_surface_stream());
    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 9);
    assert_eq!(surface.control_points[8], Point3::new(8.0, 2.0, 2.0));
}

#[test]
fn decode_float_packed_stream_transfers_an_elided_a8_surface_with_native_topology() {
    let stream = a8_elided_surface_stream_with_native_vertex_chain();
    let graph = crate::families::b5::graph::parse(&stream).expect("generated A8 topology");
    assert!(graph.complete);
    assert_eq!(graph.faces.len(), 1);
    assert_eq!(graph.loops.len(), 1);
    assert_eq!(graph.pcurves.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(graph.logical_vertex_refs, [600, 601, 602]);
    assert_eq!(
        graph.logical_vertex_points,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode elided A8 surface topology");
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points[8], Point3::new(1.0, 1.0, 0.0));
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_object_stream_does_not_promote_unbound_a8_pcurve() {
    let file = object_main_catpart(&a8_pcurve_stream());
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode unbound object-stream pcurve");
    assert!(decoded.ir().model.pcurves.is_empty());
    assert!(!decoded.ir().native_unknowns("catia").unwrap().is_empty());
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
fn native_namespace_retains_unbound_consolidated_pcurve_jets() {
    let mut bytes = Vec::new();
    for _ in 0..6 {
        bytes.extend(a5_pcurve_stream());
        bytes.extend(b2_pcurve_stream());
    }
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_pcurves.len(), 12);
    assert_eq!(
        native.consolidated_pcurves[0].family,
        crate::native::CatiaConsolidatedFamily::A
    );
    assert_eq!(
        native.consolidated_pcurves[1].family,
        crate::native::CatiaConsolidatedFamily::B
    );
    assert_eq!(native.consolidated_pcurves[0].support_id, 0x1234);
    assert_eq!(
        native.consolidated_pcurves[0].points,
        vec![[0.0, 0.0], [1.0, 1.0]]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA pcurves");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA pcurves"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_pcurves[0].degree = 4;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA pcurve for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_typed_consolidated_groups() {
    let native = crate::native::CatiaNative::decode(&b2_group_stream());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group")
    };
    assert_eq!(group.byte_offset, 9);
    assert_eq!(group.group_type, 3);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA consolidated groups");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA consolidated groups"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_groups[0].id.push_str("-changed");
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA consolidated group for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_class61_records() {
    let mut stream = b2_counted_61_stream();
    stream.extend_from_slice(&b2_long_61_stream());
    let native = crate::native::CatiaNative::decode(&stream);
    let [counted, long] = native.consolidated_class61_records.as_slice() else {
        panic!("two consolidated class-0x61 records")
    };
    let crate::native::CatiaConsolidatedClass61Payload::Counted { references, tail } =
        &counted.payload
    else {
        panic!("counted class-0x61 record")
    };
    assert_eq!(references, &[1300, 1294, 30, 74]);
    assert_eq!(tail, &[0x41, 0x03]);
    let crate::native::CatiaConsolidatedClass61Payload::Long {
        prefix,
        members,
        references,
        scalar,
    } = &long.payload
    else {
        panic!("long class-0x61 record")
    };
    assert_eq!(prefix, &[0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb]);
    assert_eq!(members, &[0x064a, 0x0650, 0x0656]);
    assert_eq!(references, &[0x0100, 0x0103, 0x0106, 0x0109, 0x010c]);
    assert_eq!(*scalar, 42.5);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA class-0x61 records");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA class-0x61 records"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaConsolidatedClass61Payload::Long { members, .. } =
        &mut invalid.consolidated_class61_records[1].payload
    else {
        panic!("long class-0x61 record")
    };
    members.swap(0, 1);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA class-0x61 record for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_parameter_point_layouts() {
    let native = crate::native::CatiaNative::decode(&b2_parameter_point_stream());
    let [uv, station_uv, five_scalars, station_uv_last] =
        native.consolidated_parameter_points.as_slice()
    else {
        panic!("four consolidated parameter points")
    };
    assert_eq!(
        [
            uv.prefix,
            station_uv.prefix,
            five_scalars.prefix,
            station_uv_last.prefix
        ],
        [0x05, 0x09, 0x0d, 0x11]
    );
    assert_eq!(uv.layout, 0x12);
    assert_eq!(uv.control, 0x12);
    assert!(matches!(
        &uv.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::Uv { uv: [2.0, 3.0] }
    ));
    assert_eq!(station_uv.layout, 0x1a);
    assert!(matches!(
        &station_uv.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::StationUv {
            station: 11.0,
            uv: [4.0, 5.0],
        }
    ));
    assert_eq!(five_scalars.layout, 0x2a);
    assert!(matches!(
        &five_scalars.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::FiveScalars {
            values: [1.0, 2.0, 3.0, 4.0, 5.0],
        }
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA parameter points");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA parameter points"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_parameter_points[0].layout = 0x1a;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA parameter point");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_plane_carrier_layouts() {
    let plane_stream = b2_plane_carrier_stream();
    let native = crate::native::CatiaNative::decode(&plane_stream);
    let [direction2, direction3, tail] = native.consolidated_plane_carriers.as_slice() else {
        panic!("three consolidated plane carriers")
    };
    assert_eq!(
        [direction2.selector, direction3.selector, tail.selector],
        [0xe4, 0xc4, 0xec]
    );
    assert!(matches!(
        &direction2.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
            point: [10.0, 20.0],
            direction: [1.0, 0.0],
            tail: [5.0, -2.0, 3.0],
        }
    ));
    assert!(matches!(
        &direction3.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
            point: [10.0, 20.0],
            direction: [1.0, 0.0, 0.0],
            tail: [5.0, -2.0, 3.0],
        }
    ));
    assert!(matches!(
        &tail.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::PointTail {
            point: [10.0, 20.0],
            tail: [-2.0, 5.0, -2.0, 3.0],
        }
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA plane carriers");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA plane carriers"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_plane_carriers[0].selector = 0xc4;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA plane carrier");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut file = standard_catpart();
    file.splice(16..16, plane_stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode CATIA plane carrier coverage");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_PLANE_CARRIER_COUNT),
        3
    );
}

#[test]
fn native_namespace_retains_unclassified_consolidated_plane_carrier_lanes() {
    let mut stream = b2_plane_carrier_stream();
    let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    stream.extend_from_slice(&[
        0xb2,
        0x03,
        0x27,
        2 + u8::try_from(values.len() * 8).expect("scalar lane fixture"),
        0x05,
        0xb4,
        0x40,
    ]);
    for value in values {
        stream.extend_from_slice(&le_f64(value));
    }

    let native = crate::native::CatiaNative::decode(&stream);
    let Some(carrier) = native.consolidated_plane_carriers.get(3) else {
        panic!("unclassified consolidated plane carrier")
    };
    assert_eq!(carrier.selector, 0x40);
    assert!(matches!(
        &carrier.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::ScalarLane { values: lane }
            if lane == &values
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store unclassified plane carrier");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load unclassified plane carrier"),
        native
    );
}

#[test]
fn native_namespace_retains_consolidated_reference_lists() {
    let native = crate::native::CatiaNative::decode(&b2_reference_list_stream());
    let [list] = native.consolidated_reference_lists.as_slice() else {
        panic!("one consolidated reference list")
    };
    assert_eq!(list.references, (0u32..26).collect::<Vec<_>>());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA reference list");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA reference list"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_reference_lists[0].references.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA reference list");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_standalone_consolidated_circle_supports() {
    let native = crate::native::CatiaNative::decode(&b2_circle_stream());
    let [circle] = native.consolidated_circles.as_slice() else {
        panic!("one consolidated circle")
    };
    assert_eq!(circle.layout, 0x34);
    assert_eq!(circle.record_id, 0x1234);
    assert_eq!(circle.frame_token, 0x05);
    assert_eq!(circle.center_pair, [4.0, -2.0]);
    assert_eq!(circle.radius, 3.0);
    assert_eq!(circle.range, [0.0, std::f64::consts::TAU * circle.radius]);
    assert!(circle.full_circle);
    assert_eq!(circle.chart_shift, 0.0);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA circle");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA circle"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_circles[0].full_circle = false;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA circle for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_cylinder_layouts() {
    let mut stream = b2_cylinder_stream();
    stream.extend_from_slice(&b2_implicit_axis_cylinder_stream());
    stream.extend_from_slice(&b2_range_origin_cylinder_stream());
    let native = crate::native::CatiaNative::decode(&stream);
    let [explicit, implicit, range_origin] = native.consolidated_cylinders.as_slice() else {
        panic!("three consolidated cylinders")
    };
    assert_eq!(explicit.layout, 0x5a);
    assert_eq!(explicit.origin, [1.0, 2.0, 3.0]);
    assert_eq!(explicit.radius, 2.0);
    assert!(matches!(
        explicit.payload,
        crate::native::CatiaConsolidatedCylinderPayload::Resolved {
            frame_token: 0x19,
            axis: [1.0, 0.0, 0.0],
            reference_direction: [0.0, 1.0, 0.0],
        }
    ));
    assert_eq!(implicit.layout, 0x52);
    assert!(matches!(
        implicit.payload,
        crate::native::CatiaConsolidatedCylinderPayload::Resolved { .. }
    ));
    assert_eq!(range_origin.layout, 0x62);
    assert_eq!(range_origin.radius, 4.0);
    assert!(matches!(
        range_origin.payload,
        crate::native::CatiaConsolidatedCylinderPayload::RangeOrigin {
            stored_vector: [0.0, 1.0],
            axis: [0.0, 1.0, 0.0],
            reference_direction: [0.0, 0.0, 1.0],
            range_origin,
        } if range_origin.to_bits()
            == ((0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0).to_bits()
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA cylinders");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cylinders"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaConsolidatedCylinderPayload::RangeOrigin { range_origin, .. } =
        &mut invalid.consolidated_cylinders[2].payload
    else {
        panic!("range-origin cylinder")
    };
    *range_origin += 1.0;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cylinder for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_cone_charts() {
    let native = crate::native::CatiaNative::decode(&b2_cone_stream());
    let [cone] = native.consolidated_cones.as_slice() else {
        panic!("one consolidated cone")
    };
    assert_eq!(cone.apex, [1.0, 2.0, 3.0]);
    assert_eq!(cone.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(cone.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(cone.axis, [0.0, 0.0, 1.0]);
    assert_eq!(cone.half_angle, 0.25);
    assert_eq!(cone.pre_angular_range_scalar, 4.0);
    assert_eq!(cone.angular_range, [0.5, 0.5 + std::f64::consts::PI]);
    assert_eq!(cone.slant_range, [2.0, 8.0]);
    assert_eq!(cone.angular_scale, 3.0);
    assert_eq!(
        cone.angular_domain,
        [
            0.5 - std::f64::consts::FRAC_PI_2,
            0.5 + 3.0 * std::f64::consts::FRAC_PI_2
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA cone");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cone"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_cones[0].angular_domain[0] += 0.25;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_cone_face_charts() {
    let native = crate::native::CatiaNative::decode(&b2_cone_face_parameter_point_stream());
    let [face] = native.consolidated_cone_faces.as_slice() else {
        panic!("one consolidated cone-face chart")
    };
    assert_eq!(face.program.len(), 16);
    assert_eq!(face.angular_scale, 1.5);
    assert_eq!(face.half_angle, std::f64::consts::FRAC_PI_4);
    assert_eq!(
        face.parameter_points,
        [
            "catia:consolidated:parameter-point#0",
            "catia:consolidated:parameter-point#1",
            "catia:consolidated:parameter-point#2",
            "catia:consolidated:parameter-point#3",
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA cone-face chart");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cone-face chart"),
        native
    );

    let mut invalid = native.clone();
    invalid.consolidated_cone_faces[0].program.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone-face chart");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native.clone();
    invalid.consolidated_cone_faces[0]
        .parameter_points
        .swap(0, 1);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone-face parameter run");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut mixed = b2_cone_face_parameter_point_stream();
    mixed.extend_from_slice(&[0xb2, 0x03, 0x18, 0x02, 0x05, 0x99, 0x99]);
    let mixed = crate::native::CatiaNative::decode(&mixed);
    assert!(mixed.consolidated_cone_faces[0].parameter_points.is_empty());

    let mut file = standard_catpart();
    file.splice(16..16, b2_cone_face_parameter_point_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode CATIA cone-face chart");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_CONE_FACE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_CONE_FACE_PARAMETER_POINT_COUNT),
        4
    );
}

#[test]
fn native_namespace_retains_resolved_consolidated_revolution_carriers() {
    let native = crate::native::CatiaNative::decode(&b2_resolved_revolution_stream());
    let [revolution] = native.consolidated_revolutions.as_slice() else {
        panic!("one consolidated revolution carrier")
    };
    assert_eq!(revolution.reference_token, 0x0a);
    assert_eq!(revolution.profile_allocation_id, 0x1234);
    assert_eq!(revolution.origin, [1.0, 2.0, 3.0]);
    assert_eq!(revolution.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(revolution.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(revolution.axis, [0.0, 0.0, 1.0]);
    assert_eq!(revolution.profile_range, [-4.0, 9.0]);
    assert_eq!(
        revolution.profile_circle.as_deref(),
        Some("catia:consolidated:circle#0")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA revolution");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA revolution"),
        native
    );

    let mut invalid = native.clone();
    invalid.consolidated_revolutions[0].profile_circle = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA revolution profile binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native;
    invalid.consolidated_revolutions[0].axis = [0.0, 0.0, -1.0];
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA revolution for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut file = standard_catpart();
    file.splice(16..16, b2_resolved_revolution_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved CATIA revolution");
    let directrix = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:surface-revolution-directrix#")
        })
        .expect("transferred revolution directrix");
    assert!(matches!(
        directrix.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius: 3.0,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 4.0, -2.0)
            && axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            && ref_direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    ));
    let revolution = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            surface
                .id
                .0
                .starts_with("catia:consolidated:surface-revolution#")
        })
        .expect("transferred revolution construction");
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id == revolution.surface
            && matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                    center,
                    axis,
                    ref_direction,
                    major_radius: 2.0,
                    minor_radius: 3.0,
                } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, -2.0)
                    && axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                    && ref_direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
            )
    }));
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    assert!(matches!(
        &revolution.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            angular_interval,
            parameter_interval: Some([-4.0, 9.0]),
            ..
        } if *angular_interval == [0.5, 0.5 + std::f64::consts::TAU]
    ));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT),
        1
    );
    assert!(!decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("consolidated surface-of-revolution record")));
}

#[test]
fn native_namespace_retains_exact_consolidated_line_profiles() {
    let native = crate::native::CatiaNative::decode(&b2_line_profile_stream());
    let [line] = native.consolidated_line_profiles.as_slice() else {
        panic!("one consolidated line profile")
    };
    assert_eq!(line.origin, [1.0, 2.0, 3.0]);
    assert_eq!(line.direction, [0.0, 0.6, 0.8]);
    assert_eq!(line.range, [-4.0, 9.0]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA line profile");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA line profile"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_line_profiles[0].direction = [0.0, 0.0, 2.0];
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA line profile for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn decode_transfers_exact_consolidated_line_profiles() {
    let mut file = standard_catpart();
    file.splice(16..16, b2_line_profile_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated line profile");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_LINE_PROFILE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT),
        1
    );
    assert!(decoded.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && direction == cadmpeg_ir::math::Vector3::new(0.0, 0.6, 0.8)
    )));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("consolidated line-profile record(s)")));
}

#[test]
fn decode_routes_a_line_profile_only_nested_stream_to_a_wire() {
    let file = standard_catpart_from_streams(&b2_line_profile_stream(), &[]);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode line-profile-only nested stream");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(cadmpeg_ir::CoverageKey(
            "attached_standalone_wire_edge_count"
        )),
        1
    );
    assert_eq!(decoded.ir().model.edges[0].param_range, Some([-4.0, 9.0]));
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_routes_a_resolved_revolution_only_nested_stream_to_freeform() {
    let file = standard_catpart_from_streams(&b2_resolved_revolution_stream(), &[]);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode revolution-only nested stream");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT),
        1
    );
    let revolution = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.id.0 == "catia:consolidated:surface-revolution#0")
        .expect("transferred freeform revolution");
    assert!(matches!(
        revolution.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            parameter_interval: Some([-4.0, 9.0]),
            ..
        }
    ));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn transferred_line_profile_identities_retain_their_native_ordinals() {
    let mut file = standard_catpart();
    file.splice(
        16..16,
        [b2_line_profile_stream(), b2_line_profile_stream()].concat(),
    );
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode mixed-metric line profiles");
    let line_ids = decoded
        .ir()
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:line-profile-curve#")
        })
        .map(|curve| curve.id.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        line_ids,
        [
            "catia:consolidated:line-profile-curve#0",
            "catia:consolidated:line-profile-curve#1",
        ]
    );
}

#[test]
fn native_namespace_retains_zero_entity_surface_support_runs() {
    let mut stream = zero_entity_face_loop_support_stream();
    let support_slot = 0x6a + 12 + 13;
    stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());
    let native = crate::native::CatiaNative::decode(&stream);
    assert!(native.zero_entity_endpoint_pair_candidates.is_empty());
    let [run] = native.zero_entity_support_runs.as_slice() else {
        panic!("one zero-entity support run")
    };
    assert_eq!(run.carrier_byte_offset, 0);
    assert_eq!(run.carrier_record_ordinal, 1);
    let face = run.face.as_ref().expect("positionally aligned face");
    assert_eq!(face.record_ordinal, 3);
    assert_eq!(face.allocations, [10, 3]);
    assert_eq!(face.loop_terminals, [7]);
    let [loop_record] = face.loops.as_slice() else {
        panic!("one loop")
    };
    assert_eq!(loop_record.member_ids, [6]);
    assert_eq!(loop_record.typed_references, [1]);
    assert_eq!(
        loop_record.typed_records,
        ["catia:zero-entity:record#1".to_string()]
    );
    assert_eq!(loop_record.terminal_id, 7);
    assert_eq!(loop_record.loop_class, 0x41);
    assert_eq!(loop_record.forward_senses, [true]);
    assert_eq!(loop_record.support_record_ordinals, [2]);
    assert!(loop_record.oriented_model_endpoints.is_empty());
    let [support] = run.supports.as_slice() else {
        panic!("one zero-entity support occurrence")
    };
    assert_eq!(support.tag, [0x21, 0x71]);
    assert_eq!(support.record_ordinal, 2);
    assert_eq!(support.face_local_slot, 1);
    assert_eq!(support.uv_endpoints, Some([[-2.0, 4.0], [6.0, 8.0]]));
    assert!(matches!(
        support.pcurve,
        Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            ref control_points,
            weights: None,
            periodic: false,
            ..
        }) if control_points.len() == 2
    ));
    assert!(matches!(
        support.model_curve,
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve {
                degree: 1,
                ref control_points,
                weights: None,
                periodic: false,
                ..
            }
        )) if control_points.len() == 2
    ));
    assert!(support.model_curve_construction.is_none());
    assert_eq!(support.model_parameters, Some([0.0, 1.0]));
    assert_eq!(
        support.model_midpoint,
        Some(cadmpeg_ir::math::Point3::new(3.0, 8.0, 3.0))
    );
    assert_eq!(
        support.model_endpoints,
        Some([
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
            cadmpeg_ir::math::Point3::new(7.0, 10.0, 3.0),
        ])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity support run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA zero-entity support run"),
        native
    );

    let mut invalid_face = native.clone();
    invalid_face.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loop_terminals[0] = 8;
    let mut invalid_face_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face
        .store(&mut invalid_face_namespace)
        .expect("store invalid CATIA zero-entity face");
    assert!(crate::native::CatiaNative::load(&invalid_face_namespace).is_err());

    let mut zero_face_terminal = native.clone();
    zero_face_terminal.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loop_terminals[0] = 0;
    let mut zero_face_terminal_namespace = cadmpeg_ir::NativeNamespace::default();
    zero_face_terminal
        .store(&mut zero_face_terminal_namespace)
        .expect("store zero CATIA zero-entity face loop terminal");
    assert!(crate::native::CatiaNative::load(&zero_face_terminal_namespace).is_err());

    let mut invalid_loop_roster = native.clone();
    invalid_loop_roster.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .loop_class = 0x50;
    let mut invalid_loop_roster_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop_roster
        .store(&mut invalid_loop_roster_namespace)
        .expect("store invalid CATIA zero-entity loop roster");
    assert!(crate::native::CatiaNative::load(&invalid_loop_roster_namespace).is_err());

    let mut invalid_face_allocation = native.clone();
    invalid_face_allocation.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .allocations[0] = 0;
    let mut invalid_face_allocation_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face_allocation
        .store(&mut invalid_face_allocation_namespace)
        .expect("store invalid CATIA zero-entity face allocation");
    assert!(crate::native::CatiaNative::load(&invalid_face_allocation_namespace).is_err());

    let mut invalid_face_control = native.clone();
    invalid_face_control.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .terminal_control = 0x04;
    let mut invalid_face_control_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face_control
        .store(&mut invalid_face_control_namespace)
        .expect("store invalid CATIA zero-entity face control");
    assert!(crate::native::CatiaNative::load(&invalid_face_control_namespace).is_err());

    let mut invalid_loop_gap = native.clone();
    invalid_loop_gap.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .gap = 0;
    let mut invalid_loop_gap_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop_gap
        .store(&mut invalid_loop_gap_namespace)
        .expect("store invalid CATIA zero-entity loop gap");
    assert!(crate::native::CatiaNative::load(&invalid_loop_gap_namespace).is_err());

    let mut invalid_support_slot = native.clone();
    invalid_support_slot.zero_entity_support_runs[0].supports[0].face_local_slot = 0;
    let mut invalid_support_slot_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_support_slot
        .store(&mut invalid_support_slot_namespace)
        .expect("store invalid CATIA zero-entity support slot");
    assert!(crate::native::CatiaNative::load(&invalid_support_slot_namespace).is_err());

    let mut invalid_loop = native.clone();
    invalid_loop.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .forward_senses
        .clear();
    let mut invalid_loop_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop
        .store(&mut invalid_loop_namespace)
        .expect("store invalid CATIA zero-entity loop");
    assert!(crate::native::CatiaNative::load(&invalid_loop_namespace).is_err());

    let mut invalid_typed_record = native.clone();
    invalid_typed_record.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .typed_records[0] = "catia:zero-entity:record#2".to_string();
    let mut invalid_typed_record_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_typed_record
        .store(&mut invalid_typed_record_namespace)
        .expect("store invalid CATIA zero-entity typed loop reference");
    assert!(crate::native::CatiaNative::load(&invalid_typed_record_namespace).is_err());

    let mut invalid_binding = native.clone();
    invalid_binding.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .support_record_ordinals[0] = 1;
    let mut invalid_binding_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_binding
        .store(&mut invalid_binding_namespace)
        .expect("store invalid CATIA zero-entity loop support binding");
    assert!(crate::native::CatiaNative::load(&invalid_binding_namespace).is_err());

    let mut invalid_pcurve = native.clone();
    let Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree, .. }) =
        invalid_pcurve.zero_entity_support_runs[0].supports[0]
            .pcurve
            .as_mut()
    else {
        panic!("NURBS support pcurve")
    };
    *degree = 2;
    let mut invalid_pcurve_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_pcurve
        .store(&mut invalid_pcurve_namespace)
        .expect("store invalid CATIA zero-entity support pcurve");
    assert!(crate::native::CatiaNative::load(&invalid_pcurve_namespace).is_err());

    let mut invalid_model_curve = native.clone();
    let Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(model_curve)) =
        invalid_model_curve.zero_entity_support_runs[0].supports[0]
            .model_curve
            .as_mut()
    else {
        panic!("NURBS support model curve")
    };
    model_curve.periodic = true;
    let mut invalid_model_curve_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_curve
        .store(&mut invalid_model_curve_namespace)
        .expect("store invalid CATIA zero-entity support model curve");
    assert!(crate::native::CatiaNative::load(&invalid_model_curve_namespace).is_err());

    let mut invalid_model_parameters = native.clone();
    invalid_model_parameters.zero_entity_support_runs[0].supports[0].model_parameters =
        Some([1.0, 1.0]);
    let mut invalid_model_parameters_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_parameters
        .store(&mut invalid_model_parameters_namespace)
        .expect("store invalid CATIA zero-entity support model parameters");
    assert!(crate::native::CatiaNative::load(&invalid_model_parameters_namespace).is_err());

    let mut missing_model_midpoint = native.clone();
    missing_model_midpoint.zero_entity_support_runs[0].supports[0].model_midpoint = None;
    let mut missing_model_midpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    missing_model_midpoint
        .store(&mut missing_model_midpoint_namespace)
        .expect("store CATIA zero-entity support without its model midpoint");
    assert!(crate::native::CatiaNative::load(&missing_model_midpoint_namespace).is_err());

    let mut invalid_model_construction = native.clone();
    invalid_model_construction.zero_entity_support_runs[0].supports[0].model_curve_construction =
        Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            major: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            minor: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 1.0,
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        });
    let mut invalid_model_construction_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_construction
        .store(&mut invalid_model_construction_namespace)
        .expect("store invalid CATIA zero-entity support model construction");
    assert!(crate::native::CatiaNative::load(&invalid_model_construction_namespace).is_err());

    let mut invalid_oriented_endpoints = native.clone();
    invalid_oriented_endpoints.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .oriented_model_endpoints
        .push([
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        ]);
    let mut invalid_oriented_endpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_oriented_endpoints
        .store(&mut invalid_oriented_endpoint_namespace)
        .expect("store invalid CATIA zero-entity oriented endpoints");
    assert!(crate::native::CatiaNative::load(&invalid_oriented_endpoint_namespace).is_err());

    let mut invalid_endpoint_pair = native.clone();
    invalid_endpoint_pair
        .zero_entity_endpoint_pair_candidates
        .push(crate::native::CatiaZeroEntityEndpointPairCandidate {
            id: "catia:zero-entity:endpoint-pair-candidate#0".to_string(),
            face_records: [
                "catia:zero-entity:record#3".to_string(),
                "catia:zero-entity:record#3".to_string(),
            ],
            support_records: [
                "catia:zero-entity:record#2".to_string(),
                "catia:zero-entity:record#2".to_string(),
            ],
            model_endpoints: [
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            ],
            model_midpoint: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        });
    let mut invalid_endpoint_pair_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_endpoint_pair
        .store(&mut invalid_endpoint_pair_namespace)
        .expect("store invalid CATIA zero-entity endpoint pair");
    assert!(crate::native::CatiaNative::load(&invalid_endpoint_pair_namespace).is_err());

    let mut invalid_endpoint_locus = native.clone();
    invalid_endpoint_locus
        .zero_entity_endpoint_locus_candidates
        .push(crate::native::CatiaZeroEntityEndpointLocusCandidate {
            id: "catia:zero-entity:endpoint-locus-candidate#0".to_string(),
            incident_endpoint_pair_endpoints: vec![
                crate::native::CatiaZeroEntityEndpointPairEndpoint {
                    endpoint_pair: "catia:zero-entity:endpoint-pair-candidate#0".to_string(),
                    endpoint_index: 0,
                },
            ],
            representative_point: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            maximum_deviation: 0.0,
        });
    let mut invalid_endpoint_locus_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_endpoint_locus
        .store(&mut invalid_endpoint_locus_namespace)
        .expect("store invalid CATIA zero-entity endpoint-locus candidate");
    assert!(crate::native::CatiaNative::load(&invalid_endpoint_locus_namespace).is_err());

    let mut invalid_model_endpoint = native.clone();
    invalid_model_endpoint.zero_entity_support_runs[0].supports[0]
        .model_endpoints
        .as_mut()
        .expect("model endpoints")[0]
        .x = f64::NAN;
    let mut invalid_model_endpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_endpoint
        .store(&mut invalid_model_endpoint_namespace)
        .expect("store invalid CATIA zero-entity model endpoint");
    assert!(crate::native::CatiaNative::load(&invalid_model_endpoint_namespace).is_err());

    let mut invalid = native;
    invalid.zero_entity_support_runs[0].supports[0].uv_endpoints = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity support run");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_closed_zero_entity_endpoint_tapes() {
    let mut stream = zero_entity_face_loop_support_stream();
    let support = 0x6a + 12;
    stream[support + 13..support + 17].copy_from_slice(&1u32.to_le_bytes());
    let first_endpoint: [u8; 16] = stream[support + 93..support + 109]
        .try_into()
        .expect("endpoint pair");
    stream[support + 109..support + 125].copy_from_slice(&first_endpoint);

    let native = crate::native::CatiaNative::decode(&stream);
    let loop_record = &native.zero_entity_support_runs[0]
        .face
        .as_ref()
        .expect("face")
        .loops[0];
    assert_eq!(
        loop_record.oriented_model_endpoints,
        [[
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
        ]]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity endpoint tape");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA zero-entity endpoint tape"),
        native
    );
}

#[test]
fn native_namespace_retains_zero_entity_ownership_root() {
    let mut stream = zero_entity_face_support_stream();
    stream.extend(zero_entity_ownership_stream(1));
    let native = crate::native::CatiaNative::decode(&stream);
    let [root] = native.zero_entity_ownership_roots.as_slice() else {
        panic!("one zero-entity ownership root")
    };
    assert_eq!(root.face_slots, [1]);
    assert_eq!(root.face_roster_record_ordinal, 4);
    assert_eq!(root.shell_record_ordinal, 5);
    assert_eq!(root.body_record_ordinal, 6);
    assert_eq!(
        native.zero_entity_records[3].logical_end,
        root.shell_byte_offset
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity ownership root");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load CATIA zero-entity ownership root"),
        native
    );

    let mut invalid = native;
    invalid.zero_entity_ownership_roots[0].face_slots.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity ownership root");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_separate_zero_entity_topology_registries() {
    let native = crate::native::CatiaNative::decode(&zero_entity_topology_stream());
    assert_eq!(native.zero_entity_records.len(), 8);
    assert_eq!(native.zero_entity_records[0].record_ordinal, 1);
    assert_eq!(native.zero_entity_records[0].tag, [0x5e, 0x1a]);
    let [edge_stride] = native.zero_entity_edge_strides.as_slice() else {
        panic!("one zero-entity edge stride")
    };
    assert_eq!(edge_stride.record_ordinal, 1);
    assert_eq!(edge_stride.allocations, [5, 7, 8, 4, 3]);
    assert_eq!(edge_stride.topology_refs, [5, 4, 3]);
    assert_eq!(edge_stride.surface_support_refs, [7, 8]);

    let [pair] = native.zero_entity_oriented_use_pairs.as_slice() else {
        panic!("one zero-entity oriented-use pair")
    };
    assert_eq!(pair.header_record_ordinal, 2);
    assert_eq!(pair.base_columns, [100, 200]);

    let [incidence] = native.zero_entity_vertex_incidences.as_slice() else {
        panic!("one zero-entity vertex incidence")
    };
    assert_eq!(incidence.record_ordinal, 5);
    assert_eq!(incidence.allocations, [1, 2, 5]);
    assert_eq!(
        incidence.vertex_record.as_deref(),
        Some("catia:zero-entity:record#6")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity topology registries");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load CATIA zero-entity topology registries"),
        native
    );

    let mut invalid = native.clone();
    invalid.zero_entity_edge_strides[0].allocations[0] = 0;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity edge allocation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native.clone();
    invalid.zero_entity_vertex_incidences[0].vertex_record = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity vertex owner");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native;
    invalid.zero_entity_oriented_use_pairs[0].uses[1].allocations[0] += 1;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity topology registries");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn zero_entity_vertex_binding_declines_atomically_when_structure_changes() {
    let bytes = zero_entity_topology_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let vertex_offset =
        usize::try_from(native.zero_entity_records[5].byte_offset).expect("fixture byte offset");

    let mut missing_vertex = bytes;
    missing_vertex[vertex_offset + 2] = 0x60;
    let missing_vertex = crate::native::CatiaNative::decode(&missing_vertex);
    assert!(missing_vertex.zero_entity_vertex_incidences.is_empty());

    let mut separated_vertex = zero_entity_topology_stream();
    separated_vertex.insert(vertex_offset, 0xff);
    let separated_vertex = crate::native::CatiaNative::decode(&separated_vertex);
    assert!(separated_vertex.zero_entity_vertex_incidences.is_empty());
}

#[test]
fn decode_reports_zero_entity_surface_support_runs() {
    let mut file = standard_catpart();
    file.splice(16..16, zero_entity_face_loop_support_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity support run");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FACE_BOUND_SUPPORT_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_03_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_05_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_TERMINAL_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_RECORD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_41_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_50_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_C1_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FORWARD_LOOP_MEMBER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_REVERSED_LOOP_MEMBER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_LOOP_MEMBER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_OCCURRENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_PCURVE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CURVE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CONSTRUCTION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_UV_ENDPOINT_PAIR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_MODEL_MIDPOINT_COUNT),
        1
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss
                .message
                .contains("1 zero-entity surface-support run(s)")
            && loss
                .message
                .contains("1 run(s) bind the complete face roster")
            && loss.message.contains("1 stored member sense(s)")
            && loss.message.contains("oriented-use")
    }));
}

#[test]
fn decode_reports_separate_zero_entity_topology_registries() {
    let mut file = standard_catpart();
    file.splice(16..16, zero_entity_topology_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity topology registries");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_RECORD_COUNT),
        8
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_ALLOCATION_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_TOPOLOGY_REF_COUNT,),
        3
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_SURFACE_SUPPORT_REF_COUNT,
        ),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_PAIR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_ALLOCATION_COUNT),
        4
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_ALLOCATION_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_VERTEX_OWNER_BINDING_COUNT),
        1
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.message.contains("1 edge-stride allocation tuple(s)")
            && loss.message.contains("1 oriented-use pair(s)")
            && loss.message.contains("1 vertex-incidence record(s)")
            && loss.message.contains("remain separate")
            && loss.message.contains("bind their adjacent vertex owner")
            && loss.message.contains("loop-to-use")
    }));
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
fn native_namespace_retains_exact_consolidated_torus_charts() {
    let native = crate::native::CatiaNative::decode(&b2_torus_stream());
    let [torus] = native.consolidated_tori.as_slice() else {
        panic!("one consolidated torus")
    };
    assert_eq!(torus.center, [1.0, 2.0, 3.0]);
    assert_eq!(torus.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(torus.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(torus.axis, [0.0, 0.0, 1.0]);
    assert_eq!(torus.major_radius, 7.0);
    assert_eq!(torus.minor_radius, 2.0);
    assert_eq!(
        torus.major_angular_range,
        [
            std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_angular_domain, [0.0, std::f64::consts::TAU]);
    assert_eq!(torus.minor_angular_range, [0.0, std::f64::consts::PI]);
    assert_eq!(
        torus.minor_angular_domain,
        [
            -std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_scale, 14.0);
    assert_eq!(torus.minor_scale, 4.0);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA torus");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA torus"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_tori[0].major_angular_domain[0] += 0.25;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA torus for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_sphere_charts() {
    let native = crate::native::CatiaNative::decode(&b2_sphere_stream());
    let [sphere] = native.consolidated_spheres.as_slice() else {
        panic!("one consolidated sphere")
    };
    assert_eq!(sphere.center, [1.0, 2.0, 3.0]);
    assert_eq!(sphere.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(sphere.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(sphere.axis, [0.0, 0.0, 1.0]);
    assert_eq!(sphere.radius, 5.0);
    assert_eq!(sphere.azimuth_range, [-2.0, 4.0]);
    assert_eq!(sphere.latitude_range, [-1.0, std::f64::consts::FRAC_PI_2]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA sphere");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA sphere"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_spheres[0].latitude_range.reverse();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA sphere for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_owner_packet_and_allocation_link() {
    let native = crate::native::CatiaNative::decode(&b2_linked_owner_stream());
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let crate::native::CatiaOwnerPacketPayload::FixedNine {
        references,
        numeric_tail,
        ..
    } = &packet.payload
    else {
        panic!("fixed-nine owner payload")
    };
    assert_eq!(*references, [1000, 1, 1001, 2, 1002, 3, 1003, 4, 1004]);
    assert_eq!(numeric_tail.header, [0x84, 0x41, 0xbb, 0x05, 0x0d]);
    assert_eq!(numeric_tail.lower, [-0.0, 4.5]);
    assert_eq!(numeric_tail.upper, [12.25, 7.0]);
    assert_eq!(numeric_tail.bounds, [[-2.0, 1.0], [3.5, 4.0], [5.25, 6.0]]);
    let link = packet.allocation_link.expect("allocation-successor link");
    assert_eq!(link.byte_len, 11);
    assert_eq!(link.target, 1003);
    assert_eq!(link.target + 1, references[8]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA owner packet");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA owner packet"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_owner_packets[0]
        .allocation_link
        .as_mut()
        .expect("allocation-successor link")
        .target -= 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid CATIA owner packet");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_count_framed_owner_packet_and_allocation_link() {
    let native = crate::native::CatiaNative::decode(&b2_linked_counted_owner_stream());
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let crate::native::CatiaOwnerPacketPayload::Counted { references, tail } = &packet.payload
    else {
        panic!("count-framed owner payload")
    };
    assert_eq!(references, &[911, 7, 263, 258, 281, 276, 917]);
    assert_eq!(tail, &[0x83, 0x41, 0x92, 0x00, 0x01]);
    let link = packet.allocation_link.expect("allocation-successor link");
    assert_eq!(link.target, 916);
    assert_eq!(
        link.target + 1,
        *references.last().expect("final owner reference")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store count-framed CATIA owner packet");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load count-framed CATIA owner packet"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaOwnerPacketPayload::Counted { tail, .. } =
        &mut invalid.consolidated_owner_packets[0].payload
    else {
        panic!("count-framed owner payload")
    };
    tail.clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid count-framed CATIA owner packet");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_historical_edge_runs() {
    let bytes = a5_native_edge_run_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_pcurves.len(), 2);
    assert_eq!(native.consolidated_edge_runs.len(), 1);
    let run = &native.consolidated_edge_runs[0];
    assert_eq!(
        run.pcurves,
        ["catia:consolidated:pcurve#0", "catia:consolidated:pcurve#1"]
    );
    assert_eq!(run.node, "catia:consolidated:edge-node#0");
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    assert_eq!(node.vertex_refs, [139, 142]);
    assert_eq!(
        node.vertices,
        [
            "catia:consolidated:vertex-identity#0",
            "catia:consolidated:vertex-identity#1"
        ]
    );
    assert_eq!(node.parameter_selectors, [2, 1]);
    let uses = node.uses.as_ref().expect("edge-owned oriented uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
    assert_eq!(uses.senses, [0x88, 0x84]);
    let definition = node.definition.as_ref().expect("edge-owned definition");
    assert_eq!(definition.class, 0x23);
    assert!(definition.byte_offset < node.byte_offset);
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(native.consolidated_vertex_identities[0].identity, 139);
    assert_eq!(
        native.consolidated_vertex_identities[0].incident_edge_nodes,
        ["catia:consolidated:edge-node#0"]
    );

    let mut file = standard_catpart();
    file.splice(16..16, bytes.clone());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated edge-run coverage");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_SUPPORT_BINDING_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_CONSOLIDATED_EDGE_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::PARTIALLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::FULLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_SHARED_LOCUS_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_ENDPOINT_LOCUS_COUNT),
        0
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA edge run"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_edge_runs[0].pcurves[1] = "missing".to_string();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA edge run for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_edge_nodes[0]
        .definition
        .as_mut()
        .expect("edge definition")
        .class = 0x26;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA edge definition");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_edge_nodes[0].uses = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store orphaned CATIA edge definition");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_vertex_identities[0]
        .incident_edge_nodes
        .clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA vertex incidence for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_merges_shared_consolidated_vertex_identity() {
    let mut bytes = a5_native_edge_run_stream(6, 139, 142);
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 142, 151));
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_edge_runs.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 3);
    let shared = native
        .consolidated_vertex_identities
        .iter()
        .find(|vertex| vertex.identity == 142)
        .expect("shared consolidated vertex identity");
    assert_eq!(
        shared.incident_edge_nodes,
        [
            "catia:consolidated:edge-node#0",
            "catia:consolidated:edge-node#1"
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertices[1],
        native.consolidated_edge_nodes[1].vertices[0]
    );
}

#[test]
fn native_namespace_retains_standalone_consolidated_edge_nodes() {
    let bytes = b2_edge_node_stream();
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one standalone consolidated edge node");
    };
    assert_eq!(node.width, 1);
    assert_eq!(node.flag, 0x03);
    assert_eq!(node.header_token, 5);
    assert_eq!(node.vertex_refs, [889, 895]);
    assert!(node.uses.is_none());
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(
        native.consolidated_vertex_identities[0].incident_edge_nodes,
        ["catia:consolidated:edge-node#0"]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store standalone consolidated edge node");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load standalone consolidated edge node"),
        native
    );
}

#[test]
fn consolidated_edge_nodes_require_canonical_headers_and_terminal_controls() {
    let bytes = b2_edge_node_stream();
    assert_eq!(crate::families::b2::records::b2_edge_nodes(&bytes).len(), 1);

    let mut noncanonical_header = bytes.clone();
    noncanonical_header[0] = 0xb3;
    noncanonical_header[4] = 0x04;
    noncanonical_header.insert(5, 1);
    assert!(crate::families::b2::records::b2_edge_nodes(&noncanonical_header).is_empty());

    let mut wide_header = bytes.clone();
    wide_header[0] = 0xb3;
    wide_header[4] = 0x04;
    wide_header.insert(5, 0x40);
    let wide_nodes = crate::families::b2::records::b2_edge_nodes(&wide_header);
    let [wide_node] = wide_nodes.as_slice() else {
        panic!("canonical wide-header edge node")
    };
    assert_eq!(wide_node.header_token, 0x4004);

    let mut invalid_terminal = bytes;
    *invalid_terminal.last_mut().expect("edge terminal") = 0x03;
    assert!(crate::families::b2::records::b2_edge_nodes(&invalid_terminal).is_empty());
}

#[test]
fn native_namespace_attaches_oriented_uses_without_pcurves() {
    let bytes = a5_native_edge_identity_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    let uses = node.uses.as_ref().expect("standalone edge-owned uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
    assert_eq!(uses.senses, [0x88, 0x84]);
}

#[test]
fn native_namespace_retains_resolved_consolidated_edge_supports_and_loci() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Cylinder { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));
    assert_eq!(
        run.endpoint_loci,
        run.shared_loci
            .as_ref()
            .map(|loci| [loci[0], loci[loci.len() - 1]])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store resolved CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load resolved CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            "consolidated_cylinders",
            &Vec::<crate::native::CatiaConsolidatedCylinder>::new(),
        )
        .expect("remove retained cylinders");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_plane_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let plane_stream = b2_plane_carrier_stream();
    let plane_carriers = crate::families::b2::records::b2_plane_carriers(&plane_stream);
    let plane_end = plane_carriers[0].end;
    let mut bytes = plane_stream[..plane_end].to_vec();
    for point in [[10.0f32, 20.0, 0.0], [11.0, 20.0, 1.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));
    bytes.extend_from_slice(&plane_stream[plane_carriers[2].pos..plane_carriers[2].end]);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated plane-bound edge run");
    };
    assert!(run
        .support_bindings
        .iter()
        .all(|binding| matches!(binding, Some(CatiaConsolidatedSupportBinding::Plane { .. }))));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store plane-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load plane-bound CATIA edge run"),
        native
    );

    let mut invalid = native.clone();
    let directionless_offset = invalid
        .consolidated_plane_carriers
        .iter()
        .find(|carrier| carrier.selector == 0xec)
        .expect("directionless class-27 carrier")
        .byte_offset;
    invalid.consolidated_edge_runs[0].support_bindings[0] =
        Some(CatiaConsolidatedSupportBinding::Plane {
            byte_offset: directionless_offset,
        });
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid directionless plane binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    namespace
        .set_arena(
            "consolidated_plane_carriers",
            &Vec::<crate::native::CatiaConsolidatedPlaneCarrier>::new(),
        )
        .expect("remove retained plane carriers");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_torus_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let native = crate::native::CatiaNative::decode(&a5_torus_bound_edge_stream());
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated torus edge run");
    };
    assert!(run
        .support_bindings
        .iter()
        .all(|binding| matches!(binding, Some(CatiaConsolidatedSupportBinding::Torus { .. }))));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store torus-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load torus-bound CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            "consolidated_tori",
            &Vec::<crate::native::CatiaConsolidatedTorus>::new(),
        )
        .expect("remove retained tori");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_sphere_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let native = crate::native::CatiaNative::decode(&a5_sphere_bound_edge_stream());
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated sphere edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Sphere { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store sphere-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load sphere-bound CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            "consolidated_spheres",
            &Vec::<crate::native::CatiaConsolidatedSphere>::new(),
        )
        .expect("remove retained spheres");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_embedded_cylinders_with_their_owning_group() {
    let native = crate::native::CatiaNative::decode(&b2_embedded_cylinder_stream());
    assert!(native.consolidated_cylinders.is_empty());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group");
    };
    let [cylinder] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("one embedded consolidated cylinder");
    };
    assert_eq!(group.group_type, 3);
    assert_eq!(cylinder.group, group.id);
    assert_eq!(cylinder.object_id, 0x5678);
    assert_eq!(cylinder.u_range, [0.0, 4.0 * std::f64::consts::PI]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store embedded CATIA cylinder");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded CATIA cylinder"),
        native
    );

    namespace
        .set_arena(
            "consolidated_groups",
            &Vec::<crate::native::CatiaConsolidatedGroup>::new(),
        )
        .expect("remove owning consolidated group");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut two_groups = b2_embedded_cylinder_stream();
    two_groups.extend_from_slice(&b2_embedded_cylinder_stream());
    let mut invalid = crate::native::CatiaNative::decode(&two_groups);
    assert_eq!(invalid.consolidated_groups.len(), 2);
    assert_eq!(invalid.consolidated_embedded_cylinders.len(), 2);
    invalid.consolidated_embedded_cylinders[1]
        .group
        .clone_from(&invalid.consolidated_groups[0].id);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store cross-group embedded cylinder");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_binds_edges_to_retained_embedded_cylinders() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder { .. })
    )));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store embedded-cylinder edge binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded-cylinder edge binding"),
        native
    );
}

#[test]
fn native_namespace_binds_embedded_cylinder_by_unique_pcurve_support_identity() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream_with_object_id(0x5678);
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x9abc));
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [first, second] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("two embedded consolidated cylinders");
    };
    assert_ne!(first.object_id, second.object_id);
    let [first_group, _second_group] = native.consolidated_groups.as_slice() else {
        panic!("two consolidated groups");
    };
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    let expected = Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder {
        byte_offset: first.byte_offset,
        wrapper_byte_offset: first_group.byte_offset,
    });
    assert_eq!(run.support_bindings, [expected.clone(), expected]);
}

#[test]
fn native_namespace_withholds_duplicate_embedded_pcurve_support_identity() {
    let mut bytes = b2_embedded_cylinder_stream_with_object_id(0x5678);
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x5678));
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert_eq!(run.support_bindings, [None, None]);
}

#[test]
fn consolidated_support_identity_mismatch_does_not_fall_back_to_geometry() {
    let mut bytes = a5_cone_bound_edge_stream();
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x1234));

    let resolved = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    let [edge] = resolved.as_slice() else {
        panic!("one consolidated edge block");
    };
    assert_eq!(edge.supports, [None, None]);
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
        .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
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
        .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
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
            .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
            .expect("resolved consolidated construction");
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
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
                .find(|surface| &surface.surface == surface_id)
                .expect("offset NURBS construction");
            let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
                support, distance, ..
            } = &construction.definition
            else {
                panic!("resolved normal offset is retained as an offset construction");
            };
            assert!((*distance - offset).abs() < 1e-12);
            assert!(decoded.ir().model.surfaces.iter().any(|surface| {
                surface.id == *support && matches!(surface.geometry, SurfaceGeometry::Nurbs(_))
            }));
        }
    }
}

#[test]
fn offset_support_binds_by_native_domain_knot_limits() {
    let mut carriers = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    let mut decoy = carriers[0].clone();
    let SurfaceGeometry::Nurbs(surface) = &mut decoy.geometry else {
        panic!("NURBS fixture");
    };
    for knot in &mut surface.v_knots {
        *knot += 10.0;
    }
    carriers.push(decoy);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let offset = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 7,
        distance: 2.0,
        domain: [
            surface.u_knots[0],
            surface.v_knots[0],
            *surface.u_knots.last().unwrap(),
            *surface.v_knots.last().unwrap(),
        ],
    };

    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[offset], &carriers),
        [Some(0)]
    );
}

#[test]
fn decode_standard_transfers_exact_offset_construction() {
    let surface_bytes = a5_surface_stream();
    let carriers = crate::families::a5a8::records::a5_surfaces(&surface_bytes);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let domain = [
        surface.u_knots[0],
        surface.v_knots[0],
        *surface.u_knots.last().unwrap(),
        *surface.v_knots.last().unwrap(),
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
        extension_flags,
        ..
    } = &procedural.definition
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
    assert!(extension_flags.is_empty());
    let Some(bounds) = procedural.record_bounds else {
        panic!("offset parameter bounds");
    };
    for (actual, expected) in bounds.into_iter().zip(domain) {
        let Some(actual) = actual else {
            panic!("offset parameter bound");
        };
        assert!((actual - expected).abs() < 1e-12);
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
        surface.u_knots[0],
        surface.v_knots[0],
        *surface.u_knots.last().unwrap(),
        *surface.v_knots.last().unwrap(),
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
        &procedural.definition
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
        assert!((actual - expected).abs() < 1e-12);
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
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = &procedural.definition
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
fn consolidated_edge_use_run_is_independent_of_pcurve_availability() {
    use crate::families::b2::records::B2UseSense;

    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(
        &a5_native_edge_identity_stream(6, 139, 142),
    );
    let [run] = runs.as_slice() else {
        panic!("one standalone edge-use run");
    };
    assert!(run.identity_chain_consistent);
    assert_eq!(run.uses[0].sense, Some(B2UseSense::Sense88));
    assert_eq!(run.uses[1].sense, Some(B2UseSense::Sense84));
    assert_eq!(run.node.start_vertex_ref, 139);
    assert_eq!(run.node.end_vertex_ref, 142);
}

#[test]
fn consolidated_edge_use_run_owns_adjacent_compact_definition() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let mut bytes = vec![0xb2, 0x03, 0x24, 0x04, 0x05, 0x81, 0x05, 0x0f, 0x87];
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));

    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one edge-use run");
    };
    let definition = run.definition.as_ref().expect("adjacent definition");
    assert_eq!(definition.class, 0x24);
    assert_eq!(definition.header_token, 5);
    assert_eq!(definition.payload, [0x81, 0x05, 0x0f, 0x87]);
    assert_eq!(
        definition.data,
        Some(ConsolidatedEdgeDefinitionData::Compact24 { operand: 1 })
    );

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .expect("native definition")
            .class,
        0x24
    );
    assert!(matches!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .and_then(|definition| definition.data.as_ref()),
        Some(
            crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 {
                operand: 1
            }
        )
    ));
}

#[test]
fn consolidated_edge_definition_decodes_class25_scalar_layouts() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let operands = [0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
    let mut plain = operands.to_vec();
    for value in [1.0_f64, 2.0, 1e-6, 3.0, 4.0, 1.0, 5.0, 1e-6] {
        plain.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &plain),
        Some(ConsolidatedEdgeDefinitionData::Scalar25 {
            operands: [1, 0xe7, 3463],
            persistent_lead: Some(0x0a),
            values: vec![1.0, 2.0, 1e-6, 3.0, 4.0, 1.0, 5.0, 1e-6],
        })
    );

    let mut segmented = operands.to_vec();
    for value in [1.0_f64, 2.0, 1e-6, 3.0, 4.0] {
        segmented.extend_from_slice(&value.to_le_bytes());
    }
    segmented.push(0x82);
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 1e-6] {
        segmented.extend_from_slice(&value.to_le_bytes());
    }
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &segmented),
        Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
            operands: [1, 0xe7, 3463],
            persistent_lead: Some(0x0a),
            marker: 0x82,
            ref trailing,
            ..
        }) if trailing.len() == 6
    ));
    segmented[46] = 0x84;
    assert!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &segmented)
            .is_none()
    );

    let mut odd_lead = plain.clone();
    odd_lead[3] = 0x0b;
    odd_lead.drain(odd_lead.len() - 8..);
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &odd_lead),
        Some(ConsolidatedEdgeDefinitionData::Scalar25 {
            persistent_lead: Some(0x0b),
            ref values,
            ..
        }) if values.len() == 7
    ));

    let mut long_segment = operands.to_vec();
    for value in [1.0_f64, 2.0, 1e-6, 3.0, 4.0] {
        long_segment.extend_from_slice(&value.to_le_bytes());
    }
    long_segment.push(0x89);
    for value in 0..20 {
        long_segment.extend_from_slice(&f64::from(value).to_le_bytes());
    }
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &long_segment),
        Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
            marker: 0x89,
            ref trailing,
            ..
        }) if trailing.len() == 20
    ));

    let mut bytes = vec![0xb2, 0x03, 0x25, plain.len() as u8, 0x05];
    bytes.extend_from_slice(&plain);
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(matches!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .and_then(|definition| definition.data.as_ref()),
        Some(
            crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Scalar25 {
                operands: [1, 0xe7, 3463],
                persistent_lead: Some(0x0a),
                ..
            }
        )
    ));

    let mut descriptor_payload = vec![0x08, 0x34, 0x12, 0x02];
    descriptor_payload.extend_from_slice(&3.0_f64.to_le_bytes());
    descriptor_payload.extend_from_slice(&7.0_f64.to_le_bytes());
    let mut described = vec![0xb2, 0x03, 0x18, descriptor_payload.len() as u8, 0x05];
    described.extend_from_slice(&descriptor_payload);
    described.extend_from_slice(&bytes);
    let runs = crate::families::consolidated::records::consolidated_class25_edge_runs(&described);
    let [run] = runs.as_slice() else {
        panic!("one described class-25 edge run");
    };
    assert_eq!(run.descriptor.record_id, 0x1234);
    assert_eq!(run.descriptor.values, [3.0, 7.0]);
    assert!(run.identity_chain_consistent);
    let native = crate::native::CatiaNative::decode(&described);
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .class25_descriptor
            .as_ref()
            .expect("native class-25 descriptor")
            .control,
        0x02
    );
}

#[test]
fn consolidated_analytic_circle_run_binds_adjacent_carrier() {
    fn record(class: u8, token: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xb2, 0x03, class, payload.len() as u8, token];
        bytes.extend_from_slice(payload);
        bytes
    }

    let mut parameter = vec![0x05, 0x00];
    parameter.extend_from_slice(&12.0_f64.to_le_bytes());
    parameter.extend_from_slice(&34.0_f64.to_le_bytes());
    let mut circle = vec![0x05];
    for value in [12.0_f64, 34.0, 5.0, 0.0, 10.0] {
        circle.extend_from_slice(&value.to_le_bytes());
    }
    circle.push(0x01);
    circle.extend_from_slice(&0.0_f64.to_le_bytes());
    let mut definition = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 10.0, 1e-6, 4.0, 9.0, 1.0, -2.0, 1e-6] {
        definition.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = record(0x18, 0x15, &parameter);
    bytes.extend_from_slice(&record(0x19, 0x05, &circle));
    bytes.extend_from_slice(&record(0x23, 0x05, &definition));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));

    let runs =
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one analytic-circle edge run");
    };
    assert_eq!(run.circle.center_pair, [12.0, 34.0]);
    assert_eq!(run.circle.radius, 5.0);
    assert_eq!(run.descriptor.header_token, 0x15);
    assert_eq!(run.definition.pos, parameter.len() + circle.len() + 10);
    assert!(run.identity_chain_consistent);

    let native = crate::native::CatiaNative::decode(&bytes);
    let binding = native.consolidated_edge_nodes[0]
        .analytic_circle
        .as_ref()
        .expect("native analytic circle");
    assert_eq!(binding.circle, "catia:consolidated:circle#0");
    assert_eq!(native.consolidated_circles[0].center_pair, [12.0, 34.0]);
    assert_eq!(native.consolidated_circles[0].range, [0.0, 10.0]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store analytic circle binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load analytic circle binding"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_edge_nodes[0]
        .analytic_circle
        .as_mut()
        .expect("analytic circle binding")
        .circle = "missing".to_string();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid analytic circle binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let circle_end = parameter.len() + circle.len() + 10;
    let mut broken = bytes[..circle_end].to_vec();
    broken.extend_from_slice(&record(0x05, 0x05, &[0x00]));
    broken.extend_from_slice(&bytes[circle_end..]);
    assert!(
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(&broken)
            .is_empty()
    );
}

#[test]
fn a5_topology_edge_run_preserves_uses_and_native_endpoint_identities() {
    use crate::families::b2::records::B2UseSense;

    let runs = crate::families::consolidated::records::consolidated_topology_edge_runs(
        &a5_topology_edge_run_stream(),
    );
    assert_eq!(runs.len(), 1);
    assert!(runs[0].edge.co_parametric);
    assert_eq!(runs[0].uses[0].sense, Some(B2UseSense::Sense84));
    assert_eq!(runs[0].uses[1].sense, Some(B2UseSense::Sense88));
    assert_eq!(runs[0].uses[0].references.as_deref(), Some(&[1, 2][..]));
    assert_eq!(runs[0].uses[1].references.as_deref(), Some(&[2, 3][..]));
    assert!(!runs[0].identity_chain_consistent);
    assert_eq!(runs[0].node.start_vertex_ref, 889);
    assert_eq!(runs[0].node.end_vertex_ref, 895);
}

#[test]
fn native_round_trips_legacy_entity_identity_runs() {
    let mut bytes = Vec::new();
    for entity_id in [1_u32, 4, 9, 12, 13] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        if entity_id == 4 {
            for (role, selector, value) in [
                ("body", vec![0x80, 4, 0, 0, 0], "#1_ + 2"),
                ("param", vec![0xd1, 8], "(#1_ : #In Real) : Real\n"),
            ] {
                bytes.push(u8::try_from(role.len() + 1).expect("short role"));
                bytes.extend(role.as_bytes());
                bytes.extend(selector);
                bytes.extend(b"\xe8\x00\x12\x01");
                bytes.push(u8::try_from(value.len() + 1).expect("short text"));
                bytes.extend(value.as_bytes());
                bytes.push(0xfe);
            }
        } else if entity_id == 9 {
            bytes.extend([8, b'p', b'a', b'r', b'a', b'm', b'i', b'n', 0x80]);
            bytes.extend(4134_u32.to_le_bytes());
            bytes.extend([0xe8, 0xe4, 0x0b, 0x01]);
            bytes.extend(b"\xfe\x84\x92\x82\x08Boolean\x83");
            bytes.extend(b"\xfe\x84\x92\x82\x96\x83");
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 9]);
            bytes.extend(b"\xe8\x00\x12\x01\x07Result\xfe");
            bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
            bytes.extend(3.5_f64.to_bits().to_le_bytes());
        } else if entity_id == 12 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 11]);
            bytes.extend(b"\xe8\x00\x12\x01\x0cResponsible\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x07String\x83");
            bytes.extend(b"\xfe\x85\x93\x82\xfe\x0cCilas Evans");
        } else if entity_id == 13 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 12]);
            bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x08Integer\x83");
            bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
        }
    }
    let catalog_offset = bytes.len();
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    bytes.extend(b"\xfe\xfe\xfe");
    let schema_program_offset = bytes.len();
    bytes.extend([0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    let schema_footer_offset = bytes.len();
    bytes.extend(b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA");

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.legacy_entity_runs.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0]
            .identities
            .iter()
            .map(|identity| identity.entity_id)
            .collect::<Vec<_>>(),
        [1, 4, 9, 12, 13]
    );
    assert!(native.legacy_entity_runs[0]
        .identities
        .iter()
        .all(|identity| identity.lead == 0x81));
    assert_eq!(
        native.legacy_entity_runs[0].catalog_offset,
        catalog_offset as u64
    );
    let schema_program = native.legacy_entity_runs[0]
        .schema_program
        .as_ref()
        .expect("complete compact schema program");
    assert_eq!(schema_program.byte_offset, schema_program_offset as u64);
    assert_eq!(
        schema_program.boundary_byte_offset,
        schema_footer_offset as u64
    );
    assert_eq!(
        schema_program.boundary,
        crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
    );
    assert_eq!(
        schema_program.data,
        [0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]
    );
    assert_eq!(schema_program.identifiers.len(), 1);
    assert_eq!(
        schema_program.identifiers[0].byte_offset,
        schema_program_offset as u64 + 1
    );
    assert_eq!(schema_program.identifiers[0].value, "Foo");
    assert_eq!(native.legacy_entity_runs[0].text_fields.len(), 5);
    assert_eq!(
        native.legacy_entity_runs[0]
            .role_selectors
            .iter()
            .map(|role| {
                (
                    role.entity_id,
                    role.name.literal().expect("literal role"),
                    role.encoding,
                    role.selector,
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                4,
                "body",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32),
                4,
            ),
            (
                4,
                "param",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                9,
            ),
            (
                9,
                "paramin",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32),
                4134,
            ),
            (
                9,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                10,
            ),
            (
                12,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                12,
            ),
            (
                13,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                13,
            ),
        ]
    );
    assert_eq!(native.legacy_entity_runs[0].text_fields[0].entity_id, 4);
    assert_eq!(native.legacy_entity_runs[0].text_fields[0].value, "#1_ + 2");
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[0]
            .role
            .as_ref()
            .map(|role| { (role.name.literal().expect("literal role"), role.selector,) }),
        Some(("body", 4))
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[1]
            .role
            .as_ref()
            .map(|role| { (role.name.literal().expect("literal role"), role.selector,) }),
        Some(("param", 9))
    );
    assert_eq!(native.legacy_entity_runs[0].relations.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].parameter_entity_id,
        Some(9)
    );
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].inputs[0].parameter,
        "#1_"
    );
    assert_eq!(native.legacy_entity_runs[0].type_descriptors.len(), 4);
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[0].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "Boolean".to_string()
        }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[1].value,
        crate::native::CatiaLegacyTypeValue::Selector { value: 22 }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[2].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "String".to_string()
        }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[3].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "Integer".to_string()
        }
    );
    assert_eq!(native.legacy_entity_runs[0].scalar_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].scalar_values[0]
            .name
            .as_deref(),
        Some("Result")
    );
    assert_eq!(
        native.legacy_entity_runs[0].scalar_values[0].encoding,
        crate::native::CatiaLegacyScalarEncoding::Named84
    );
    assert!(native.legacy_entity_runs[0].scalar_values[0]
        .id
        .starts_with("catia:legacy:scalar#00000000-"));
    assert!(matches!(
        native.legacy_entity_runs[0].scalar_values[0].evaluation,
        crate::native::CatiaLegacyScalarEvaluation::Value { bits }
            if bits == 3.5_f64.to_bits()
    ));
    assert_eq!(native.legacy_entity_runs[0].string_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].string_values[0]
            .name
            .as_deref(),
        Some("Responsible")
    );
    assert_eq!(
        native.legacy_entity_runs[0].string_values[0].value,
        "Cilas Evans"
    );
    assert_eq!(native.legacy_entity_runs[0].integer_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].integer_values[0]
            .name
            .as_deref(),
        Some("Count")
    );
    assert_eq!(native.legacy_entity_runs[0].integer_values[0].value, 11);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store legacy entity run");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load legacy entity run");
    assert_eq!(loaded.legacy_entity_runs, native.legacy_entity_runs);

    let mut previous_schema_namespace = namespace.clone();
    let mut previous_schema_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_schema_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous schema-program runs");
    previous_schema_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .identifiers
        .clear();
    previous_schema_namespace
        .set_arena("legacy_entity_runs", &previous_schema_runs)
        .expect("store previous schema-program runs");
    previous_schema_namespace.version = 221;
    let migrated_schema = crate::native::CatiaNative::load(&previous_schema_namespace)
        .expect("migrate schema identifiers");
    assert_eq!(
        migrated_schema.legacy_entity_runs[0]
            .schema_program
            .as_ref()
            .expect("migrated schema program")
            .identifiers,
        schema_program.identifiers
    );

    let mut previous_boundary_namespace = namespace.clone();
    let mut previous_boundary_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_boundary_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous schema-program boundary");
    previous_boundary_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .boundary = crate::native::CatiaLegacySchemaProgramBoundary::StreamDirectory;
    previous_boundary_namespace
        .set_arena("legacy_entity_runs", &previous_boundary_runs)
        .expect("store previous schema-program boundary");
    previous_boundary_namespace.version = 222;
    let migrated_boundary = crate::native::CatiaNative::load(&previous_boundary_namespace)
        .expect("migrate schema-program boundary");
    assert_eq!(
        migrated_boundary.legacy_entity_runs[0]
            .schema_program
            .as_ref()
            .expect("migrated schema program")
            .boundary,
        crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
    );

    let mut invalid_schema_program = native.clone();
    invalid_schema_program.legacy_entity_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .data
        .pop();
    let mut invalid_schema_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_schema_program
        .store(&mut invalid_schema_namespace)
        .expect("store invalid schema program");
    assert!(crate::native::CatiaNative::load(&invalid_schema_namespace).is_err());

    let mut invalid_schema_identifier = native.clone();
    invalid_schema_identifier.legacy_entity_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .identifiers[0]
        .value = "Bar".to_string();
    let mut invalid_identifier_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_schema_identifier
        .store(&mut invalid_identifier_namespace)
        .expect("store invalid schema identifier");
    assert!(crate::native::CatiaNative::load(&invalid_identifier_namespace).is_err());

    let mut previous_field_namespace = namespace.clone();
    let mut previous_field_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_field_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous field-binding runs");
    for run in &mut previous_field_runs {
        for role in &mut run.role_selectors {
            role.field_code = None;
        }
        for role in run
            .text_fields
            .iter_mut()
            .filter_map(|field| field.role.as_mut())
        {
            role.field_code = None;
        }
    }
    previous_field_namespace
        .set_arena("legacy_entity_runs", &previous_field_runs)
        .expect("store previous field-binding runs");
    previous_field_namespace.version = 219;
    let migrated_field_bindings = crate::native::CatiaNative::load(&previous_field_namespace)
        .expect("load previous field bindings");
    assert!(migrated_field_bindings.legacy_entity_runs[0]
        .role_selectors
        .iter()
        .all(|role| role.field_code.is_none()));

    let mut previous_identity_namespace = namespace.clone();
    let mut previous_identity_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_identity_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous identity runs");
    for identity in previous_identity_runs
        .iter_mut()
        .flat_map(|run| &mut run.identities)
    {
        identity.lead = 0;
    }
    previous_identity_namespace
        .set_arena("legacy_entity_runs", &previous_identity_runs)
        .expect("store previous identity runs");
    previous_identity_namespace.version = 215;
    let migrated_identity = crate::native::CatiaNative::load(&previous_identity_namespace)
        .expect("migrate legacy identity leads");
    assert!(migrated_identity.legacy_entity_runs[0]
        .identities
        .iter()
        .all(|identity| identity.lead == 0x81));

    let mut previous_namespace = namespace.clone();
    let mut previous_runs: Vec<crate::native::CatiaLegacyEntityRun> = previous_namespace
        .arena_as("legacy_entity_runs")
        .expect("load legacy entity runs");
    previous_runs[0].role_selectors.clear();
    previous_runs[0].schema_fields.clear();
    for field in &mut previous_runs[0].text_fields {
        if let Some(role) = &mut field.role {
            role.entity_id = 0;
        }
    }
    previous_namespace
        .set_arena("legacy_entity_runs", &previous_runs)
        .expect("store previous legacy entity runs");
    previous_namespace.version = 211;
    let migrated =
        crate::native::CatiaNative::load(&previous_namespace).expect("migrate legacy text roles");
    assert_eq!(migrated.legacy_entity_runs[0].role_selectors.len(), 5);
    assert!(migrated.legacy_entity_runs[0]
        .role_selectors
        .iter()
        .all(|role| role.entity_id != 0));

    let mut invalid_type_name = native.clone();
    invalid_type_name.legacy_entity_runs[0].type_descriptors[0].value =
        crate::native::CatiaLegacyTypeValue::Name {
            value: "1Boolean".to_string(),
        };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_type_name
        .store(&mut namespace)
        .expect("store invalid legacy type name");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_lead = native.clone();
    invalid_lead.legacy_entity_runs[0].identities[0].lead = 0xe6;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_lead
        .store(&mut namespace)
        .expect("store invalid legacy identity lead");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_name = native.clone();
    invalid_name.legacy_entity_runs[0].scalar_values[0].name = Some("Other".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_name
        .store(&mut namespace)
        .expect("store invalid legacy scalar name");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_scalar_id = native.clone();
    invalid_scalar_id.legacy_entity_runs[0].scalar_values[0].id =
        "catia:legacy:scalar#00000000-0".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_scalar_id
        .store(&mut namespace)
        .expect("store invalid legacy scalar identity");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_integer = native.clone();
    invalid_integer.legacy_entity_runs[0].integer_values[0].value = -1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_integer
        .store(&mut namespace)
        .expect("store invalid inline legacy integer");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_parameter = native.clone();
    invalid_parameter.legacy_entity_runs[0].relations[0].parameter_entity_id = Some(4);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_parameter
        .store(&mut namespace)
        .expect("store invalid legacy relation parameter");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].identities[1].entity_id = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid legacy entity run");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn legacy_parameters_retain_and_require_the_part_container_binding() {
    let graph = object_graph_stream();
    let legacy_offset = graph.len();
    let mut stream = graph;
    stream.push(0xea);
    stream.extend(1_u32.to_le_bytes());
    stream.push(0x81);
    stream.extend([0xfd, 0x8c]);
    stream.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    stream.extend(b"\xe8\x00\x12\x01");
    stream.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    stream.extend(b"\xfe\x84\x92\x82");
    stream.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    stream.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    stream.extend(12.5_f64.to_bits().to_le_bytes());
    stream.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    let (bytes, stream_offset) = outer_container_catpart(&stream);

    let native = crate::native::CatiaNative::decode(&bytes);
    let run = native
        .legacy_entity_runs
        .iter()
        .find(|run| run.byte_offset == stream_offset + legacy_offset as u64)
        .expect("declared-stream legacy run");
    assert_eq!(
        run.outer_container.as_ref(),
        native.object_graphs[0].outer_container.as_ref()
    );
    let expected_binding = run.outer_container.clone();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store container-bound legacy run");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load container-bound legacy run");
    assert_eq!(
        loaded.legacy_entity_runs[0].outer_container,
        expected_binding
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode container-bound legacy parameter");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT),
        1
    );
    assert_eq!(decoded.ir().model.parameters.len(), 1);
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
        .find(|curve| curve.id.0.starts_with("catia:guide:curve#"))
        .expect("typed guide curve");
    let CurveGeometry::Nurbs(nurbs) = &guide.geometry else {
        panic!("guide curve must be NURBS");
    };
    assert_eq!(nurbs.degree, 5);
    assert_eq!(nurbs.control_points.first().unwrap().x, 0.0);
    assert_eq!(nurbs.control_points.last().unwrap().z, 4.0);
}

#[test]
fn decode_object_stream_transfers_a8_rolling_ball_jet() {
    let file = object_main_catpart(&a8_freeform_curve_stream());
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::FloatPackedInnerNoFbb
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode rolling-ball object stream");
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = &procedural.definition
    else {
        panic!("rolling-ball jet");
    };
    assert_eq!(*degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].first_limit, Point3::new(2.0, 0.0, 0.0));
    assert_eq!(sites[1].angle, std::f64::consts::FRAC_PI_2);
    let provenance = &decoded.source_fidelity().annotations.provenance[&procedural.id.0];
    assert_eq!(
        decoded.source_fidelity().annotations.streams[provenance.stream as usize],
        "catia:object_stream_a8_03_32"
    );
    let tag = provenance
        .tag
        .as_deref()
        .expect("rolling-ball provenance tag");
    assert!(tag.contains("object_id:12345678"));
    assert!(tag.contains("multiplicities:[6, 6]"));
    assert_eq!(
        decoded.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:12345678"))
    );
}

#[test]
fn decode_float_packed_stream_transfers_a8_nurbs() {
    assert_eq!(
        crate::container::scan_bytes(a8_catpart()).variant,
        Variant::FloatPackedInnerNoFbb
    );
    let mut cur = Cursor::new(a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Nurbs(_)
    ));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}

#[test]
fn decode_float_packed_stream_transfers_reference_closed_b5_topology() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(
        &mut stream,
        0x5e,
        900,
        &[
            0x85, 0x81, 0x18, 0x85, 0x03, 0x18, 0x85, 0x03, 0x81, 0x81, 0x2a,
        ],
    );
    append_b5_record(&mut stream, 0x5d, 901, &[0x81, 0x81, 0x04]);
    crate::families::b5::graph::parse(&stream).expect("generated B5 topology");
    let file = object_main_catpart(&stream);
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::FloatPackedInnerNoFbb
    );

    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.curves.len(), 3);
    assert!(result.ir().model.surfaces.iter().all(|surface| {
        surface.source_object.as_ref().is_some_and(|source| {
            source.format == "catia" && source.object_id.starts_with("cgm-surface:")
        })
    }));
    assert!(result.ir().model.curves.iter().all(|curve| {
        curve.source_object.as_ref().is_some_and(|source| {
            source.format == "catia" && source.object_id.starts_with("cgm-edge:")
        })
    }));
    assert_eq!(result.ir().model.procedural_curves.len(), 3);
    assert!(result.ir().model.procedural_curves.iter().all(|curve| {
        matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                ref context,
                ..
            } if context.sides[0].surface.is_some()
                && context.sides[0].pcurve.is_some()
                && context.sides[1].surface.is_none()
        )
    }));
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_UNCOUNTED_FACE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_2A_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_EXTENDED_LOOP_METADATA_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::RESOLVED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT
        ),
        3
    );
    assert!(result
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.parameter_range == Some([0.0, 1.0])));
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_float_packed_stream_transfers_a_complete_native_vertex_chain() {
    let stream = b5_closed_triangle_stream_with_native_vertex_chain();
    let graph = crate::families::b5::graph::parse(&stream).expect("generated B5 topology");
    assert!(graph.complete);
    assert_eq!(graph.vertex_incidence_links.len(), 3);
    assert_eq!(graph.parameter_incidences.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(graph.edge_parameter_incidences.len(), 3);
    assert_eq!(graph.logical_vertex_refs, [600, 601, 602]);
    assert_eq!(
        graph.logical_vertex_points,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode complete native vertex chain");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

/// A `b5 03` object id reaches the neutral model as an unpadded decimal key, so
/// an edge triple such as `9`, `10`, `11` is emitted in ascending native order
/// and sorts the other way. The route must still transfer the topology: a
/// cross-reference is an id string, so arena order carries no reference
/// semantics and the pipeline restores it.
#[test]
fn decode_float_packed_stream_transfers_topology_under_decimal_object_ids() {
    let mut stream = b5_closed_triangle_stream_over_edges([9, 10, 11]);
    append_b5_record(
        &mut stream,
        0x5e,
        900,
        &[
            0x85, 0x81, 0x18, 0x85, 0x03, 0x18, 0x85, 0x03, 0x81, 0x81, 0x2a,
        ],
    );
    append_b5_record(&mut stream, 0x5d, 901, &[0x81, 0x81, 0x04]);
    crate::families::b5::graph::parse(&stream).expect("generated B5 topology");

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode object-stream topology");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .map(|edge| edge.id.0.as_str())
            .collect::<Vec<_>>(),
        ["catia:b5:edge#10", "catia:b5:edge#11", "catia:b5:edge#9"]
    );
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_does_not_transfer_a_loop_with_multiple_face_owners() {
    let mut stream = b5_closed_triangle_stream();
    let mut face_payload = vec![0x82];
    face_payload.extend_from_slice(&b5_object_ref(100));
    face_payload.extend_from_slice(&b5_object_ref(400));
    face_payload.push(0x03);
    append_b5_record(&mut stream, 0x5f, 902, &face_payload);

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode duplicate loop-owner stream");

    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.faces.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code
            == cadmpeg_ir::report::LossKind::shared(
                cadmpeg_ir::LossTaxonomy::TopologyNotTransferred,
            )
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
}

#[test]
fn decode_reports_structurally_typed_unresolved_b5_faces() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(
        &mut stream,
        0x5f,
        902,
        &[0x82, 0x18, 100, 0, 0x18, 0xe7, 0x03, 0x03],
    );
    append_b5_record(&mut stream, 0x5e, 903, &[]);
    let graph = crate::families::b5::graph::parse(&stream).expect("typed unresolved face graph");
    assert_eq!(graph.face_records.len(), 2);
    assert_eq!(graph.faces.len(), 1);
    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed unresolved face");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT),
        1
    );
}

#[test]
fn decode_reports_typed_distinct_surface_b5_faces() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(&mut stream, 0x27, 101, &b5_plane_payload([0.0, 0.0, 1.0]));
    let mut face_payload = vec![0x83];
    face_payload.extend_from_slice(&b5_object_ref(100));
    face_payload.extend_from_slice(&b5_object_ref(101));
    face_payload.extend_from_slice(&b5_object_ref(400));
    face_payload.push(0x05);
    append_b5_record(&mut stream, 0x5f, 902, &face_payload);

    let graph = crate::families::b5::graph::parse(&stream).expect("typed multi-surface graph");
    assert_eq!(graph.face_records.len(), 2);
    assert_eq!(graph.faces.len(), 1);

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed multi-surface face");
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_MULTI_SURFACE_OBJECT_STREAM_FACE_COUNT),
        1
    );
}

#[test]
fn decode_reports_typed_b5_faces_without_a_resolved_topology_graph() {
    let mut stream = b2_sphere_stream();
    append_b5_record(&mut stream, 0x27, 100, &b5_plane_payload([0.0; 3]));
    append_b5_record(
        &mut stream,
        0x21,
        9,
        &b5_linear_pcurve_payload(100, [0.0, 0.0], [1.0, 0.0]),
    );
    append_b5_record(
        &mut stream,
        0x62,
        103,
        &[
            0x83, 0x89, 0x8a, 0xe4, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
            0x01,
        ],
    );
    append_b5_record(
        &mut stream,
        0x5f,
        101,
        &[0x82, 0x18, 100, 0, 0x18, 102, 0, 0x03],
    );
    append_b5_record(&mut stream, 0x5e, 102, &[]);
    append_b5_record(
        &mut stream,
        0x5e,
        104,
        &[0x85, 0x81, 0xe9, 0x83, 0x84, 0x85, 0x21],
    );
    append_b5_record(&mut stream, 0x5d, 105, &[0x81, 0x86, 0x04]);
    let mut incidence_payload = vec![0x81, 0x89, 0x81];
    incidence_payload.extend_from_slice(&le_f64(0.0));
    incidence_payload.push(0x81);
    append_b5_record(&mut stream, 0x06, 4, &incidence_payload);
    append_b5_record(&mut stream, 0x05, 6, &[0x81, 0x84]);
    assert!(crate::families::b5::graph::parse(&stream).is_none());
    assert_eq!(
        crate::families::b5::graph::typed_face_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_loop_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_edge_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_vertex_incidence_links(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_class_21_pcurves(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_parameter_incidences(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_vertex_incidence_rosters(&stream).len(),
        1
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed face without resolved topology");
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_LOOP_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_21_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_MEMBER_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_MEMBER_COUNT
        ),
        1
    );
}

#[test]
fn decode_inner_no_directory_transfers_a8_nurbs() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_a8_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Nurbs(_)
    ));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}

#[test]
fn decode_inner_no_directory_transfers_b2_cylinder() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_b2_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_b2_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Cylinder { radius: 2.0, .. }
    ));
}

#[test]
fn decode_e5_stream_transfers_circle_carrier() {
    let scan = crate::container::scan_bytes(e5_catpart());
    assert_eq!(scan.variant, Variant::E5Stream);
    let mut cur = Cursor::new(e5_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert!(result.ir().model.edges.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle { .. }
    ));
    assert!(result.ir().native_unknowns("catia").unwrap()[0]
        .links
        .contains(&"catia:e5:surf#0".to_string()));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_transfers_reference_closed_torus_topology() {
    let stream = e5_torus_topology_stream();
    crate::families::e5::graph::parse_topology(&stream).expect("generated E5 topology");
    let file = object_main_catpart(&stream);
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::E5Stream
    );

    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(
        result.ir().model.loops[0].boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(result.ir().model.coedges.len(), 4);
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.pcurves.len(), 4);
    assert_eq!(result.ir().model.curves.len(), 4);
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(matches!(
        result.ir().model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
            family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric,
            ..
        }
    ));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some() && edge.param_range.is_some()));
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.category() != cadmpeg_ir::report::LossCategory::Topology
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("two trailing orientation signs")
    }));

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_binds_file_level_vertex_run() {
    let mut stream = e5_torus_topology_stream();
    let vertex_start = stream
        .windows(3)
        .position(|bytes| bytes == [0x05, 0x08, 0x01])
        .expect("E5 vertex run");
    let vertex_bytes = stream
        .drain(vertex_start..vertex_start + 4 * 15)
        .collect::<Vec<_>>();

    stream.extend_from_slice(b"FINJPL  ");
    stream.extend_from_slice(&0x0000_0080u32.to_be_bytes());
    stream.extend_from_slice(&vertex_bytes);
    let file = object_main_catpart(&stream);
    let vertex_file_start = file
        .windows(vertex_bytes.len())
        .position(|bytes| bytes == vertex_bytes)
        .expect("file-level E5 vertex run");

    let record_range = crate::container::e5_record_stream(&file).expect("coherent E5 walk");
    assert!(!record_range.contains(&vertex_file_start));
    assert!(crate::families::e5::records::e5_vertices(&file[record_range], 4).is_empty());
    assert_eq!(crate::families::e5::records::e5_vertices(&file, 4).len(), 4);
    let scan = crate::container::scan_bytes(file.clone());
    assert_eq!(scan.variant, Variant::E5Stream);

    let result = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("E5 decode");
    assert_eq!(result.ir().model.points.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 4);
}
