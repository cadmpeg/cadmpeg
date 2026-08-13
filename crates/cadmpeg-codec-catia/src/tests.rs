// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.CATPart` byte image
//! whose bytes exercise the real container, variant-detection, and geometry
//! decode paths and fail if the code regresses.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use cadmpeg_ir::document::CadIr;

use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};

use cadmpeg_ir::math::{Point3, Vector3};

use cadmpeg_ir::Annotations;

use crate::variant::Variant;
use crate::CatiaCodec;

pub(crate) use crate::test_support::*;

struct NativeFieldsMut<'a> {
    record: &'a mut cadmpeg_ir::NativeRecord,
    fields: Option<serde_json::Map<String, serde_json::Value>>,
}

impl std::ops::Deref for NativeFieldsMut<'_> {
    type Target = serde_json::Map<String, serde_json::Value>;

    fn deref(&self) -> &Self::Target {
        self.fields.as_ref().expect("native fields guard")
    }
}

impl std::ops::DerefMut for NativeFieldsMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.fields.as_mut().expect("native fields guard")
    }
}

impl Drop for NativeFieldsMut<'_> {
    fn drop(&mut self) {
        let id = self.record.id().to_owned();
        let fields = self.fields.take().expect("native fields guard");
        *self.record = cadmpeg_ir::NativeRecord::new(id, fields);
    }
}

trait NativeRecordTestExt {
    fn fields_mut(&mut self) -> NativeFieldsMut<'_>;
}

impl NativeRecordTestExt for cadmpeg_ir::NativeRecord {
    fn fields_mut(&mut self) -> NativeFieldsMut<'_> {
        let fields = self.fields();
        NativeFieldsMut {
            record: self,
            fields: Some(fields),
        }
    }
}

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
fn decode_persists_external_references_in_native_namespace() {
    let mut file = standard_catpart();
    file.extend_from_slice(&external_reference_segment("Support.CATPart"));
    let file_len = u32::try_from(file.len()).expect("external-reference fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode external-reference fixture");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load CATIA native namespace");
    let [reference] = native.external_references.as_slice() else {
        panic!("one external reference");
    };
    assert_eq!(reference.target, "Support.CATPart");
    assert!(native
        .finjpl_segments
        .iter()
        .any(|segment| segment.id == reference.segment));
}

#[test]
fn native_namespace_retains_summary_preview_bytes() {
    let bytes = summary_preview_segment();
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.preview_images.len(), 1);
    let preview = &native.preview_images[0];
    assert_eq!(
        (preview.width, preview.height, preview.components),
        (640, 288, 1)
    );
    assert_eq!(preview.data.len() as u64, preview.byte_len);
    assert_eq!(&preview.data[..2], [0xff, 0xd8]);
    assert_eq!(&preview.data[preview.data.len() - 2..], [0xff, 0xd9]);
    assert_eq!(native.finjpl_segments.len(), 1);
    assert_eq!(
        native.finjpl_segments[0].name.as_deref(),
        Some("CATSummaryInformation")
    );
    assert_eq!(native.finjpl_segments[0].family, "project-flags");
    assert_eq!(native.finjpl_segments[0].data, bytes);
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
fn outer_object_graph_parser_reads_nested_heads_and_payload_fields() {
    use crate::object_graph::{PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_stream()).unwrap();
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::Mixed);
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Reference { value: 5, .. },
            PayloadField::Scalar {
                tag: 0x3a,
                value: 7,
                ..
            },
            PayloadField::Terminator
        ]
    ));
    assert_eq!(graph.records[1].subtype, PayloadSubtype::Blob);
}

#[test]
fn outer_object_graph_uses_the_unique_length_closing_child_frame() {
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x7c, 0x0a, 0xff, 0xff, 0xff, 0xff, 0x82, 0x83],
            &[0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x84], &[0xfe]),
    ];
    let graph = crate::object_graph::parse(&object_graph_from_records(&records))
        .expect("length-closing object payload");
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(
        &graph.records[0].head[graph.records[0].head.len() - 2..],
        [
            crate::object_graph::HeadToken::Reference(2),
            crate::object_graph::HeadToken::Reference(3),
        ]
    );
}

#[test]
fn outer_object_graph_rejects_ambiguous_length_closing_child_frames() {
    let mut first = object_graph_record(&[0x04, 0x01, 0x82, 0x83], &[0xfe]);
    let fake = 8;
    first.splice(fake..fake, [0x7c, 0x0a, 0, 0, 0, 0]);
    let closing_len = u32::try_from(first.len() - fake).expect("fixture child length");
    first[fake + 2..fake + 6].copy_from_slice(&closing_len.to_le_bytes());
    let record_len = u32::try_from(first.len()).expect("fixture record length");
    first[2..6].copy_from_slice(&record_len.to_le_bytes());

    let second = object_graph_record(&[0x04, 0x01, 0x82, 0x84], &[0xfe]);
    assert!(crate::object_graph::parse(&object_graph_from_records(&[first, second])).is_none());
}

#[test]
fn outer_object_graph_requires_records_to_cover_the_root_extent() {
    let mut bytes = object_graph_stream();
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    let declared_len = u32::try_from(bytes.len()).expect("fixture graph length");
    bytes[2..6].copy_from_slice(&declared_len.to_le_bytes());

    assert!(crate::object_graph::parse(&bytes).is_none());
}

#[test]
fn outer_object_graph_requires_a_final_payload_terminator() {
    for payload in [&[0xfe, 0xaa][..], &[0xe5, 1, 0, 0, 0, 0xfe][..]] {
        let bytes =
            object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], payload)]);
        assert!(crate::object_graph::parse(&bytes).is_none());
    }
}

#[test]
fn object_graph_payload_assigns_blobs_only_inside_the_terminator_boundary() {
    use crate::object_graph::PayloadField;

    let valid = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[0xe5, 1, 0, 0, 0, 0xaa, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&valid).expect("bounded blob");
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Blob {
                declared_len: 1,
                bytes,
                ..
            },
            PayloadField::Terminator
        ] if bytes.as_slice() == [0xaa]
    ));

    let unbounded = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[0xe5, 0xfd, 0xd8, 0xc1, 0x74, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&unbounded).expect("literal E5 atom");
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Atom {
                value: 0xe5,
                offset: 0
            },
            ..,
            PayloadField::Terminator
        ]
    ));
}

#[test]
fn object_graph_payload_preserves_the_complete_terminator_run() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04], &[0x83, 0xfe, 0xfe, 0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("multi-terminator payload");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Atom { value: 3, .. },
            crate::object_graph::PayloadField::Terminator,
            crate::object_graph::PayloadField::Terminator,
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_payload_reads_tagged_fixed_width_references() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[
            0x81, 0x80, 0xfe, 0x1e, 0, 0, 0x81, 0x32, 0xeb, 0, 0, 0, 0xfe,
        ],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("tagged fixed-width references");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Reference { value: 7934, .. },
            crate::object_graph::PayloadField::Reference { value: 235, .. },
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_lists_retain_direct_fixed_width_references() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[0x3b, 0x81, 0x32, 2, 0, 0, 0, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(matches!(
        native.object_graphs[0].records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 1,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
    assert_eq!(
        native.object_graphs[0].records[0].references[0].entity_id,
        2
    );
}

#[test]
fn outer_object_graph_requires_a_stored_head_lead() {
    let bytes = object_graph_from_records(&[object_graph_record(&[], &[0xfe])]);
    assert!(crate::object_graph::parse(&bytes).is_none());
}

#[test]
fn outer_object_graph_accepts_one_length_closed_record() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("one-record object graph");

    assert_eq!(graph.records.len(), 1);
    assert_eq!(graph.records[0].owner_ref, Some(1));
    assert_eq!(graph.records[0].class_ref, Some(1));
    assert_eq!(
        graph.records[0].subtype,
        crate::object_graph::PayloadSubtype::Empty
    );
}

#[test]
fn outer_object_graph_preserves_inline_records() {
    let nested = object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]);
    let inline = inline_object_graph_record(&[
        0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
    ]);
    let graph = crate::object_graph::parse(&object_graph_from_records(&[nested, inline]))
        .expect("inline control record");

    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[1].lead, 0x10);
    assert!(graph.records[1].head.is_empty());
    assert_eq!(
        graph.records[1].inline_body.as_deref(),
        Some(&[0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,][..])
    );
    assert!(graph.records[1].payload.fields.is_empty());
}

#[test]
fn outer_object_graph_accepts_each_inline_layout() {
    let bodies = [
        vec![
            0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x82, 0xd3, 0x79, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd4, 0x33, 0x82, 0x32, 0xe6, 0x00, 0x00, 0x00, 0x32, 0xe4, 0x00, 0x00,
            0x00, 0x82, 0xb1, 0x81, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd4, 0x32, 0x82, 0x32, 0xe6, 0x00, 0x00, 0x00, 0x32, 0xe4, 0x00, 0x00,
            0x00, 0x82, 0xd1, 0xfd, 0x82, 0xd4, 0x34, 0x06,
        ],
    ];

    for body in bodies {
        let graph =
            crate::object_graph::parse(&object_graph_from_records(&[inline_object_graph_record(
                &body,
            )]))
            .expect("assigned inline control layout");
        assert_eq!(
            graph.records[0].inline_body.as_deref(),
            Some(body.as_slice())
        );
    }
}

#[test]
fn outer_object_graph_rejects_unassigned_childless_records() {
    let valid = [
        0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
    ];
    for index in [0, 1, 4, 10, 11] {
        let mut body = valid;
        body[index] ^= 1;
        assert!(crate::object_graph::parse(&object_graph_from_records(&[
            inline_object_graph_record(&body)
        ]))
        .is_none());
    }
    assert!(
        crate::object_graph::parse(&object_graph_from_records(&[inline_object_graph_record(
            &[0x10, 0xfe, 0x81, 0x06]
        )]))
        .is_none()
    );
}

#[test]
fn paired_entity_table_admits_an_opaque_childless_object_record() {
    let body = [0x00, 0x90, 0x32, 0x01, 0x00, 0x00, 0x00, 0x81, 0x81, 0x00];
    let bytes = object_graph_from_records(&[inline_object_graph_record(&body)]);
    let paired_roots = std::collections::HashMap::from([(0, 1)]);

    let [graph] = crate::object_graph::parse_all_with_paired_roots(&bytes, &paired_roots)
        .try_into()
        .expect("one entity-paired object graph");
    assert_eq!(graph.records[0].lead, 0x00);
    assert_eq!(
        graph.records[0].inline_body.as_deref(),
        Some(body.as_slice())
    );
    assert!(graph.records[0].head.is_empty());

    assert!(crate::object_graph::parse_all_with_paired_roots(
        &bytes,
        &std::collections::HashMap::from([(0, 2)]),
    )
    .is_empty());
}

#[test]
fn inline_entity_and_object_records_pair_by_extent_and_cardinality() {
    let mut entity = vec![0x7c, 0x05, 12, 0, 0, 0, 0x03, 0xea];
    entity.extend_from_slice(&1_u32.to_le_bytes());
    let graph_offset = entity.len() + 1;
    entity.push(0xde);
    entity.extend(object_graph_from_records(&[inline_object_graph_record(&[
        0x00, 0x90, 0x81, 0x81, 0x00,
    ])]));

    let native = crate::native::CatiaNative::decode(&entity);
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| graph.byte_offset == graph_offset as u64)
        .expect("entity-paired graph");
    assert_eq!(graph.records.len(), 1);
    assert_eq!(graph.records[0].entity_id, Some(1));
    let record = native
        .entity_records
        .iter()
        .find(|record| record.object_graph == graph.id)
        .expect("paired inline entity");
    assert_eq!(
        record.inline_body.as_deref(),
        Some(&[0x03, 0xea, 1, 0, 0, 0][..])
    );
    assert_eq!(record.object_record, graph.records[0].id);
}

#[test]
fn object_graph_payload_lists_keep_direct_fixed_width_atoms() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0x81, 0x80, 0x78, 0x56, 0x34, 0x12, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("fixed-width list atom");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 1,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Atom {
            value: 0x1234_5678,
            offset: 2,
        }]
    ));
}

#[test]
fn object_graph_payload_preserves_nonterminal_fe_atoms() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x85, 0x81, 0xfe, 0x81, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("interior FE atom");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Atom { value: 5, .. },
            crate::object_graph::PayloadField::Reference {
                value: 0xfe,
                offset: 1,
            },
            crate::object_graph::PayloadField::Atom { value: 0x81, .. },
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_payload_lists_preserve_nonterminal_fe_atoms() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0x82, 0xfe, 0x85, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("interior FE list atom");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 2,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[
            crate::object_graph::ListItem::Atom {
                value: 0xfe,
                offset: 2,
            },
            crate::object_graph::ListItem::Atom {
                value: 5,
                offset: 3,
            },
        ]
    ));
}

#[test]
fn outer_object_graph_keeps_adjacent_compact_head_references_separate() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83, 0x84],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("compact object head");
    let record = &graph.records[0];

    assert_eq!(record.owner_ref, Some(1));
    assert_eq!(record.class_ref, Some(3));
    assert_eq!(record.storage_ref, Some(4));
    assert_eq!(
        &record.head[2..],
        [
            crate::object_graph::HeadToken::Reference(1),
            crate::object_graph::HeadToken::Reference(3),
            crate::object_graph::HeadToken::Reference(4),
        ]
    );
}

#[test]
fn outer_object_graph_does_not_slide_head_roles_across_null_handles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x82, 0xff, 0xff, 0xff, 0xff, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("null-interrupted object head");
    let record = &graph.records[0];

    assert_eq!(record.owner_ref, Some(2));
    assert_eq!(record.class_ref, None);
    assert_eq!(record.storage_ref, None);
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_does_not_promote_unassigned_head_bytes() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0xe5, 0xff, 0xff, 0xff, 0xe4],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal head bytes");

    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[0].storage_ref, None);
    assert_eq!(
        &graph.records[0].head[2..],
        [
            crate::object_graph::HeadToken::Literal(0xe5),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xe4),
        ]
    );
}

#[test]
fn outer_object_graph_requires_the_head_separator_for_relations() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x82, 0x83, 0x84], &[0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("retained malformed head");

    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[0].storage_ref, None);
    assert!(graph.records[0]
        .head
        .iter()
        .any(|token| matches!(token, crate::object_graph::HeadToken::Reference(2))));
}

#[test]
fn outer_object_graph_reads_compact_owner_and_field_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x02, 0x82], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x82, 0x83, 0x84], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("compact heads");

    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[1].owner_ref, Some(2));
    assert_eq!(graph.records[1].class_ref, Some(3));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[2].owner_ref, Some(2));
    assert_eq!(graph.records[2].class_ref, Some(3));
    assert_eq!(graph.records[2].storage_ref, Some(4));
}

#[test]
fn outer_object_graph_reads_extended_compact_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x80, 0x83, 0, 0], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x80, 0xe8, 0x16, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.owner_ref, Some(2));
        assert_eq!(record.class_ref, None);
        assert_eq!(record.storage_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_incomplete_extended_compact_owner_framing() {
    for head in [
        &[0x12, 0x82, 0x80, 0x83, 0][..],
        &[0x12, 0x82, 0x80, 0x83, 0, 1][..],
        &[0x12, 0x80, 0x80, 0x83, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x16, 0x94, 0x80, 0x95, 22, 0, 0, 0x80, 0x96, 0, 0],
            &[0xfe],
        ),
        object_graph_record(&[0x16, 0x94, 0x80, 0x95, 0, 0, 0x80, 17, 28, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
        assert_eq!(record.owner_ref, Some(21));
    }
}

#[test]
fn outer_object_graph_reads_short_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0, 0], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x95, 0x80, 17, 21, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("short extended compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(21));
        assert_eq!(record.owner_ref, Some(0));
    }
}

#[test]
fn outer_object_graph_reads_reference_terminated_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x96, 0x80, 0x97, 0, 0], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x80, 0x96, 23, 0, 0, 0xd2, 0x2b], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x80, 123, 21, 0, 0, 0xd2, 0x2b], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("reference-terminated compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
    }
    assert_eq!(graph.records[0].storage_ref, Some(22));
    assert_eq!(graph.records[0].owner_ref, Some(0));
    for record in &graph.records[1..] {
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[1].owner_ref, Some(22));
    assert_eq!(graph.records[2].owner_ref, None);
    for record in &graph.records[1..] {
        assert!(matches!(
            record.head.last(),
            Some(crate::object_graph::HeadToken::Reference(300))
        ));
    }
}

#[test]
fn outer_object_graph_rejects_partial_reference_terminated_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x96, 23, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x96, 22, 0, 0, 0x97][..],
        &[0x16, 0x94, 0x80, 0x96, 23, 0, 0, 97][..],
        &[0x16, 0x94, 0x80, 0x80, 0x97, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_partial_short_extended_class_storage_owner_roles() {
    for head in [
        &[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0][..],
        &[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0, 1][..],
        &[0x16, 0x94, 0x80, 0x80, 0x96, 20, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_two_block_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[0x16, 0x94, 0x80, 95, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[0x16, 0x94, 0x80, 1, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("two-block extended compact heads");

    assert_eq!(graph.records[0].owner_ref, Some(21));
    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[1].owner_ref, None);
    assert_eq!(graph.records[2].owner_ref, None);
}

#[test]
fn outer_object_graph_retains_roles_before_a_literal_short_extended_owner() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x16, 0x94, 0x80, 66, 23, 0, 0, 0x80, 0x97, 0, 0],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal-owner extended head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(0));
    assert_eq!(record.owner_ref, None);
    assert_eq!(record.owner_literal, Some(66));
}

#[test]
fn outer_object_graph_rejects_partial_two_block_extended_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 25, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 24, 0, 0, 0x80, 0x96, 25, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 26, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_partial_extended_class_storage_owner_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x95, 22, 0, 1, 0x80, 0x96, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 0, 0, 0x80, 17, 29, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x80, 22, 0, 0, 0x80, 0x96, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_class_storage_owner_compact_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x16, 0x92, 0xd2, 0x2b, 0xd2, 0x39],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("class-storage-owner compact head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(18));
    assert_eq!(record.storage_ref, Some(300));
    assert_eq!(record.owner_ref, Some(314));
}

#[test]
fn outer_object_graph_retains_class_first_roles_before_an_unassigned_slot() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x95, 95], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 95, 0x96], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("class-first compact heads");

    assert_eq!(graph.records[0].class_ref, Some(20));
    assert_eq!(graph.records[0].storage_ref, Some(21));
    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[1].class_ref, Some(20));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[1].owner_ref, None);
}

#[test]
fn outer_object_graph_reads_null_lane_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b],
            &[0xfe],
        ),
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[
                0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 23, 0, 0,
            ],
            &[0xfe],
        ),
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 95, 23, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("null-lane compact head");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[0].owner_ref, Some(300));
    for record in &graph.records[1..] {
        assert_eq!(record.owner_ref, Some(0));
    }
}

#[test]
fn outer_object_graph_reads_terminal_null_lane_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminal null-lane head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(0));
    assert_eq!(record.owner_ref, Some(300));
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_rejects_incomplete_terminal_null_lane_roles() {
    for head in [
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b][..],
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b, 0x84][..],
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x83][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained terminal null-lane head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_terminal_lane_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x56, 0x94, 0x95, 0x96, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminal-lane compact head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(21));
    assert_eq!(record.owner_ref, Some(22));
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_reads_extended_terminal_lane_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x56, 0x94, 0x80, 0x96, 22, 0, 0, 0x80, 0x97, 0, 0, 0x83],
            &[0xfe],
        ),
        object_graph_record(
            &[0x56, 0x94, 0x80, 96, 23, 0, 0, 0x80, 97, 25, 0, 0, 0x83],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended terminal-lane heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[0].owner_ref, Some(22));
    assert_eq!(graph.records[1].owner_ref, None);
}

#[test]
fn outer_object_graph_rejects_incomplete_terminal_lane_roles() {
    for head in [
        &[0x56, 0x94, 0x95, 0x96][..],
        &[0x56, 0x94, 0x95, 0x96, 0x84][..],
        &[0x56, 0x94, 0x95, 0x80, 0x83][..],
        &[0x56, 0x94, 0x80, 0x96, 22, 0, 0, 0x80, 0x97, 0, 0, 0x84][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_incomplete_null_lane_roles() {
    for head in [
        &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff][..],
        &[0x1a, 0x94, 0x80, 0, 0, 0, 0, 0xd2, 0x2b][..],
        &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80][..],
        &[
            0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 24, 0, 0,
        ][..],
        &[
            0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 23, 0, 1,
        ][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_extended_owner_class_storage_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 22, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 0, 0, 0x80, 0x95, 0, 0], &[0xfe]),
        object_graph_record(
            &[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x80, 95, 22, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.owner_ref, Some(18));
        assert_eq!(record.class_ref, Some(0));
    }
    assert_eq!(graph.records[0].storage_ref, Some(21));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[2].storage_ref, Some(21));
    assert_eq!(graph.records[3].storage_ref, Some(21));
    assert_eq!(graph.records[4].storage_ref, None);
}

#[test]
fn outer_object_graph_rejects_incomplete_extended_owner_class_storage_roles() {
    for head in [
        &[0x52, 0x92, 0x80, 0x95, 22, 0, 0, 0x84][..],
        &[0x52, 0x92, 0x80, 0x95, 0, 0, 0x80, 0x96, 0, 0][..],
        &[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x80, 95, 23, 0, 0][..],
        &[0x52, 0x80, 0x80, 0x95, 22, 0, 0, 0x83][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn object_graph_payload_reads_fixed_width_escaped_values() {
    use crate::object_graph::PayloadField;

    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[
                0x80, 0x78, 0x56, 0x34, 0x12, 0x32, 2, 0, 0, 0, 0x32, 0xef, 0xcd, 0xab, 0x89, 0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
    ];
    let bytes = object_graph_from_records(&records);
    let graph = crate::object_graph::parse(&bytes).expect("fixed-width object payload");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 0x1234_5678,
                offset: 0,
            },
            PayloadField::Reference {
                value: 2,
                offset: 5,
            },
            PayloadField::Reference {
                value: 0x89ab_cdef,
                offset: 10,
            },
            PayloadField::Terminator,
        ]
    );
    let native =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&records));
    assert_eq!(
        native.object_graphs[0].records[0].references,
        [
            crate::native::CatiaObjectRecordReference {
                entity_id: 2,
                payload_offset: 5,
                source: crate::native::CatiaObjectRecordReferenceSource::Field,
                is_null: false,
                target: Some(native.object_graphs[0].records[1].id.clone()),
                design_object: native.object_graphs[0].records[1].design_object.clone(),
            },
            crate::native::CatiaObjectRecordReference {
                entity_id: 0x89ab_cdef,
                payload_offset: 10,
                source: crate::native::CatiaObjectRecordReferenceSource::Field,
                is_null: false,
                target: None,
                design_object: None,
            },
        ]
    );
}

#[test]
fn incomplete_object_payload_tags_do_not_consume_the_terminator() {
    for tag in [0x81, 0x3a, 0x39, 0x7a] {
        let bytes = object_graph_from_records(&[object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[tag, 0xfe],
        )]);
        let graph = crate::object_graph::parse(&bytes).expect("terminated tagged payload");
        let record = &graph.records[0];

        assert_eq!(
            record.payload.fields,
            [
                crate::object_graph::PayloadField::Atom {
                    value: u32::from(tag),
                    offset: 0,
                },
                crate::object_graph::PayloadField::Terminator,
            ]
        );
        assert!(
            crate::native::CatiaNative::decode(&bytes).object_graphs[0].records[0]
                .references
                .is_empty()
        );
    }
}

#[test]
fn native_design_objects_preserve_payload_references_to_target_owners() {
    let bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[0x3b, 0x82, 0x81, 0x83, 0x81, 0x83, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x81, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0x81, 0x81, 0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 1);
    assert_eq!(native.design_objects[0].ordinal, 0);
    assert_eq!(
        native.design_objects[0].first_field_byte_offset,
        native.object_graphs[0].records[0].byte_offset
    );
    assert_eq!(native.design_objects[0].fields.len(), 2);
    assert!(native.design_objects[0].field_classes.is_empty());
    let graph = &native.object_graphs[0];
    assert_eq!(
        graph.records[0].design_object.as_deref(),
        Some(native.design_objects[0].id.as_str())
    );
    assert_eq!(
        graph.records[0].references,
        [
            crate::native::CatiaObjectRecordReference {
                entity_id: 3,
                payload_offset: 2,
                source: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 0,
                },
                is_null: false,
                target: Some(graph.records[2].id.clone()),
                design_object: graph.records[2].design_object.clone(),
            },
            crate::native::CatiaObjectRecordReference {
                entity_id: 3,
                payload_offset: 4,
                source: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 1,
                },
                is_null: false,
                target: Some(graph.records[2].id.clone()),
                design_object: graph.records[2].design_object.clone(),
            },
        ]
    );
    assert_eq!(
        native.design_objects[0].relations,
        [
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 2,
                    container: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: 0,
                        item_ordinal: 0,
                    },
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 4,
                    container: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: 0,
                        item_ordinal: 1,
                    },
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[1].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 0,
                    container: crate::native::CatiaObjectRecordReferenceSource::Field,
                },
                target_entity_id: 1,
                target_field: graph.records[0].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[0].id.clone()),
            },
        ]
    );
    assert_eq!(
        graph.records[1].references,
        [crate::native::CatiaObjectRecordReference {
            entity_id: 1,
            payload_offset: 0,
            source: crate::native::CatiaObjectRecordReferenceSource::Field,
            is_null: false,
            target: Some(graph.records[0].id.clone()),
            design_object: graph.records[0].design_object.clone(),
        }]
    );
    assert_eq!(native.design_objects[1].owner_entity_id, 3);
    assert_eq!(native.design_objects[1].ordinal, 1);
    assert_eq!(
        native.design_objects[1].first_field_byte_offset,
        native.object_graphs[0].records[2].byte_offset
    );
    assert_eq!(
        native.design_objects[1].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: graph.records[2].id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 1,
            target_field: graph.records[0].id.clone(),
            target_class: None,
            target_design_object: Some(native.design_objects[0].id.clone()),
        }]
    );
}

#[test]
fn native_design_objects_preserve_storage_relations_before_payload_relations() {
    let bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84, 0x83], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x86], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = &native.object_graphs[0];
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode storage and payload relations");

    assert_eq!(
        native.design_objects[0].relations,
        [
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Storage,
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 0,
                    container: crate::native::CatiaObjectRecordReferenceSource::Field,
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
        ]
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_RELATION_COUNT),
        2
    );

    let mut malformed = native.clone();
    malformed.design_objects[0].relations.swap(0, 1);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store reordered design relations");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_design_objects_preserve_relations_to_unowned_fields() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81], &[0x81, 0x82, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0xe5, 0xff, 0xff, 0xff, 0xe4], &[0xfe]),
    ];
    let bytes = sequential_entity_backed_object_graph(&records);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode relation to unowned field");
    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load relation to unowned field");
    let graph = &native.object_graphs[0];

    assert_eq!(native.design_objects.len(), 1);
    assert_eq!(
        native.design_objects[0].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: graph.records[0].id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 2,
            target_field: graph.records[1].id.clone(),
            target_class: None,
            target_design_object: None,
        }]
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_UNOWNED_FIELD_RELATION_COUNT),
        1
    );
}

#[test]
fn native_design_objects_preserve_reflexive_field_relations() {
    let records = [object_graph_record(
        &[0x04, 0x01, 0x81],
        &[0x81, 0x81, 0xfe],
    )];
    let bytes = sequential_entity_backed_object_graph(&records);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode reflexive field relation");
    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load reflexive field relation");
    let field = &native.object_graphs[0].records[0];

    assert_eq!(
        native.design_objects[0].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: field.id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 1,
            target_field: field.id.clone(),
            target_class: None,
            target_design_object: Some(native.design_objects[0].id.clone()),
        }]
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_SAME_OBJECT_RELATION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_REFLEXIVE_FIELD_RELATION_COUNT),
        1
    );
}

#[test]
fn native_object_references_select_sparse_entity_identities() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x87, 0x86], &[0xfe]),
    ];
    let native =
        crate::native::CatiaNative::decode(&entity_backed_object_graph(&records, &[1, 3, 7]));
    let graph = &native.object_graphs[0];

    assert_eq!(
        native
            .entity_records
            .iter()
            .map(|record| record.entity_id)
            .collect::<Vec<_>>(),
        [1, 3, 7]
    );
    assert_eq!(
        graph
            .records
            .iter()
            .map(|record| record.entity_id)
            .collect::<Vec<_>>(),
        [Some(1), Some(3), Some(7)]
    );
    assert_eq!(
        graph.records[0].references[0].target.as_deref(),
        Some(graph.records[1].id.as_str())
    );
    assert_ne!(
        graph.records[0].references[0].target.as_deref(),
        Some(graph.records[2].id.as_str())
    );
    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [1, 3, 7]
    );
}

#[test]
fn native_design_relations_preserve_both_endpoint_schema_classes() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x86], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Limit",
        "Pad",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let relation = &native.design_objects[0].relations[0];
    assert_eq!(
        relation
            .source_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Profile")
    );
    assert_eq!(
        relation
            .target_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Pad")
    );
    assert_eq!(
        relation
            .source_class
            .as_ref()
            .map(|class| class.entry.as_str()),
        native.object_graphs[0].records[0].class_entry.as_deref()
    );
    assert_eq!(
        relation
            .target_class
            .as_ref()
            .map(|class| class.entry.as_str()),
        native.object_graphs[0].records[2].class_entry.as_deref()
    );
}

#[test]
fn compact_design_objects_use_field_vocabulary_not_anchor_class() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "BaseFeature",
        "Groove",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.owner_entity_id, 2);
    assert!(object.owner_record.is_some());
    assert_eq!(object.owner_class, None);
    assert_eq!(object.owner_storage_ref, None);
    assert_eq!(
        object.field_classes,
        [
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[4].id.clone(),
                name: "BaseFeature".to_string(),
            },
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[5].id.clone(),
                name: "Groove".to_string(),
            },
        ]
    );
}

#[test]
fn null_storage_roles_are_not_unresolved_storage_links() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x16, 0x84, 0x80, 0x82], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "BaseFeature",
    ]));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode null storage role");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_STORAGE_RECORD_COUNT),
        0
    );
}

#[test]
fn pattern_schema_definition_does_not_create_a_feature_instance() {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0];
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &definition,
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    native.entity_records[0]
        .definition_value
        .as_mut()
        .expect("definition value")
        .definition
        .value = "CircPattern".to_string();
    native.object_graphs[0].records[0].class_name = Some("Element1".to_string());

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);
    assert!(ir.model.features.is_empty());
    assert!(transfer.consumed_records().is_empty());
}

#[test]
fn prt_sketch_schema_field_does_not_create_a_feature_instance() {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x83], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "PRTSketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.object_graphs[0].records[1].class_name.as_deref(),
        Some("PRTSketch")
    );

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().is_empty());
}

#[test]
fn exact_sketch_owner_declaration_transfers_identity_without_geometry() {
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &[0x00, 0x08, 0x32, 4, 0, 0, 0],
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    let owner_record = native
        .object_graphs
        .iter()
        .flat_map(|graph| graph.records.iter())
        .find(|record| record.design_object.is_some())
        .expect("synthetic owner declaration record")
        .clone();
    let owner_record_id = owner_record.id.clone();
    let owner_design_object = owner_record.design_object.clone();
    let owner_class_entry = "synthetic-sketch-class".to_string();
    let owner_record_mut = native
        .object_graphs
        .iter_mut()
        .flat_map(|graph| graph.records.iter_mut())
        .find(|record| record.id == owner_record_id)
        .expect("mutable synthetic owner declaration record");
    owner_record_mut.class_name = Some("Sketch".to_string());
    owner_record_mut.class_entry = Some(owner_class_entry.clone());

    let object = native
        .design_objects
        .first_mut()
        .expect("synthetic design object");
    object.owner_record = Some(owner_record_id.clone());
    object.owner_design_object = owner_design_object;
    object.owner_class = Some(crate::native::CatiaDesignClass {
        entry: owner_class_entry,
        name: "Sketch".to_string(),
    });

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    let parameter_entity = native
        .entity_records
        .iter()
        .find(|entity| {
            native
                .object_graphs
                .iter()
                .flat_map(|graph| graph.records.iter())
                .find(|record| record.id == entity.object_record)
                .and_then(|record| record.design_object.as_deref())
                == Some(native.design_objects[0].id.as_str())
        })
        .expect("synthetic feature-owned parameter entity");
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId("synthetic:parameter".to_string()),
            owner: None,
            ordinal: 0,
            name: "Value".to_string(),
            expression: String::new(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(parameter_entity.id.clone()),
        });
    transfer.assign_parameter_owners(&mut ir, &native);

    assert_eq!(ir.model.sketches.len(), 1);
    assert!(matches!(
        ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: Some(_),
        }
    ));
    assert!(ir.model.sketches[0].profiles.is_empty());
    assert_eq!(
        ir.model.sketches[0].placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved
    );
    assert_eq!(
        ir.model.parameters[0].owner,
        Some(cadmpeg_ir::features::FeatureId(
            crate::design_feature::neutral_history_id(&native.design_objects[0].id, "feature"),
        ))
    );
    assert_eq!(
        transfer.sketch_owner_records,
        std::collections::HashSet::from([owner_record_id])
    );
}

#[test]
fn incompatible_exact_feature_candidates_on_one_object_remain_unresolved() {
    let records = [
        object_graph_record(&[0x12, 0x84, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x84, 0x84], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x85, 0x85], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3, 4]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "xy-plane",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);

    let candidate = native
        .design_objects
        .iter()
        .find(|object| object.owner_entity_id == 4)
        .expect("synthetic dual-candidate object");
    assert_eq!(candidate.field_classes[0].name, "xy-plane");
    assert_eq!(candidate.owner_entity_id, 4);
    assert_eq!(
        candidate
            .owner_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Sketch")
    );
    assert_eq!(
        candidate.owner_design_object,
        Some(native.design_objects[1].id.clone())
    );

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().is_empty());
    assert!(transfer.feature_ids.is_empty());
}

#[test]
fn parameter_owner_follows_one_exact_child_design_object() {
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &[0x00, 0x08, 0x32, 4, 0, 0, 0],
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    let owner_record = native
        .object_graphs
        .iter()
        .flat_map(|graph| graph.records.iter())
        .find(|record| record.design_object.is_some())
        .expect("synthetic owner declaration record")
        .clone();
    let owner_record_id = owner_record.id.clone();
    let owner_design_object = owner_record.design_object.clone();
    let owner_class_entry = "synthetic-sketch-class".to_string();
    let owner_record_mut = native
        .object_graphs
        .iter_mut()
        .flat_map(|graph| graph.records.iter_mut())
        .find(|record| record.id == owner_record_id)
        .expect("mutable synthetic owner declaration record");
    owner_record_mut.class_name = Some("Sketch".to_string());
    owner_record_mut.class_entry = Some(owner_class_entry.clone());

    let feature_object = native
        .design_objects
        .first_mut()
        .expect("synthetic design object");
    feature_object.owner_record = Some(owner_record_id);
    feature_object.owner_design_object = owner_design_object.clone();
    feature_object.owner_class = Some(crate::native::CatiaDesignClass {
        entry: owner_class_entry,
        name: "Sketch".to_string(),
    });
    let feature_id = feature_object.id.clone();

    let child_record_id = "synthetic-child-record".to_string();
    let child_entity_id = "synthetic-child-entity".to_string();
    let mut child_record = owner_record.clone();
    child_record.id.clone_from(&child_record_id);
    child_record.entity_record = Some(child_entity_id.clone());
    child_record.entity_id = Some(2);
    child_record.owner = Some(crate::native::CatiaObjectOwner::Entity(2));
    child_record.design_object = Some("synthetic-child-object".to_string());
    native.object_graphs[0].records.push(child_record);

    let mut child_entity = native.entity_records[0].clone();
    child_entity.id.clone_from(&child_entity_id);
    child_entity.object_record = child_record_id.clone();
    child_entity.entity_id = 2;
    child_entity.ordinal = native.entity_records.len() as u64;
    native.entity_records.push(child_entity);

    let mut child_object = native.design_objects[0].clone();
    child_object.id = "synthetic-child-object".to_string();
    child_object.ordinal += 1;
    child_object.first_field_byte_offset += 1;
    child_object.owner_entity_id = 2;
    child_object.owner_record = Some(child_record_id);
    child_object.owner_design_object = Some(feature_id.clone());
    child_object.owner_class = None;
    child_object.owner_storage_ref = None;
    child_object.fields = vec!["synthetic-child-record".to_string()];
    child_object.field_classes.clear();
    child_object.definition_values.clear();
    child_object.definition_chain_values.clear();
    child_object.relations.clear();
    child_object.parallel_reference_table = None;
    native.design_objects.push(child_object);

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId("synthetic:child-parameter".to_string()),
            owner: None,
            ordinal: 0,
            name: "Value".to_string(),
            expression: String::new(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some(child_entity_id),
        });

    transfer.assign_parameter_owners(&mut ir, &native);

    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(
        ir.model.parameters[0].owner,
        Some(cadmpeg_ir::features::FeatureId(
            crate::design_feature::neutral_history_id(&feature_id, "feature"),
        ))
    );
}

#[test]
fn complete_standalone_principal_plane_declarations_transfer_one_history_node() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    for (class, plane) in [
        ("xy-plane", PrincipalPlane::Top),
        ("yz-plane", PrincipalPlane::Right),
        ("zx-plane", PrincipalPlane::Front),
    ] {
        let records = [
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        ];
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            class,
        ]));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());

        let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.sketches.is_empty());
        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(
            ir.model.features[0].definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
        assert_eq!(ir.model.features[0].source_tag.as_deref(), Some(class));
        assert_eq!(
            ir.model.features[0].ordinal,
            native.design_objects[0].first_field_byte_offset
        );
        assert_eq!(
            transfer.principal_plane_records,
            native.design_objects[0].fields.iter().cloned().collect()
        );

        let mut excluded_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let excluded = crate::design_feature::transfer_design_features(
            &mut excluded_ir,
            &native,
            Some(&std::collections::HashSet::new()),
        );
        assert!(excluded_ir.model.features.is_empty());
        assert!(excluded.consumed_records().is_empty());
    }
}

#[test]
fn mixed_or_payload_bearing_principal_plane_fields_do_not_transfer() {
    for (records, catalog) in [
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
                "yz-plane",
            ],
        ),
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x84], &[0x80, 0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
            ],
        ),
    ] {
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&catalog));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());

        let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.features.is_empty());
        assert!(transfer.principal_plane_records.is_empty());
    }
}

#[test]
fn design_field_vocabulary_distinguishes_equal_names_from_distinct_entries() {
    let mut bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Feature",
        "Feature",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let classes = &native.design_objects[0].field_classes;

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, classes[1].name);
    assert_ne!(classes[0].entry, classes[1].entry);
}

#[test]
fn native_design_objects_preserve_unresolved_owner_identities() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x80, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x84, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = &native.object_graphs[0];

    assert_eq!(graph.records[0].owner_entity_id(), Some(0));
    assert_eq!(graph.records[1].owner_entity_id(), Some(4));
    assert!(graph
        .records
        .iter()
        .all(|record| record.design_object.is_some()));
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 0);
    assert_eq!(native.design_objects[1].owner_entity_id, 4);
    assert!(native
        .design_objects
        .iter()
        .all(|object| object.owner_record.is_none()));
}

#[test]
fn native_design_objects_retain_and_validate_parallel_reference_tables() {
    let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x84, 0x85, 0xfe];
    let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
        object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Limit",
        "Profile",
        "Limit",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let table = native.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table");
    assert_eq!(
        table
            .columns
            .iter()
            .map(|column| &column.field)
            .collect::<Vec<_>>(),
        native.design_objects[0].fields.iter().collect::<Vec<_>>()
    );
    assert!(table
        .columns
        .iter()
        .all(|column| column.field_class.is_some()));
    assert!(table
        .columns
        .iter()
        .all(|column| column.list_payload_offset == 0));
    assert_eq!(table.rows.len(), 2);
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.entity_id)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![3, 4], vec![4, 3]]
    );
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.payload_offset)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![2, 2], vec![4, 4]]
    );
    assert!(table.rows.iter().flat_map(|row| &row.cells).all(|cell| {
        cell.field.is_some() && cell.field_class.is_some() && cell.design_object.is_some()
    }));
    assert_eq!(
        table.rows[0].matching_design_object,
        table.rows[0].cells[0].design_object
    );
    assert!(table.rows[0].matching_design_object.is_some());
    assert!(table.rows[1].matching_design_object.is_none());

    let expected = table.clone();
    let mut malformed = native.clone();
    malformed.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[0]
        .cells[0]
        .entity_id += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed parallel reference table");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_offset = native.clone();
    malformed_offset.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[0]
        .cells[0]
        .payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_offset
        .store(&mut namespace)
        .expect("store malformed parallel-reference cell offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_list_offset = native.clone();
    malformed_list_offset.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .columns[0]
        .list_payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_list_offset
        .store(&mut namespace)
        .expect("store malformed parallel-reference list offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut version_256_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_256_namespace)
        .expect("store pre-column-incidence parallel reference table");
    let mut stored_fields = version_256_namespace
        .arenas
        .get_mut("design_objects")
        .expect("stored design objects")[0]
        .fields_mut();
    let columns = stored_fields
        .get_mut("parallel_reference_table")
        .expect("stored parallel reference table")
        .as_object_mut()
        .expect("stored parallel reference table")
        .get_mut("columns")
        .expect("stored parallel reference columns")
        .as_array_mut()
        .expect("stored parallel reference columns");
    for column in columns {
        *column = column
            .as_object()
            .expect("stored parallel reference column")["field"]
            .clone();
    }
    version_256_namespace.version =
        crate::native::CATIA_PARALLEL_REFERENCE_COLUMN_INCIDENCE_VERSION - 1;
    drop(stored_fields);
    let migrated = crate::native::CatiaNative::load(&version_256_namespace)
        .expect("migrate parallel-reference column incidences");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_255_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_255_namespace)
        .expect("store pre-offset parallel reference table");
    let mut version_255_objects: Vec<crate::native::CatiaDesignObject> = version_255_namespace
        .arena_as("design_objects")
        .expect("load version 255 design objects");
    for cell in version_255_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        cell.payload_offset = 0;
    }
    version_255_namespace
        .set_arena("design_objects", &version_255_objects)
        .expect("store version 255 design objects");
    version_255_namespace.version = crate::native::CATIA_PARALLEL_REFERENCE_CELL_OFFSET_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_255_namespace)
        .expect("migrate parallel-reference cell offsets");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current parallel reference table");
    let mut previous_objects: Vec<crate::native::CatiaDesignObject> = previous_namespace
        .arena_as("design_objects")
        .expect("load stored design objects");
    previous_objects[0].parallel_reference_table = None;
    previous_namespace
        .set_arena("design_objects", &previous_objects)
        .expect("store previous design objects");
    previous_namespace.version = 200;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous parallel reference table");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_203_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_203_namespace)
        .expect("store current parallel reference row matches");
    let mut version_203_objects: Vec<crate::native::CatiaDesignObject> = version_203_namespace
        .arena_as("design_objects")
        .expect("load version 203 design objects");
    for row in &mut version_203_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
    {
        row.matching_design_object = None;
    }
    version_203_namespace
        .set_arena("design_objects", &version_203_objects)
        .expect("store version 203 design objects");
    version_203_namespace.version = 203;
    let migrated = crate::native::CatiaNative::load(&version_203_namespace)
        .expect("migrate version 203 parallel reference row matches");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_202_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_202_namespace)
        .expect("store current classified parallel reference columns");
    let mut version_202_objects: Vec<crate::native::CatiaDesignObject> = version_202_namespace
        .arena_as("design_objects")
        .expect("load version 202 design objects");
    for column in &mut version_202_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .columns
    {
        column.field_class = None;
    }
    version_202_namespace
        .set_arena("design_objects", &version_202_objects)
        .expect("store version 202 design objects");
    version_202_namespace.version = 202;
    let migrated = crate::native::CatiaNative::load(&version_202_namespace)
        .expect("migrate version 202 source field classes");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_201_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_201_namespace)
        .expect("store current classified parallel reference table");
    let mut version_201_objects: Vec<crate::native::CatiaDesignObject> = version_201_namespace
        .arena_as("design_objects")
        .expect("load version 201 design objects");
    for cell in version_201_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        cell.field_class = None;
    }
    version_201_namespace
        .set_arena("design_objects", &version_201_objects)
        .expect("store version 201 design objects");
    version_201_namespace.version = 201;
    let migrated = crate::native::CatiaNative::load(&version_201_namespace)
        .expect("migrate version 201 target field classes");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected)
    );

    let null_list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x85, 0x85, 0xfe];
    let null_list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x85, 0x86, 0xfe];
    let terminal_null =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x83], &null_list_a),
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &null_list_b),
            object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
            object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
        ]));
    let null_table = terminal_null.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table with terminal null row");
    assert!(null_table.rows[1].cells.iter().all(|cell| {
        cell.entity_id == 5 && cell.is_null && cell.field.is_none() && cell.design_object.is_none()
    }));

    let mut version_210_namespace = cadmpeg_ir::NativeNamespace::default();
    terminal_null
        .store(&mut version_210_namespace)
        .expect("store terminal null parallel reference cells");
    let mut version_210_records: Vec<crate::native::CatiaObjectRecord> = version_210_namespace
        .arena_as("object_graph_records")
        .expect("load version 210 object records");
    for reference in version_210_records
        .iter_mut()
        .flat_map(|record| &mut record.references)
    {
        reference.is_null = false;
    }
    version_210_namespace
        .set_arena("object_graph_records", &version_210_records)
        .expect("store version 210 object records");
    let mut version_210_objects: Vec<crate::native::CatiaDesignObject> = version_210_namespace
        .arena_as("design_objects")
        .expect("load version 210 design objects");
    for cell in version_210_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        cell.is_null = false;
    }
    version_210_namespace
        .set_arena("design_objects", &version_210_objects)
        .expect("store version 210 design objects");
    version_210_namespace.version = 210;
    let migrated = crate::native::CatiaNative::load(&version_210_namespace)
        .expect("migrate terminal null parallel reference cells");
    assert!(migrated.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("migrated parallel reference table")
        .rows[1]
        .cells
        .iter()
        .all(|cell| cell.is_null));

    let three_references = [0x3b, 0x83, 0x81, 0x83, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mismatched = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &three_references),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x84, 0x86], &[0xfe]),
    ]);
    assert!(
        crate::native::CatiaNative::decode(&mismatched).design_objects[0]
            .parallel_reference_table
            .is_none()
    );
}

#[test]
fn parallel_reference_row_match_requires_distinct_target_fields() {
    let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x83, 0x85, 0xfe];
    let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
        object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Profile",
        "Profile",
        "Profile",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let table = native.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table");

    assert!(table.rows[0].matching_design_object.is_some());
    assert!(table.rows[1].matching_design_object.is_none());
    assert_eq!(table.rows[1].cells[0].field, table.rows[1].cells[1].field);

    let mut version_204_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_204_namespace)
        .expect("store current parallel reference row matches");
    let mut version_204_objects: Vec<crate::native::CatiaDesignObject> = version_204_namespace
        .arena_as("design_objects")
        .expect("load version 204 design objects");
    version_204_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[1]
        .matching_design_object = table.rows[1].cells[0].design_object.clone();
    version_204_namespace
        .set_arena("design_objects", &version_204_objects)
        .expect("store version 204 design objects");
    version_204_namespace.version = 204;

    let migrated = crate::native::CatiaNative::load(&version_204_namespace)
        .expect("migrate version 204 parallel reference row matches");
    assert!(migrated.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("migrated parallel reference table")
        .rows[1]
        .matching_design_object
        .is_none());
}

#[test]
fn native_design_objects_follow_first_field_order() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [3, 1]
    );
    assert_eq!(native.design_objects[0].fields.len(), 2);
    assert_eq!(native.design_objects[1].fields.len(), 1);
    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| (object.ordinal, object.first_field_byte_offset))
            .collect::<Vec<_>>(),
        [
            (0, native.object_graphs[0].records[0].byte_offset),
            (1, native.object_graphs[0].records[1].byte_offset),
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store source-ordered design objects");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load source-ordered design objects");
    assert_eq!(
        loaded
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [3, 1]
    );
}

#[test]
fn incomplete_object_lists_do_not_assert_reference_links() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0x3b, 0x83, 0x81, 0x82, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.object_graphs[0].records[0].references.is_empty());
    assert!(native.design_objects[0].relations.is_empty());
    assert!(matches!(
        &native.object_graphs[0].records[0].payload.fields[0],
        crate::object_graph::PayloadField::List {
            declared_count: 3,
            items,
            ..
        } if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
}

#[test]
fn incomplete_object_list_tags_do_not_consume_the_payload_terminator() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[0x3b, 0x82, 0x81, 0x82, 0x81, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let record = &native.object_graphs[0].records[0];

    assert!(record.references.is_empty());
    assert!(native.design_objects[0].relations.is_empty());
    assert!(matches!(
        record.payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 2,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
}

#[test]
fn incomplete_object_list_headers_do_not_consume_the_payload_terminator() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0xfe],
    )]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let record = &native.object_graphs[0].records[0];

    assert_eq!(
        record.payload.fields,
        [
            crate::object_graph::PayloadField::Atom {
                value: 0x3b,
                offset: 0,
            },
            crate::object_graph::PayloadField::Terminator,
        ]
    );
    assert!(record.references.is_empty());
    assert!(native.design_objects[0].relations.is_empty());
}

#[test]
fn outer_object_graph_resolves_class_names_from_following_schema() {
    let mut bytes = object_graph_stream();
    let graph_len = bytes.len();
    bytes.extend(value_block_stream(&[0x81]));
    let catalog_pos = bytes.len();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let graph = crate::object_graph::parse(&bytes).expect("object graph with schema");
    assert_eq!(graph.total_len, graph_len);
    assert_eq!(graph.catalog_pos, Some(catalog_pos));
    assert_eq!(graph.records[0].class_name.as_deref(), Some(""));
    assert_eq!(graph.records[1].class_name.as_deref(), Some("Sketch"));
    let mut native_bytes = entity_table_record(1);
    native_bytes.extend(entity_table_record(2));
    native_bytes.push(0xde);
    native_bytes.extend_from_slice(&bytes);
    let native = crate::native::CatiaNative::decode(&native_bytes);
    assert_eq!(
        native.object_graphs[0].catalog,
        Some(native.catalogs[0].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[0].class_entry,
        Some(native.catalogs[0].entries[3].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[1].class_entry,
        Some(native.catalogs[0].entries[4].id.clone())
    );
    assert_eq!(
        native.design_objects[0].field_classes,
        [
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[3].id.clone(),
                name: String::new(),
            },
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[4].id.clone(),
                name: "Sketch".to_string(),
            },
        ]
    );
    assert_eq!(
        native.design_objects[0].owner_class,
        Some(crate::native::CatiaDesignClass {
            entry: native.catalogs[0].entries[4].id.clone(),
            name: "Sketch".to_string(),
        })
    );
    assert_eq!(native.design_objects[0].owner_storage_ref, None);
}

#[test]
fn outer_object_graph_parser_preserves_every_root() {
    let first = object_graph_stream();
    let mut bytes = first.clone();
    bytes.extend(object_graph_vm_stream());
    let graphs = crate::object_graph::parse_all(&bytes);
    assert_eq!(graphs.len(), 2);
    assert_eq!(graphs[0].pos, 0);
    assert_eq!(graphs[1].pos, first.len());
}

#[test]
fn outer_object_graph_suppresses_roots_inside_framed_payloads() {
    let nested =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut payload = vec![0xe5];
    payload.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("fixture nested graph length")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&nested);
    payload.push(0xfe);
    let outer =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)]);

    let graphs = crate::object_graph::parse_all(&outer);
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].pos, 0);
}

#[test]
fn outer_object_graph_resolves_paged_class_ordinals() {
    let records = [
        object_graph_record(&[0x14, 0x01, 0x82, 0xd1, 0x88], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82], &[0xfe]),
    ];
    let mut bytes = object_graph_from_records(&records);
    let mut names = vec!["field"; 138];
    names[0] = "CATCatalogManager";
    names[1] = "catalogManager";
    names[2] = "catalogLinks";
    names[3] = "";
    names[137] = "Pad";
    let mut schema = vec![0x7c, 0x02, 0, 0, 0, 0, 0xd1, 0x8a];
    for name in names {
        schema.push(u8::try_from(name.len() + 1).expect("fixture schema name length"));
        schema.extend_from_slice(name.as_bytes());
    }
    let schema_len = u32::try_from(schema.len()).expect("fixture schema length");
    schema[2..6].copy_from_slice(&schema_len.to_le_bytes());
    bytes.extend(schema);
    let graph = crate::object_graph::parse(&bytes).expect("paged class graph");
    assert_eq!(graph.records[0].class_ref, Some(137));
    assert_eq!(graph.records[0].class_name.as_deref(), Some("Pad"));
}

#[test]
fn catalog_parser_reads_exact_inclusive_length_dictionary() {
    let entries = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Pad",
    ];
    let catalogs = crate::catalog::parse(&catalog_stream(&entries));

    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].declared_count, 7);
    assert_eq!(catalogs[0].entries.len(), entries.len());
    assert_eq!(catalogs[0].entries[4].ordinal, 4);
    assert_eq!(catalogs[0].entries[4].value, "Sketch");
    assert_eq!(catalogs[0].entries[5].value, "Pad");
}

#[test]
fn value_block_parser_reads_length_to_terminator_boundary() {
    let payload = [0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82];
    let mut bytes = value_block_stream(&payload);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let blocks = crate::value_block::parse(&bytes);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].pos, 0);
    assert_eq!(blocks[0].declared_len, 15);
    assert_eq!(blocks[0].total_len, 16);
    assert_eq!(blocks[0].payload, payload);
}

#[test]
fn native_value_blocks_require_a_complete_adjacent_catalog() {
    let mut bytes = value_block_stream(&[0x81]);
    bytes.extend_from_slice(&[0x7c, 0x02]);

    assert_eq!(crate::value_block::parse(&bytes).len(), 1);
    assert!(crate::native::CatiaNative::decode(&bytes)
        .value_blocks
        .is_empty());
}

#[test]
fn native_value_blocks_distinguish_the_terminal_schema_sentinel() {
    let mut bytes = value_block_stream(&[0x32, 4, 0, 0, 0, 0x83, 0x32, 5, 0, 0, 0, 0x82]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let block = &native.value_blocks[0];
    assert_eq!(block.schema_selections.len(), 1);
    assert_eq!(block.schema_selections[0].ordinal, 4);
    assert_eq!(block.schema_selections[0].entry, None);
    assert_eq!(block.schema_selections[0].name, None);
    assert!(block.schema_selections[0].encoded_value.is_empty());
    assert!(block.fields.iter().any(|field| matches!(
        field,
        crate::value_block::ValueField::SchemaSelector { ordinal: 5, .. }
    )));
}

#[test]
fn native_value_blocks_frame_values_between_catalog_valid_selectors() {
    let mut bytes = value_block_stream(&[
        0x32, 3, 0, 0, 0, 0x83, 0x32, 5, 0, 0, 0, 0x84, 0x32, 2, 0, 0, 0, 0x32, 1, 0, 0, 0, 0x82,
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let selections = &native.value_blocks[0].schema_selections;
    assert_eq!(selections.len(), 3);
    assert_eq!(selections[0].parent, native.value_blocks[0].id);
    assert_eq!(
        selections[0].id,
        format!(
            "catia:outer:value-selection#{:010}",
            native.value_blocks[0].byte_offset + 6 + selections[0].offset
        )
    );
    assert_eq!(selections[0].ordinal, 3);
    assert!(matches!(
        selections[0].encoded_value.as_slice(),
        [
            crate::value_block::ValueField::Atom { value: 3, .. },
            crate::value_block::ValueField::SchemaSelector { ordinal: 5, .. },
            crate::value_block::ValueField::Atom { value: 4, .. },
        ]
    ));
    assert_eq!(selections[1].ordinal, 2);
    assert!(selections[1].encoded_value.is_empty());
    assert_eq!(selections[2].ordinal, 1);
    assert!(matches!(
        selections[2].encoded_value.as_slice(),
        [crate::value_block::ValueField::Atom { value: 2, .. }]
    ));
}

#[test]
fn native_design_inventory_excludes_records_inside_object_payloads() {
    let mut nested = value_block_stream(&[0x81]);
    nested.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));
    nested.extend(surface_alias_stream());
    let mut payload = vec![0xe5];
    payload.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("fixture nested design length")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&nested);
    payload.push(0xfe);
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)]);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.object_graphs.len(), 1);
    assert!(native.alias_rows.is_empty());
    assert!(native.catalogs.is_empty());
    assert!(native.value_blocks.is_empty());
}

#[test]
fn native_design_inventory_excludes_records_inside_value_payloads() {
    let mut nested = value_block_stream(&[0x81]);
    nested.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));
    nested.extend(surface_alias_stream());
    let mut bytes = value_block_stream(&nested);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    assert_eq!(crate::value_block::parse(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(native.alias_rows.is_empty());
    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
}

#[test]
fn native_design_inventory_excludes_object_graphs_inside_value_payloads() {
    let nested =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut bytes = value_block_stream(&nested);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    assert_eq!(crate::object_graph::parse_all(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(native.object_graphs.is_empty());
    assert!(native.design_objects.is_empty());
    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
}

#[test]
fn native_design_inventory_excludes_alias_rows_inside_catalog_entries() {
    let mut alias = 1u32.to_le_bytes().to_vec();
    alias.extend_from_slice(&[0x01, 0x00, 0x04, 0x00]);
    alias.extend_from_slice(&0x0012_3456u32.to_le_bytes());
    alias.extend_from_slice(&[1, 2, 3, 4]);
    alias.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    alias.extend_from_slice(&0x5566_7744u32.to_le_bytes());
    let entry = String::from_utf8(alias).expect("alias-shaped UTF-8 entry bytes");
    let bytes = catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        &entry,
    ]);

    assert_eq!(crate::object_graph::surface_aliases(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.catalogs.len(), 1);
    assert!(native.alias_rows.is_empty());
}

#[test]
fn object_graph_payload_does_not_consume_terminator_as_fixed_width_atom_data() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x8d, 0x80, 0x8f, 0x81, 0x8b, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminator-bounded object payload");

    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 13,
                offset: 0,
            },
            PayloadField::Atom {
                value: 0,
                offset: 1,
            },
            PayloadField::Atom {
                value: 15,
                offset: 2,
            },
            PayloadField::Reference {
                value: 11,
                offset: 3,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn object_graph_payload_does_not_consume_terminator_as_paged_atom_data() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x8d, 0xd2, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminator-bounded paged atom");

    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 13,
                offset: 0,
            },
            PayloadField::Atom {
                value: 0xd2,
                offset: 1,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn outer_object_graph_vm_reads_lists_paged_atoms_and_null_handles() {
    use crate::object_graph::{HeadToken, ListItem, PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_vm_stream()).unwrap();
    assert!(graph.records[0].head.contains(&HeadToken::NullHandle));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::ListAggregator);
    assert!(matches!(
        &graph.records[0].payload.fields[0],
        PayloadField::List { items, .. }
            if items == &vec![
                ListItem::Reference {
                    value: 5,
                    offset: 2,
                },
                ListItem::Atom {
                    value: 6,
                    offset: 4,
                },
                ListItem::Atom {
                    value: 10,
                    offset: 6,
                },
            ]
    ));
}

#[test]
fn outer_object_graph_rejects_an_ambiguous_3c_bulk_row_id() {
    assert!(crate::object_graph::parse(&object_graph_ambiguous_3c_stream()).is_none());
}

#[test]
fn object_graph_payload_decodes_3c_bulk_table_rows() {
    use crate::object_graph::{BulkTableRow, PayloadField};

    let graph = crate::object_graph::parse(&object_graph_bulk_table_stream()).expect("bulk table");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::BulkTable {
                count: 0,
                table_count: 3,
                rows: vec![
                    BulkTableRow {
                        row_id: 17,
                        handle: 0x692f,
                        offset: 6,
                    },
                    BulkTableRow {
                        row_id: 257,
                        handle: 0x6931,
                        offset: 13,
                    },
                    BulkTableRow {
                        row_id: 5121,
                        handle: 0x6933,
                        offset: 21,
                    },
                ],
                offset: 0,
            },
            PayloadField::Scalar {
                tag: 0x3a,
                value: 5,
                offset: 32,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn object_graph_payload_keeps_3c_as_literal_when_no_bulk_extent_is_possible() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x3c, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal 3c payload");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 0x3c,
                offset: 0,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn decode_retains_outer_object_graph_order_and_references() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_object_graph()),
            &DecodeOptions::default(),
        )
        .expect("decode generated object graph part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.object_graphs.len(), 1);
    let graph = &native.object_graphs[0];
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].ordinal, 0);
    assert_eq!(graph.records[0].owner_entity_id(), Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[1].ordinal, 1);
    assert_eq!(graph.records[1].owner_entity_id(), Some(2));
    assert_eq!(graph.records[1].class_ref, Some(4));
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.parent, graph.id);
    assert_eq!(object.owner_entity_id, 2);
    assert_eq!(
        object.owner_record.as_deref(),
        Some(graph.records[1].id.as_str())
    );
    assert_eq!(
        object.fields,
        graph
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_GRAPH_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_RECORD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_FIELD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::CLASSIFIED_DESIGN_OBJECT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_OWNER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FEATURE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_SKETCH_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_SKETCH_CONSTRAINT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONFIGURATION_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 design object(s)")
            && loss.message.contains("2 object-graph field record(s)")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation
        .findings
        .iter()
        .all(|finding| finding.check != cadmpeg_ir::report::Check::Identity));
}

#[test]
fn unresolved_modeling_scope_accounts_for_every_retained_object_record() {
    let (mut bytes, _) = outer_container_object_graph_catpart();
    let class_offset = bytes
        .windows(b"CATPrtCont".len())
        .position(|window| window == b"CATPrtCont")
        .expect("part-container declaration");
    bytes[class_offset..class_offset + b"CATPrtCont".len()].copy_from_slice(b"CATFooCont");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode object graph without a declared part container");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_GRAPH_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_RECORD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::MODELING_OBJECT_GRAPH_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::MODELING_OBJECT_RECORD_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::RETAINED_UNSCOPED_OBJECT_GRAPH_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::RETAINED_UNSCOPED_OBJECT_RECORD_COUNT),
        2
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 retained object graph(s)")
            && loss.message.contains("2 field record(s)")
    }));
}

#[test]
fn decode_links_design_objects_through_their_owner_record_group() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_nested_design_objects()),
            &DecodeOptions::default(),
        )
        .expect("decode nested design objects");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 2);
    assert_eq!(native.design_objects[1].owner_entity_id, 3);
    assert_eq!(
        native.design_objects[0].owner_design_object.as_deref(),
        Some(native.design_objects[1].id.as_str())
    );
    assert_eq!(native.design_objects[1].owner_design_object, None);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DESIGN_OBJECT_OWNER_LINK_COUNT),
        1
    );
}

#[test]
fn native_load_rejects_orphaned_and_ambiguously_owned_design_records() {
    let mut bytes = object_graph_stream();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA native namespace");

    for arena_name in ["catalogs", "object_graphs"] {
        let mut malformed = namespace.clone();
        malformed
            .arenas
            .get_mut(arena_name)
            .expect("owner arena")
            .clear();
        assert!(matches!(
            crate::native::CatiaNative::load(&malformed),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }

    for arena_name in ["catalogs", "object_graphs"] {
        let mut malformed = namespace.clone();
        let arena = malformed.arenas.get_mut(arena_name).expect("owner arena");
        arena.push(arena.first().expect("owner record").clone());
        assert!(matches!(
            crate::native::CatiaNative::load(&malformed),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }

    let mut stale_design_objects = namespace.clone();
    stale_design_objects
        .arenas
        .get_mut("design_objects")
        .expect("derived design-object arena")
        .clear();
    assert!(matches!(
        crate::native::CatiaNative::load(&stale_design_objects),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_dangling_cross_arena_links() {
    let mut value_native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    value_native.value_blocks[0].catalog = "catia:missing-catalog".to_string();
    let mut value_namespace = cadmpeg_ir::NativeNamespace::default();
    value_native
        .store(&mut value_namespace)
        .expect("store malformed value link");
    assert!(matches!(
        crate::native::CatiaNative::load(&value_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut omitted_value_graph =
        crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    omitted_value_graph.value_blocks[0].object_graph = None;
    let mut omitted_value_namespace = cadmpeg_ir::NativeNamespace::default();
    omitted_value_graph
        .store(&mut omitted_value_namespace)
        .expect("store omitted value-block graph link");
    assert!(matches!(
        crate::native::CatiaNative::load(&omitted_value_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut external_native =
        crate::native::CatiaNative::decode(&external_reference_segment("Support.CATPart"));
    external_native.external_references[0].segment = "catia:missing-segment".to_string();
    let mut external_namespace = cadmpeg_ir::NativeNamespace::default();
    external_native
        .store(&mut external_namespace)
        .expect("store malformed external-reference link");
    assert!(matches!(
        crate::native::CatiaNative::load(&external_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut alias_native = crate::native::CatiaNative::decode(&surface_alias_stream());
    alias_native.alias_rows[0].object_graph = Some("catia:missing-graph".to_string());
    alias_native.alias_rows[0].object_record = Some("catia:missing-record".to_string());
    let mut alias_namespace = cadmpeg_ir::NativeNamespace::default();
    alias_native
        .store(&mut alias_namespace)
        .expect("store malformed alias link");
    assert!(matches!(
        crate::native::CatiaNative::load(&alias_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut linked_alias = surface_alias_stream();
    linked_alias[15] = 1;
    let mut linked_stream = graph;
    linked_stream.extend(linked_alias);
    let (linked_bytes, _) = outer_container_catpart(&linked_stream);
    let mut omitted_alias_links = crate::native::CatiaNative::decode(&linked_bytes);
    assert!(omitted_alias_links.alias_rows[0].object_graph.is_some());
    omitted_alias_links.alias_rows[0].object_graph = None;
    omitted_alias_links.alias_rows[0].object_record = None;
    let mut omitted_alias_namespace = cadmpeg_ir::NativeNamespace::default();
    omitted_alias_links
        .store(&mut omitted_alias_namespace)
        .expect("store omitted alias links");
    assert!(matches!(
        crate::native::CatiaNative::load(&omitted_alias_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_catalog_and_record_views() {
    let mut bytes = object_graph_stream();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);

    let mut invalid_count = native.clone();
    invalid_count.catalogs[0].declared_count += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_count
        .store(&mut namespace)
        .expect("store invalid catalog count");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_entry_ordinal = native.clone();
    invalid_entry_ordinal.catalogs[0].entries[0].ordinal = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_entry_ordinal
        .store(&mut namespace)
        .expect("store invalid catalog ordinal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_record_ordinal = native.clone();
    invalid_record_ordinal.object_graphs[0].records[0].ordinal = 9;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_record_ordinal
        .store(&mut namespace)
        .expect("store invalid record ordinal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_design_link = native.clone();
    invalid_design_link.object_graphs[0].records[0].design_object = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_design_link
        .store(&mut namespace)
        .expect("store invalid design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_references = native;
    invalid_references.object_graphs[0].records[0]
        .references
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_references
        .store(&mut namespace)
        .expect("store invalid payload-reference links");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_value_block_views() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    let mut canonical_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut canonical_namespace)
        .expect("store canonical value selections");
    assert!(canonical_namespace
        .arenas
        .get("value_blocks")
        .is_some_and(|blocks| blocks
            .iter()
            .all(|block| !block.fields().contains_key("schema_selections"))));
    assert_eq!(
        canonical_namespace
            .arenas
            .get("value_schema_selections")
            .map(Vec::len),
        Some(native.value_blocks[0].schema_selections.len())
    );
    let mut orphaned_selections: Vec<crate::native::CatiaValueSchemaSelection> =
        canonical_namespace
            .arena_as("value_schema_selections")
            .expect("load stored value selections");
    orphaned_selections[0].parent = "catia:missing-value-block".to_string();
    canonical_namespace
        .set_arena("value_schema_selections", &orphaned_selections)
        .expect("store orphaned value selection");
    assert!(matches!(
        crate::native::CatiaNative::load(&canonical_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed value-block view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut invalid_length = native.clone();
    invalid_length.value_blocks[0].declared_len += 1;
    assert_rejected(invalid_length);

    let mut invalid_payload = native.clone();
    invalid_payload.value_blocks[0].payload.push(0x80);
    assert_rejected(invalid_payload);

    let mut invalid_fields = native.clone();
    invalid_fields.value_blocks[0].fields.clear();
    assert_rejected(invalid_fields);

    let mut invalid_selections = native;
    assert!(!invalid_selections.value_blocks[0]
        .schema_selections
        .is_empty());
    invalid_selections.value_blocks[0].schema_selections.clear();
    assert_rejected(invalid_selections);
}

#[test]
fn native_load_rejects_noncanonical_entity_frame_lengths() {
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let native =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&records));

    for mutate in [
        |record: &mut crate::native::CatiaEntityRecord| record.definition_len += 1,
        |record: &mut crate::native::CatiaEntityRecord| record.value_len += 1,
        |record: &mut crate::native::CatiaEntityRecord| record.byte_len += 1,
    ] as [fn(&mut crate::native::CatiaEntityRecord); 3]
    {
        let mut malformed = native.clone();
        mutate(&mut malformed.entity_records[0]);
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed entity frame");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }
}

#[test]
fn native_namespace_retains_and_validates_definition_schema_selections() {
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes =
        entity_table_record_with_definition_and_value(1, &[0, 0, 0x32, 4, 0, 0, 0], &[]);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.entity_records[0].definition_schema_selections,
        [crate::native::CatiaDefinitionSchemaSelection {
            offset: 2,
            ordinal: 4,
            entry: Some(native.catalogs[0].entries[4].id.clone()),
            name: Some("Sketch".to_string()),
        }]
    );

    let mut malformed = native;
    malformed.entity_records[0].definition_schema_selections[0].name = Some("Pad".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed definition-schema view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_retains_and_validates_repeated_reference_suffixes() {
    let payload = [
        0xb0, 0x83, 0x81, 0xbc, 0x81, 0xbe, 0x81, 0xb1, 0x83, 0x81, 0xbc, 0x81, 0xbe, 0xd1, 0x80,
        0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)];
    let native = crate::native::CatiaNative::decode(&entity_backed_object_graph(&records, &[1]));
    let suffix = native.object_graphs[0].records[0]
        .repeated_reference_suffix
        .as_ref()
        .expect("repeated reference suffix");
    assert_eq!(suffix.schema_preamble, None);
    assert_eq!(suffix.repeated_references, [60, 62]);
    assert_eq!(suffix.terminal_reference, 49);

    let mut malformed = native;
    malformed.object_graphs[0].records[0]
        .repeated_reference_suffix
        .as_mut()
        .expect("repeated reference suffix")
        .terminal_reference += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed repeated-reference-suffix view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_resolves_and_validates_repeated_reference_schema_selections() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_repeated_reference_schema_selection(),
    );
    let selection = native.object_graphs[0].records[0]
        .repeated_reference_schema_selection
        .as_ref()
        .expect("reference schema selection");
    assert_eq!(
        selection.order,
        crate::native::CatiaRepeatedReferenceSchemaOrder::BlobThenSchema
    );
    assert_eq!(selection.ordinal, 4);
    assert_eq!(selection.offset, 67);
    assert_eq!(selection.name.as_deref(), Some("TargetSchema"));
    assert!(selection.entry.is_some());

    let mut malformed = native;
    malformed.object_graphs[0].records[0]
        .repeated_reference_schema_selection
        .as_mut()
        .expect("reference schema selection")
        .name = Some("WrongSchema".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-schema view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_retains_and_validates_complete_entity_numeric_pairs() {
    let value = [
        0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6, 0, 0, 0, 0, 0, 0, 0x12, 0x40, 0xe8,
        0xfe, 0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    let pair = native.entity_records[0]
        .numeric_pair
        .as_ref()
        .expect("complete numeric pair");
    assert_eq!(
        pair.slots,
        [
            crate::entity_table::NumericPairSlot::Binary64 {
                bits: 4.5_f64.to_bits(),
                offset: 8,
            },
            crate::entity_table::NumericPairSlot::ControlE8 { offset: 17 },
        ]
    );

    let mut legacy = native.clone();
    legacy.entity_records[0].numeric_pair = None;
    let mut legacy_namespace = cadmpeg_ir::NativeNamespace::default();
    legacy
        .store(&mut legacy_namespace)
        .expect("store legacy numeric-pair view");
    legacy_namespace.version = crate::native::CATIA_REFERENCE_SIGNATURE_COHORT_VERSION;
    let migrated =
        crate::native::CatiaNative::load(&legacy_namespace).expect("migrate numeric-pair view");
    assert!(migrated.entity_records[0].numeric_pair.is_some());

    let mut malformed = native;
    malformed.entity_records[0]
        .numeric_pair
        .as_mut()
        .expect("complete numeric pair")
        .slots[0] = crate::entity_table::NumericPairSlot::ControlE8 { offset: 8 };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed numeric-pair view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn decode_reports_complete_numeric_entity_value_pairs_separately_from_packets() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_numeric_entity_value_pair()),
            &DecodeOptions::default(),
        )
        .expect("decode complete numeric entity-value pair");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NUMERIC_ENTITY_VALUE_PAIR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NUMERIC_ENTITY_VALUE_PACKET_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("1 complete numeric entity-value pair(s)")
            && loss
                .message
                .contains("0 embedded numeric entity-value packet(s)")
    }));
}

#[test]
fn native_namespace_retains_and_validates_complete_entity_reference_signatures() {
    let value = [
        0x32, 3, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x85, 0x81, b'2', b'(', b'E', b')', 0xfe,
        0x32, 4, 0, 0, 0, 0x82, 0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
    ];
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
    ];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.extend(entity_table_record_with_value(2, &value));
    bytes.extend(entity_table_record_with_definition_and_value(
        3,
        &[0x01],
        &[0xfe],
    ));
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    let signature = native.entity_records[0]
        .reference_signature
        .as_ref()
        .expect("complete reference signature");
    assert_eq!(signature.production.first_reference, 3);
    assert_eq!(signature.first_entity.entity_id, 3);
    assert_eq!(
        signature.first_entity.entity.as_deref(),
        Some(native.entity_records[2].id.as_str())
    );
    assert!(!signature.first_entity.is_null);
    assert_eq!(signature.production.second_reference, 4);
    assert_eq!(signature.second_entity.entity_id, 4);
    assert!(signature.second_entity.entity.is_none());
    assert!(signature.second_entity.is_null);
    assert_eq!(signature.production.second_reference_offset, 17);
    assert_eq!(signature.production.signature, "2(E)");
    assert_eq!(signature.production.signature_offset, 12);
    let [cohort] = native.reference_signature_cohorts.as_slice() else {
        panic!("one reference-signature cohort");
    };
    let graph_key = cohort
        .parent
        .split_once('#')
        .expect("object graph identity")
        .1;
    assert_eq!(
        cohort.id,
        format!("catia:outer:reference-signature-cohort#{graph_key}:00000000")
    );
    assert_eq!(cohort.ordinal, 0);
    assert_eq!(cohort.first_reference, 3);
    assert_eq!(cohort.second_reference, 4);
    assert!(cohort.schema_selection.is_none());
    assert_eq!(
        cohort.members,
        [
            native.entity_records[0].id.clone(),
            native.entity_records[1].id.clone()
        ]
    );

    let expected = signature.clone();
    let expected_cohort = cohort.clone();
    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature incidences");
    stored.version = crate::native::CATIA_REFERENCE_SIGNATURE_INCIDENCE_VERSION - 1;
    let mut stored_fields = stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let stored_signature = stored_fields
        .get_mut("reference_signature")
        .expect("stored reference signature")
        .as_object_mut()
        .expect("stored reference-signature object");
    stored_signature.remove("signature_offset");
    stored_signature.remove("second_reference_offset");
    drop(stored_fields);
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate reference-signature incidences");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected.clone())
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store resolved reference-signature incidences");
    stored.version = crate::native::CATIA_REFERENCE_SIGNATURE_ENTITY_VERSION - 1;
    let mut stored_fields = stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let stored_signature = stored_fields
        .get_mut("reference_signature")
        .expect("stored reference signature")
        .as_object_mut()
        .expect("stored reference-signature object");
    stored_signature.remove("first_entity");
    stored_signature.remove("second_entity");
    drop(stored_fields);
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("resolve reference-signature incidences");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected.clone())
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature program");
    stored.version = crate::native::CATIA_REFERENCE_SIGNATURE_FRAME_VERSION - 1;
    let mut stored_fields = stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let stored_signature = stored_fields
        .get_mut("reference_signature")
        .expect("stored reference signature")
        .as_object_mut()
        .expect("stored reference-signature object");
    stored_signature.remove("prefix");
    stored_signature.remove("signature_program");
    drop(stored_fields);
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("parse reference-signature program");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected.clone())
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store consecutive reference-signature pair");
    stored.version = crate::native::CATIA_REFERENCE_SIGNATURE_PAIR_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("validate reference-signature pair");
    assert_eq!(
        migrated.entity_records[0].reference_signature,
        Some(expected)
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature schema incidence");
    stored.version = crate::native::CATIA_REFERENCE_SIGNATURE_SCHEMA_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("derive reference-signature schema");
    assert_eq!(
        migrated.reference_signature_cohorts.as_slice(),
        std::slice::from_ref(&expected_cohort)
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut stored)
        .expect("store reference-signature cohort");
    stored.version = crate::native::CATIA_REFERENCE_SIGNATURE_COHORT_VERSION - 1;
    stored.arenas.remove("reference_signature_cohorts");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("derive reference-signature cohort");
    assert_eq!(
        migrated.reference_signature_cohorts.as_slice(),
        std::slice::from_ref(&expected_cohort)
    );

    let mut file = standard_catpart();
    file.splice(16..16, bytes.clone());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode reference-signature incidences");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_2_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_35_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_COHORT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_MULTI_MEMBER_REFERENCE_SIGNATURE_COHORT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_COHORT_MEMBER_COUNT),
        2
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_REFERENCE_SIGNATURE_COHORT_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_INSTRUCTION_COUNT),
        8
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCE_SIGNATURE_TOKEN_COUNT),
        8
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_REFERENCE_SIGNATURE_ENTITY_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNRESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT),
        0
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .second_entity
        .entity_id += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(&bytes);
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .production
        .signature_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(&bytes);
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .production
        .signature_program
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature program");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(&bytes);
    malformed.reference_signature_cohorts[0].members.clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature cohort");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_tokenizes_and_validates_complete_entity_values() {
    let mut value = vec![0x32, 4, 0, 0, 0, 0x87, 0xe6];
    value.extend_from_slice(&12.5_f64.to_bits().to_le_bytes());
    value.extend_from_slice(&[0x87, 0xe8, 0xfe]);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.entity_records[0].value_fields,
        [
            crate::value_block::ValueField::SchemaSelector {
                ordinal: 4,
                offset: 0,
            },
            crate::value_block::ValueField::Binary64 {
                bits: 12.5_f64.to_bits(),
                offset: 5,
            },
            crate::value_block::ValueField::Marker {
                code: 0xe8,
                offset: 15,
            },
            crate::value_block::ValueField::Terminator { offset: 17 },
        ]
    );

    let mut malformed = native;
    malformed.entity_records[0].value_fields.pop();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed entity-value view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_resolves_and_validates_entity_value_schema_selections() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_entity_value_schema_selection());
    let selection = &native.entity_records[0].value_schema_selections[0];
    assert_eq!(selection.ordinal, 4);
    assert_eq!(selection.offset, 0);
    assert_eq!(selection.name, "TargetValue");
    assert!(!selection.entry.is_empty());
    assert_eq!(
        selection.encoded_value,
        [
            crate::value_block::ValueField::Binary64 {
                bits: 12.5_f64.to_bits(),
                offset: 5,
            },
            crate::value_block::ValueField::Opcode {
                code: 0xe8,
                offset: 15,
            },
            crate::value_block::ValueField::Atom {
                value: 3851,
                width: 2,
                offset: 16,
            },
            crate::value_block::ValueField::Separator { offset: 18 },
            crate::value_block::ValueField::Terminator { offset: 19 },
            crate::value_block::ValueField::Terminator { offset: 20 },
        ]
    );
    assert_eq!(
        selection.packets,
        [crate::entity_table::EntityValuePacket::Compact {
            offset: 15,
            value_selector: 0x0ae0,
        }]
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed entity-value schema view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut wrong_name = native.clone();
    wrong_name.entity_records[0].value_schema_selections[0].name = "WrongValue".to_string();
    assert_rejected(wrong_name);

    let mut wrong_packet = native;
    let crate::entity_table::EntityValuePacket::Compact { value_selector, .. } =
        &mut wrong_packet.entity_records[0].value_schema_selections[0].packets[0]
    else {
        panic!("compact value packet");
    };
    *value_selector += 1;
    assert_rejected(wrong_packet);
}

#[test]
fn native_namespace_types_and_validates_complete_relation_expressions() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression("param"));
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("complete relation expression");
    let crate::native::CatiaRelationExpressionFraming::PlaceholderState {
        placeholder,
        state_role,
    } = &expression.framing
    else {
        panic!("placeholder-state framing")
    };
    assert_eq!(placeholder.value, "#1_ ");
    assert_eq!(state_role.value, "opened");
    assert_eq!(expression.expression.value, "#1_ /2-2mm");
    assert_eq!(expression.parameter_role.value, "param");
    assert_eq!(
        expression.type_signature.value,
        "(#1_ : #In LENGTH) : LENGTH"
    );
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature.inputs,
        [crate::native::CatiaRelationTypeInput {
            parameter: "#1_".to_string(),
            input_type: "LENGTH".to_string(),
        }]
    );
    assert_eq!(signature.result_type, "LENGTH");
    assert_eq!(expression.function_role.value, "RelationExpFct");

    let mut malformed = native;
    malformed.entity_records[0]
        .relation_expression
        .as_mut()
        .expect("complete relation expression")
        .expression
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed relation expression");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_parser_version_relation_expression("Boolean", "ParserVersion"),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::BooleanParserVersion {
        prefix_role,
        parser_version_role,
    } = &expression.framing
    else {
        panic!("parser-version framing")
    };
    assert_eq!(prefix_role.value, "Boolean");
    assert_eq!(
        expression.expression.value,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2"
    );
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(expression.parameter_role.value, "param");
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature
            .inputs
            .iter()
            .map(|input| (input.parameter.as_str(), input.input_type.as_str()))
            .collect::<Vec<_>>(),
        [("#1_", "LENGTH"), ("#2_", "LENGTH")]
    );
    assert_eq!(signature.result_type, "Real");
}

#[test]
fn opened_parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_opened_parser_version_relation_expression(
            "Boolean",
            "ParserVersion",
            "opened",
        ),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("opened parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
        prefix_role,
        parser_version_role,
        state_role,
    } = &expression.framing
    else {
        panic!("opened parser-version framing")
    };
    assert_eq!(prefix_role.value, "Boolean");
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(state_role.value, "opened");
    assert_eq!(
        expression.expression.value,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2"
    );
    assert_eq!(expression.parameter_role.value, "param");
    assert!(expression.signature.is_some());
}

#[test]
fn opened_parser_version_relation_expression_requires_every_exact_role() {
    for (prefix_role, parser_version_role, state_role) in [
        ("Real", "ParserVersion", "opened"),
        ("Boolean", "ParserRevision", "opened"),
        ("Boolean", "ParserVersion", "closed"),
    ] {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_opened_parser_version_relation_expression(
                prefix_role,
                parser_version_role,
                state_role,
            ),
        );

        assert!(native.entity_records[0].relation_expression.is_none());
    }
}

#[test]
fn decode_retains_an_opened_parser_version_expression_without_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_opened_parser_version_relation_expression(
                    "Boolean",
                    "ParserVersion",
                    "opened",
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode opened parser-version expression");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_OPENED_BOOLEAN_PARSER_VERSION_RELATION_EXPRESSION_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_TYPED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
}

#[test]
fn unprefixed_parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_unprefixed_parser_version_relation_expression("ParserVersion"),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("unprefixed parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::ParserVersion {
        parser_version_role,
    } = &expression.framing
    else {
        panic!("unprefixed parser-version framing")
    };
    assert_eq!(expression.expression.value, "360.0*1 deg/#1_");
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(expression.parameter_role.value, "param");
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature.inputs,
        [crate::native::CatiaRelationTypeInput {
            parameter: "#1_".to_string(),
            input_type: "Integer".to_string(),
        }]
    );
    assert_eq!(signature.result_type, "ANGLE");
}

#[test]
fn unprefixed_parser_version_relation_expression_requires_the_exact_version_role() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_unprefixed_parser_version_relation_expression("ParserRevision"),
    );

    assert!(native.entity_records[0].relation_expression.is_none());
}

#[test]
fn decode_retains_an_unprefixed_parser_version_expression_without_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_unprefixed_parser_version_relation_expression(
                    "ParserVersion",
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unprefixed parser-version expression");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_PARSER_VERSION_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_TYPED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
}

#[test]
fn relation_program_instance_requires_the_complete_identity_frame() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(1, 1, 1, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("complete instance frame");
    assert_eq!(
        instance.framing,
        crate::native::CatiaRelationProgramInstanceFraming::Lead12
    );
    assert_eq!(instance.program_entity.entity_id, 1);
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(instance.program_entity.class_name.as_deref(), Some("body"));
    assert_eq!(instance.repeated_entity.entity_id, 1);
    assert_eq!(
        instance.repeated_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(instance.repeated_entity.class_name.as_deref(), Some("body"));
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.reference.entity_id)
            .collect::<Vec<_>>(),
        [20, 21, 23, 25, 1, 1, 21, 27]
    );
    assert_eq!(
        instance.reference_incidences[4]
            .reference
            .class_name
            .as_deref(),
        Some("body")
    );
    assert_eq!(
        instance.reference_incidences[5]
            .reference
            .class_name
            .as_deref(),
        Some("body")
    );
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.payload_offset)
            .collect::<Vec<_>>(),
        [0, 10, 40, 50, 60, 65, 75, 85]
    );
    assert_eq!(
        instance.relation_expression.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance
            .parameter_dependencies
            .iter()
            .map(|dependency| dependency.symbol.as_str())
            .collect::<Vec<_>>(),
        ["#1_", "#2_", "#2_"]
    );
    assert_eq!(
        instance
            .parameter_dependencies
            .iter()
            .map(|dependency| dependency.source_offset)
            .collect::<Vec<_>>(),
        [19, 23, 28]
    );
    assert!(instance
        .parameter_dependencies
        .iter()
        .all(|dependency| dependency.candidates.is_empty()));
    assert!(instance.inputs.is_none());
    let context = instance
        .lead12_context_entity
        .as_ref()
        .expect("lead-12 context entity");
    assert_eq!(context.entity_id, 1);
    assert_eq!(
        context.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(context.class_name.as_deref(), Some("body"));
    assert!(instance.output_entity.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(2, 1, 3, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("resolved non-expression program");
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert!(instance.relation_expression.is_none());
    let context = instance
        .lead12_context_entity
        .as_ref()
        .expect("lead-12 context entity");
    assert_eq!(context.entity_id, 3);
    assert!(context.entity.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(3, 3, 1, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("unresolved program identity");
    assert!(instance.program_entity.entity.is_none());
    assert_eq!(instance.repeated_entity.entity_id, 3);
    assert!(instance.repeated_entity.entity.is_none());
    assert!(instance.relation_expression.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(1, 1, 1, 3),
    );
    assert!(native
        .entity_records
        .iter()
        .all(|entity| entity.relation_program_instance.is_none()));
}

#[test]
fn relation_program_output_selects_only_the_framing_specific_paramout_slot() {
    let lead12 = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance_class(1, 1, 1, 2, "paramout"),
    );
    let lead12_instance = lead12.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-12 relation-program instance");
    assert_eq!(
        lead12_instance.output_entity,
        lead12_instance.lead12_context_entity
    );
    assert_eq!(
        lead12_instance
            .output_entity
            .as_ref()
            .and_then(|output| output.class_name.as_deref()),
        Some("paramout")
    );
    assert!(lead12_instance.lead54_trailing_entity.is_none());
    let lead12_decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_relation_program_instance_class(
                1, 1, 1, 2, "paramout",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode lead-12 paramout relation-program instance");
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_RELATION_PROGRAM_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        lead12_decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_RELATION_PROGRAM_OUTPUT_COUNT),
        0
    );

    let lead12_body = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance_class(1, 1, 1, 2, "body"),
    );
    assert!(lead12_body.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-12 body relation-program instance")
        .output_entity
        .is_none());

    let lead54 = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance_class(1, 1, 1, 2, "paramout"),
    );
    let lead54_instance = lead54.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-54 relation-program instance");
    assert_eq!(
        lead54_instance.output_entity,
        lead54_instance.lead54_trailing_entity
    );
    assert_eq!(
        lead54_instance
            .output_entity
            .as_ref()
            .and_then(|output| output.class_name.as_deref()),
        Some("paramout")
    );
    assert!(lead54_instance.lead12_context_entity.is_none());

    let lead54_body = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance_class(1, 1, 1, 2, "body"),
    );
    assert!(lead54_body.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("lead-54 body relation-program instance")
        .output_entity
        .is_none());
}

#[test]
fn relation_program_inputs_require_complete_unique_signature_bindings() {
    let signature = crate::native::CatiaRelationTypeSignature {
        inputs: vec![
            crate::native::CatiaRelationTypeInput {
                parameter: "#1_".to_string(),
                input_type: "LENGTH".to_string(),
            },
            crate::native::CatiaRelationTypeInput {
                parameter: "#2_".to_string(),
                input_type: "Real".to_string(),
            },
        ],
        result_type: "Real".to_string(),
    };
    let reference = |entity_id: u32| crate::native::CatiaEntityReference {
        entity_id,
        is_null: false,
        entity: Some(format!("entity-{entity_id}")),
        class_name: Some("param".to_string()),
    };
    let dependency =
        |symbol: &str, candidates: Vec<_>| crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: symbol.to_string(),
            candidates,
        };
    let complete = vec![
        dependency("#1_", vec![reference(10)]),
        dependency("#1_ /2", vec![reference(10)]),
        dependency("#2_", vec![reference(11)]),
    ];
    let inputs = crate::native::resolved_relation_program_inputs(&signature, &complete)
        .expect("complete ordered input bindings");
    assert_eq!(
        inputs
            .iter()
            .map(|input| (input.parameter.as_str(), input.entity.entity_id))
            .collect::<Vec<_>>(),
        [("#1_", 10), ("#2_", 11)]
    );

    let compact_ordinal = vec![
        dependency("#1_", vec![reference(10)]),
        dependency("#1_/2", vec![reference(10)]),
        dependency("#2_", vec![reference(11)]),
    ];
    assert!(
        crate::native::resolved_relation_program_inputs(&signature, &compact_ordinal).is_some()
    );

    let zero = crate::native::CatiaRelationTypeSignature {
        inputs: Vec::new(),
        result_type: "Real".to_string(),
    };
    assert_eq!(
        crate::native::resolved_relation_program_inputs(&zero, &[]),
        Some(Vec::new())
    );
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[dependency("#1_", vec![reference(10)])]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#2_", vec![reference(10)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#1_ /2", vec![reference(12)]),
            dependency("#2_", vec![reference(11)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#1_ /ordinal", vec![reference(10)]),
            dependency("#2_", vec![reference(11)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10), reference(12)]),
            dependency("#2_", vec![reference(11)])
        ]
    )
    .is_none());
    assert!(crate::native::resolved_relation_program_inputs(
        &signature,
        &[
            dependency("#1_", vec![reference(10)]),
            dependency("#2_", vec![reference(11)]),
            dependency("#3_", vec![reference(12)])
        ]
    )
    .is_none());
}

#[test]
fn complete_relation_program_inputs_transfer_typed_parameters() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let parameter_entity = native.entity_records[2].clone();
    native.entity_records[0].formula_relation = None;
    native.entity_records[0].relation_program_instance =
        Some(crate::native::CatiaRelationProgramInstance {
            framing: crate::native::CatiaRelationProgramInstanceFraming::Lead12,
            program_entity: crate::native::CatiaEntityReference::default(),
            repeated_entity: crate::native::CatiaEntityReference::default(),
            reference_incidences: Vec::new(),
            relation_expression: None,
            parameter_dependencies: Vec::new(),
            inputs: Some(vec![crate::native::CatiaRelationProgramInput {
                parameter: "#1_".to_string(),
                value_type: "LENGTH".to_string(),
                entity: crate::native::CatiaEntityReference {
                    entity_id: parameter_entity.entity_id,
                    is_null: false,
                    entity: Some(parameter_entity.id.clone()),
                    class_name: Some("param".to_string()),
                },
            }]),
            output_entity: None,
            lead12_context_entity: None,
            lead54_trailing_entity: None,
        });

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = Annotations::default();
    let transfer = crate::formula::transfer_parameters(&mut ir, &native, &mut annotations, None);
    let [parameter] = ir.model.parameters.as_slice() else {
        panic!("one relation-program input parameter")
    };
    assert_eq!(transfer.relation_program_parameter_count, 1);
    assert_eq!(parameter.name, "Thickness");
    assert_eq!(parameter.expression, "35 mm");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(
        parameter.properties.get("value_type").map(String::as_str),
        Some("LENGTH")
    );
    assert_eq!(
        parameter
            .properties
            .get("catia_binding")
            .map(String::as_str),
        Some("#1_ /2")
    );
    assert_eq!(parameter.native_ref, Some(parameter_entity.id.clone()));

    let mut empty_binding_native = native.clone();
    empty_binding_native.entity_records[2]
        .parameter_value
        .as_mut()
        .expect("complete input parameter")
        .binding
        .value
        .clear();
    let mut empty_binding_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let empty_binding_transfer = crate::formula::transfer_parameters(
        &mut empty_binding_ir,
        &empty_binding_native,
        &mut Annotations::default(),
        None,
    );
    let [empty_binding_parameter] = empty_binding_ir.model.parameters.as_slice() else {
        panic!("one empty-binding input parameter")
    };
    assert_eq!(empty_binding_transfer.relation_program_parameter_count, 1);
    assert_eq!(
        empty_binding_parameter
            .properties
            .get("catia_binding")
            .map(String::as_str),
        Some("")
    );

    let mut conflicting_native = native.clone();
    let mut conflicting_instance = conflicting_native.entity_records[0]
        .relation_program_instance
        .clone()
        .expect("complete relation-program instance");
    conflicting_instance
        .inputs
        .as_mut()
        .expect("complete relation-program inputs")[0]
        .value_type = "Real".to_string();
    conflicting_native.entity_records[1].relation_program_instance = Some(conflicting_instance);
    let mut conflicting_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let conflicting_transfer = crate::formula::transfer_parameters(
        &mut conflicting_ir,
        &conflicting_native,
        &mut Annotations::default(),
        None,
    );
    assert_eq!(conflicting_transfer.relation_program_parameter_count, 0);
    assert!(conflicting_ir.model.parameters.is_empty());
}

#[test]
fn complete_relation_program_output_transfers_a_typed_result() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let expression_entity = native.entity_records[1].clone();
    let input_entity = native.entity_records[2].clone();
    let output_entity = native.entity_records[3].clone();
    native.entity_records[0].formula_relation = None;
    native.entity_records[0].relation_program_instance =
        Some(crate::native::CatiaRelationProgramInstance {
            framing: crate::native::CatiaRelationProgramInstanceFraming::Lead12,
            program_entity: crate::native::CatiaEntityReference::default(),
            repeated_entity: crate::native::CatiaEntityReference::default(),
            reference_incidences: Vec::new(),
            relation_expression: Some(expression_entity.id.clone()),
            parameter_dependencies: Vec::new(),
            inputs: Some(vec![crate::native::CatiaRelationProgramInput {
                parameter: "#1_".to_string(),
                value_type: "LENGTH".to_string(),
                entity: crate::native::CatiaEntityReference {
                    entity_id: input_entity.entity_id,
                    is_null: false,
                    entity: Some(input_entity.id.clone()),
                    class_name: Some("param".to_string()),
                },
            }]),
            output_entity: Some(crate::native::CatiaEntityReference {
                entity_id: output_entity.entity_id,
                is_null: false,
                entity: Some(output_entity.id.clone()),
                class_name: Some("paramout".to_string()),
            }),
            lead12_context_entity: None,
            lead54_trailing_entity: None,
        });

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = Annotations::default();
    let transfer = crate::formula::transfer_parameters(&mut ir, &native, &mut annotations, None);
    let [input, output] = ir.model.parameters.as_slice() else {
        panic!("typed relation-program input and output")
    };
    assert_eq!(transfer.relation_program_parameter_count, 1);
    assert_eq!(input.name, "Thickness");
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(output.name, "Result");
    assert_eq!(output.expression, "#1_ /2-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.properties["catia_binding"], "#result_ /1");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.native_ref, Some(output_entity.id));

    let mut ambiguous_native = native;
    let duplicate_program = ambiguous_native.entity_records[0]
        .relation_program_instance
        .clone()
        .expect("compound relation-program instance");
    ambiguous_native.entity_records[1].relation_program_instance = Some(duplicate_program);
    let mut ambiguous_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let ambiguous_transfer = crate::formula::transfer_parameters(
        &mut ambiguous_ir,
        &ambiguous_native,
        &mut Annotations::default(),
        None,
    );
    let [ambiguous_input] = ambiguous_ir.model.parameters.as_slice() else {
        panic!("ambiguous compound output keeps its typed input")
    };
    assert_eq!(ambiguous_transfer.relation_program_parameter_count, 1);
    assert_eq!(ambiguous_input.name, "Thickness");
}

#[test]
fn lead54_relation_program_instance_requires_its_complete_identity_frame() {
    let file = standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 2);
    let native = crate::native::CatiaNative::decode(&file);
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("complete lead-54 instance frame");
    assert_eq!(
        instance.framing,
        crate::native::CatiaRelationProgramInstanceFraming::Lead54
    );
    assert!(instance.lead12_context_entity.is_none());
    let trailing = instance
        .lead54_trailing_entity
        .as_ref()
        .expect("lead-54 trailing entity");
    assert_eq!(trailing.entity_id, 1);
    assert_eq!(
        trailing.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(trailing.class_name.as_deref(), Some("body"));
    assert!(instance.output_entity.is_none());
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance.repeated_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.reference.entity_id)
            .collect::<Vec<_>>(),
        [5, 20, 1, 21, 5]
    );
    assert_eq!(
        instance.reference_incidences[2]
            .reference
            .class_name
            .as_deref(),
        Some("body")
    );
    assert_eq!(
        instance
            .reference_incidences
            .iter()
            .map(|incidence| incidence.payload_offset)
            .collect::<Vec<_>>(),
        [10, 35, 55, 60, 70]
    );
    assert_eq!(
        instance.relation_expression.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance
            .parameter_dependencies
            .iter()
            .map(|dependency| dependency.symbol.as_str())
            .collect::<Vec<_>>(),
        ["#1_", "#2_", "#2_"]
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode lead-54 relation-program instance");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT),
        3
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT
        ),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_TYPED_RELATION_PROGRAM_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DISTINCT_RELATION_PROGRAM_INPUT_ENTITY_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_RELATION_PROGRAM_INPUT_PARAMETER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT
        ),
        0
    );

    let unresolved = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance(1, 3, 3, 2),
    );
    let instance = unresolved.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("unresolved repeated identity");
    assert_eq!(instance.repeated_entity.entity_id, 3);
    assert!(instance.repeated_entity.entity.is_none());
    let trailing = instance
        .lead54_trailing_entity
        .as_ref()
        .expect("lead-54 trailing entity");
    assert_eq!(trailing.entity_id, 3);
    assert!(trailing.entity.is_none());

    let malformed = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 3),
    );
    assert!(malformed
        .entity_records
        .iter()
        .all(|entity| entity.relation_program_instance.is_none()));
}

#[test]
fn decode_reports_exact_relation_program_instances() {
    for (
        program_entity_id,
        repeated_reference_entity_id,
        resolved,
        expression,
        other,
        unresolved,
        resolved_repeated,
    ) in [
        (1, 1, 1, 1, 0, 0, 1),
        (2, 1, 1, 0, 1, 0, 1),
        (3, 1, 0, 0, 0, 0, 1),
        (1, 3, 1, 1, 0, 0, 0),
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_relation_program_instance(
                    program_entity_id,
                    repeated_reference_entity_id,
                    1,
                    2,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode relation-program instance");
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_RELATION_PROGRAM_INSTANCE_COUNT),
            1
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            8
        );
        let resolved_reference_incidences = 1 + usize::from(repeated_reference_entity_id == 1);
        let null_reference_incidences = usize::from(repeated_reference_entity_id == 3);
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            resolved_reference_incidences
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_NULL_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            null_reference_incidences
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            8 - resolved_reference_incidences - null_reference_incidences
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT
            ),
            resolved_reference_incidences
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT),
            1
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
            ),
            1
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
            ),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT
            ),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT
            ),
            1
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT
            ),
            0
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INSTANCE_COUNT),
            resolved
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RELATION_EXPRESSION_PROGRAM_INSTANCE_COUNT
            ),
            expression
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_OTHER_RELATION_PROGRAM_INSTANCE_COUNT),
            other
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::UNRESOLVED_RELATION_PROGRAM_INSTANCE_COUNT),
            unresolved,
            "program entity {program_entity_id}"
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_NULL_RELATION_PROGRAM_INSTANCE_COUNT),
            usize::from(program_entity_id == 3)
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT
            ),
            resolved_repeated
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT
            ),
            usize::from(repeated_reference_entity_id > 3)
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_NULL_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT
            ),
            usize::from(repeated_reference_entity_id == 3)
        );
        let classified_program = usize::from(program_entity_id <= 2);
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT),
            classified_program
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT),
            1 - classified_program
        );
        let classified_repeated = usize::from(repeated_reference_entity_id == 1);
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT
            ),
            classified_repeated
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT
            ),
            1 - classified_repeated
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_INSTANCED_RELATION_EXPRESSION_COUNT),
            expression
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
            expression
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT
            ),
            0
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT
            ),
            expression
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
            1 - expression
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT
            ),
            expression * 3
        );
        assert_eq!(
            decoded.report().coverage_count(
                crate::coverage::UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT
            ),
            expression * 3
        );
        assert_eq!(
            decoded
                .report()
                .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
            0
        );
        assert!(decoded.ir().model.parameters.is_empty());
    }
}

#[test]
fn native_load_derives_relation_program_instances_from_older_namespaces() {
    for native in [
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_program_instance(
            1, 1, 1, 2,
        )),
        crate::native::CatiaNative::decode(
            &standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 2),
        ),
    ] {
        let expected = native.entity_records[1]
            .relation_program_instance
            .clone()
            .expect("decoded relation-program instance");
        let mut stored = cadmpeg_ir::NativeNamespace::default();
        native
            .store(&mut stored)
            .expect("store older relation-program namespace");
        for (version, remove_context, remove_trailing, remove_framing) in [
            (
                crate::native::CATIA_RELATION_PROGRAM_INPUT_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_TYPED_REFERENCE_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_CONTEXT_VERSION - 1,
                true,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_CONTEXT_VERSION - 2,
                true,
                true,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_INSTANCE_VERSION,
                true,
                true,
                true,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_INSTANCE_VERSION - 1,
                true,
                true,
                true,
            ),
        ] {
            let mut namespace = stored.clone();
            namespace.version = version;
            let mut stored_fields = namespace
                .arenas
                .get_mut("entity_records")
                .expect("stored entity records")[1]
                .fields_mut();
            let stored_instance = stored_fields
                .get_mut("relation_program_instance")
                .expect("stored relation-program field")
                .as_object_mut()
                .expect("stored relation-program instance");
            if remove_context {
                stored_instance.remove("lead12_context_entity");
            }
            if remove_trailing {
                stored_instance.remove("lead54_trailing_entity");
            }
            if remove_framing {
                stored_instance.remove("framing");
            }
            stored_instance.remove("output_entity");
            stored_instance.remove("inputs");
            stored_instance.remove("reference_incidences");
            stored_instance.remove("parameter_dependencies");
            stored_instance.remove("program_entity");
            stored_instance.remove("repeated_entity");
            for field in ["lead12_context_entity", "lead54_trailing_entity"] {
                if let Some(reference) = stored_instance
                    .get_mut(field)
                    .and_then(|value| value.as_object_mut())
                {
                    reference.remove("class_name");
                }
            }

            drop(stored_fields);
            let migrated = crate::native::CatiaNative::load(&namespace)
                .expect("migrate relation-program instance");
            assert_eq!(
                migrated.entity_records[1]
                    .relation_program_instance
                    .as_ref(),
                Some(&expected)
            );
        }

        let mut namespace = stored.clone();
        namespace.version = crate::native::CATIA_RELATION_REFERENCE_OFFSET_VERSION - 1;
        let mut stored_fields = namespace
            .arenas
            .get_mut("entity_records")
            .expect("stored entity records")[1]
            .fields_mut();
        let incidences = stored_fields
            .get_mut("relation_program_instance")
            .expect("stored relation-program field")
            .as_object_mut()
            .expect("stored relation-program instance")
            .get_mut("reference_incidences")
            .expect("stored reference incidences")
            .as_array_mut()
            .expect("stored reference incidences");
        for incidence in incidences {
            *incidence =
                incidence.as_object().expect("stored reference incidence")["reference"].clone();
        }
        drop(stored_fields);
        let migrated = crate::native::CatiaNative::load(&namespace)
            .expect("migrate relation-program reference offsets");
        assert_eq!(
            migrated.entity_records[1]
                .relation_program_instance
                .as_ref(),
            Some(&expected)
        );

        let mut namespace = stored.clone();
        namespace.version = crate::native::CATIA_RELATION_DEPENDENCY_OFFSET_VERSION - 1;
        let mut stored_fields = namespace
            .arenas
            .get_mut("entity_records")
            .expect("stored entity records")[1]
            .fields_mut();
        let dependencies = stored_fields
            .get_mut("relation_program_instance")
            .expect("stored relation-program field")
            .as_object_mut()
            .expect("stored relation-program instance")
            .get_mut("parameter_dependencies")
            .expect("stored parameter dependencies")
            .as_array_mut()
            .expect("stored parameter dependencies");
        for dependency in dependencies {
            dependency
                .as_object_mut()
                .expect("stored parameter dependency")
                .remove("source_offset");
        }
        drop(stored_fields);
        let migrated = crate::native::CatiaNative::load(&namespace)
            .expect("migrate relation-program dependency offsets");
        assert_eq!(
            migrated.entity_records[1]
                .relation_program_instance
                .as_ref(),
            Some(&expected)
        );

        let mut malformed_dependencies = native.clone();
        malformed_dependencies.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .parameter_dependencies[0]
            .symbol = "#999_".to_string();
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed_dependencies
            .store(&mut namespace)
            .expect("store malformed relation-program dependencies");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));

        let mut malformed_inputs = native.clone();
        malformed_inputs.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .inputs = Some(Vec::new());
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed_inputs
            .store(&mut namespace)
            .expect("store malformed relation-program inputs");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));

        let mut malformed_offset = native.clone();
        malformed_offset.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .reference_incidences[0]
            .payload_offset = u64::MAX;
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed_offset
            .store(&mut namespace)
            .expect("store malformed relation-program incidence offset");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));

        let mut malformed = native;
        malformed.entity_records[1]
            .relation_program_instance
            .as_mut()
            .expect("decoded relation-program instance")
            .reference_incidences[0]
            .reference
            .entity_id = u32::MAX;
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed relation-program incidences");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }
}

#[test]
fn native_load_rederives_relation_program_paramout_outputs_from_older_namespaces() {
    for native in [
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_program_instance_class(
            1, 1, 1, 2, "paramout",
        )),
        crate::native::CatiaNative::decode(
            &standard_catpart_with_lead54_relation_program_instance_class(1, 1, 1, 2, "paramout"),
        ),
    ] {
        let expected = native.entity_records[1]
            .relation_program_instance
            .clone()
            .expect("decoded paramout relation-program instance");
        assert!(expected.output_entity.is_some());
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        native
            .store(&mut namespace)
            .expect("store paramout relation-program instance");
        namespace.version = crate::native::CATIA_RELATION_PROGRAM_OUTPUT_VERSION - 1;
        namespace
            .arenas
            .get_mut("entity_records")
            .expect("stored entity records")[1]
            .fields_mut()
            .get_mut("relation_program_instance")
            .expect("stored relation-program field")
            .as_object_mut()
            .expect("stored relation-program instance")
            .remove("output_entity");

        let migrated = crate::native::CatiaNative::load(&namespace)
            .expect("migrate paramout relation-program output");
        assert_eq!(
            migrated.entity_records[1]
                .relation_program_instance
                .as_ref(),
            Some(&expected)
        );
    }
}

#[test]
fn schema_configuration_productions_retain_exact_same_graph_incidence() {
    let file = standard_catpart_with_configuration_incidences(8, 5, 7);
    let native = crate::native::CatiaNative::decode(&file);
    let configuration = native.entity_records[0]
        .schema_configuration_record
        .as_ref()
        .expect("complete schema-configuration production");
    assert_eq!(configuration.schema_ordinal, 8);
    assert_eq!(configuration.schema_name, "Boolean");
    assert_eq!(configuration.schema_payload_offset, 0);
    assert_eq!(configuration.entity_reference.payload_offset, 10);
    assert_eq!(configuration.entity_reference.reference.entity_id, 5);
    assert_eq!(
        configuration.entity_reference.reference.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        configuration
            .entity_reference
            .reference
            .class_name
            .as_deref(),
        Some("Configuration")
    );
    let row = native.entity_records[1]
        .schema_configuration_row_link
        .as_ref()
        .expect("complete configrow production");
    assert_eq!(row.class_reference.entity_id, 6);
    assert_eq!(
        row.class_reference.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(row.class_reference.class_name.as_deref(), Some("configrow"));
    assert_eq!(row.successor_payload_offset, 5);
    assert_eq!(row.successor.entity_id, 7);
    assert_eq!(
        row.successor.entity.as_deref(),
        Some(native.entity_records[2].id.as_str())
    );
    assert_eq!(row.successor.class_name.as_deref(), Some("body"));
    assert_eq!(native.schema_configuration_row_chains.len(), 1);
    let chain = &native.schema_configuration_row_chains[0];
    assert_eq!(chain.object_graph, native.entity_records[1].object_graph);
    let graph_key = chain
        .object_graph
        .split_once('#')
        .expect("object graph identity")
        .1;
    assert_eq!(
        chain.id,
        format!("catia:outer:schema-configuration-row-chain#{graph_key}:6")
    );
    assert_eq!(chain.links.len(), 1);
    assert_eq!(chain.links[0].row, row.class_reference);
    assert_eq!(
        chain.links[0].successor_payload_offset,
        row.successor_payload_offset
    );
    assert_eq!(
        chain
            .links
            .iter()
            .map(|link| link.row.entity_id)
            .collect::<Vec<_>>(),
        [6]
    );
    assert_eq!(
        chain.links[0].row.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(chain.links[0].successor, row.successor);
    assert!(native.entity_records[2]
        .schema_configuration_record
        .is_none());
    assert!(native.entity_records[2]
        .schema_configuration_row_link
        .is_none());

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode configuration incidences");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_CONFIGURATION_RECORD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_CONFIGURATION_SELECTOR_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNCLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONFIGURATION_COUNT),
        0
    );
    assert!(decoded.ir().model.configurations.is_empty());
}

#[test]
fn schema_configuration_row_chain_retains_complete_source_order() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_schema_configuration_row_chain());
    assert_eq!(native.schema_configuration_row_chains.len(), 1);
    let chain = &native.schema_configuration_row_chains[0];
    assert_eq!(chain.links[0].row.entity_id, 5);
    assert_eq!(
        chain
            .links
            .iter()
            .map(|link| link.row.entity_id)
            .collect::<Vec<_>>(),
        [5, 7, 9]
    );
    assert!(chain
        .links
        .iter()
        .all(|link| link.row.class_name.as_deref() == Some("configrow")));
    assert_eq!(
        chain
            .links
            .iter()
            .map(|link| link.successor_payload_offset)
            .collect::<Vec<_>>(),
        [5, 5, 5]
    );
    assert_eq!(
        chain
            .links
            .iter()
            .map(|link| {
                link.intervening_entities
                    .as_ref()
                    .expect("source-ordered row interval")
                    .iter()
                    .map(|entity| entity.entity_id)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![6], vec![8], vec![10]]
    );
    assert!(chain
        .links
        .iter()
        .flat_map(|link| {
            link.intervening_entities
                .as_ref()
                .expect("source-ordered row interval")
        })
        .all(|reference| reference.class_name.as_deref() == Some("body")));
    assert_eq!(chain.links[2].successor.entity_id, 11);
    assert_eq!(chain.links[2].successor.class_name.as_deref(), Some("body"));

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_schema_configuration_row_chain()),
            &DecodeOptions::default(),
        )
        .expect("decode configuration row intervals");
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_ENTITY_COUNT
        ),
        3
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_SCHEMA_CONFIGURATION_COUNT
        ),
        0
    );
}

#[test]
fn schema_configuration_productions_preserve_unresolved_identities() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 15, 16),
    );
    let configuration = native.entity_records[0]
        .schema_configuration_record
        .as_ref()
        .expect("complete schema-configuration production");
    assert_eq!(configuration.schema_name, "Boolean");
    assert!(configuration.entity_reference.reference.entity.is_none());
    let row = native.entity_records[1]
        .schema_configuration_row_link
        .as_ref()
        .expect("complete configrow production");
    assert!(row.successor.entity.is_none());

    let mismatched_schema = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(14, 15, 16),
    );
    assert!(mismatched_schema.entity_records[0]
        .schema_configuration_record
        .is_none());

    let mut malformed = standard_catpart_with_configuration_incidences(8, 15, 16);
    let marker = [0x80, 250, 0, 0, 0];
    let offset = malformed
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("configrow marker");
    malformed[offset + 1] = 249;
    let malformed = crate::native::CatiaNative::decode(&malformed);
    assert!(malformed
        .entity_records
        .iter()
        .all(|entity| entity.schema_configuration_row_link.is_none()));

    let cyclic_file = standard_catpart_with_configuration_incidences(8, 15, 6);
    let cyclic_native = crate::native::CatiaNative::decode(&cyclic_file);
    assert!(cyclic_native.schema_configuration_row_chains.is_empty());
    let cyclic = CatiaCodec
        .decode(&mut Cursor::new(cyclic_file), &DecodeOptions::default())
        .expect("decode cyclic configuration row");
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT),
        0
    );
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT),
        0
    );
    assert_eq!(
        cyclic
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT),
        1
    );

    let descending = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 15, 5),
    );
    assert_eq!(descending.schema_configuration_row_chains.len(), 1);
    assert!(descending.schema_configuration_row_chains[0].links[0]
        .intervening_entities
        .is_none());
}

#[test]
fn schema_configuration_productions_distinguish_terminal_null_identities() {
    let file = standard_catpart_with_configuration_incidences(8, 8, 8);
    let native = crate::native::CatiaNative::decode(&file);
    let configuration = native.entity_records[0]
        .schema_configuration_record
        .as_ref()
        .expect("complete schema-configuration production");
    assert!(configuration.entity_reference.reference.is_null);
    assert!(configuration.entity_reference.reference.entity.is_none());
    let row = native.entity_records[1]
        .schema_configuration_row_link
        .as_ref()
        .expect("complete configrow production");
    assert!(!row.class_reference.is_null);
    assert!(row.successor.is_null);
    assert!(row.successor.entity.is_none());
    assert_eq!(native.schema_configuration_row_chains.len(), 1);
    assert!(
        native.schema_configuration_row_chains[0].links[0]
            .successor
            .is_null
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode terminal-null configuration incidences");
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT
        ),
        0
    );
}

#[test]
fn native_load_migrates_and_validates_configuration_incidences() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 5, 7),
    );
    let mut legacy_named = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut legacy_named)
        .expect("store schema-configuration namespace");
    let entity = legacy_named
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")
        .first_mut()
        .expect("stored schema-configuration entity");
    let mut fields = entity.fields_mut();
    let configuration = fields
        .remove("schema_configuration_record")
        .expect("stored schema-configuration record");
    fields.insert("configuration_record".to_string(), configuration);
    drop(fields);
    let row_entity = legacy_named
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")
        .get_mut(1)
        .expect("stored schema-configuration-row entity");
    let mut fields = row_entity.fields_mut();
    let row_link = fields
        .remove("schema_configuration_row_link")
        .expect("stored schema-configuration-row link");
    fields.insert("configuration_row_link".to_string(), row_link);
    drop(fields);
    let row_chains = legacy_named
        .arenas
        .remove("schema_configuration_row_chains")
        .expect("stored schema-configuration-row chains");
    legacy_named
        .arenas
        .insert("configuration_row_chains".to_string(), row_chains);
    legacy_named.version = crate::native::CATIA_SCHEMA_CONFIGURATION_NAMING_VERSION - 1;
    let chain = legacy_named
        .arenas
        .get_mut("configuration_row_chains")
        .expect("stored legacy-named schema-configuration-row chains")
        .first_mut()
        .expect("stored legacy-named schema-configuration-row chain");
    let legacy_id = chain.id().replace(
        ":schema-configuration-row-chain#",
        ":configuration-row-chain#",
    );
    let fields = chain.fields();
    *chain = cadmpeg_ir::NativeRecord::new(legacy_id, fields);
    let loaded = crate::native::CatiaNative::load(&legacy_named)
        .expect("load legacy-named schema-configuration incidences");
    assert_eq!(
        loaded.entity_records[0].schema_configuration_record,
        native.entity_records[0].schema_configuration_record
    );
    assert_eq!(
        loaded.entity_records[1].schema_configuration_row_link,
        native.entity_records[1].schema_configuration_row_link
    );
    assert_eq!(
        loaded.schema_configuration_row_chains,
        native.schema_configuration_row_chains
    );

    let mut older = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut older)
        .expect("store configuration namespace");
    older.version = crate::native::CATIA_SCHEMA_CONFIGURATION_REFERENCE_VERSION - 1;
    for entity in older
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")
    {
        let id = entity.id().to_owned();
        let mut fields = entity.fields();
        fields.remove("schema_configuration_record");
        fields.remove("schema_configuration_row_link");
        *entity = cadmpeg_ir::NativeRecord::new(id, fields);
    }
    let migrated =
        crate::native::CatiaNative::load(&older).expect("migrate configuration incidences");
    assert_eq!(
        migrated.entity_records[0].schema_configuration_record,
        native.entity_records[0].schema_configuration_record
    );
    assert_eq!(
        migrated.entity_records[1].schema_configuration_row_link,
        native.entity_records[1].schema_configuration_row_link
    );
    assert_eq!(
        migrated.schema_configuration_row_chains,
        native.schema_configuration_row_chains
    );

    let mut version_250 = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_250)
        .expect("store configuration payload offsets");
    let entities = version_250
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records");
    let mut stored_fields = entities[0].fields_mut();
    let configuration = stored_fields
        .get_mut("schema_configuration_record")
        .expect("stored schema-configuration record")
        .as_object_mut()
        .expect("stored configuration object");
    configuration.remove("schema_payload_offset");
    let entity_reference = configuration["entity_reference"]
        .as_object()
        .expect("stored configuration incidence")["reference"]
        .clone();
    configuration.insert("entity_reference".to_string(), entity_reference);
    drop(stored_fields);
    entities[1]
        .fields()
        .get_mut("schema_configuration_row_link")
        .expect("stored schema-configuration-row link")
        .as_object_mut()
        .expect("stored schema-configuration-row object")
        .remove("successor_payload_offset");
    version_250.version = crate::native::CATIA_CONFIGURATION_PAYLOAD_OFFSET_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_250)
        .expect("migrate configuration payload offsets");
    assert_eq!(
        migrated.entity_records[0].schema_configuration_record,
        native.entity_records[0].schema_configuration_record
    );
    assert_eq!(
        migrated.entity_records[1].schema_configuration_row_link,
        native.entity_records[1].schema_configuration_row_link
    );

    let interval_native =
        crate::native::CatiaNative::decode(&standard_catpart_with_schema_configuration_row_chain());
    let mut older = cadmpeg_ir::NativeNamespace::default();
    interval_native
        .store(&mut older)
        .expect("store pre-interval configuration namespace");
    older.version = crate::native::CATIA_SCHEMA_CONFIGURATION_ROW_INTERVAL_VERSION - 1;
    for chain in older
        .arenas
        .get_mut("schema_configuration_row_chains")
        .expect("stored schema-configuration-row chains")
    {
        let id = chain.id().to_owned();
        let mut fields = chain.fields();
        for link in fields
            .get_mut("links")
            .expect("stored schema-configuration-row links")
            .as_array_mut()
            .expect("stored schema-configuration-row links")
        {
            link.as_object_mut()
                .expect("stored schema-configuration-row link")
                .remove("intervening_entities");
        }
        *chain = cadmpeg_ir::NativeRecord::new(id, fields);
    }
    let migrated = crate::native::CatiaNative::load(&older)
        .expect("migrate schema-configuration-row successor intervals");
    assert_eq!(
        migrated.schema_configuration_row_chains,
        interval_native.schema_configuration_row_chains
    );

    let mut older = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut older)
        .expect("store pre-chain configuration namespace");
    older.version = crate::native::CATIA_SCHEMA_CONFIGURATION_ROW_CHAIN_VERSION - 1;
    older.arenas.remove("schema_configuration_row_chains");
    let migrated =
        crate::native::CatiaNative::load(&older).expect("migrate schema-configuration-row chains");
    assert_eq!(
        migrated.schema_configuration_row_chains,
        native.schema_configuration_row_chains
    );

    let mut version_254 = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_254)
        .expect("store pre-link-incidence configuration namespace");
    for chain in version_254
        .arenas
        .get_mut("schema_configuration_row_chains")
        .expect("stored schema-configuration-row chains")
    {
        let id = chain.id().to_owned();
        let mut fields = chain.fields();
        fields.remove("links");
        *chain = cadmpeg_ir::NativeRecord::new(id, fields);
    }
    version_254.version = crate::native::CATIA_SCHEMA_CONFIGURATION_ROW_LINK_INCIDENCE_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_254)
        .expect("migrate schema-configuration-row link incidences");
    assert_eq!(
        migrated.schema_configuration_row_chains,
        native.schema_configuration_row_chains
    );

    let mut expected_nulls = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 8, 8),
    );
    let mut stale_nulls = expected_nulls.clone();
    let configuration = stale_nulls.entity_records[0]
        .schema_configuration_record
        .as_mut()
        .expect("complete schema-configuration production");
    configuration.entity_reference.reference.is_null = false;
    let row = stale_nulls.entity_records[1]
        .schema_configuration_row_link
        .as_mut()
        .expect("complete configrow production");
    row.successor.is_null = false;
    stale_nulls.schema_configuration_row_chains[0].links[0]
        .successor
        .is_null = false;
    let mut version_239 = cadmpeg_ir::NativeNamespace::default();
    stale_nulls
        .store(&mut version_239)
        .expect("store pre-null-incidence namespace");
    version_239.version = crate::native::CATIA_TYPED_INCIDENCE_NULL_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&version_239).expect("migrate incidence null states");
    expected_nulls.version = migrated.version;
    assert_eq!(migrated, expected_nulls);

    let mut malformed_chain = native.clone();
    malformed_chain.schema_configuration_row_chains[0].links[0]
        .successor
        .entity_id = 6;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_chain
        .store(&mut current)
        .expect("store malformed configuration chain");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_chain_offset = native.clone();
    malformed_chain_offset.schema_configuration_row_chains[0].links[0].successor_payload_offset +=
        1;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_chain_offset
        .store(&mut current)
        .expect("store malformed configuration-chain offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_offsets = native.clone();
    let configuration = malformed_offsets.entity_records[0]
        .schema_configuration_record
        .as_mut()
        .expect("decoded schema-configuration record");
    configuration.schema_payload_offset += 1;
    configuration.entity_reference.payload_offset += 1;
    malformed_offsets.entity_records[1]
        .schema_configuration_row_link
        .as_mut()
        .expect("decoded configrow link")
        .successor_payload_offset += 1;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_offsets
        .store(&mut current)
        .expect("store malformed configuration offsets");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_intervals = interval_native;
    malformed_intervals.schema_configuration_row_chains[0].links[0]
        .intervening_entities
        .as_mut()
        .expect("source-ordered row interval")[0]
        .entity_id = 8;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_intervals
        .store(&mut current)
        .expect("store malformed schema-configuration-row intervals");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[1]
        .schema_configuration_row_link
        .as_mut()
        .expect("decoded configrow link")
        .successor
        .entity_id = 6;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut current)
        .expect("store malformed current namespace");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn parser_version_relation_expression_requires_both_exact_framing_roles() {
    for (prefix_role, parser_version_role) in
        [("Real", "ParserVersion"), ("Boolean", "ParserRevision")]
    {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_parser_version_relation_expression(
                prefix_role,
                parser_version_role,
            ),
        );

        assert!(native.entity_records[0].relation_expression.is_none());
    }
}

#[test]
fn decode_retains_a_parser_version_expression_without_fabricating_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parser_version_relation_expression(
                "Boolean",
                "ParserVersion",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode parser-version expression");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_RELATION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        0
    );
}

#[test]
fn relation_expression_signature_preserves_ordered_typed_inputs() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ :  #In LENGTH,#2_ :  #In ANGLE) : Real",
        ));
    let signature = native.entity_records[0]
        .relation_expression
        .as_ref()
        .and_then(|expression| expression.signature.as_ref())
        .expect("multi-input signature");

    assert_eq!(
        signature.inputs,
        [
            crate::native::CatiaRelationTypeInput {
                parameter: "#1_".to_string(),
                input_type: "LENGTH".to_string(),
            },
            crate::native::CatiaRelationTypeInput {
                parameter: "#2_".to_string(),
                input_type: "ANGLE".to_string(),
            },
        ]
    );
    assert_eq!(signature.result_type, "Real");
}

#[test]
fn relation_expression_signature_accepts_an_empty_input_list_with_an_empty_placeholder() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "", "() : LENGTH"),
    );
    let signature = native.entity_records[0]
        .relation_expression
        .as_ref()
        .and_then(|expression| expression.signature.as_ref())
        .expect("zero-input signature");

    assert!(signature.inputs.is_empty());
    assert_eq!(signature.result_type, "LENGTH");

    let nonempty_placeholder = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "#1_ ", "() : LENGTH"),
    );
    assert!(nonempty_placeholder.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_requires_exact_outer_whitespace() {
    for signature in [
        "( ) : LENGTH",
        "() :  LENGTH",
        "() : LENGTH ",
        "() : LENGTH\n\n",
    ] {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_relation_expression_signature("param", "", signature),
        );

        assert!(
            native.entity_records[0]
                .relation_expression
                .as_ref()
                .expect("relation expression")
                .signature
                .is_none(),
            "{signature:?}"
        );
    }
}

#[test]
fn native_migrates_and_validates_relation_signature_outer_whitespace() {
    let mut native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "", "( ) : LENGTH"),
    );
    let entity = &mut native.entity_records[0];
    let expression = entity
        .relation_expression
        .as_mut()
        .expect("relation expression");
    assert!(expression.signature.is_none());
    expression.signature = Some(crate::native::CatiaRelationTypeSignature {
        inputs: Vec::new(),
        result_type: "LENGTH".to_string(),
    });

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store pre-canonical signature");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    namespace.version = crate::native::CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate signature whitespace");
    assert!(migrated.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_rejects_duplicate_inputs() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ : #In LENGTH,#1_ : #In ANGLE) : Real",
        ));

    assert!(native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_requires_canonical_parameter_symbols() {
    for parameter in ["value", "#_", "#1", "#1_ /2", "#１_"] {
        let signature = format!("({parameter} : #In LENGTH) : Real");
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_relation_expression_signature("param", parameter, &signature),
        );

        assert!(
            native.entity_records[0]
                .relation_expression
                .as_ref()
                .expect("relation expression")
                .signature
                .is_none(),
            "{parameter}"
        );
    }
}

#[test]
fn native_migrates_and_validates_relation_signature_parameter_symbols() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ : #In LENGTH) : Real",
        ));
    let expected = native.entity_records[0]
        .relation_expression
        .clone()
        .expect("relation expression");
    let mut malformed = native;
    malformed.entity_records[0]
        .relation_expression
        .as_mut()
        .expect("relation expression")
        .signature
        .as_mut()
        .expect("typed signature")
        .inputs[0]
        .parameter = "value".to_string();

    let mut current_namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut current_namespace)
        .expect("store malformed relation signature");
    assert!(matches!(
        crate::native::CatiaNative::load(&current_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    current_namespace.version = crate::native::CATIA_RELATION_SIGNATURE_PARAMETER_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&current_namespace)
        .expect("migrate relation signature parameters");
    assert_eq!(
        migrated.entity_records[0].relation_expression,
        Some(expected)
    );
}

#[test]
fn relation_expression_requires_every_exact_role() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression("parameter"));

    assert!(native.entity_records[0].relation_expression.is_none());
}

#[test]
fn relation_expression_signature_requires_the_selected_placeholder() {
    let mut file = standard_catpart_with_relation_expression("param");
    let signature = file
        .windows("(#1_ : #In LENGTH) : LENGTH".len())
        .position(|bytes| bytes == b"(#1_ : #In LENGTH) : LENGTH")
        .expect("relation type signature");
    file[signature + 2] = b'2';

    let native = crate::native::CatiaNative::decode(&file);
    assert!(native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("complete relation expression")
        .signature
        .is_none());
}

#[test]
fn native_namespace_types_and_validates_named_parameter_values() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer, CatiaEntitySuffixValue,
    };

    let scalar = 35.0_f64.to_bits();
    let mut scalar_suffix = vec![0x85, 0x96, 0x82, 0x6a, 0xe6];
    scalar_suffix.extend_from_slice(&scalar.to_le_bytes());
    scalar_suffix.extend_from_slice(&[0x81, 0x52]);
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&scalar_suffix));
    let parameter = native.entity_records[0]
        .parameter_value
        .as_ref()
        .expect("complete named parameter value");
    assert_eq!(parameter.name.value, "Thickness");
    assert_eq!(parameter.binding.value, "#1_ /2");
    assert_eq!(
        parameter.evaluation,
        CatiaEntityEvaluation::Scalar { bits: scalar }
    );
    assert_eq!(
        native.entity_records[0].suffix_value,
        Some(CatiaEntitySuffixValue {
            prefix_atoms: [5, 22, 2],
            prefix_atom_widths: [1, 1, 1],
            prefix_code: 0x6a,
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 4,
                evaluation: CatiaEntityEvaluation::Scalar { bits: scalar },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            trailer: CatiaEntitySuffixTrailer::Token8152,
        })
    );
    assert_eq!(parameter.evaluation_opcode_offset, 4);

    let unset = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x85, 0x96, 0x82, 0x6a, 0xe7, 0x81, 0x52,
    ]));
    assert_eq!(
        unset.entity_records[0]
            .parameter_value
            .as_ref()
            .expect("complete unset parameter")
            .evaluation,
        CatiaEntityEvaluation::Unset
    );

    let mut stale_offsets = native.clone();
    let CatiaEntitySuffixPayload::Evaluation { opcode_offset, .. } = &mut stale_offsets
        .entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete named parameter suffix")
        .payload
    else {
        panic!("named parameter evaluation");
    };
    *opcode_offset = 0;
    stale_offsets.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .evaluation_opcode_offset = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    stale_offsets
        .store(&mut namespace)
        .expect("store stale named parameter offsets");
    namespace.version = crate::native::CATIA_SUFFIX_EVALUATION_OFFSET_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate named parameter offsets");
    assert_eq!(
        migrated.entity_records[0].parameter_value,
        native.entity_records[0].parameter_value
    );

    let mut malformed_offset = native.clone();
    malformed_offset.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .evaluation_opcode_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_offset
        .store(&mut namespace)
        .expect("store malformed named parameter offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .name
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed parameter value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_types_dimension_constraint_ranges() {
    use crate::native::{CatiaConstraintRangeFraming, CatiaEntityEvaluation};

    let scalar = 128.0_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xc1, 0xe6];
    suffix.extend_from_slice(&scalar.to_le_bytes());
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let range = native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete dimension constraint range");
    assert!(!range.range.entry.is_empty());
    assert_eq!(range.range.value, "Range");
    assert!(!range.constraint.entry.is_empty());
    assert_eq!(range.constraint.value, "CstAttr_Dimension");
    assert_eq!(range.framing, CatiaConstraintRangeFraming::DimensionC1);
    assert_eq!(
        range.evaluation,
        CatiaEntityEvaluation::Scalar { bits: scalar }
    );
    assert_eq!(range.evaluation_opcode_offset, 4);

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode constraint range");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert!(!decoded
        .report()
        .coverage
        .contains_key("decoded_structurally_owned_constraint_range_count"));
    assert!(!decoded
        .report()
        .coverage
        .contains_key("unresolved_constraint_range_owner_count"));

    let referenced_file = |reference_count: usize, storage_reference: bool| {
        let value = [
            0x32, 4, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x07, 0x37, 0x81, 0xfe, 0x32, 5, 0, 0, 0, 0xfe,
        ];
        let mut range_entity = entity_table_record_with_definition_and_value(1, &[0x01], &value);
        range_entity[6] = 2;
        range_entity.extend_from_slice(&suffix);
        let range_len = u32::try_from(range_entity.len()).expect("bounded range entity");
        range_entity[2..6].copy_from_slice(&range_len.to_le_bytes());
        let mut stream = range_entity;
        stream.extend(entity_table_record_with_definition_and_value(
            2,
            &[0x01],
            &[0xfe],
        ));
        let mut reference_payload = [0x81, 0x81].repeat(reference_count);
        reference_payload.push(0xfe);
        let reference_head = if storage_reference {
            vec![0x04, 0x01, 0x82, 0x84, 0x81]
        } else {
            vec![0x04, 0x01, 0x82, 0x84]
        };
        stream.push(0xde);
        stream.extend(object_graph_from_records(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
            object_graph_record(&reference_head, &reference_payload),
        ]));
        stream.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            "Range",
            "CstAttr_Dimension",
        ]));
        let mut file = standard_catpart();
        file.splice(16..16, stream);
        let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
        file[8..12].copy_from_slice(&be32(file_len));
        file
    };
    let unique_file = referenced_file(1, false);
    let unique_native = crate::native::CatiaNative::decode(&unique_file);
    let incoming = &unique_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete referenced constraint range")
        .incoming_references;
    assert_eq!(
        unique_native.entity_records[0]
            .range_interval
            .as_ref()
            .expect("complete referenced range interval")
            .incoming_references
            .as_slice(),
        incoming.as_slice()
    );
    assert_eq!(incoming.len(), 1);
    assert_eq!(
        incoming[0].object_record,
        unique_native.object_graphs[0].records[1].id
    );
    let source_entity = incoming[0]
        .source_entity
        .as_ref()
        .expect("source record has a paired entity");
    assert_eq!(source_entity.entity_id, 2);
    assert_eq!(
        source_entity.entity.as_deref(),
        Some(unique_native.entity_records[1].id.as_str())
    );
    assert_eq!(
        source_entity.class_name,
        unique_native.object_graphs[0].records[1].class_name
    );
    assert_eq!(
        incoming[0].payload_offset,
        unique_native.object_graphs[0].records[1].references[0].payload_offset
    );
    assert_eq!(
        incoming[0].source,
        unique_native.object_graphs[0].records[1].references[0].source
    );

    let uniquely_referenced = CatiaCodec
        .decode(&mut Cursor::new(unique_file), &DecodeOptions::default())
        .expect("decode uniquely referenced constraint range");
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        uniquely_referenced.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT
        ),
        usize::from(source_entity.class_name.is_some())
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT),
        usize::from(source_entity.class_name.is_none())
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_INCOMING_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        uniquely_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_RANGE_INTERVAL_COUNT),
        1
    );

    let storage_file = referenced_file(0, true);
    let storage_native = crate::native::CatiaNative::decode(&storage_file);
    let incoming_storage = &storage_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete storage-referenced constraint range")
        .incoming_storage_references;
    assert_eq!(
        storage_native.entity_records[0]
            .range_interval
            .as_ref()
            .expect("complete storage-referenced range interval")
            .incoming_storage_references
            .as_slice(),
        incoming_storage.as_slice()
    );
    assert_eq!(incoming_storage.len(), 1);
    assert_eq!(
        incoming_storage[0].object_record,
        storage_native.object_graphs[0].records[1].id
    );
    let storage_source_entity = incoming_storage[0]
        .source_entity
        .as_ref()
        .expect("storage source has a paired entity");
    assert_eq!(storage_source_entity.entity_id, 2);
    assert_eq!(
        storage_source_entity.entity.as_deref(),
        Some(storage_native.entity_records[1].id.as_str())
    );
    assert_eq!(
        storage_source_entity.class_name,
        storage_native.object_graphs[0].records[1].class_name
    );

    let storage_referenced = CatiaCodec
        .decode(&mut Cursor::new(storage_file), &DecodeOptions::default())
        .expect("decode storage-referenced constraint range");
    assert_eq!(
        storage_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        storage_referenced.report().coverage_count(
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_PAYLOAD_REFERENCE_COUNT
        ),
        0
    );
    assert_eq!(
        storage_referenced.report().coverage_count(
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_STORAGE_REFERENCE_COUNT
        ),
        1
    );
    assert_eq!(
        storage_referenced
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        storage_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );

    let combined = CatiaCodec
        .decode(
            &mut Cursor::new(referenced_file(1, true)),
            &DecodeOptions::default(),
        )
        .expect("decode constraint range with both incidence forms");
    assert_eq!(
        combined
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        2
    );
    assert_eq!(
        combined
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );

    let multiple_file = referenced_file(2, false);
    let multiple_native = crate::native::CatiaNative::decode(&multiple_file);
    let incoming = &multiple_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete multiply referenced constraint range")
        .incoming_references;
    assert_eq!(incoming.len(), 2);
    assert_eq!(
        incoming
            .iter()
            .map(|reference| reference.payload_offset)
            .collect::<Vec<_>>(),
        multiple_native.object_graphs[0].records[1]
            .references
            .iter()
            .map(|reference| reference.payload_offset)
            .collect::<Vec<_>>()
    );

    let multiply_referenced = CatiaCodec
        .decode(&mut Cursor::new(multiple_file), &DecodeOptions::default())
        .expect("decode multiply referenced constraint range");
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT),
        2
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        multiply_referenced
            .report()
            .coverage_count(crate::coverage::MULTIPLY_REFERENCED_RANGE_INTERVAL_COUNT),
        1
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete dimension constraint range")
        .framing = CatiaConstraintRangeFraming::DimensionB8;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint range");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(
        &standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix),
    );
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete dimension constraint range")
        .constraint
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint role");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = unique_native.clone();
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete referenced constraint range")
        .incoming_references[0]
        .payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint-range incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = unique_native.clone();
    malformed.entity_records[0]
        .range_interval
        .as_mut()
        .expect("complete referenced range interval")
        .incoming_references[0]
        .payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed range-interval incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = storage_native.clone();
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete storage-referenced constraint range")
        .incoming_storage_references[0]
        .object_record = unique_native.object_graphs[0].records[0].id.clone();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint-range storage incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    unique_native
        .store(&mut stored)
        .expect("store older constraint-range namespace");
    stored.version = crate::native::CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION - 1;
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .remove("incoming_references");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate constraint-range incidence");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_references
            .len(),
        1
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    unique_native
        .store(&mut stored)
        .expect("store older range-interval incidence namespace");
    stored.version = crate::native::CATIA_RANGE_INTERVAL_INCIDENCE_VERSION - 1;
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("range_interval")
        .expect("stored range interval")
        .as_object_mut()
        .expect("stored range-interval object")
        .remove("incoming_references");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate range-interval incidence");
    assert_eq!(
        migrated.entity_records[0]
            .range_interval
            .as_ref()
            .expect("migrated range interval")
            .incoming_references
            .len(),
        1
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    unique_native
        .store(&mut stored)
        .expect("store older constraint-range source namespace");
    stored.version = crate::native::CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION - 1;
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .get_mut("incoming_references")
        .expect("stored incoming references")
        .as_array_mut()
        .expect("stored incoming-reference array")[0]
        .as_object_mut()
        .expect("stored incoming-reference object")
        .remove("source_entity");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate constraint-range source entity");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_references[0]
            .source_entity,
        unique_native.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("source constraint range")
            .incoming_references[0]
            .source_entity
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    storage_native
        .store(&mut stored)
        .expect("store older constraint-range storage namespace");
    stored.version = crate::native::CATIA_CONSTRAINT_RANGE_STORAGE_INCIDENCE_VERSION - 1;
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .remove("incoming_storage_references");
    let migrated = crate::native::CatiaNative::load(&stored)
        .expect("migrate constraint-range storage incidence");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_storage_references,
        storage_native.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("source constraint range")
            .incoming_storage_references
    );
}

#[test]
fn native_namespace_types_and_validates_range_intervals_independently_of_constraint_roles() {
    use crate::entity_table::{RangeIntervalPrefix, RangeIntervalSlot};
    use crate::native::CatiaRangeNominalFraming;

    let lower_bits = (-0.2032_f64).to_bits();
    let upper_bits = 0.2032_f64.to_bits();
    let mut encoded_range = vec![0x87, 0xe8, 0xe0, 0x07, 0x37, 0x83, 0x81, 0xe6];
    encoded_range.extend_from_slice(&lower_bits.to_le_bytes());
    encoded_range.push(0xe6);
    encoded_range.extend_from_slice(&upper_bits.to_le_bytes());
    encoded_range.extend_from_slice(&[0xfe, 0xfe]);
    let nominal_bits = 6.35_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xdc, 0xe6];
    suffix.extend_from_slice(&nominal_bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0xdb]);

    let file = standard_catpart_with_range_interval(&encoded_range, &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let entity = &native.entity_records[0];
    assert!(entity.constraint_range.is_none());
    let range = entity
        .range_interval
        .as_ref()
        .expect("complete schema-selected range interval");
    assert_eq!(range.range.value, "Range");
    assert_eq!(
        range.interval.prefix,
        RangeIntervalPrefix::Compact { value: 7, width: 1 }
    );
    assert_eq!(
        range.interval.slots,
        Some([
            RangeIntervalSlot::Binary64 {
                bits: lower_bits,
                offset: 12,
            },
            RangeIntervalSlot::Binary64 {
                bits: upper_bits,
                offset: 21,
            },
        ])
    );
    let nominal = range.nominal.as_ref().expect("finite Range nominal");
    assert_eq!(nominal.framing, CatiaRangeNominalFraming::DCToken81DB);
    assert_eq!(nominal.bits, nominal_bits);
    assert_eq!(nominal.evaluation_opcode_offset, 4);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode range interval");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NO_SLOT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NOMINAL_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_FINITE_SLOT_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT),
        0
    );
    let no_slot = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_range_interval(
                &[0x82, 0xe8, 0xe0, 0x07, 0x37, 0x81, 0xfe],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode no-slot range interval");
    assert_eq!(
        no_slot
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NO_SLOT_COUNT),
        1
    );
    assert_eq!(
        no_slot
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_NOMINAL_COUNT),
        0
    );
    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_range_interval(
                &[
                    0x80, 0x6e, 0x89, 1, 0, 0xe8, 0xe0, 0x07, 0x37, 0x83, 0x81, 0xe8, 0xe8, 0xfe,
                    0xfe,
                ],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset range interval");
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT),
        2
    );

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store range-interval namespace");
    previous_namespace.version = crate::native::CATIA_RANGE_INTERVAL_VERSION - 1;
    previous_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut()
        .remove("range_interval");
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate range-interval production");
    assert_eq!(
        migrated.entity_records[0].range_interval,
        Some(range.clone())
    );

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store pre-nominal range namespace");
    previous_namespace.version = crate::native::CATIA_RANGE_NOMINAL_VERSION - 1;
    previous_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut()
        .get_mut("range_interval")
        .expect("stored range interval")
        .as_object_mut()
        .expect("stored range interval object")
        .remove("nominal");
    let migrated =
        crate::native::CatiaNative::load(&previous_namespace).expect("migrate Range nominal");
    assert_eq!(
        migrated.entity_records[0]
            .range_interval
            .as_ref()
            .expect("migrated range interval")
            .nominal,
        Some(nominal.clone())
    );

    let mut malformed_nominal = native.clone();
    malformed_nominal.entity_records[0]
        .range_interval
        .as_mut()
        .expect("complete range interval")
        .nominal
        .as_mut()
        .expect("finite Range nominal")
        .bits = 12.0_f64.to_bits();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_nominal
        .store(&mut namespace)
        .expect("store malformed Range nominal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .range_interval
        .as_mut()
        .expect("complete range interval")
        .interval
        .prefix = RangeIntervalPrefix::Compact { value: 8, width: 1 };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed range interval");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn dimension_constraint_ranges_accept_db_terminated_dc_frames() {
    use crate::native::{
        CatiaConstraintRangeFraming, CatiaEntityEvaluation, CatiaEntitySuffixTrailer,
    };

    let bits = 15.875_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xdc, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0xdb]);
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
        "Range",
        "CstAttr_Dimension",
        &suffix,
    ));
    let entity = &native.entity_records[0];
    let range = entity
        .constraint_range
        .as_ref()
        .expect("DB-terminated dimension range");
    assert_eq!(range.framing, CatiaConstraintRangeFraming::DimensionDC);
    assert_eq!(range.evaluation, CatiaEntityEvaluation::Scalar { bits });
    assert_eq!(
        entity
            .suffix_value
            .as_ref()
            .expect("DB-terminated suffix value")
            .trailer,
        CatiaEntitySuffixTrailer::Token81DB
    );

    for suffix in [
        {
            let mut suffix = vec![0x84, 0x96, 0x82, 0xd8, 0xe6];
            suffix.extend_from_slice(&bits.to_le_bytes());
            suffix.extend_from_slice(&[0x81, 0xdb]);
            suffix
        },
        vec![0x84, 0x96, 0x82, 0xdc, 0xe7],
        vec![0x84, 0x96, 0x82, 0xc1, 0xe7, 0x81, 0xdb],
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            "Range",
            "CstAttr_Dimension",
            &suffix,
        ));
        assert!(native.entity_records[0].constraint_range.is_none());
    }
}

#[test]
fn entity_suffix_values_accept_8193_trailers() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer,
    };

    let bits = 11.0_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xd8, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x93]);
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
        "Range",
        "CstAttr_Dimension",
        &suffix,
    ));
    let suffix_value = native.entity_records[0]
        .suffix_value
        .as_ref()
        .expect("81 93-terminated suffix value");
    assert_eq!(suffix_value.prefix_code, 0xd8);
    assert_eq!(suffix_value.trailer, CatiaEntitySuffixTrailer::Token8193);
    assert_eq!(
        suffix_value.payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 4,
            evaluation: CatiaEntityEvaluation::Scalar { bits },
            encoding: CatiaEntityEvaluationEncoding::Direct,
        }
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store 81 93-terminated suffix value");
    namespace.version = crate::native::CATIA_SUFFIX_TRAILER_8193_VERSION - 1;
    namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut()
        .remove("suffix_value");
    let migrated = crate::native::CatiaNative::load(&namespace)
        .expect("migrate 81 93-terminated suffix value");
    assert_eq!(
        migrated.entity_records[0].suffix_value.as_ref(),
        Some(suffix_value)
    );
}

#[test]
fn constraint_range_requires_an_exact_role_and_framing_pair() {
    use crate::native::CatiaConstraintRangeFraming;

    for (constraint, code, expected) in [
        (
            "CstAttr_Dimension",
            0xb8,
            CatiaConstraintRangeFraming::DimensionB8,
        ),
        ("ComplexCst", 0xc9, CatiaConstraintRangeFraming::ComplexC9),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            "Range",
            constraint,
            &[0x84, 0x96, 0x82, code, 0xe7],
        ));
        assert_eq!(
            native.entity_records[0]
                .constraint_range
                .as_ref()
                .expect("complete constraint range")
                .framing,
            expected
        );
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_two_selector_value(
                "Range",
                "ComplexCst",
                &[0x84, 0x96, 0x82, 0xc9, 0xe7],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset complex constraint range");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_CONSTRAINT_RANGE_COUNT),
        1
    );

    for (range, constraint, code) in [
        ("Tolerance", "CstAttr_Dimension", 0xc1),
        ("Range", "ComplexCst", 0xc1),
        ("Range", "CstAttr_Dimension", 0xc9),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            range,
            constraint,
            &[0x84, 0x96, 0x82, code, 0xe7],
        ));
        assert!(native.entity_records[0].constraint_range.is_none());
    }
}

#[test]
fn native_namespace_types_and_validates_generic_entity_suffix_values() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer, CatiaEntitySuffixValue,
    };

    let bits = 0.1_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xad, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode generic entity suffix");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_SCHEMA_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load generic entity suffix");

    assert_eq!(native.entity_records[0].parameter_value, None);
    assert_eq!(
        native.entity_records[0].suffix_value,
        Some(CatiaEntitySuffixValue {
            prefix_atoms: [4, 22, 2],
            prefix_atom_widths: [1, 1, 1],
            prefix_code: 0xad,
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 4,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            trailer: CatiaEntitySuffixTrailer::Token8149,
        })
    );
    let mut stale_evaluation_offset = native.clone();
    let CatiaEntitySuffixPayload::Evaluation { opcode_offset, .. } = &mut stale_evaluation_offset
        .entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete scalar suffix")
        .payload
    else {
        panic!("scalar suffix evaluation");
    };
    *opcode_offset = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    stale_evaluation_offset
        .store(&mut namespace)
        .expect("store stale evaluation offset");
    namespace.version = crate::native::CATIA_SUFFIX_EVALUATION_OFFSET_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate suffix evaluation offset");
    assert_eq!(
        migrated.entity_records[0].suffix_value,
        native.entity_records[0].suffix_value
    );

    let mut malformed_evaluation_offset = native.clone();
    let CatiaEntitySuffixPayload::Evaluation { opcode_offset, .. } =
        &mut malformed_evaluation_offset.entity_records[0]
            .suffix_value
            .as_mut()
            .expect("complete scalar suffix")
            .payload
    else {
        panic!("scalar suffix evaluation");
    };
    *opcode_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_evaluation_offset
        .store(&mut namespace)
        .expect("store malformed evaluation offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wide_scalar_bits = 0.001_f64.to_bits();
    let mut wide_scalar_suffix = vec![0xd1, 0x53, 0x96, 0x82, 0xa6, 0xe6];
    wide_scalar_suffix.extend_from_slice(&wide_scalar_bits.to_le_bytes());
    let wide_scalar = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&wide_scalar_suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode wide-prefix scalar suffix");
    assert_eq!(
        wide_scalar
            .report()
            .coverage_count(crate::coverage::DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let wide_scalar = crate::native::CatiaNative::load(
        wide_scalar
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load wide-prefix scalar suffix");
    assert_eq!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .prefix_atoms,
        [84, 22, 2]
    );
    assert_eq!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .prefix_atom_widths,
        [2, 1, 1]
    );
    assert!(matches!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 5,
            ..
        }
    ));
    let mut malformed_wide_scalar = wide_scalar;
    malformed_wide_scalar.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete wide-prefix scalar")
        .prefix_atom_widths[0] = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_wide_scalar
        .store(&mut namespace)
        .expect("store malformed wide-prefix scalar");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wide_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49,
        ]));
    assert!(matches!(
        wide_control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix control"),
        CatiaEntitySuffixValue {
            prefix_atoms: [104, 8, 1],
            prefix_atom_widths: [2, 1, 1],
            payload: CatiaEntitySuffixPayload::ControlE8,
            ..
        }
    ));

    let truncated_wide_prefix =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0xd1, 0x53, 0xd1,
        ]));
    assert_eq!(truncated_wide_prefix.entity_records[0].suffix_value, None);

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic unset entity suffix");
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );

    let incomplete = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49, 0x00,
    ]));
    assert_eq!(incomplete.entity_records[0].suffix_value, None);

    let unknown_trailer =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x50,
        ]));
    assert_eq!(unknown_trailer.entity_records[0].suffix_value, None);

    let invalid_prefix =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x7f, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49,
        ]));
    assert_eq!(invalid_prefix.entity_records[0].suffix_value, None);

    let control = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x96, 0x81, 0xa6, 0xe8,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic control entity suffix");
    assert_eq!(
        control
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        control
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        control
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    let control = crate::native::CatiaNative::load(
        control.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load generic control suffix");
    assert!(matches!(
        control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete control suffix")
            .payload,
        CatiaEntitySuffixPayload::ControlE8
    ));

    let control_e9 = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x88, 0x82, 0xf0, 0xe9, 0x81, 0x4a,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode E9 control entity suffix");
    assert_eq!(
        control_e9
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        control_e9
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        control_e9
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let control_e9 = crate::native::CatiaNative::load(
        control_e9
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load E9 control suffix");
    assert!(matches!(
        control_e9.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete E9 control suffix")
            .payload,
        CatiaEntitySuffixPayload::ControlE9
    ));
    let mut malformed_control_e9 = control_e9.clone();
    malformed_control_e9.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete E9 control suffix")
        .payload = CatiaEntitySuffixPayload::ControlE8;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_control_e9
        .store(&mut namespace)
        .expect("store malformed E9 control suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
    let malformed_control_e9 =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x82, 0xf0, 0xe9, 0x81, 0x4a, 0x00,
        ]));
    assert_eq!(malformed_control_e9.entity_records[0].suffix_value, None);

    let malformed_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x96, 0x81, 0xa6, 0xe8, 0x81,
        ]));
    assert_eq!(malformed_control.entity_records[0].suffix_value, None);

    let separator = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x93, 0x81, 0xa1, 0x37, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic separator entity suffix");
    assert_eq!(
        separator
            .report()
            .coverage_count(crate::coverage::DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let separator = crate::native::CatiaNative::load(
        separator.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load generic separator suffix");
    assert!(matches!(
        separator.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete separator suffix")
            .payload,
        CatiaEntitySuffixPayload::Separator37
    ));

    let malformed_separator =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x93, 0x81, 0xa1, 0x37, 0x81, 0x49, 0,
        ]));
    assert_eq!(malformed_separator.entity_records[0].suffix_value, None);

    let atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x81, 0x92, 0x81, 0xb3, 0x83, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic atom entity suffix");
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let atom =
        crate::native::CatiaNative::load(atom.ir().native.namespace("catia").expect("namespace"))
            .expect("load generic atom suffix");
    assert!(matches!(
        atom.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete atom suffix")
            .payload,
        CatiaEntitySuffixPayload::Atom { value: 3 }
    ));
    let mut malformed_atom = atom;
    malformed_atom.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete atom suffix")
        .payload = CatiaEntitySuffixPayload::Atom { value: 4 };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_atom
        .store(&mut namespace)
        .expect("store malformed atom suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let truncated_compact_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x81, 0xb3, 0xd1,
        ]));
    assert_eq!(truncated_compact_atom.entity_records[0].suffix_value, None);

    let schema_selected_atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x81, 0x92, 0x82, 0x32, 4, 0, 0, 0, 0x81, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected atom entity suffix");
    assert_eq!(
        schema_selected_atom.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT
        ),
        1
    );
    assert_eq!(
        schema_selected_atom
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let schema_selected_atom = crate::native::CatiaNative::load(
        schema_selected_atom
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected atom suffix");
    assert!(matches!(
        schema_selected_atom.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete schema-selected atom suffix")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector: 4,
            value: crate::native::CatiaEntitySuffixSelectedValue::Atom { value: 1 },
            ..
        }
    ));
    assert_eq!(
        schema_selected_atom.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved suffix selector"),
        &crate::native::CatiaEntitySuffixSchemaSelection {
            offset: 3,
            ordinal: 4,
            entry: schema_selected_atom.catalogs[0].entries[4].id.clone(),
            name: "Thickness".to_string(),
            value: crate::native::CatiaEntitySuffixSchemaValue::Atom { value: 1 },
        }
    );
    let mut stale_schema_selected_atom = schema_selected_atom.clone();
    if let CatiaEntitySuffixPayload::SchemaSelected {
        selector_offset, ..
    } = &mut stale_schema_selected_atom.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete schema-selected atom suffix")
        .payload
    {
        *selector_offset = 0;
    } else {
        panic!("schema-selected atom payload");
    }
    stale_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved suffix selector")
        .offset = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    stale_schema_selected_atom
        .store(&mut namespace)
        .expect("store stale suffix schema offsets");
    namespace.version = crate::native::CATIA_SUFFIX_SCHEMA_OFFSET_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate suffix schema offsets");
    assert_eq!(
        migrated.entity_records[0].suffix_value,
        schema_selected_atom.entity_records[0].suffix_value
    );
    assert_eq!(
        migrated.entity_records[0].suffix_schema_selection,
        schema_selected_atom.entity_records[0].suffix_schema_selection
    );

    let mut malformed_schema_selected_atom = schema_selected_atom.clone();
    malformed_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved suffix selector")
        .offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_schema_selected_atom
        .store(&mut namespace)
        .expect("store malformed schema-selected atom suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let out_of_range_schema_selected_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x82, 0x32, 0xcf, 0, 0, 0, 0x81, 0x81, 0x49,
        ]));
    assert!(out_of_range_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .is_none());

    let selected_scalar_bits = 17.25_f64.to_bits();
    let mut selected_scalar_suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    selected_scalar_suffix.extend_from_slice(&selected_scalar_bits.to_le_bytes());
    selected_scalar_suffix.extend_from_slice(&[0x81, 0x4a]);
    let selected_scalar = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(
                &selected_scalar_suffix,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected scalar suffix");
    assert_eq!(
        selected_scalar.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT
        ),
        1
    );
    let selected_scalar = crate::native::CatiaNative::load(
        selected_scalar
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected scalar suffix");
    assert!(matches!(
        selected_scalar.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected scalar"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            name,
            value: crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
                opcode_offset: 8,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
            },
            ..
        } if name == "Thickness" && *bits == selected_scalar_bits
    ));
    let mut malformed_selected_evaluation_offset = selected_scalar;
    let crate::native::CatiaEntitySuffixSchemaValue::Evaluation { opcode_offset, .. } =
        &mut malformed_selected_evaluation_offset.entity_records[0]
            .suffix_schema_selection
            .as_mut()
            .expect("resolved schema-selected scalar")
            .value
    else {
        panic!("schema-selected scalar evaluation");
    };
    *opcode_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_selected_evaluation_offset
        .store(&mut namespace)
        .expect("store malformed selected evaluation offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let selected_unset =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7,
        ]));
    assert!(matches!(
        selected_unset.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected unset"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            value: crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
                evaluation: CatiaEntityEvaluation::Unset,
                ..
            },
            ..
        }
    ));

    let selected_control = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x88, 0x81, 0x32, 4, 0, 0, 0, 0xe8, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected control suffix");
    assert_eq!(
        selected_control.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT
        ),
        1
    );
    assert_eq!(
        selected_control
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let selected_control = crate::native::CatiaNative::load(
        selected_control
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected control suffix");
    assert!(matches!(
        selected_control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete schema-selected control suffix")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector: 4,
            value: crate::native::CatiaEntitySuffixSelectedValue::ControlE8,
            ..
        }
    ));
    assert!(matches!(
        selected_control.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected control suffix"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            name,
            value: crate::native::CatiaEntitySuffixSchemaValue::ControlE8,
            ..
        } if name == "Thickness"
    ));
    let mut malformed_selected_control = selected_control.clone();
    malformed_selected_control.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved schema-selected control suffix")
        .value = crate::native::CatiaEntitySuffixSchemaValue::Separator37;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_selected_control
        .store(&mut namespace)
        .expect("store malformed schema-selected control suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
    let malformed_selected_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x81, 0x32, 4, 0, 0, 0, 0xe8, 0x81, 0x49, 0x00,
        ]));
    assert_eq!(
        malformed_selected_control.entity_records[0].suffix_value,
        None
    );

    let selected_separator =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x82, 0x93, 0x81, 0x32, 4, 0, 0, 0, 0x37, 0x81, 0x52,
        ]));
    assert!(matches!(
        &selected_separator.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected separator")
            .value,
        crate::native::CatiaEntitySuffixSchemaValue::Separator37
    ));

    let selected_schema =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x93, 0x82, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0, 0x81, 0x49,
        ]));
    assert!(matches!(
        selected_schema.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete nested suffix selector")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset: 3,
            value: crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector {
                offset: 8,
                ordinal: 5,
            },
            ..
        }
    ));
    assert!(matches!(
        &selected_schema.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved nested suffix selector")
            .value,
        crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector {
            ordinal: 5,
            ref name,
            ..
        } if name.as_deref() == Some("#1_ /2")
    ));
    let mut malformed_nested_offset = selected_schema;
    let crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector { offset, .. } =
        &mut malformed_nested_offset.entity_records[0]
            .suffix_schema_selection
            .as_mut()
            .expect("resolved nested suffix selector")
            .value
    else {
        panic!("nested suffix schema selector");
    };
    *offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_nested_offset
        .store(&mut namespace)
        .expect("store malformed nested suffix offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut nonfinite_selected_scalar = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    nonfinite_selected_scalar.extend_from_slice(&f64::NAN.to_bits().to_le_bytes());
    let nonfinite_selected_scalar = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&nonfinite_selected_scalar),
    );
    assert_eq!(
        nonfinite_selected_scalar.entity_records[0].suffix_value,
        None
    );

    let malformed_schema_selected_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x82, 0x32, 0xcf, 0, 0, 0, 0x81, 0, 0, 0,
        ]));
    assert_eq!(
        malformed_schema_selected_atom.entity_records[0].suffix_value,
        None
    );

    let mut bare_scalar = vec![0x84, 0x96, 0x82, 0xb1, 0xe6];
    bare_scalar.extend_from_slice(&6.75_f64.to_bits().to_le_bytes());
    let bare_scalar =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&bare_scalar));
    assert_eq!(
        bare_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete bare scalar suffix")
            .trailer,
        CatiaEntitySuffixTrailer::Empty
    );

    let bare_unset = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x84, 0x96, 0x82, 0xb1, 0xe7,
    ]));
    assert!(matches!(
        bare_unset.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete bare unset suffix")
            .payload,
        CatiaEntitySuffixPayload::Evaluation {
            evaluation: CatiaEntityEvaluation::Unset,
            ..
        }
    ));

    let nested_bits = 11.725_f64.to_bits();
    let mut nested_scalar = vec![0x84, 0x88, 0x82, 0x32, 0xe6, 0, 0, 0, 0xe6];
    nested_scalar.extend_from_slice(&nested_bits.to_le_bytes());
    let nested_scalar =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&nested_scalar));
    let nested_value = nested_scalar.entity_records[0]
        .suffix_value
        .as_ref()
        .expect("complete zero-padded scalar suffix");
    assert_eq!(
        nested_value.payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 8,
            evaluation: CatiaEntityEvaluation::Scalar { bits: nested_bits },
            encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
        }
    );
    assert_eq!(nested_value.trailer, CatiaEntitySuffixTrailer::Empty);

    let mut nonfinite_nested = vec![0x84, 0x88, 0x82, 0x32, 0xe6, 0, 0, 0, 0xe6];
    nonfinite_nested.extend_from_slice(&f64::INFINITY.to_bits().to_le_bytes());
    let nonfinite_nested = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&nonfinite_nested),
    );
    assert_eq!(nonfinite_nested.entity_records[0].suffix_value, None);

    let mut zero_frame_scalar = vec![0x84, 0x96, 0x82, 0x55, 0xe6];
    zero_frame_scalar.extend_from_slice(&(-26.703_618_806_753_155_f64).to_bits().to_le_bytes());
    zero_frame_scalar.extend_from_slice(&[0xfe, 0xf6]);
    zero_frame_scalar.extend_from_slice(&[0; 16]);
    let zero_frame_scalar = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&zero_frame_scalar),
    );
    assert_eq!(
        zero_frame_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete zero-frame scalar suffix")
            .trailer,
        CatiaEntitySuffixTrailer::FixedZeroFrame
    );

    let mut malformed_zero_frame = vec![0x84, 0x96, 0x82, 0x55, 0xe6];
    malformed_zero_frame.extend_from_slice(&1.0_f64.to_bits().to_le_bytes());
    malformed_zero_frame.extend_from_slice(&[0xfe, 0xf6]);
    malformed_zero_frame.extend_from_slice(&[0; 15]);
    malformed_zero_frame.push(1);
    let malformed_zero_frame = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&malformed_zero_frame),
    );
    assert_eq!(malformed_zero_frame.entity_records[0].suffix_value, None);

    let mut malformed_encoding = native.clone();
    malformed_encoding.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete suffix value")
        .payload = CatiaEntitySuffixPayload::Evaluation {
        opcode_offset: 4,
        evaluation: CatiaEntityEvaluation::Scalar { bits },
        encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_encoding
        .store(&mut namespace)
        .expect("store malformed suffix encoding");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete suffix value")
        .trailer = CatiaEntitySuffixTrailer::Token814A;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed suffix value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_binds_two_definition_value_chains() {
    use crate::native::{
        CatiaDefinitionChainValue, CatiaEntityEvaluation, CatiaEntitySchemaValue,
        CatiaEntitySuffixSchemaValue,
    };

    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode definition-chain evaluation");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_EVALUATION_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_VALUE_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_EVALUATED_DEFINITION_CHAIN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_DEFINITION_CHAIN_COUNT),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_EVALUATION_COUNT
        ),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_CHAIN_EVALUATION_OWNER_COUNT),
        0
    );
    let mut native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load definition-chain evaluation");
    assert_eq!(
        native.entity_records[0].definition_chain_value,
        Some(CatiaDefinitionChainValue {
            selector: CatiaEntitySchemaValue {
                offset: native.entity_records[0].definition_schema_selections[0].offset,
                ordinal: native.entity_records[0].definition_schema_selections[0].ordinal,
                entry: native.catalogs[0].entries[4].id.clone(),
                value: "FeatureFEDGE".to_string(),
            },
            role: CatiaEntitySchemaValue {
                offset: native.entity_records[0].definition_schema_selections[1].offset,
                ordinal: native.entity_records[0].definition_schema_selections[1].ordinal,
                entry: native.catalogs[0].entries[5].id.clone(),
                value: "Real".to_string(),
            },
            value: CatiaEntitySuffixSchemaValue::Evaluation {
                opcode_offset: 8,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
            },
        })
    );
    assert_eq!(
        native.design_objects[0].definition_chain_values,
        [native.entity_records[0].id.clone()]
    );

    let mut malformed_ownership = native.clone();
    malformed_ownership.design_objects[0]
        .definition_chain_values
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_ownership
        .store(&mut namespace)
        .expect("store malformed definition-chain ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    native.entity_records[0]
        .definition_chain_value
        .as_mut()
        .expect("definition-chain evaluation")
        .role
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store malformed definition-chain evaluation");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wrong_selector =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_chain_value(&[
            0x84, 0x88, 0x82, 0x32, 5, 0, 0, 0, 0xe7,
        ]));
    assert!(wrong_selector.entity_records[0]
        .definition_chain_value
        .is_none());

    let atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x87,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode definition-chain atom");
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_ATOM_COUNT),
        1
    );
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_EVALUATION_COUNT),
        0
    );
    let atom_native =
        crate::native::CatiaNative::load(atom.ir().native.namespace("catia").expect("namespace"))
            .expect("load definition-chain atom");
    assert_eq!(
        atom_native.entity_records[0]
            .definition_chain_value
            .as_ref()
            .map(|value| &value.value),
        Some(&CatiaEntitySuffixSchemaValue::Atom { value: 7 })
    );

    for (payload, coverage) in [
        (0xe8, "decoded_definition_chain_control_count"),
        (0x37, "decoded_definition_chain_separator_count"),
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                    0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, payload,
                ])),
                &DecodeOptions::default(),
            )
            .expect("decode definition-chain state");
        assert_eq!(
            decoded
                .report()
                .coverage
                .get(coverage)
                .copied()
                .unwrap_or(0),
            1
        );
    }

    let nested = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode nested definition-chain selector");
    assert_eq!(
        nested
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_SCHEMA_SELECTOR_COUNT),
        1
    );
    let nested_native =
        crate::native::CatiaNative::load(nested.ir().native.namespace("catia").expect("namespace"))
            .expect("load nested definition-chain selector");
    assert_eq!(
        nested_native.entity_records[0]
            .definition_chain_value
            .as_ref()
            .map(|value| &value.value),
        Some(&CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset: 8,
            ordinal: 5,
            entry: Some(nested_native.catalogs[0].entries[5].id.clone()),
            name: Some("Real".to_string()),
        })
    );
}

#[test]
fn typed_definition_chain_values_transfer_as_parameters() {
    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode typed definition-chain parameter");

    let [parameter] = decoded.ir().model.parameters.as_slice() else {
        panic!("expected one typed definition-chain parameter");
    };
    assert_eq!(parameter.name, "FeatureFEDGE");
    assert_eq!(parameter.expression, "12.5");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(12.5))
    );
    assert_eq!(parameter.owner, None);
    assert_eq!(parameter.properties["value_type"], "Real");
    assert!(!parameter.properties.contains_key("catia_binding"));
    assert_eq!(
        parameter.properties["catia_definition_evaluation_opcode_offset"],
        "8"
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        1
    );

    let boolean = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_type(
                "Boolean",
                &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x81],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean definition-chain parameter");
    let [boolean_parameter] = boolean.ir().model.parameters.as_slice() else {
        panic!("expected one Boolean definition-chain parameter");
    };
    assert_eq!(
        boolean_parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Boolean(true))
    );
    assert_eq!(boolean_parameter.expression, "true");
    assert_eq!(boolean_parameter.properties["value_type"], "Boolean");
    assert_eq!(
        boolean_parameter.properties["catia_definition_value_kind"],
        "atom"
    );
    assert_eq!(
        boolean_parameter.properties["catia_definition_atom_value"],
        "1"
    );
    assert!(!boolean_parameter
        .properties
        .contains_key("catia_definition_evaluation_opcode_offset"));
    assert_eq!(
        boolean
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        1
    );

    let invalid_boolean = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_type(
                "Boolean",
                &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x82],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode invalid Boolean definition-chain atom");
    assert!(invalid_boolean.ir().model.parameters.is_empty());
    assert_eq!(
        invalid_boolean
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        0
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode unset definition-chain parameter");
    let [unset_parameter] = unset.ir().model.parameters.as_slice() else {
        panic!("expected one unset definition-chain parameter");
    };
    assert!(unset_parameter.value.is_none());
    assert!(unset_parameter.expression.is_empty());
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT),
        1
    );

    let mut native =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_chain_value(&[
            0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6, 0, 0, 0, 0, 0, 0, 0, 0,
        ]));
    let parameter_entity = native.entity_records[0].clone();
    native.entity_records[0].relation_program_instance =
        Some(crate::native::CatiaRelationProgramInstance {
            framing: crate::native::CatiaRelationProgramInstanceFraming::Lead12,
            program_entity: crate::native::CatiaEntityReference::default(),
            repeated_entity: crate::native::CatiaEntityReference::default(),
            reference_incidences: Vec::new(),
            relation_expression: None,
            parameter_dependencies: Vec::new(),
            inputs: Some(vec![crate::native::CatiaRelationProgramInput {
                parameter: "#1_".to_string(),
                value_type: "Real".to_string(),
                entity: crate::native::CatiaEntityReference {
                    entity_id: parameter_entity.entity_id,
                    is_null: false,
                    entity: Some(parameter_entity.id.clone()),
                    class_name: Some("param".to_string()),
                },
            }]),
            output_entity: None,
            lead12_context_entity: None,
            lead54_trailing_entity: None,
        });
    let mut relation_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let relation_transfer = crate::formula::transfer_parameters(
        &mut relation_ir,
        &native,
        &mut Annotations::default(),
        None,
    );
    assert_eq!(relation_transfer.definition_chain_parameter_count, 1);
    assert_eq!(relation_transfer.relation_program_parameter_count, 1);
    assert_eq!(relation_ir.model.parameters.len(), 1);
}

#[test]
fn design_objects_retain_definition_chain_values_in_field_order() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_two_definition_chain_values());
    assert_eq!(native.design_objects.len(), 1);
    assert_eq!(
        native.design_objects[0].definition_chain_values,
        native
            .entity_records
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<Vec<_>>()
    );

    let mut reversed = native;
    reversed.design_objects[0].definition_chain_values.reverse();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    reversed
        .store(&mut namespace)
        .expect("store misordered definition-chain ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_two_definition_chain_values());
    let expected = native.design_objects[0].definition_chain_values.clone();
    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current definition-chain ownership");
    let mut previous_design_objects: Vec<crate::native::CatiaDesignObject> = previous_namespace
        .arena_as("design_objects")
        .expect("load stored design objects");
    for object in &mut previous_design_objects {
        object.definition_chain_values.clear();
    }
    previous_namespace
        .set_arena("design_objects", &previous_design_objects)
        .expect("store previous design objects");
    previous_namespace.version = 195;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous definition-chain ownership");
    assert_eq!(migrated.design_objects[0].definition_chain_values, expected);
}

#[test]
fn literal_owner_slots_remain_unassigned_and_migrate_from_previous_namespaces() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_unassigned_definition_chain_value()),
            &DecodeOptions::default(),
        )
        .expect("decode literal owner slot");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNASSIGNED_DEFINITION_CHAIN_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNASSIGNED_DEFINITION_CHAIN_EVALUATION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNASSIGNED_OBJECT_OWNER_SLOT_COUNT),
        1
    );

    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load literal owner slot");
    let record = &native.object_graphs[0].records[0];
    assert_eq!(
        record.owner,
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(66))
    );
    assert!(record.design_object.is_none());
    assert!(native.design_objects.is_empty());

    let mut malformed = native.clone();
    malformed.object_graphs[0].records[0].owner =
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(67));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed literal owner slot");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current literal owner slot");
    let mut previous_records: Vec<crate::native::CatiaObjectRecord> = previous_namespace
        .arena_as("object_graph_records")
        .expect("load stored object records");
    previous_records[0].owner = None;
    previous_namespace
        .set_arena("object_graph_records", &previous_records)
        .expect("store previous object records");
    previous_namespace.version = 197;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous literal owner slot");
    assert_eq!(
        migrated.object_graphs[0].records[0].owner,
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(66))
    );
}

#[test]
fn native_namespace_binds_and_validates_definition_values() {
    use crate::native::{
        CatiaDefinitionValue, CatiaEntityEvaluation, CatiaEntityEvaluationEncoding,
        CatiaEntitySchemaValue, CatiaEntitySuffixPayload,
    };

    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0xd1, 0x53, 0x96, 0x82, 0xa6, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0];
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_value(
                &definition,
                &[0xfe],
                &suffix,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode definition-bound value");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_DEFINITION_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OWNED_DEFINITION_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DEFINITION_VALUE_OWNER_COUNT),
        0
    );
    let mut native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load definition-bound value");
    assert_eq!(
        native.entity_records[0].definition_value,
        Some(CatiaDefinitionValue {
            definition: CatiaEntitySchemaValue {
                offset: native.entity_records[0].definition_schema_selections[0].offset,
                ordinal: native.entity_records[0].definition_schema_selections[0].ordinal,
                entry: native.catalogs[0].entries[4].id.clone(),
                value: "Thickness".to_string(),
            },
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 5,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            schema_selection: None,
        })
    );
    assert_eq!(
        native.design_objects[0].definition_values,
        [native.entity_records[0].id.clone()]
    );
    assert_eq!(
        native.object_graphs[0].records[0].storage_record,
        Some(native.object_graphs[0].records[0].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[0].storage_design_object,
        Some(native.design_objects[0].id.clone())
    );

    let mut malformed_storage = native.clone();
    malformed_storage.object_graphs[0].records[0].storage_record = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_storage
        .store(&mut namespace)
        .expect("store malformed storage link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_ownership = native.clone();
    malformed_ownership.design_objects[0]
        .definition_values
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_ownership
        .store(&mut namespace)
        .expect("store malformed definition-value ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let definition_value = native.entity_records[0]
        .definition_value
        .as_mut()
        .expect("definition-bound value");
    definition_value.payload = CatiaEntitySuffixPayload::Evaluation {
        opcode_offset: 5,
        evaluation: CatiaEntityEvaluation::Unset,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store malformed definition value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let control = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &definition,
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    assert!(matches!(
        control.entity_records[0]
            .definition_value
            .as_ref()
            .expect("definition-bound control")
            .payload,
        CatiaEntitySuffixPayload::ControlE8
    ));

    let schema_selected =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
            &definition,
            &[0xfe],
            &[0x84, 0x96, 0x82, 0x32, 4, 0, 0, 0, 0xe7, 0x81, 0x49],
        ));
    let definition_value = schema_selected.entity_records[0]
        .definition_value
        .as_ref()
        .expect("definition-bound schema-selected value");
    assert!(matches!(
        definition_value.payload,
        CatiaEntitySuffixPayload::SchemaSelected { selector: 4, .. }
    ));
    assert_eq!(
        definition_value
            .schema_selection
            .as_ref()
            .expect("resolved suffix schema")
            .name,
        "Thickness"
    );

    for (definition, value) in [
        (
            vec![0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0],
            vec![0xfe],
        ),
        (definition.to_vec(), vec![0x80, 0xfe]),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
            &definition,
            &value,
            &suffix,
        ));
        assert_eq!(native.entity_records[0].definition_value, None);
    }
}

#[test]
fn named_parameter_value_requires_the_complete_finite_suffix() {
    let nonfinite = f64::NAN.to_bits();
    let mut suffix = vec![0x85, 0x96, 0x82, 0x6a, 0xe6];
    suffix.extend_from_slice(&nonfinite.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x52]);

    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&suffix));
    assert!(native.entity_records[0].suffix_value.is_none());
    assert!(native.entity_records[0].parameter_value.is_none());

    let control = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x85, 0x96, 0x82, 0x6a, 0xe8, 0x81, 0x52,
    ]));
    assert!(matches!(
        control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete control suffix")
            .payload,
        crate::native::CatiaEntitySuffixPayload::ControlE8
    ));
    assert!(control.entity_records[0].parameter_value.is_none());
}

#[test]
fn native_retains_migrates_and_validates_typed_schema_selector_incidences() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(4, false));
    let expression_entity = &native.entity_records[1];
    let expression = expression_entity
        .relation_expression
        .as_ref()
        .expect("complete relation expression");
    assert_eq!(
        (expression.expression.offset, expression.expression.ordinal),
        (
            expression_entity.value_schema_selections[1].offset,
            expression_entity.value_schema_selections[1].ordinal,
        )
    );
    let parameter_entity = &native.entity_records[2];
    let parameter = parameter_entity
        .parameter_value
        .as_ref()
        .expect("complete named parameter");
    assert_eq!(
        (parameter.name.offset, parameter.name.ordinal),
        (
            parameter_entity.value_schema_selections[0].offset,
            parameter_entity.value_schema_selections[0].ordinal,
        )
    );
    assert_eq!(
        (parameter.binding.offset, parameter.binding.ordinal),
        (
            parameter_entity.value_schema_selections[1].offset,
            parameter_entity.value_schema_selections[1].ordinal,
        )
    );

    let mut stale = native.clone();
    let expression = stale.entity_records[1]
        .relation_expression
        .as_mut()
        .expect("complete relation expression");
    expression.expression.offset = 0;
    expression.expression.ordinal = 0;
    let parameter = stale.entity_records[2]
        .parameter_value
        .as_mut()
        .expect("complete named parameter");
    parameter.name.offset = 0;
    parameter.name.ordinal = 0;
    parameter.binding.offset = 0;
    parameter.binding.ordinal = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    stale
        .store(&mut namespace)
        .expect("store stale typed schema incidences");
    namespace.version = crate::native::CATIA_ENTITY_SCHEMA_VALUE_INCIDENCE_VERSION - 1;
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate typed schema incidences");
    assert_eq!(
        migrated.entity_records[1].relation_expression,
        native.entity_records[1].relation_expression
    );
    assert_eq!(
        migrated.entity_records[2].parameter_value,
        native.entity_records[2].parameter_value
    );

    let mut malformed = native;
    malformed.entity_records[2]
        .parameter_value
        .as_mut()
        .expect("complete named parameter")
        .name
        .offset = u64::MAX;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed typed schema incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_types_and_validates_formula_relations() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let formula = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation");
    assert_eq!(formula.expression_entity.payload_offset, 4);
    assert_eq!(formula.output_entity.payload_offset, 6);
    assert_eq!(formula.expression_entity.reference.entity_id, 2);
    assert_eq!(
        formula.expression_entity.reference.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(
        formula.expression_entity.reference.class_name,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id == Some(2))
            .and_then(|record| record.class_name.clone())
    );
    assert_eq!(formula.output_entity.reference.entity_id, 99);
    assert_eq!(formula.output_entity.reference.entity, None);
    let parameter_entity = &native.entity_records[2];
    assert_eq!(
        formula.parameter_dependencies,
        [crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: "#1_ /2".to_string(),
            candidates: vec![crate::native::CatiaEntityReference {
                entity_id: parameter_entity.entity_id,
                is_null: false,
                entity: Some(parameter_entity.id.clone()),
                class_name: native
                    .object_graphs
                    .iter()
                    .flat_map(|graph| &graph.records)
                    .find(|record| record.entity_id == Some(parameter_entity.entity_id))
                    .and_then(|record| record.class_name.clone()),
            }],
        }]
    );
    let expected_formula = formula.clone();

    let mut version_235_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_235_namespace)
        .expect("store current formula output reference");
    let mut stored_fields = version_235_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let formula_fields = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    let expression = formula_fields
        .remove("expression_entity")
        .expect("stored expression entity");
    let expression = expression.as_object().expect("stored expression incidence")["reference"]
        .as_object()
        .expect("stored expression-entity object");
    formula_fields.insert("expression".to_string(), expression["entity"].clone());
    let output = formula_fields
        .remove("output_entity")
        .expect("stored output entity");
    let output = output.as_object().expect("stored output incidence")["reference"]
        .as_object()
        .expect("stored output-entity object");
    formula_fields.insert(
        "parameter_entity_id".to_string(),
        output["entity_id"].clone(),
    );
    formula_fields.insert(
        "parameter_is_null".to_string(),
        output.get("is_null").cloned().unwrap_or_default(),
    );
    formula_fields.insert(
        "parameter".to_string(),
        output.get("entity").cloned().unwrap_or_default(),
    );
    version_235_namespace.version = crate::native::CATIA_FORMULA_OUTPUT_REFERENCE_VERSION - 1;
    drop(stored_fields);
    let migrated = crate::native::CatiaNative::load(&version_235_namespace)
        .expect("migrate formula output reference");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_236_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_236_namespace)
        .expect("store current formula expression reference");
    let mut stored_fields = version_236_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let formula_fields = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    let expression = formula_fields
        .remove("expression_entity")
        .expect("stored expression entity");
    formula_fields.insert(
        "expression".to_string(),
        expression.as_object().expect("stored expression incidence")["reference"]
            .as_object()
            .expect("stored expression-entity object")["entity"]
            .clone(),
    );
    version_236_namespace.version = crate::native::CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION - 1;
    drop(stored_fields);
    let migrated = crate::native::CatiaNative::load(&version_236_namespace)
        .expect("migrate formula expression reference");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_249_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_249_namespace)
        .expect("store current formula reference offsets");
    let mut stored_fields = version_249_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let formula_fields = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    for field in ["expression_entity", "output_entity"] {
        let reference = formula_fields[field]
            .as_object()
            .expect("stored formula incidence")["reference"]
            .clone();
        formula_fields.insert(field.to_string(), reference);
    }
    version_249_namespace.version = crate::native::CATIA_FORMULA_REFERENCE_OFFSET_VERSION - 1;
    drop(stored_fields);
    let migrated = crate::native::CatiaNative::load(&version_249_namespace)
        .expect("migrate formula reference offsets");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_237_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_237_namespace)
        .expect("store current formula dependency references");
    let mut stored_fields = version_237_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let candidates = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter-dependency array")[0]
        .as_object_mut()
        .expect("stored parameter dependency")
        .get_mut("candidates")
        .expect("stored dependency candidates")
        .as_array_mut()
        .expect("stored candidate array");
    for candidate in candidates {
        *candidate = candidate.as_object().expect("stored candidate reference")["entity"].clone();
    }
    version_237_namespace.version = crate::native::CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION - 1;
    drop(stored_fields);
    let migrated = crate::native::CatiaNative::load(&version_237_namespace)
        .expect("migrate formula dependency references");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_245_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_245_namespace)
        .expect("store current formula dependency offsets");
    version_245_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter-dependency array")[0]
        .as_object_mut()
        .expect("stored parameter dependency")
        .remove("source_offset");
    version_245_namespace.version = crate::native::CATIA_RELATION_DEPENDENCY_OFFSET_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_245_namespace)
        .expect("migrate formula dependency offsets");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_205_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_205_namespace)
        .expect("store current formula dependency candidates");
    let mut version_205_entities: Vec<crate::native::CatiaEntityRecord> = version_205_namespace
        .arena_as("entity_records")
        .expect("load version 205 entity records");
    version_205_entities[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .parameter_dependencies[0]
        .candidates
        .clear();
    version_205_namespace
        .set_arena("entity_records", &version_205_entities)
        .expect("store version 205 entity records");
    version_205_namespace.version = 205;
    let migrated = crate::native::CatiaNative::load(&version_205_namespace)
        .expect("migrate version 205 formula dependency candidates");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula)
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .output_entity
        .reference
        .entity_id = 98;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed formula relation");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_offset =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    malformed_offset.entity_records[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .expression_entity
        .payload_offset = u64::MAX;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_offset
        .store(&mut namespace)
        .expect("store malformed formula incidence offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn formula_relation_requires_a_complete_relation_expression_target() {
    let mut file = standard_catpart_with_formula_relation(0x63, false);
    let role = file
        .windows("param".len())
        .position(|bytes| bytes == b"param")
        .expect("formula parameter role");
    file[role..role + "param".len()].copy_from_slice(b"other");

    let native = crate::native::CatiaNative::decode(&file);
    assert!(native.entity_records[0].formula_relation.is_none());
}

#[test]
fn formula_parameter_dependency_requires_a_unique_binding() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, true));
    let dependency = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0];

    assert_eq!(dependency.symbol, "#1_ /2");
    assert_eq!(dependency.candidates.len(), 2);
}

#[test]
fn formula_parameter_dependency_retains_an_unmatched_symbol() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#2_ /2", 35.0)],
        "LENGTH",
        Some(33.0),
        "µ+#1_ /2-2mm",
    ));
    let dependency = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0];

    assert_eq!(dependency.symbol, "#1_ /2");
    assert_eq!(dependency.source_offset, 3);
    assert!(dependency.candidates.is_empty());
}

#[test]
fn formula_parameter_dependencies_exclude_string_literal_contents() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "Integer", "Count", "#1_ /2", 35.0)],
        "String",
        None,
        "\"literal #1_ /2\"+ToString(#1_ /2)",
    ));
    let dependencies = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies;

    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].symbol, "#1_ /2");
    assert_eq!(dependencies[0].source_offset, 26);
    assert_eq!(dependencies[0].candidates.len(), 1);

    let expected_formula = native.entity_records[0]
        .formula_relation
        .clone()
        .expect("complete formula relation");
    let mut old_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut old_namespace)
        .expect("store relation dependencies");
    let mut stored_fields = old_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let dependencies = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula relation")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter dependencies");
    let mut literal_dependency = dependencies[0].clone();
    literal_dependency
        .as_object_mut()
        .expect("stored parameter dependency")
        .insert("source_offset".to_string(), 9_u64.into());
    dependencies.insert(0, literal_dependency);
    old_namespace.version = crate::native::CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION - 1;
    drop(stored_fields);
    let migrated = crate::native::CatiaNative::load(&old_namespace)
        .expect("migrate string-literal relation dependencies");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula)
    );

    let unterminated =
        crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
            4,
            false,
            &[("#1_", "Integer", "Count", "#1_ /2", 35.0)],
            "String",
            None,
            "\"unterminated #1_ /2",
        ));
    assert!(unterminated.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies
        .is_empty());
}

#[test]
fn formula_relation_resolves_bare_expression_symbols() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#1_", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_-2mm",
    ));

    assert_eq!(
        native.entity_records[0]
            .formula_relation
            .as_ref()
            .expect("complete formula relation")
            .parameter_dependencies,
        [crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: "#1_".to_string(),
            candidates: vec![crate::native::CatiaEntityReference {
                entity_id: native.entity_records[2].entity_id,
                is_null: false,
                entity: Some(native.entity_records[2].id.clone()),
                class_name: native.object_graphs[0]
                    .records
                    .iter()
                    .find(|record| record.entity_id == Some(native.entity_records[2].entity_id))
                    .and_then(|record| record.class_name.clone()),
            }],
        }]
    );
}

#[test]
fn decode_transfers_a_complete_typed_input_when_the_formula_output_is_unresolved() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_relation(0x63, false)),
            &DecodeOptions::default(),
        )
        .expect("decode formula with unresolved output");
    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("independently typed formula input")
    };

    assert_eq!(input.name, "Thickness");
    assert_eq!(input.ordinal, 0);
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert!(input.dependencies.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_FORMULA_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT),
        1
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn terminal_entity_identity_is_a_null_formula_output() {
    let bytes = standard_catpart_with_formula_relation(5, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let formula = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation");
    assert_eq!(formula.output_entity.reference.entity_id, 5);
    assert!(formula.output_entity.reference.is_null);
    assert_eq!(formula.output_entity.reference.entity, None);
    let formula_record = native.object_graphs[0]
        .records
        .iter()
        .find(|record| record.id == native.entity_records[0].object_record)
        .expect("formula object record");
    assert!(formula_record.references[2].is_null);
    assert_eq!(formula_record.references[2].target, None);

    let mut version_210_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_210_namespace)
        .expect("store terminal null references");
    let mut version_210_records: Vec<crate::native::CatiaObjectRecord> = version_210_namespace
        .arena_as("object_graph_records")
        .expect("load version 210 object records");
    for record in &mut version_210_records {
        for reference in &mut record.references {
            reference.is_null = false;
        }
    }
    version_210_namespace
        .set_arena("object_graph_records", &version_210_records)
        .expect("store version 210 object records");
    let mut version_210_entities: Vec<crate::native::CatiaEntityRecord> = version_210_namespace
        .arena_as("entity_records")
        .expect("load version 210 entity records");
    version_210_entities[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .output_entity
        .reference
        .is_null = false;
    version_210_namespace
        .set_arena("entity_records", &version_210_entities)
        .expect("store version 210 entity records");
    version_210_namespace.version = 210;
    let migrated = crate::native::CatiaNative::load(&version_210_namespace)
        .expect("migrate terminal null references");
    assert!(migrated.object_graphs[0].records[0].references[2].is_null);
    assert!(
        migrated.entity_records[0]
            .formula_relation
            .as_ref()
            .expect("migrated formula relation")
            .output_entity
            .reference
            .is_null
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode formula with null output");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_FORMULA_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_OBJECT_RECORD_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_OBJECT_RECORD_REFERENCE_COUNT),
        0
    );
}

#[test]
fn formula_input_with_additional_object_payload_remains_unresolved() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    0x63,
                    false,
                    &[("#1_", "LENGTH", "Thickness", "#1_ /2", 35.0)],
                    "LENGTH",
                    Some(33.0),
                    "#1_ /2-2mm",
                    (&[0x81, 0xfe], None),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode formula input with additional object payload");

    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        4
    );
}

#[test]
fn decode_transfers_a_closed_length_formula_and_its_input() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let bytes = standard_catpart_with_formula_relation(4, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let output_entity = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .output_entity
        .reference;
    assert_eq!(output_entity.entity_id, 4);
    assert!(output_entity.entity.is_some());
    assert_eq!(
        output_entity.class_name,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id == Some(4))
            .and_then(|record| record.class_name.clone())
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode closed length formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("closed formula parameters")
    };

    assert_eq!(input.name, "Thickness");
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(input.properties["value_type"], "LENGTH");
    assert_eq!(input.properties["catia_binding"], "#1_ /2");
    assert!(input.dependencies.is_empty());
    assert_eq!(output.name, "Result");
    assert_eq!(output.ordinal, 1);
    assert_eq!(output.expression, "#1_ /2-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.properties["catia_binding"], "#result_ /1");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PARAMETER_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        4
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_FORMULA_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        usize::from(output_entity.class_name.is_some())
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        usize::from(output_entity.class_name.is_none())
    );
    let expression_classified = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .expression_entity
        .reference
        .class_name
        .is_some();
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT),
        usize::from(expression_classified)
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT),
        usize::from(!expression_classified)
    );
    let dependency_candidate = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0]
        .candidates[0];
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT),
        1
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_CLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT
        ),
        usize::from(dependency_candidate.class_name.is_some())
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::UNCLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT
        ),
        usize::from(dependency_candidate.class_name.is_none())
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        0
    );
    assert!(decoded.report().losses.iter().all(|loss| {
        loss.code.category() != cadmpeg_ir::report::LossCategory::DesignIntent
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    assert_eq!(
        decoded.source_fidelity().annotations.exactness[&input.id.0].fields["expression"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        decoded.source_fidelity().annotations.exactness[&input.id.0].fields["properties"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        decoded.source_fidelity().annotations.exactness[&output.id.0].fields["properties"],
        cadmpeg_ir::Exactness::Derived
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_keeps_a_mismatched_formula_result_unresolved() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 35.0)],
                "LENGTH",
                Some(34.0),
                "#1_ /2-2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula with mismatched stored result");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
    assert!(input.dependencies.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        3
    );
}

#[test]
fn decode_evaluates_formula_precedence_and_parentheses() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                Some(30.0),
                "(#1_ /2+3mm)*2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode parenthesized formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("validated formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(30.0)
        ))
    );
}

#[test]
fn decode_transfers_a_closed_constant_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(12.0),
                "10mm+2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode constant formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("constant formula output")
    };
    assert_eq!(output.name, "Result");
    assert_eq!(output.expression, "10mm+2mm");
    assert!(output.dependencies.is_empty());
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.0)
        ))
    );
    assert!(decoded
        .source_fidelity()
        .annotations
        .exactness
        .get(&output.id.0)
        .is_none_or(|annotation| !annotation.fields.contains_key("expression")));
}

#[test]
fn decode_rejects_a_constant_formula_that_disagrees_with_its_stored_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(13.0),
                "10mm+2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched constant formula");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        0
    );
}

#[test]
fn decode_converts_degree_literals_to_radians() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "Integer", "Count", "#1_ /2", 4.0)],
                "ANGLE",
                Some(std::f64::consts::FRAC_PI_2),
                "360.0*1 deg/#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode degree formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("degree formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)
        ))
    );
}

#[test]
fn decode_evaluates_the_dimensionless_pi_constant_in_an_angle_expression() {
    let output_value = std::f64::consts::PI - 1.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "ANGLE", "Angle", "#1_ /2", 1.0)],
                "ANGLE",
                Some(output_value),
                "PI*1rad-#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula with PI");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("PI formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(output_value)
        ))
    );
}

#[test]
fn decode_evaluates_dimensionless_trigonometric_arguments_as_radians() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(0.0),
                "sin(0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode scalar-radian trigonometric formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("scalar-radian trigonometric formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(0.0))
    );
}

#[test]
fn decode_evaluates_dimension_checked_trigonometric_calls() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[(
                    "#1_",
                    "ANGLE",
                    "Sweep",
                    "#1_ /2",
                    std::f64::consts::FRAC_PI_2,
                )],
                "Real",
                Some(1.0),
                "sin(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode trigonometric formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("trigonometric formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(1.0))
    );
}

#[test]
fn decode_evaluates_nested_logarithm_and_extrema_calls() {
    let output_value = -(4.0_f64.log10()) / 100.0_f64.log10() / 2.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "Gain", "#1_ /2", 2.0),
                    ("#2_", "Real", "Reference", "#2_ /3", 10.0),
                ],
                "Real",
                Some(output_value),
                "-log(min(100,max(20*#1_ /2,#2_ /3)/#2_ /3))/log(100)/2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode logarithmic formula");

    let [first, second, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("logarithmic formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(output_value))
    );
}

#[test]
fn decode_distinguishes_common_and_natural_logarithms() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(3.0),
                "log(100)+ln(E)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode logarithm formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("logarithm formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(3.0))
    );
}

#[test]
fn decode_normalizes_every_admitted_formula_length_unit_to_millimetres() {
    let expected = 0.001 + 1_609_344.0 + 914.4 + 1.0 + 10.0 + 1_000_000.0 + 304.8 + 25.4 + 1_000.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(expected),
                "1micron+1mile+1yard+1mm+1cm+1km+1ft+1in+1m",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode complete length-unit formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("length-unit formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(expected)
        ))
    );
}

#[test]
fn decode_normalizes_every_admitted_formula_angle_unit_to_radians() {
    let expected = 1.0 + std::f64::consts::PI / 200.0 + std::f64::consts::PI / 180.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(expected),
                "1rad+1grad+1deg",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode complete angle-unit formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("angle-unit formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(expected)
        ))
    );
}

#[test]
fn decode_evaluates_exponential_and_hyperbolic_functions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(2.0),
                "exp(0)+sinh(0)+cosh(0)+tanh(0)+asinh(0)+acosh(1)+atanh(0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponential and hyperbolic formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("exponential and hyperbolic formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(2.0))
    );
}

#[test]
fn decode_evaluates_scalar_rounding_functions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(8.0),
                "ceil(1.2)+floor(1.8)+int(-1.8)+round(2.5)+round(3.5)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode scalar rounding formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("scalar rounding formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(8.0))
    );
}

#[test]
fn decode_evaluates_dimensioned_rounding_in_the_selected_unit() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(1_230.0),
                "round(1234mm,\"cm\",0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned rounding formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("dimensioned rounding formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1_230.0)
        ))
    );
}

#[test]
fn decode_evaluates_integer_part_as_an_integer_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Integer",
                Some(-1.0),
                "int(-1.8)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode integer-part formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("integer-part formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(-1))
    );
}

#[test]
fn decode_evaluates_variadic_extrema_and_integer_part_remainder() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(9.0),
                "min(8,5,7,3)+max(1,4,2)+mod(7.8,3)+max(1)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode variadic extrema and remainder formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("variadic extrema and remainder formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(9.0))
    );
}

#[test]
fn decode_evaluates_remainder_of_a_negative_real_dividend_integer_part() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(-1.0),
                "mod(-7.5,3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode negative real remainder formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("negative real remainder formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(-1.0))
    );
}

#[test]
fn decode_evaluates_a_square_root_of_a_dimensioned_product() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Width", "#1_ /2", 3.0),
                    ("#2_", "LENGTH", "Height", "#2_ /3", 4.0),
                ],
                "LENGTH",
                Some(5.0),
                "sqrt(#1_ /2*#1_ /2+#2_ /3*#2_ /3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned square root");

    let [first, second, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("square-root formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(5.0)
        ))
    );
}

#[test]
fn decode_evaluates_right_associative_exponentiation_above_unary_signs() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(-512.0),
                "-2**3**2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponent formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("exponent formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(-512.0))
    );
}

#[test]
fn decode_evaluates_an_integral_power_of_a_dimensioned_value() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 3.0)],
                "LENGTH",
                Some(3.0),
                "sqrt((#1_ /2)**2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned exponent formula");

    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("dimensioned exponent formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(3.0)
        ))
    );
}

#[test]
fn decode_evaluates_inverse_trigonometric_calls_as_angles() {
    let output_value = 0.5_f64.asin() + 0.5_f64.acos() + 1.0_f64.atan();
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(output_value),
                "asin(0.5)+acos(0.5)+atan(1)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode inverse trigonometric formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("inverse trigonometric formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(output_value)
        ))
    );
}

#[test]
fn decode_evaluates_dimension_safe_absolute_and_tangent_calls() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Offset", "#1_ /2", -2.0),
                    ("#2_", "ANGLE", "Slope", "#2_ /3", 0.0),
                ],
                "LENGTH",
                Some(2.0),
                "abs(#1_ /2)*(1+tan(#2_ /3))",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode absolute and tangent formula");

    let [first, second, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("absolute and tangent formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
}

#[test]
fn decode_rejects_a_square_root_with_an_odd_dimension_exponent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "AreaLike", "#1_ /2", 4.0)],
                "LENGTH",
                Some(2.0),
                "sqrt(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid square root");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "AreaLike");
}

#[test]
fn decode_rejects_a_fractional_power_of_a_dimensioned_value() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 4.0)],
                "LENGTH",
                Some(2.0),
                "(#1_ /2)**0.5",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid exponent formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_dimension_exponent_overflow() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "LENGTH",
                Some(1.0),
                "((#1_ /2)**2147483647)**2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponent-overflow formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_inverse_trigonometry_outside_its_scalar_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "ANGLE",
                Some(std::f64::consts::FRAC_PI_4),
                "atan(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid inverse trigonometric formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_inverse_trigonometry_outside_its_numeric_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(0.0),
                "asin(2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode out-of-domain inverse trigonometric formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_rejects_scalar_functions_with_dimensioned_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "Real",
                Some(1.0),
                "exp(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid exponential formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_invalid_inverse_hyperbolic_domains() {
    for expression in ["acosh(0.5)", "atanh(1)"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    3,
                    false,
                    &[],
                    "Real",
                    Some(0.0),
                    expression,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode out-of-domain inverse hyperbolic formula");

        assert!(decoded.ir().model.parameters.is_empty(), "{expression}");
    }
}

#[test]
fn decode_rejects_nonfinite_exponential_results() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(0.0),
                "exp(1000)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode overflowing exponential formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_rejects_invalid_remainder_divisors() {
    for expression in ["mod(7,0)", "mod(7,2.5)", "mod(7,1mm)"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    3,
                    false,
                    &[],
                    "Real",
                    Some(0.0),
                    expression,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode invalid remainder formula");

        assert!(decoded.ir().model.parameters.is_empty(), "{expression}");
    }
}

#[test]
fn decode_rejects_a_logarithm_outside_its_dimensionless_positive_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "Real", "Ratio", "#1_ /2", 0.0)],
                "Real",
                Some(0.0),
                "log(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode out-of-domain logarithm");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Ratio");
}

#[test]
fn decode_transfers_linear_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode linear interpolation formula");

    let [start, end, fraction, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("linear interpolation parameters")
    };
    assert_eq!(start.value, Some(cadmpeg_ir::ParameterValue::Real(2.0)));
    assert_eq!(end.value, Some(cadmpeg_ir::ParameterValue::Real(10.0)));
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(output.value, Some(cadmpeg_ir::ParameterValue::Real(4.0)));
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_transfers_cubic_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(3.25),
                "CubicInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode cubic interpolation formula");

    let [start, end, fraction, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("cubic interpolation parameters")
    };
    assert_eq!(start.value, Some(cadmpeg_ir::ParameterValue::Real(2.0)));
    assert_eq!(end.value, Some(cadmpeg_ir::ParameterValue::Real(10.0)));
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(output.value, Some(cadmpeg_ir::ParameterValue::Real(3.25)));
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_rejects_a_dimensioned_cubic_interpolation_fraction() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "LENGTH", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(3.25),
                "CubicInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid cubic interpolation");

    assert_eq!(decoded.ir().model.parameters.len(), 3);
}

#[test]
fn decode_transfers_dimensioned_linear_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Start", "#1_ /2", 2.0),
                    ("#2_", "LENGTH", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "LENGTH",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned linear interpolation formula");

    let [start, end, fraction, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("dimensioned linear interpolation parameters")
    };
    assert_eq!(
        start.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
    assert_eq!(
        end.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(10.0)
        ))
    );
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(4.0)
        ))
    );
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_converts_metric_length_literals_to_millimetres() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(1_023.0),
                "1m+2cm+3mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode metric length formula");

    let [output] = decoded.ir().model.parameters.as_slice() else {
        panic!("metric length formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1_023.0)
        ))
    );
}

#[test]
fn decode_rejects_mixed_dimension_linear_interpolation_endpoints() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid linear interpolation");

    assert_eq!(decoded.ir().model.parameters.len(), 3);
}

#[test]
fn decode_rejects_extrema_between_different_dimensions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Length", "#1_ /2", 2.0),
                    ("#2_", "ANGLE", "Angle", "#2_ /3", 1.0),
                ],
                "LENGTH",
                Some(2.0),
                "max(#1_ /2,#2_ /3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid maximum");

    let [first, second] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed inputs")
    };
    assert_eq!(first.name, "Length");
    assert_eq!(second.name, "Angle");
}

#[test]
fn decode_rejects_trigonometric_calls_with_length_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Offset", "#1_ /2", 0.0)],
                "Real",
                Some(0.0),
                "sin(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid trigonometric formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Offset");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_dimensionally_invalid_formula_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                Some(12.0),
                "#1_ /2+1rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_transfers_typed_integer_to_angle_formula() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "Integer",
                "ANGLE",
                2.0,
                0.5,
                "#1_ /2*0.25rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode typed formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("typed formula parameters")
    };

    assert_eq!(input.expression, "2");
    assert_eq!(input.value, Some(ParameterValue::Integer(2)));
    assert_eq!(output.value, Some(ParameterValue::Angle(Angle(0.5))));
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_transfers_dimensionless_real_formula() {
    use cadmpeg_ir::features::ParameterValue;

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4, false, "Real", "R", 2.5, 1.25, "#1_ /2/2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode real formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("real formula parameters")
    };

    assert_eq!(input.expression, "2.5");
    assert_eq!(input.value, Some(ParameterValue::Real(2.5)));
    assert_eq!(input.properties["value_type"], "Real");
    assert_eq!(output.value, Some(ParameterValue::Real(1.25)));
    assert_eq!(output.properties["value_type"], "Real");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    for parameter in [input, output] {
        assert_eq!(
            decoded.source_fidelity().annotations.exactness[&parameter.id.0].fields["properties"],
            cadmpeg_ir::Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_an_unset_typed_formula_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                None,
                "#1_ /2+1mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset formula result");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("unset formula parameters")
    };

    assert_eq!(output.value, None);
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.expression, "#1_ /2+1mm");
    assert_eq!(output.properties["value_type"], "LENGTH");
}

#[test]
fn decode_transfers_a_typed_boolean_predicate_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "X", "#1_ /2", 5.0),
                    ("#2_", "Real", "Y", "#2_ /2", 3.0),
                ],
                "Boolean",
                None,
                "(#1_ /2>#2_ /2) and (#1_ /2>=0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean predicate formula");
    let [x, y, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("predicate formula parameters")
    };

    assert_eq!(output.value, None);
    assert_eq!(output.properties["value_type"], "Boolean");
    assert_eq!(output.expression, "(#1_ /2>#2_ /2) and (#1_ /2>=0)");
    assert_eq!(output.dependencies, [x.id.clone(), y.id.clone()]);
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        5
    );
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_rejects_a_conditional_with_different_branch_dimensions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(5.0),
                "true ? 5mm ; 1rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid conditional formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_transfers_an_unset_typed_formula_input_as_an_unset_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    4,
                    false,
                    &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                    "LENGTH",
                    None,
                    "#1_ /2+1mm",
                    (&[0xfe], Some(0)),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unset formula input");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("unset formula parameters")
    };

    assert_eq!(input.name, "Width");
    assert_eq!(input.value, None);
    assert!(input.expression.is_empty());
    assert!(input.dependencies.is_empty());
    assert_eq!(input.properties["value_type"], "LENGTH");
    assert_eq!(input.properties["catia_binding"], "#1_ /2");
    assert_eq!(output.value, None);
    assert_eq!(output.expression, "#1_ /2+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.properties["catia_binding"], "#result_ /1");
}

#[test]
fn decode_transfers_unset_non_numeric_formula_inputs_without_deriving_the_output() {
    for parameter_type in ["Boolean", "String"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(
                    standard_catpart_with_typed_formula_inputs_and_object_payload(
                        4,
                        false,
                        &[("#1_", parameter_type, "Value", "#1_ /2", 1.0)],
                        "Real",
                        Some(1.0),
                        "#1_ /2",
                        (&[0xfe], Some(0)),
                    ),
                ),
                &DecodeOptions::default(),
            )
            .expect("decode unset non-numeric formula input");
        let [input] = decoded.ir().model.parameters.as_slice() else {
            panic!("only the independently typed unset input")
        };

        assert_eq!(input.name, "Value");
        assert_eq!(input.value, None);
        assert!(input.expression.is_empty());
        assert!(input.dependencies.is_empty());
        assert_eq!(input.properties["value_type"], parameter_type);
        assert_eq!(input.properties["catia_binding"], "#1_ /2");
        assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn decode_transfers_an_unset_string_formula_result_without_evaluation() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    4,
                    false,
                    &[("#1_", "String", "Value", "#1_", 1.0)],
                    "String",
                    None,
                    "#1_",
                    (&[0xfe], Some(0)),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unset String formula result");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("unset String formula parameters")
    };

    assert_eq!(input.value, None);
    assert_eq!(input.properties["value_type"], "String");
    assert_eq!(output.value, None);
    assert_eq!(output.expression, "#1_");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.properties["value_type"], "String");
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_does_not_treat_numeric_packets_as_non_numeric_formula_values() {
    for parameter_type in ["Boolean", "String"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    4,
                    false,
                    &[("#1_", parameter_type, "Value", "#1_ /2", 1.0)],
                    "Real",
                    Some(1.0),
                    "#1_ /2",
                )),
                &DecodeOptions::default(),
            )
            .expect("decode non-numeric formula input with numeric packet");

        assert!(decoded.ir().model.parameters.is_empty());
    }
}

#[test]
fn decode_rejects_nonintegral_integer_formula_input() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "Integer",
                "I",
                3.5,
                4.0,
                "#1_ /2-2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode invalid integer formula");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_deduplicates_repeated_single_input_formula_symbols() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "ANGLE",
                "ANGLE",
                0.25,
                0.5,
                "#1_ /2+#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode repeated formula input");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("repeated formula input parameters")
    };

    assert_eq!(input.expression, "0.25 rad");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
}

#[test]
fn decode_transfers_ordered_multi_input_formula_dependencies() {
    use cadmpeg_ir::features::ParameterValue;

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "Width", "#1_ /2", 12.0),
                    ("#2_", "Integer", "Count", "#2_ /3", 3.0),
                ],
                "Real",
                Some(15.0),
                "#1_ /2+#2_ /3",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode multi-input formula");
    let [width, count, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("multi-input formula parameters")
    };

    assert_eq!(width.value, Some(ParameterValue::Real(12.0)));
    assert_eq!([width.ordinal, count.ordinal, output.ordinal], [0, 1, 2]);
    assert_eq!(count.value, Some(ParameterValue::Integer(3)));
    assert_eq!(
        output.dependencies,
        [width.id.clone(), count.id.clone()].as_slice()
    );
    assert_eq!(output.value, Some(ParameterValue::Real(15.0)));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_transfers_a_closed_formula_with_bare_symbols() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let bytes = standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#1_", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_-2mm",
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("decode bare-symbol formula");
    let [input, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("closed bare-symbol formula parameters")
    };

    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(output.expression, "#1_-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));

    let native = crate::native::CatiaNative::decode(&bytes);
    let mut excluded_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::Annotations::default();
    let excluded = crate::formula::transfer_parameters(
        &mut excluded_ir,
        &native,
        &mut annotations,
        Some(&std::collections::HashSet::new()),
    );
    assert!(excluded_ir.model.parameters.is_empty());
    assert!(excluded.consumed_object_records.is_empty());
}

#[test]
fn decode_transfers_each_supported_formula_input_independently() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Width", "#1_ /2", 12.0),
                    ("#2_", "String", "Label", "#2_ /3", 0.25),
                    ("#3_", "Real", "Depth", "#3_ /4", 6.5),
                ],
                "Real",
                Some(3.0),
                "#1_ /2+#2_ /3+#3_ /4",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode incomplete multi-input formula");

    let [width, depth] = decoded.ir().model.parameters.as_slice() else {
        panic!("independently bound formula inputs")
    };
    assert_eq!(width.name, "Width");
    assert_eq!(
        width.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.0)
        ))
    );
    assert!(width.dependencies.is_empty());
    assert_eq!(depth.name, "Depth");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(6.5))
    );
    assert!(depth.dependencies.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT),
        4
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("4 modeling-scope field record(s)")
    }));
}

#[test]
fn decode_transfers_a_chained_formula_definition_once() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::Linear,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula chain");
    let [input, intermediate, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("formula chain parameters")
    };

    assert_eq!(intermediate.expression, "#1_ /2+1mm");
    assert_eq!(intermediate.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.expression, "#2_ /3+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&intermediate.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_rejects_multiple_formula_definitions_for_one_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::DuplicateTerminal,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode duplicate formula output");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed formula input")
    };
    assert_eq!(input.name, "Input");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_retains_a_typed_input_with_ambiguous_formula_definitions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::DuplicateIntermediate,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode ambiguous intermediate formula output");
    let [input, intermediate, output] = decoded.ir().model.parameters.as_slice() else {
        panic!("scalar fallback and downstream formula parameters")
    };

    assert_eq!(input.name, "Input");
    assert_eq!(intermediate.name, "Intermediate");
    assert_eq!(intermediate.expression, "2 mm");
    assert_eq!(
        intermediate.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
    assert!(intermediate.dependencies.is_empty());
    assert_eq!(output.expression, "#2_ /3+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&intermediate.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_rejects_an_incompatible_downstream_formula_without_erasing_its_input() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::IncompatibleDownstream,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode incompatible downstream formula");
    let [input, intermediate] = decoded.ir().model.parameters.as_slice() else {
        panic!("upstream formula parameters")
    };

    assert_eq!(input.name, "Input");
    assert_eq!(intermediate.name, "Intermediate");
    assert_eq!(intermediate.expression, "#1_ /2+1mm");
    assert_eq!(intermediate.dependencies, std::slice::from_ref(&input.id));
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_does_not_infer_a_fallback_from_conflicting_formula_input_types() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode conflicting formula input types");
    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the unambiguous scalar root")
    };

    assert_eq!(input.name, "Input");
    assert!(input.dependencies.is_empty());
    assert!(
        cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
            .findings
            .is_empty()
    );
}

#[test]
fn decode_rejects_a_cyclic_formula_component() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::Cyclic,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode cyclic formula component");

    assert!(decoded.ir().model.parameters.is_empty());
}

#[test]
fn decode_rejects_a_formula_exceeding_the_expression_depth_limit() {
    let boundary_expression = format!("{}#1_ /2", "+".repeat(128));
    let boundary = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "LENGTH",
                "LENGTH",
                12.0,
                12.0,
                &boundary_expression,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula at depth limit");
    assert_eq!(boundary.ir().model.parameters.len(), 2);

    let expression = format!("{}#1_ /2", "+".repeat(129));
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "LENGTH",
                "LENGTH",
                12.0,
                12.0,
                &expression,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode depth-limited formula");

    let [input] = decoded.ir().model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Thickness");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_a_formula_with_ambiguous_input_binding() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_relation(5, true)),
            &DecodeOptions::default(),
        )
        .expect("decode ambiguous formula");

    assert!(decoded.ir().model.parameters.is_empty());
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_RESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::AMBIGUOUS_FORMULA_PARAMETER_DEPENDENCY_COUNT),
        1
    );
}

#[test]
fn entity_value_schema_selection_excludes_a_packet_crossing_its_boundary() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_crossing_entity_value_packet());
    assert_eq!(native.entity_records[0].value_packets.len(), 1);
    assert_eq!(native.entity_records[0].value_schema_selections.len(), 2);
    assert!(native.entity_records[0]
        .value_schema_selections
        .iter()
        .all(|selection| selection.packets.is_empty()));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store crossing packet fixture");
    crate::native::CatiaNative::load(&namespace).expect("validate canonical packet ownership");
}

#[test]
fn native_load_rejects_noncanonical_graph_catalog_views() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    assert!(native.object_graphs[0].catalog_byte_offset.is_some());
    assert!(native.object_graphs[0].catalog.is_some());
    assert!(native.object_graphs[0].records[0].class_name.is_some());
    assert!(native.object_graphs[0].records[0].class_entry.is_some());
    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed graph-catalog view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut missing_catalog_link = native.clone();
    missing_catalog_link.object_graphs[0].catalog_byte_offset = None;
    assert_rejected(missing_catalog_link);

    let mut missing_catalog_identity = native.clone();
    missing_catalog_identity.object_graphs[0].catalog = None;
    assert_rejected(missing_catalog_identity);

    let mut invalid_class = native.clone();
    invalid_class.object_graphs[0].records[0].class_name = Some("WrongClass".to_string());
    assert_rejected(invalid_class);

    let mut invalid_class_entry = native;
    invalid_class_entry.object_graphs[0].records[0].class_entry = None;
    assert_rejected(invalid_class_entry);
}

#[test]
fn native_load_rejects_invalid_source_identities_and_extents() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed source identity");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut invalid_catalog_extent = native.clone();
    invalid_catalog_extent.catalogs[0].byte_len += 1;
    assert_rejected(invalid_catalog_extent);

    let mut invalid_entry_offset = native.clone();
    invalid_entry_offset.catalogs[0].entries[0].byte_offset += 1;
    assert_rejected(invalid_entry_offset);

    let mut invalid_record_offset = native.clone();
    invalid_record_offset.object_graphs[0].records[0].byte_offset += 1;
    assert_rejected(invalid_record_offset);

    let mut invalid_value_id = native;
    invalid_value_id.value_blocks[0].id = "catia:outer:value-block#wrong".to_string();
    assert_rejected(invalid_value_id);

    let mut invalid_alias_id = crate::native::CatiaNative::decode(&surface_alias_stream());
    invalid_alias_id.alias_rows[0].id = "catia:outer:alias-row#wrong".to_string();
    assert_rejected(invalid_alias_id);
}

#[test]
fn native_store_paths_write_the_current_schema_version() {
    let catalogue_names = crate::native::CATIA_FAMILIES
        .iter()
        .map(|row| row.arena)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(crate::native::CATIA_FAMILIES.len(), 42);
    assert_eq!(
        catalogue_names,
        crate::native::CATIA_ARENA_NAMES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );

    let borrowed = crate::native::CatiaNative {
        version: 1,
        ..crate::native::CatiaNative::default()
    };
    let mut borrowed_namespace = cadmpeg_ir::NativeNamespace::default();
    borrowed
        .store(&mut borrowed_namespace)
        .expect("store borrowed CATIA namespace");
    assert_eq!(
        borrowed_namespace.version,
        crate::native::CATIA_NATIVE_VERSION
    );

    let owned = crate::native::CatiaNative {
        version: 1,
        ..crate::native::CatiaNative::default()
    };
    let mut owned_namespace = cadmpeg_ir::NativeNamespace::default();
    owned
        .store_owned(&mut owned_namespace)
        .expect("store owned CATIA namespace");
    assert_eq!(owned_namespace.version, crate::native::CATIA_NATIVE_VERSION);

    let rich = crate::native::CatiaNative::decode(&standard_catpart());
    let mut rich_borrowed = cadmpeg_ir::NativeNamespace::default();
    rich.store(&mut rich_borrowed)
        .expect("store populated borrowed CATIA namespace");
    let mut rich_owned = cadmpeg_ir::NativeNamespace::default();
    rich.clone()
        .store_owned(&mut rich_owned)
        .expect("store populated owned CATIA namespace");
    assert_eq!(rich_borrowed, rich_owned);
    assert_eq!(
        crate::native::CatiaNative::load(&rich_borrowed).expect("reload populated namespace"),
        rich
    );
}

#[test]
fn native_migrates_and_validates_evaluated_value_names() {
    let mut bytes = Vec::new();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0x58, 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
    bytes.extend([0x5f, 0xd1, 9]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let native = crate::native::CatiaNative::decode(&bytes);
    let value = &native.legacy_entity_runs[0].integer_values[0];
    assert_eq!(value.name.as_deref(), Some("Count"));

    let mut invalid = native.clone();
    invalid.legacy_entity_runs[0].integer_values[0].name = None;
    invalid.legacy_entity_runs[0].integer_values[0].name_field = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store noncanonical evaluated value name");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut previous_namespace = invalid_namespace;
    previous_namespace.version = 223;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate evaluated value name");
    assert_eq!(
        migrated.legacy_entity_runs[0].integer_values[0]
            .name
            .as_deref(),
        Some("Count")
    );
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
fn native_load_restores_segment_source_order_and_validates_retained_views() {
    let mut bytes = Vec::new();
    for index in 0..12 {
        bytes.extend(external_reference_segment(&format!(
            "Support{index}.CATPart"
        )));
    }
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store indexed FINJPL segments");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load indexed FINJPL segments");
    assert_eq!(
        loaded
            .finjpl_segments
            .iter()
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:finjpl#{index}"))
            .collect::<Vec<_>>()
    );
    assert!(loaded
        .finjpl_segments
        .windows(2)
        .all(|pair| pair[0].byte_offset < pair[1].byte_offset));
    assert_eq!(
        loaded
            .external_references
            .iter()
            .map(|reference| reference.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:external-reference#{index}"))
            .collect::<Vec<_>>()
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed FINJPL view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };
    let mut invalid_length = native.clone();
    invalid_length.finjpl_segments[0].byte_len += 1;
    assert_rejected(invalid_length);
    let mut invalid_family = native.clone();
    invalid_family.finjpl_segments[0].family = "other".to_string();
    assert_rejected(invalid_family);
    let mut missing_reference = native.clone();
    missing_reference.external_references.pop();
    assert_rejected(missing_reference);
    let mut invalid_target = native.clone();
    invalid_target.external_references[0].target = "Wrong.CATPart".to_string();
    assert_rejected(invalid_target);
    let mut invalid_reference_offset = native.clone();
    invalid_reference_offset.external_references[0].byte_offset += 1;
    assert_rejected(invalid_reference_offset);
    let mut invalid_type = native;
    invalid_type.finjpl_segments[0].type_word ^= 1;
    assert_rejected(invalid_type);

    let mut invalid_offset = crate::native::CatiaNative::decode(&bytes);
    invalid_offset.finjpl_segments[1].byte_offset += 1;
    assert_rejected(invalid_offset);
}

#[test]
fn object_graphs_retain_exact_finjpl_containment() {
    let preamble_graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let segment_graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x82, 0x82], &[0xfe])]);
    let mut bytes = preamble_graph;
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0101_0001u32.to_be_bytes());
    bytes.extend_from_slice(&segment_graph);

    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.object_graphs.len(), 2);
    assert_eq!(native.object_graphs[0].finjpl_segment, None);
    assert_eq!(
        native.object_graphs[1].finjpl_segment.as_deref(),
        Some(native.finjpl_segments[0].id.as_str())
    );

    let mut invalid = native;
    invalid.object_graphs[1].finjpl_segment = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store malformed graph segment link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn object_graphs_retain_exact_outer_container_declarations() {
    let (bytes, graph_offset) = outer_container_object_graph_catpart();

    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| graph.byte_offset == graph_offset)
        .expect("declared-stream object graph");
    let container = graph
        .outer_container
        .as_ref()
        .expect("outer container binding");
    assert_eq!(container.data_offset, 0);
    assert_eq!(container.ordinal, 2);
    assert_eq!(container.class_name, "CATPrtCont");
    assert_eq!(container.base_class, "CATProdCont");
    assert_eq!(container.stream_name, "1048_62eb7b6f_1825");
    let expected = container.clone();

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store outer container binding");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load outer container binding");
    assert_eq!(
        loaded
            .object_graphs
            .iter()
            .find(|graph| graph.byte_offset == graph_offset)
            .and_then(|graph| graph.outer_container.as_ref()),
        Some(&expected)
    );
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
fn native_load_derives_complete_source_ordered_preview_views() {
    let mut bytes = Vec::new();
    for _ in 0..12 {
        bytes.extend(summary_preview_segment());
    }
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.preview_images.len(), 12);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store indexed preview views");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load indexed preview views");
    assert_eq!(
        loaded
            .preview_images
            .iter()
            .map(|preview| preview.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:preview#{index}"))
            .collect::<Vec<_>>()
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed preview view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };
    let mut missing = native.clone();
    missing.preview_images.pop();
    assert_rejected(missing);
    let mut invalid_width = native.clone();
    invalid_width.preview_images[0].width += 1;
    assert_rejected(invalid_width);
    let mut invalid_data = native;
    invalid_data.preview_images[0].data[0] = 0;
    assert_rejected(invalid_data);
}

#[test]
fn decode_retains_catalog_schema_names_without_promoting_features() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_catalog()),
            &DecodeOptions::default(),
        )
        .expect("decode generated catalog part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.catalogs[0].entries[4].value, "Sketch");
    assert_eq!(native.catalogs[0].entries[5].value, "Pad");
    assert_eq!(native.catalogs[0].entries[6].value, "GSMLoft");
    assert_eq!(native.catalogs[0].entries[7].value, "GSMPointBetweenValues");
    assert_eq!(native.catalogs[0].entries[8].value, "GSMPlaneAngle");
    assert!(decoded.ir().model.features.is_empty());
}

#[test]
fn decode_retains_value_blocks_at_their_schema_boundary() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_value_block()),
            &DecodeOptions::default(),
        )
        .expect("decode generated value block part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(
        native.value_blocks[0].byte_offset,
        u64::try_from(16 + object_graph_stream().len()).unwrap()
    );
    assert_eq!(native.value_blocks[0].byte_len, 16);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
    assert_eq!(
        native.value_blocks[0].object_graph.as_deref(),
        Some(native.object_graphs[0].id.as_str())
    );
    assert_eq!(
        native.value_blocks[0].payload,
        [0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82]
    );
    assert_eq!(native.value_blocks[0].schema_selections.len(), 1);
    assert_eq!(native.value_blocks[0].schema_selections[0].ordinal, 4);
    assert_eq!(
        native.value_blocks[0].schema_selections[0].entry.as_deref(),
        Some(native.catalogs[0].entries[4].id.as_str())
    );
    assert_eq!(
        native.value_blocks[0].schema_selections[0].name.as_deref(),
        Some("VPGlobal")
    );
    assert_eq!(
        native.value_blocks[0].schema_selections[0].encoded_value,
        [
            crate::value_block::ValueField::Atom {
                value: 3,
                width: 1,
                offset: 7,
            },
            crate::value_block::ValueField::Atom {
                value: 2,
                width: 1,
                offset: 8,
            },
        ]
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Attribute
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("1 visualization value block(s)")
            && loss
                .message
                .contains("1 schema-selected presentation value(s)")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("neutral features")
            && !loss.message.contains("value block")
    }));
}

#[test]
fn visualization_values_do_not_assert_missing_design_intent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_visualization_values_only()),
            &DecodeOptions::default(),
        )
        .expect("decode visualization-only values");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Attribute
            && loss.message.contains("schema-selected presentation value")
    }));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.category() != cadmpeg_ir::report::LossCategory::DesignIntent));
}

#[test]
fn decode_does_not_promote_operation_field_class_names_to_features() {
    for class in ["Groove", "GSMHelix"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_design_class(class)),
                &DecodeOptions::default(),
            )
            .expect("decode field-class vocabulary");

        assert!(decoded.ir().model.features.is_empty());
        let native = crate::native::CatiaNative::load(
            decoded
                .ir()
                .native
                .namespace("catia")
                .expect("CATIA native namespace"),
        )
        .expect("load retained field-class vocabulary");
        assert_eq!(
            native.design_objects[0]
                .field_classes
                .iter()
                .map(|class| class.name.as_str())
                .collect::<Vec<_>>(),
            ["CurrentFeature", class]
        );
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
                && loss.message.contains("neutral features")
        }));
    }
}

#[test]
fn outer_surface_alias_parser_reads_fixed_core() {
    use crate::object_graph::AliasLead;

    let rows = crate::object_graph::surface_aliases(&surface_alias_stream());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].lead, AliasLead::SurfaceSupportStorage);
    assert_eq!(rows[0].tag, 0x0012_3456);
    assert_eq!(rows[0].tag_raw, 0xab12_3456);
    assert_eq!(rows[0].entity_record_ordinal, 7);
    assert_eq!((rows[0].f2, rows[0].f3), (0x1122_3344, 0x5566_7788));
}

#[test]
fn outer_alias_parser_classifies_both_ordinal_linked_storage_leads() {
    use crate::object_graph::AliasLead;

    for (lead, expected) in [
        (0x8eu32, AliasLead::E5LinkedSurfaceStorage),
        (0x8fu32, AliasLead::OrdinalLinkedStorage8f),
    ] {
        let mut bytes = surface_alias_stream();
        bytes[..4].copy_from_slice(&lead.to_le_bytes());
        let [row] = crate::object_graph::surface_aliases(&bytes)
            .try_into()
            .expect("one ordinal-linked alias row");
        assert_eq!(row.lead, expected);
        assert_eq!(row.entity_record_ordinal, 7);
    }
}

#[test]
fn outer_alias_parser_rejects_marker_literals_without_an_alias_lead() {
    for lead in [0u32, 0x15] {
        let mut bytes = surface_alias_stream();
        bytes[..4].copy_from_slice(&lead.to_le_bytes());
        assert!(crate::object_graph::surface_aliases(&bytes).is_empty());
    }
}

#[test]
fn outer_alias_parser_closes_group_header_and_overlapping_target_slot() {
    let mut bytes = vec![0x02, 0x00];
    bytes.extend_from_slice(&0xafu32.to_le_bytes());
    bytes.extend_from_slice(&0x148u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
    let mut alias = surface_alias_stream();
    alias[15..19].copy_from_slice(&0x0000_017bu32.to_le_bytes());
    bytes.extend(alias);

    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one grouped alias row");
    let group = row.group.expect("exact group header");
    assert_eq!(group.prototype, 0xaf);
    assert_eq!(group.group_id, 0x148);
    assert_eq!(group.target_slot, 0x17b);
    assert_eq!(group.storage_prefix, [0x01, 0x00, 0x00, 0x00]);
    assert_eq!(row.entity_record_ordinal, 0x7b);

    bytes[10] = 1;
    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one ungrouped alias row");
    assert!(row.group.is_none());
}

#[test]
fn outer_alias_group_parser_accepts_each_bounded_storage_prefix() {
    for storage in [
        &[0x00, 0x00, 0x00][..],
        &[0x01, 0x00, 0x00, 0x00],
        &[0x01, 0x01, 0x00, 0x7c, 0x02, 0x00, 0x00],
        &[0x01, 0x00, 0x01, 0x00, 0x7c, 0x02, 0x00, 0x00],
    ] {
        let mut bytes = vec![0x02, 0x00];
        bytes.extend_from_slice(&0xafu32.to_le_bytes());
        bytes.extend_from_slice(&0x147u32.to_le_bytes());
        bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
        bytes.extend_from_slice(storage);
        let mut alias = surface_alias_stream();
        alias.drain(..4);
        alias[11..15].copy_from_slice(&0x0000_017du32.to_le_bytes());
        bytes.extend(alias);

        let [row] = crate::object_graph::surface_aliases(&bytes)
            .try_into()
            .expect("one grouped alias row");
        let group = row.group.expect("bounded group storage");
        assert_eq!(group.storage_prefix, storage);
        assert_eq!(group.target_slot, 0x17d);
    }
}

#[test]
fn native_namespace_retains_and_validates_alias_group_membership() {
    let mut bytes = vec![0x02, 0x00];
    bytes.extend_from_slice(&0xafu32.to_le_bytes());
    bytes.extend_from_slice(&0x148u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
    let mut alias = surface_alias_stream();
    alias[15..19].copy_from_slice(&0x0000_017bu32.to_le_bytes());
    bytes.extend(alias);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.alias_rows[0]
            .group
            .as_ref()
            .expect("group membership")
            .target_slot,
        0x17b
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store grouped alias row");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load grouped alias row");
    assert_eq!(loaded, native);

    let mut invalid = native;
    invalid.alias_rows[0]
        .group
        .as_mut()
        .expect("group membership")
        .target_slot += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid grouped alias row");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid = loaded;
    invalid.alias_rows[0]
        .group
        .as_mut()
        .expect("group membership")
        .storage_prefix = vec![2, 0, 0];
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid group storage");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn outer_surface_alias_parser_retains_zero_low_tag_bits() {
    let mut bytes = surface_alias_stream();
    bytes[8..12].copy_from_slice(&0xab00_0000u32.to_le_bytes());

    let rows = crate::object_graph::surface_aliases(&bytes);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tag, 0);
    assert_eq!(rows[0].tag_raw, 0xab00_0000);
    assert_eq!(rows[0].entity_record_ordinal, 7);
    assert_eq!((rows[0].f2, rows[0].f3), (0x1122_3344, 0x5566_7788));
}

#[test]
fn outer_surface_alias_parser_requires_the_lead_word() {
    let bytes = surface_alias_stream();
    assert!(crate::object_graph::surface_aliases(&bytes[4..]).is_empty());
}

#[test]
fn native_namespace_retains_surface_alias_core() {
    let native = crate::native::CatiaNative::decode(&surface_alias_stream());
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    assert_eq!(row.byte_offset, 4);
    assert_eq!(row.tag, 0x0012_3456);
    assert_eq!(row.tag_raw, 0xab12_3456);
    assert_eq!(row.entity_record_ordinal, 7);
    assert!(row.design_object.is_none());
    assert_eq!((row.f2, row.f3), (0x1122_3344, 0x5566_7788));
    assert!(row.group.is_none());

    let mut invalid = native;
    invalid.alias_rows[0].design_object = Some("catia:missing-design-object".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store unresolved alias with a design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_alias_f1_without_part_container_remains_unbound() {
    let graph = object_graph_stream();
    let mut alias = surface_alias_stream();
    alias[13..16].copy_from_slice(&[3, 0, 2]);
    let mut bytes = graph;
    bytes.extend(alias);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    assert!(row.object_graph.is_none());
    assert!(row.object_record.is_none());
    assert!(row.design_object.is_none());

    let mut invalid = native;
    invalid.alias_rows[0].design_object = Some("catia:missing-design-object".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid alias design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_alias_f1_resolves_record_in_declared_part_container() {
    let mut stream = object_graph_stream();
    let mut alias = surface_alias_stream();
    alias[13..16].copy_from_slice(&[3, 0, 2]);
    stream.extend(alias);
    let (bytes, _) = outer_container_catpart(&stream);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| {
            graph
                .outer_container
                .as_ref()
                .is_some_and(|container| container.class_name == "CATPrtCont")
        })
        .expect("declared part-container graph");
    let record = &graph.records[1];
    assert_eq!(row.object_graph.as_deref(), Some(graph.id.as_str()));
    assert_eq!(row.object_record.as_deref(), Some(record.id.as_str()));
    assert_eq!(row.design_object, record.design_object);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store alias linked to declared part container");
    crate::native::CatiaNative::load(&namespace)
        .expect("load alias linked to declared part container");
}

#[test]
fn unresolved_7cd9_scanner_preserves_bounded_context_and_spacing() {
    let markers = crate::object_graph::markers_7cd9(&marker_7cd9_stream(), 5);
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].pos, 1);
    assert_eq!(markers[0].context, [0x7c, 0xd9, 1, 2, 3]);
    assert_eq!(markers[0].next_delta, Some(5));
    assert_eq!(markers[1].next_delta, None);
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

#[test]
fn container_only_stops_before_geometry() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let opts = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let result = CatiaCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report().geometry_transferred);
    assert!(result.report().container_only);
    // The reconstructed BREP stream is preserved as an unknown passthrough.
    let unknowns = result.ir().native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = &result.source_fidelity().retained_records[0];
    assert_eq!(retained.sha256.len(), 64);
    assert!(retained.data.is_some());
}

#[test]
fn every_decode_path_populates_v1_annotations() {
    let fixtures = [
        standard_catpart(),
        fbb_only_catpart(),
        zero_entity_catpart(),
        zero_entity_cylinder_catpart(),
        e5_catpart(),
        a8_catpart(),
        inner_no_directory_a8_catpart(),
    ];
    for fixture in fixtures {
        let decoded = CatiaCodec
            .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
            .unwrap();
        assert_every_entity_has_v1_annotation(decoded.ir(), &decoded.source_fidelity().annotations);
    }

    let container_only = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_every_entity_has_v1_annotation(
        container_only.ir(),
        &container_only.source_fidelity().annotations,
    );
}

#[path = "integration_tests.rs"]
mod integration_tests;
