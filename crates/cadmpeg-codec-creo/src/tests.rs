// SPDX-License-Identifier: Apache-2.0
//! Synthetic byte-literal tests for the container framing and honest decode.
//!
//! No external CAD file is used; every fixture is a hand-built PSB byte image
//! exercising the `#UGC:2` framing, the `#\n#<name>\n` section-boundary rule, the
//! ND/DEPDB layout signals, and the `srf_array`/`crv_array` count headers.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::{decode, CreoCodec};

/// Assemble a minimal PSB file: the `#UGC:2` header, a TOC, then the given
/// `(header_name, payload)` sections joined by the `#\n` terminator rule.
fn build_prt(version: &str, sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("#UGC:2 P {version}\n").as_bytes());
    out.extend_from_slice(b"#-END_OF_UGC_HEADER\n");
    out.extend_from_slice(b"#UGC_TOC\n");
    out.extend_from_slice(b"toc entry line\n");
    out.extend_from_slice(b"#END_OF_TOC_HEADER\n");
    for (name, payload) in sections {
        // The previous payload's terminator `#` plus `\n` precede each header;
        // for the first section the TOC's trailing newline serves as the `\n`.
        out.push(b'#');
        out.push(b'\n');
        out.push(b'#');
        out.extend_from_slice(name.as_bytes());
        out.push(b'\n');
        out.extend_from_slice(payload);
    }
    out
}

/// A `VisibGeom` payload with byte-backed `srf_array`/`crv_array` count headers.
fn visibgeom_payload(srf: u8, crv: u8) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"srf_array\0");
    p.extend_from_slice(&[0xf8, srf]); // f8 <count>
    p.extend_from_slice(&[0xe0, 0x22, b'p', 0]); // some noise resembling a row
    p.extend_from_slice(b"crv_array\0");
    p.extend_from_slice(&[0xf3, 0xf8, crv]); // [f3] f8 <count>
    p
}

fn push_generated_scalar(bytes: &mut Vec<u8>, value: f64) {
    match value {
        0.0 => bytes.push(0x0f),
        1.0 => bytes.push(0xe4),
        -1.0 => bytes.extend_from_slice(&[0x43, 0xf0, 0x00]),
        2.0 => bytes.extend_from_slice(&[0x2f, 0x00, 0x00]),
        -2.0 => bytes.extend_from_slice(&[0x48, 0x00, 0x00]),
        0.5 => {
            bytes.push(0x71);
            bytes.extend_from_slice(&value.to_be_bytes()[1..]);
        }
        _ => panic!("generated fixture scalar is not encoded"),
    }
}

fn push_generated_plane_row(
    payload: &mut Vec<u8>,
    surface_id: u8,
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    origin: [f64; 3],
) {
    payload.extend_from_slice(&[surface_id, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    for value in u_axis
        .into_iter()
        .chain([0.0; 3])
        .chain(v_axis)
        .chain(origin)
    {
        push_generated_scalar(payload, value);
    }
    payload.push(0xe3);
}

fn push_generated_topology_row(
    payload: &mut Vec<u8>,
    curve_id: u8,
    faces: [u8; 2],
    next_edges: [u8; 2],
) {
    payload.extend_from_slice(&[curve_id, 0x08, 0x04, 0x01, 0xf6]);
    payload.extend_from_slice(&faces);
    payload.extend_from_slice(&next_edges);
    payload.extend_from_slice(&[0, 0, 0xe3, 0xe1, 0xf5, 0x05, 0xf6, 0xe3]);
}

fn jpeg_payload() -> Vec<u8> {
    vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]
}

fn assert_annotation(
    annotations: &cadmpeg_ir::Annotations,
    id: &str,
    stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let provenance = &annotations.provenance[id];
    assert_eq!(annotations.streams[provenance.stream as usize], stream);
    assert_eq!(provenance.offset, offset);
    assert_eq!(provenance.tag.as_deref(), Some(tag));
    if exactness == Exactness::ByteExact {
        assert!(!annotations.exactness.contains_key(id));
    } else {
        assert_eq!(annotations.exactness[id].entity, exactness);
        assert!(annotations.exactness[id].fields.is_empty());
    }
}

#[test]
fn detect_matches_ugc_magic_only() {
    let codec = CreoCodec;
    assert_eq!(codec.detect(b"#UGC:2 P foo"), Confidence::High);
    // A Siemens NX `.prt` (shares the extension) must not be claimed here.
    assert_eq!(codec.detect(b"\x0e\x93\x13\x01NX"), Confidence::No);
    assert_eq!(codec.detect(b"PK\x03\x04"), Confidence::No);
    assert_eq!(codec.detect(b""), Confidence::No);
}

#[test]
fn scan_enumerates_and_classifies_sections() {
    let data = build_prt(
        "test",
        &[
            ("VisibGeom", visibgeom_payload(5, 12)),
            ("AllFeatur", vec![0x01, 0x02, 0x03]),
            ("THMB_IMG_MAIN", jpeg_payload()),
        ],
    );
    let scan = container::scan_bytes(data);

    assert_eq!(scan.version_line, "#UGC:2 P test");
    assert_eq!(scan.sections.len(), 3);
    assert_eq!(scan.sections[0].name, "VisibGeom");
    assert_eq!(scan.sections[0].role, role::GEOMETRY);
    assert_eq!(scan.sections[1].name, "AllFeatur");
    assert_eq!(scan.sections[1].role, role::MODEL_DATA);
    assert_eq!(scan.sections[2].role, role::THUMBNAIL);
    assert!(container::has_thumbnail(&scan));
}

#[test]
fn scan_reads_namespace_counts() {
    let data = build_prt("c", &[("VisibGeom", visibgeom_payload(5, 12))]);
    let scan = container::scan_bytes(data);
    assert_eq!(scan.census.srf_array_count, Some(5));
    assert_eq!(scan.census.crv_array_count, Some(12));
}

#[test]
fn scan_sums_concatenated_depdb_surface_namespaces() {
    let mut payload = visibgeom_payload(3, 4);
    payload.extend_from_slice(&visibgeom_payload(5, 6));
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert_eq!(scan.layout, Layout::Depdb);
    assert_eq!(scan.census.srf_array_count, Some(8));
    assert_eq!(scan.census.crv_array_count, Some(10));
}

#[test]
fn scan_discovers_typed_surface_rows() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 0x01, 0]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surface_rows.len(), 2);
    assert_eq!(scan.surface_rows[0].id, 7);
    assert_eq!(scan.surface_rows[1].id, 8);
}

#[test]
fn scan_bounds_surface_parameter_bodies_and_decodes_scalars() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 6, 0]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surface_parameters.len(), 2);
    assert_eq!(scan.surface_parameters[0].surface_id, 7);
    assert_eq!(scan.surface_parameters[0].body, vec![0x0f, 0xe4]);
    assert_eq!(scan.surface_parameters[0].scalar_values, vec![0.0, 1.0]);
    assert_eq!(
        scan.surface_parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
    assert_eq!(scan.surface_parameters[1].surface_id, 8);
    assert_eq!(scan.surface_parameters[1].scalar_values, vec![3.0]);
    assert_eq!(
        scan.surface_parameters[1].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn scan_ignores_surface_header_candidates_inside_a_preceding_header() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0x24]);
    payload.extend_from_slice(&[0x22, 4, 0x01, 0, 0, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surface_parameters.len(), 1);
    assert_eq!(scan.surface_parameters[0].surface_id, 7);
}

#[test]
fn scan_decodes_plane_local_system_support_frame() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    payload.extend_from_slice(&[
        0x0f, 0xe4, 0x0f, // first in-plane direction
        0x0f, 0x0f, 0x0f, // structural zero row
        0xe4, 0x0f, 0x0f, // second in-plane direction
    ]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.plane_local_systems.len(), 1);
    let frame = &scan.plane_local_systems[0];
    assert_eq!(frame.surface_id, 7);
    assert_eq!(frame.slots.len(), 12);
    assert_eq!(frame.origin, Some([3.0, 0.0, 1.0]));
    assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal, Some([0.0, 0.0, -1.0]));
    assert_eq!(
        frame.classification,
        crate::surface::LocalSystemClassification::Simple
    );
}

#[test]
fn scan_resolves_section_scalar_cache_in_surface_rows() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0, 0x18, 0x00, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surface_parameters.len(), 1);
    assert_eq!(scan.surface_parameters[0].surface_id, 7);
    assert_eq!(scan.surface_parameters[0].scalar_values, vec![3.0]);
}

#[test]
fn decode_transfers_complete_plane_local_system() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let expected_offset = container::scan_bytes(data.clone()).plane_local_systems[0].offset as u64;
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(result.ir.model.surfaces.len(), 1);
    let surface = &result.ir.model.surfaces[0];
    assert_eq!(surface.id.as_str(), "creo:visibgeom:surface#7");
    assert_eq!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(3.0, 0.0, 1.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
    assert_annotation(
        &result.source_fidelity.annotations,
        surface.id.as_str(),
        "creo:VisibGeom",
        expected_offset,
        "plane_local_system",
        Exactness::Derived,
    );
    assert!(result.report.geometry_transferred);
}

#[test]
fn scan_decodes_standard_and_compact_plane_envelopes() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f, 0xe4]);
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x22, 4, 0xf6, 0, 0, 0x0e]);
    payload.extend_from_slice(&[0xe4, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f, 0xe4]);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.plane_envelopes.len(), 2);
    let crate::surface::PlaneEnvelope::Standard {
        bounds_2d,
        corners_3d,
    } = &scan.plane_envelopes[0].envelope
    else {
        panic!("standard plane envelope");
    };
    assert_eq!(*bounds_2d, [[Some(0.0), Some(1.0)], [Some(1.0), Some(0.0)]]);
    assert_eq!(
        *corners_3d,
        [
            [Some(0.0), Some(0.0), Some(1.0)],
            [Some(1.0), Some(0.0), Some(1.0)]
        ]
    );
    let crate::surface::PlaneEnvelope::Compact { prefix, corners_3d } =
        &scan.plane_envelopes[1].envelope
    else {
        panic!("compact plane envelope");
    };
    assert_eq!(*prefix, [Some(1.0), Some(0.0), Some(1.0)]);
    assert_eq!(
        *corners_3d,
        [
            [Some(0.0), Some(0.0), Some(1.0)],
            [Some(1.0), Some(0.0), Some(1.0)]
        ]
    );
}

#[test]
fn scan_discovers_labeled_surface_namespace_row() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(
        b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan
        .surface_rows
        .iter()
        .any(|row| { row.id == 7 && row.feature_id == 4 && row.next_surface == 0 }));
}

#[test]
fn scan_decodes_named_surface_prototype_parameter_wrappers() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cylinder)\0");
    payload.extend_from_slice(b"\xe0\x02local_sys\0\xf9\x04\x03");
    payload.extend([0xe4; 12]);
    payload.extend_from_slice(b"\xe0\x01radius\0\xe4");
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x02\x07\x08");
    payload.extend_from_slice(b"\xe0\x00i_pnts\0\xf8\x03\xf7\x80\x80\xfb");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surface_prototype_records.len(), 1);
    let prototype = &scan.surface_prototype_records[0];
    assert_eq!(
        prototype.family,
        crate::surface::SurfacePrototypeFamily::Cylinder
    );
    assert_eq!(
        prototype.field("local_sys").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::ScalarArray {
            dimensions: 4,
            count: 3,
            values: vec![Some(1.0); 12],
        })
    );
    assert_eq!(
        prototype.field("radius").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::ScalarSequence(vec![
            1.0
        ]))
    );
    assert_eq!(
        prototype.field("parent_feats").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactIntArray(vec![
            7, 8
        ]))
    );
    assert_eq!(
        prototype.field("i_pnts").map(|field| &field.value),
        Some(
            &crate::surface::SurfaceNamedValue::ContiguousEntityReferences {
                start_id: 128,
                entity_ids: vec![128, 129, 130],
            }
        )
    );
}

#[test]
fn scan_collects_feature_owners_from_rows_and_parent_lists() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"parent_feats\0\xf8\x02\x04\x09");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.feature_ids, vec![4, 9]);
}

#[test]
fn scan_binds_allfeatur_mixed_entity_table_to_known_feature() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, // feature row for owner 4
        0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, // two mixed entity references
        7, 0xe3, // a materialized surface id
        0xf7, 0x1e, 9, 0xe3, // a prefixed non-surface entity id
    ];
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_entity_tables.len(), 1);
    let table = &scan.feature_entity_tables[0];
    assert_eq!(table.feature_id, Some(4));
    assert_eq!(table.entry_ids, vec![7, 9]);
    assert_eq!(table.surface_ids, vec![7]);
    assert_eq!(table.non_surface_entity_ids, vec![9]);
}

#[test]
fn scan_bounds_known_allfeatur_feature_rows() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 9, 0x01, 0, 0]);
    let allfeatur = vec![4, 0xeb, 0x04, 0xaa, 0xbb, 9, 0x90, 0x01, 0xcc];
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_rows.len(), 2);
    assert_eq!(scan.feature_rows[0].feature_id, 4);
    assert_eq!(scan.feature_rows[0].header, [0xeb, 0x04]);
    assert_eq!(scan.feature_rows[0].body, vec![0xeb, 0x04, 0xaa, 0xbb]);
    assert_eq!(scan.feature_rows[1].feature_id, 9);
    assert_eq!(scan.feature_rows[1].body, vec![0x90, 0x01, 0xcc]);
}

#[test]
fn scan_resolves_allfeatur_walker_order_entity_references() {
    let allfeatur = b"\xe0\x22first\0\xf7\x01\xe3\xe0\x24second\0\xf7\x00\xe3".to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("AllFeatur", allfeatur)]));

    assert_eq!(scan.feature_entities.len(), 2);
    assert_eq!(scan.feature_entities[0].entity_id, 0);
    assert_eq!(scan.feature_entities[0].name, "first");
    assert_eq!(scan.feature_entities[1].entity_id, 1);
    assert_eq!(scan.feature_entity_references.len(), 2);
    assert_eq!(scan.feature_entity_references[0].source_entity_id, Some(0));
    assert_eq!(scan.feature_entity_references[0].target_entity_id, 1);
    assert!(scan.feature_entity_references[0].target_resolved);
    assert_eq!(scan.feature_entity_references[1].source_entity_id, Some(1));
    assert_eq!(scan.feature_entity_references[1].target_entity_id, 0);
}

#[test]
fn scan_bounds_allfeatur_procedural_choice_spans() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur =
        b"\x04\xeb\x04\xe0\x22blend_choice\0\x11\x12\xe0\x24depth_choice\0\x07".to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_choices.len(), 2);
    assert_eq!(scan.feature_choices[0].feature_id, 4);
    assert_eq!(scan.feature_choices[0].label, "blend_choice");
    assert_eq!(scan.feature_choices[0].type_byte, Some(0x22));
    assert_eq!(scan.feature_choices[0].payload, vec![0x11, 0x12]);
    assert_eq!(scan.feature_choices[1].label, "depth_choice");
    assert_eq!(scan.feature_choices[1].payload, vec![0x07]);
}

#[test]
fn scan_decodes_allfeatur_choice_field_wrappers() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur =
        b"\x04\xeb\x04\xe0\x22blend_choice\0\xe0\x21count\0\x07\xe0\x22refs\0\xf8\x02\x03\x04"
            .to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_choice_fields.len(), 2);
    assert_eq!(scan.feature_choice_fields[0].name, "count");
    assert_eq!(
        scan.feature_choice_fields[0].value,
        crate::feature::FeatureFieldValue::CompactInt(7)
    );
    assert_eq!(scan.feature_choice_fields[1].name, "refs");
    assert_eq!(
        scan.feature_choice_fields[1].value,
        crate::feature::FeatureFieldValue::CompactIntArray(vec![3, 4])
    );
}

#[test]
fn scan_decodes_complete_allfeatur_f9_scalar_slots() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur =
        b"\x04\xeb\x04\xe0\x22blend_choice\0\xe0\x21values\0\xf9\x01\x03\x0f\xe4".to_vec();
    allfeatur.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(
        scan.feature_choice_fields[0].value,
        crate::feature::FeatureFieldValue::ScalarArray {
            dimensions: 1,
            count: 3,
            body: vec![0x0f, 0xe4, 0x46, 0x08, 0, 0, 0, 0, 0, 0],
            decoded_values: Some(vec![0.0, 1.0, 3.0]),
        }
    );
}

#[test]
fn scan_decodes_allfeatur_generated_geometry_manifest() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04edg_id_tab_ptr\0\xf1\xf8\x03\xf7\x53\xfb\xe3used_bodies\0\xf8\x01\xf7\x60\xfb\xe2".to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_geometry_tables.len(), 2);
    assert_eq!(scan.feature_geometry_tables[0].feature_id, 4);
    assert_eq!(
        scan.feature_geometry_tables[0].kind,
        crate::feature::FeatureGeometryTableKind::EdgeIds
    );
    assert_eq!(scan.feature_geometry_tables[0].count, 3);
    assert_eq!(scan.feature_geometry_tables[0].entity_class, 0x53);
    assert_eq!(
        scan.feature_geometry_tables[1].kind,
        crate::feature::FeatureGeometryTableKind::UsedBodies
    );
}

#[test]
fn scan_decodes_allfeatur_affected_id_arrays() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\xe0\x21geoms_affected\0\xf8\x03\x07\x80\x80\x09\xe0\x22contours\0\xf8\x01\x2a".to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_affected_ids.len(), 2);
    assert_eq!(
        scan.feature_affected_ids[0].kind,
        crate::feature::AffectedIdKind::Geometry
    );
    assert_eq!(scan.feature_affected_ids[0].ids, vec![7, 128, 9]);
    assert_eq!(
        scan.feature_affected_ids[1].kind,
        crate::feature::AffectedIdKind::Contours
    );
    assert_eq!(scan.feature_affected_ids[1].ids, vec![42]);
}

#[test]
fn scan_decodes_allfeatur_positional_replay_affected_ids() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur =
        b"\x04\xeb\x04\xf1\xf7\x42\xd8\x80\x01\xe3\xf8\x03\x07\x80\x80\x09".to_vec();
    allfeatur.extend_from_slice(&[0xf5, 0x96, 0x92]);
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_replay_affected_ids.len(), 1);
    assert_eq!(scan.feature_replay_affected_ids[0].feature_id, 4);
    assert_eq!(scan.feature_replay_affected_ids[0].ids, vec![7, 128, 9]);
    assert!(scan.feature_replay_affected_ids[0].has_count_opener);
}

#[test]
fn scan_preserves_allfeatur_recipe_direction_bytes() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\xe0\x21geoms_affected\0\xf8\x01\x07\xe0\x20direction\0\x00\xe0\x20direction2\0\x43".to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert_eq!(scan.feature_direction_bytes.len(), 2);
    assert_eq!(
        scan.feature_direction_bytes[0].value,
        crate::feature::DirectionValue::SideFlag(false)
    );
    assert_eq!(
        scan.feature_direction_bytes[1].value,
        crate::feature::DirectionValue::Raw(0x43)
    );
}

#[test]
fn scan_decodes_featdefs_records_and_parameter_frames() {
    let mut payload = b"feat_defs_40\0local_sys\0\xf9\x04\x03".to_vec();
    payload.extend([0x0f; 12]);
    payload.extend_from_slice(b"\xe0\x21transf\0\xf9\x04\x03");
    payload.extend([0xe4; 12]);
    payload.extend_from_slice(b"feat_defs_81\0opaque");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    assert_eq!(scan.feature_definitions.len(), 2);
    assert_eq!(scan.feature_definitions[0].id, 40);
    assert_eq!(scan.feature_definitions[0].parameter_frames.len(), 2);
    assert_eq!(
        scan.feature_definitions[0].parameter_frames[0].kind,
        crate::feature::FeatureParameterFrameKind::LocalSystem
    );
    assert_eq!(
        scan.feature_definitions[0].parameter_frames[0].decoded_values,
        Some(vec![0.0; 12])
    );
    assert_eq!(
        scan.feature_definitions[0].parameter_frames[1].kind,
        crate::feature::FeatureParameterFrameKind::Transform
    );
    assert_eq!(
        scan.feature_definitions[0].parameter_frames[1].decoded_values,
        Some(vec![1.0; 12])
    );
}

#[test]
fn scan_decodes_featdefs_feature_local_outlines() {
    let mut payload = b"feat_defs_40\0\xe0\x00feat_outl_info\0outline\0\xf9\x02\x03".to_vec();
    payload.extend([0x0f; 6]);
    payload.extend_from_slice(b"\xe0\x00post_roll_back\0\xe3\xf7\x01\xf5\x96\x92\x02");
    payload.extend([0xe4; 6]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let outlines = &scan.feature_definitions[0].outlines;
    assert_eq!(outlines.len(), 2);
    assert_eq!(outlines[0].phase, crate::feature::OutlinePhase::PreRollback);
    assert_eq!(outlines[0].local_values, vec![Some(0.0); 6]);
    assert_eq!(
        outlines[1].phase,
        crate::feature::OutlinePhase::PostRollback
    );
    assert_eq!(outlines[1].local_values, vec![Some(1.0); 6]);
}

#[test]
fn scan_decodes_featdefs_var_arr_section_points() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let variables = scan.feature_definitions[0]
        .variables
        .as_ref()
        .expect("var_arr");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(1));
    assert_eq!(variables.rows.len(), 2);
    assert_eq!(variables.rows[0].value, Some(1.0));
    assert_eq!(variables.rows[1].value, Some(3.0));
    assert_eq!(variables.points.len(), 1);
    assert_eq!(variables.points[0].point_id, 7);
    assert_eq!(variables.points[0].u, Some(1.0));
    assert_eq!(variables.points[0].v, Some(3.0));
}

#[test]
fn decode_transfers_featdefs_sketch_variables_as_native_design_data() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let offset = container::scan_bytes(data.clone()).feature_definitions[0].offset as u64;
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    let namespace = result.ir.native.namespace("creo").expect("creo namespace");
    assert_eq!(namespace.version, 1);
    let sketches = &namespace.arenas["sketches"];
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].id, "creo:featdefs:sketch#40");
    let variables = sketches[0].fields["variables"]
        .as_array()
        .expect("variables array");
    assert_eq!(variables.len(), 2);
    assert_eq!(variables[0]["key"], 7);
    assert_eq!(variables[0]["value"], 1.0);
    assert_eq!(variables[1]["value"], 3.0);
    assert_annotation(
        &result.source_fidelity.annotations,
        "creo:featdefs:sketch#40",
        "creo:FeatDefs",
        offset,
        "feature_sketch",
        Exactness::Derived,
    );
}

#[test]
fn scan_decodes_featdefs_segtab_line_and_arc_rows() {
    let mut payload =
        b"feat_defs_40\0segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let segments = scan.feature_definitions[0]
        .segments
        .as_ref()
        .expect("segtab");
    assert_eq!(segments.declared_count, 2);
    assert_eq!(segments.rows.len(), 2);
    assert_eq!(
        segments.rows[0].kind,
        crate::feature::FeatureSegmentKind::Line
    );
    assert_eq!(segments.rows[0].point_ids, [7, 8]);
    assert_eq!(segments.rows[0].center_id, None);
    assert_eq!(segments.rows[0].external_id, 42);
    assert_eq!(
        segments.rows[1].kind,
        crate::feature::FeatureSegmentKind::Arc
    );
    assert_eq!(segments.rows[1].center_id, Some(10));
}

#[test]
fn scan_decodes_featdefs_ent_tab_trimmed_entities() {
    let mut payload =
        b"feat_defs_40\0ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3".to_vec();
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 101, 102, 103, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let entities = scan.feature_definitions[0]
        .trim_entities
        .as_ref()
        .expect("ent_tab");
    assert_eq!(entities.rows.len(), 2);
    assert_eq!(entities.rows[0].external_id, 42);
    assert_eq!(entities.rows[0].vertices, [100, 101]);
    assert_eq!(entities.rows[0].center_vertex, None);
    assert_eq!(entities.rows[0].kind, crate::feature::TrimEntityKind::Line);
    assert_eq!(entities.rows[1].kind, crate::feature::TrimEntityKind::Arc);
    assert_eq!(entities.solved_external_ids, vec![42, 43]);
}

#[test]
fn scan_decodes_featdefs_vert_tab_entity_pairs() {
    let mut payload =
        b"feat_defs_40\0ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3".to_vec();
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 100, 102, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0chains\0\xf8\x01\xf7\x80\xa2\xfb\xe2");
    payload.extend_from_slice(b"\xf3\xf7\x80\xa2\xe2\x01\xf8\x01\xf7\x80\xa3\xfb\xe3\xf7\x80\xa4");
    payload.extend_from_slice(&[42, 43, 100, 0]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let vertices = scan.feature_definitions[0]
        .trim_vertices
        .as_ref()
        .expect("vert_tab");
    assert_eq!(vertices.rows.len(), 1);
    assert_eq!(vertices.rows[0].vertex_id, 100);
    assert_eq!(vertices.rows[0].entities, [42, 43]);
}

#[test]
fn scan_solves_featdefs_trim_vertex_line_intersection() {
    fn variable_row(payload: &mut Vec<u8>, variable_type: u8, key: u8, value: f64) {
        payload.extend_from_slice(&[variable_type, key]);
        match value {
            0.0 => payload.push(0x0f),
            1.0 => payload.push(0xe4),
            2.0 => payload.extend_from_slice(&[0x46, 0, 0, 0, 0, 0, 0, 0]),
            _ => unreachable!("generated fixture uses defined scalar constants"),
        }
        payload.extend_from_slice(&[0x0f, 1, 0, key, 0xe2]);
    }

    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x08\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    for (point, u, v) in [(7, 0.0, 0.0), (8, 2.0, 2.0), (9, 0.0, 2.0), (10, 2.0, 0.0)] {
        variable_row(&mut payload, 1, point, u);
        variable_row(&mut payload, 2, point, v);
    }
    payload.extend_from_slice(b"\xffsegtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2");
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[2, 0, 0, 0, 9, 10, 0xf6, 0, 0, 0xf6, 0xf6, 43, 0xe2, 0xe3]);
    payload.extend_from_slice(b"ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3");
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 100, 102, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0chains\0\xf8\x01\xf7\x80\xa2\xfb\xe2");
    payload.extend_from_slice(b"\xf3\xf7\x80\xa2\xe2\x01\xf8\x01\xf7\x80\xa3\xfb\xe3\xf7\x80\xa4");
    payload.extend_from_slice(&[42, 43, 100, 0]);

    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));
    let vertex = &scan.feature_definitions[0]
        .trim_vertices
        .as_ref()
        .expect("vert_tab")
        .rows[0];
    assert_eq!(vertex.section_coordinates, Some([1.0, 1.0]));
}

#[test]
fn scan_decodes_featdefs_generated_entity_order_table() {
    let payload = b"feat_defs_40\0gsec3d_ptr\0order_table\0\xf8\x02\xf7\x81\x02\xfb\xe2\
        \xe0\x01ext_id\0\xe0\x01int_id\0\xe0\x01bitmask\0\
        \xf1\xf7\x81\x02\xe2\x81\x1b\x08\x00\xe2\x81\x36\x0c\x01\xe2"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let order = scan.feature_definitions[0]
        .order_table
        .as_ref()
        .expect("order_table");
    assert_eq!(order.declared_count, 2);
    assert_eq!(order.entity_ref, Some(258));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.rows[0].external_id, 283);
    assert_eq!(order.rows[0].internal_id, 8);
    assert_eq!(order.rows[0].bitmask, 0);
    assert_eq!(order.external_id(12), Some(310));
    assert_eq!(order.internal_id(283), Some(8));
}

#[test]
fn scan_decodes_featdefs_gsec3d_placement_references() {
    let payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\
        plane_id\0\x83\x01plane_flip\0\x01\
        \xe0\x00ref_planes\0\xf8\x02\xf7\x05\xf7\x81\x00\xfb\xe2\
        \xe0\x01plane_id\0\x09\
        \xe0\x01flip\0\x01\xe0\x01ref_type\0\x02\
        \xe0\x01seg_id\0\x81\x2c\xe0\x01flip_flag\0\x00\
        dim_id_tab\0\xf3\xf8\x02\x07\x81\x01"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let section = scan.feature_definitions[0]
        .section_3d
        .as_ref()
        .expect("gsec3d");
    assert_eq!(section.sketch_plane_entity_id, Some(769));
    assert_eq!(
        section.sketch_plane_flip,
        Some(crate::feature::BinaryFlag::Set)
    );
    assert_eq!(section.reference_plane_entity_ids, vec![5, 256]);
    assert_eq!(section.reference_plane_datum_geometry_id, Some(9));
    assert_eq!(
        section.orientation.section_flip,
        Some(crate::feature::BinaryFlag::Set)
    );
    assert_eq!(section.orientation.reference_type, Some(2));
    assert_eq!(section.orientation.segment_id, Some(300));
    assert_eq!(
        section.orientation.reference_flip,
        Some(crate::feature::BinaryFlag::Clear)
    );
    assert_eq!(section.dimension_ids, vec![7, 257]);
}

#[test]
fn scan_decodes_featdefs_dimension_prototype_and_replay() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x02\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a"
        .to_vec();
    payload.extend_from_slice(b"\xf3\xf7\x81\x02\xe2");
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0xe4, 43]);
    payload.extend_from_slice(b"\xe0\x00relat_ptr\0");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let dimensions = scan.feature_definitions[0]
        .dimensions
        .as_ref()
        .expect("dimtab");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(258));
    assert_eq!(dimensions.rows.len(), 2);
    assert_eq!(dimensions.rows[0].dimension_type, 10);
    assert_eq!(dimensions.rows[0].value, Some(1.0));
    assert_eq!(
        dimensions.rows[0].value_unit,
        crate::feature::DimensionUnit::Radians
    );
    assert_eq!(dimensions.rows[0].direction_byte, 1);
    assert_eq!(dimensions.rows[0].auxiliary_value, Some(0.0));
    assert_eq!(dimensions.rows[0].external_id, 42);
    assert_eq!(dimensions.rows[1].value, Some(3.0));
    assert_eq!(dimensions.rows[1].external_id, 43);
}

#[test]
fn scan_decodes_counted_featdefs_constraint_relations() {
    let payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x04\xf7\x6a\xfb\xe2\
        \xe0\x01id\0\xe0\x01used\0\xe0\x01type\0\xf1\xf7\x6a\xe2\
        \x34\x00\x05\x01\xe2\x35\x01\x07\x02\xe2"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let relations = scan.feature_definitions[0]
        .relations
        .as_ref()
        .expect("relat_ptr");
    assert_eq!(relations.declared_count, 4);
    assert_eq!(relations.entity_ref, Some(106));
    assert_eq!(relations.rows.len(), 2);
    assert_eq!(relations.rows[0].relation_id, 52);
    assert_eq!(relations.rows[0].used, 0);
    assert_eq!(relations.rows[0].body, [0x34, 0x00, 0x05, 0x01]);
    assert_eq!(relations.rows[1].relation_id, 53);
    assert_eq!(relations.rows[1].used, 1);
}

#[test]
fn scan_decodes_featdefs_saved_line_prototype_and_replay() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\
        \xe0\x00p_saved_result\0\xe3\
        \xe0\x00entity(line)\0\xe3\xf7\x01\x00\xf7\x02\xe2\
        \xf1\xf7\x03\x2a\xe2"
        .to_vec();
    payload.extend_from_slice(&[0x0f, 0xe4, 0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(b"\xf0\xf7\x04\xeb\x01\x02\x03\x04\x05\x2b\xe2");
    payload.extend_from_slice(&[0xe4, 0xe4, 0x0f, 0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(b"\xe0\x02local_sys\0");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let saved = scan.feature_definitions[0]
        .saved_section
        .as_ref()
        .expect("p_saved_result");
    assert_eq!(saved.entities.len(), 2);
    let crate::feature::FeatureSavedEntity::Line(first) = &saved.entities[0] else {
        panic!("saved line prototype");
    };
    assert_eq!(first.entity_id, 42);
    assert_eq!(first.references, vec![3]);
    assert_eq!(
        first.endpoints,
        [
            [Some(0.0), Some(1.0), Some(3.0)],
            [Some(1.0), Some(0.0), Some(1.0)]
        ]
    );
    let crate::feature::FeatureSavedEntity::Line(second) = &saved.entities[1] else {
        panic!("saved line replay");
    };
    assert_eq!(second.entity_id, 43);
    assert_eq!(second.references, vec![4]);
    assert_eq!(second.attributes, vec![[1, 2, 3, 4, 5]]);
}

#[test]
fn scan_decodes_featdefs_saved_circular_and_dummy_entities() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\
        \xe0\x00p_saved_result\0\xe3\
        \xe0\x00entity(arc)\0\xe0\x01id\0\x2c\
        \xe0\x02center\0\xf1\xf8\x03\x0f\xe4"
        .to_vec();
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"\xe0\x02radius\0\xe4");
    payload.extend_from_slice(b"\xe0\x02end1\0\xf8\x03\x0f\x0f\x0f");
    payload.extend_from_slice(b"\xe0\x02end2\0\xf8\x03\xe4\xe4\xe4");
    payload.extend_from_slice(b"\xe0\x02t0\0\x0f\xe0\x02t1\0\xe4");
    payload.extend_from_slice(
        b"\xe0\x00entity(circle)\0\xe0\x01id\0\x2d\
          \xe0\x02center\0\xf8\x03\xe4\x0f\xe4\
          \xe0\x02radius\0\xe4",
    );
    payload.extend_from_slice(b"\xe0\x00entity(dummy_ent)\0\xe0\x01id\0\x2e");
    payload.extend_from_slice(b"\xe0\x02local_sys\0");
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let entities = &scan.feature_definitions[0]
        .saved_section
        .as_ref()
        .expect("p_saved_result")
        .entities;
    assert_eq!(entities.len(), 3);
    let crate::feature::FeatureSavedEntity::Arc(arc) = &entities[0] else {
        panic!("saved arc");
    };
    assert_eq!(arc.entity_id, 44);
    assert_eq!(arc.center, [Some(0.0), Some(1.0), Some(3.0)]);
    assert_eq!(arc.radius, Some(1.0));
    assert_eq!(arc.parameters, [Some(0.0), Some(1.0)]);
    let crate::feature::FeatureSavedEntity::Circle(circle) = &entities[1] else {
        panic!("saved circle");
    };
    assert_eq!(circle.entity_id, 45);
    assert_eq!(circle.center, [Some(1.0), Some(0.0), Some(1.0)]);
    let crate::feature::FeatureSavedEntity::Dummy(dummy) = &entities[2] else {
        panic!("saved dummy");
    };
    assert_eq!(dummy.entity_id, Some(46));
}

#[test]
fn scan_reads_declared_geomlists_body_count() {
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("Geomlists", b"n_bodies\0\x83\x01".to_vec())],
    ));

    assert_eq!(scan.declared_body_count, Some(769));
}

#[test]
fn scan_reads_geomlists_first_quilt_discriminator() {
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("Geomlists", b"first_quilt_ptr\0\x00".to_vec())],
    ));

    assert_eq!(scan.first_quilt_ptr, Some(0));
}

#[test]
fn scan_discovers_labeled_curve_prototypes() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curve_prototypes.len(), 1);
    assert_eq!(scan.curve_prototypes[0].id, 7);
    assert_eq!(scan.curve_prototypes[0].type_byte, 8);
    assert_eq!(scan.curve_prototypes[0].feature_id, Some(4));
}

#[test]
fn scan_discovers_curve_halfedge_topology() {
    let mut payload = visibgeom_payload(0, 1);
    payload
        .extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curve_topology_rows.len(), 1);
    assert_eq!(scan.curve_topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curve_topology_rows[0].next_edges, [7, 7]);
    assert_eq!(scan.half_edges.len(), 2);
}

#[test]
fn scan_decodes_long_terminated_rows_in_each_curve_namespace() {
    let mut payload = b"crv_array\0topol_ref_data\0".to_vec();
    payload.extend_from_slice(b"\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3");
    payload.extend_from_slice(b"\xe1\xf5\x05\xf6\xe3");
    payload.extend_from_slice(b"crv_array\0topol_ref_data\0");
    payload.extend_from_slice(b"\x08\x08\x05\x01\xf6\x0c\x0d\x08\x08\0\0\xe3");
    payload.extend_from_slice(b"\xe1\xf5\x05\xf6\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curve_topology_rows.len(), 2);
    assert_eq!(scan.curve_topology_rows[0].id, 7);
    assert_eq!(scan.curve_topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curve_topology_rows[1].id, 8);
    assert_eq!(scan.curve_topology_rows[1].faces, [12, 13]);
}

#[test]
fn scan_bounds_curve_parameter_body_before_topology_suffix() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4, 0xf7, 0x81, 0x00]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curve_parameters.len(), 1);
    let parameters = &scan.curve_parameters[0];
    assert_eq!(parameters.curve_id, 7);
    assert_eq!(parameters.type_byte, 8);
    assert_eq!(parameters.scalar_values, vec![0.0, 1.0, 3.0]);
    assert_eq!(parameters.skipped_references, vec![256]);
    assert_eq!(parameters.suffix, crate::curve::CurveSuffixStatus::Unique);
    assert_eq!(parameters.body.last(), Some(&0xff));
}

#[test]
fn scan_resolves_section_scalar_cache_in_curve_rows() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x18, 0x00, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curve_parameters.len(), 1);
    assert_eq!(scan.curve_parameters[0].scalar_values, vec![3.0]);
}

#[test]
fn scan_decodes_pcurve_endpoints_in_both_face_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.pcurves.len(), 1);
    let pcurve = &scan.pcurves[0];
    assert_eq!(pcurve.curve_id, 7);
    assert_eq!(pcurve.faces, [10, 11]);
    assert_eq!(pcurve.face_0_endpoints, [[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(pcurve.face_1_endpoints, [[3.0, 0.0], [3.0, 1.0]]);
}

#[test]
fn scan_decodes_fc_curve_world_coordinate_lane() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x09\x04\x01\xf6\xfc\x08");
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x2d, 0, 0, 0, 0, 0, 0, 0, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.fc_curve_control_points.len(), 1);
    let control_points = &scan.fc_curve_control_points[0];
    assert_eq!(control_points.curve_id, 7);
    assert_eq!(control_points.subtype, 8);
    assert_eq!(control_points.values_mm, vec![3.0, -3.0, 2.0, -2.0]);
}

#[test]
fn scan_validates_fc05_circle_from_record_points() {
    fn world(payload: &mut Vec<u8>, value: f64) {
        let raw = value.to_be_bytes();
        payload.push(match raw[0] {
            0x40 => 0x46,
            0xc0 => 0x2d,
            _ => panic!("generated FC05 value must use a world-token exponent"),
        });
        payload.extend_from_slice(&raw[1..]);
    }

    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x09\x04\x01\xf6\xfc\x05");
    for [x, z, t, y] in [
        [4.0, 3.0, 2.0, 2.0],
        [3.0, 4.0, 3.0, 2.0],
        [2.0, 3.0, 4.0, 2.0],
        [3.0, 2.0, 3.0, 2.0],
    ] {
        world(&mut payload, x);
        world(&mut payload, z);
        world(&mut payload, t);
        world(&mut payload, y);
    }
    payload.push(0xff);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.fc05_circles.len(), 1);
    let circle = &scan.fc05_circles[0];
    assert_eq!(circle.curve_id, 7);
    assert_eq!(circle.center_row_frame, [3.0, 3.0]);
    assert_eq!(circle.radius_mm, 1.0);
    assert_eq!(circle.cap_ordinate_row_frame, Some(2.0));
    assert_eq!(circle.point_count, 4);
    assert_eq!(circle.max_residual, 0.0);
    assert!(!circle.angle_parameter_consistent);
}

#[test]
fn scan_decodes_labeled_prototype_pcurve_uvs() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.prototype_pcurves.len(), 1);
    let prototype = &scan.prototype_pcurves[0];
    assert_eq!(prototype.curve_id, 44);
    assert_eq!(prototype.face_0_endpoints, [[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(prototype.face_1_endpoints, [[3.0, 0.0], [3.0, 1.0]]);
}

#[test]
fn scan_decodes_and_binds_labeled_prototype_topology() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00");
    payload.extend_from_slice(b"crv_hdr_geom_ptr[0]\0\x0a crv_hdr_geom_ptr[1]\0\x0b");
    payload.extend_from_slice(b"next_crv_hdr_ptr[0]\0\x2c next_crv_hdr_ptr[1]\0\x2c");
    payload.extend_from_slice(b"crv_pnt_arr\0\xf9\x02\x04");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.push(0xe4);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curve_prototype_topology.len(), 1);
    assert_eq!(scan.curve_prototype_topology[0].curve_id, 44);
    assert_eq!(scan.curve_prototype_topology[0].faces, [10, 11]);
    assert_eq!(scan.curve_prototype_topology[0].next_edges, [44, 44]);
    assert_eq!(scan.bound_prototype_pcurves.len(), 1);
    assert_eq!(scan.bound_prototype_pcurves[0].faces, [10, 11]);
    assert_eq!(
        scan.bound_prototype_pcurves[0].face_0_endpoints,
        [[0.0, 1.0], [1.0, 0.0]]
    );
}

#[test]
fn scan_groups_connected_nonzero_face_references() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3\x08\x08\x04\x01\xf6\x0b\x0c\x08\x08\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.face_components.len(), 1);
    assert_eq!(scan.face_components[0].face_ids, vec![10, 11, 12]);
    assert_eq!(scan.face_components[0].curve_ids, vec![7, 8]);
}

#[test]
fn scan_builds_topological_vertex_orbits_and_incidence() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x08\x08\0\0\xe3\xe1\xe3\
          \x08\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topological_vertices.len(), 2);
    assert_eq!(
        scan.topological_vertices[0].half_edges,
        vec![
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 0
            },
            crate::topology::HalfEdgeId {
                curve_id: 8,
                side: 1
            },
        ]
    );
    let incidence = scan
        .half_edge_vertex_incidence
        .iter()
        .find(|incidence| {
            incidence.half_edge
                == crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: 0,
                }
        })
        .expect("half-edge incidence");
    assert_eq!(incidence.start_vertex_id, 1);
    assert_eq!(incidence.end_vertex_id, Some(2));
}

#[test]
fn decode_transfers_closed_plane_intersection_brep() {
    let mut payload = b"srf_array\0\xf8\x04".to_vec();
    push_generated_plane_row(
        &mut payload,
        1,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        2,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        3,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        4,
        [-2.0, -1.0, 2.0],
        [2.0, -2.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x06topol_ref_data\0");
    for (curve, faces, next) in [
        (10, [1, 2], [12, 13]),
        (11, [1, 3], [10, 15]),
        (12, [1, 4], [11, 14]),
        (13, [2, 3], [14, 11]),
        (14, [2, 4], [10, 15]),
        (15, [3, 4], [13, 12]),
    ] {
        push_generated_topology_row(&mut payload, curve, faces, next);
    }

    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.plane_local_systems.len(), 4);
    assert_eq!(scan.curve_topology_rows.len(), 6);
    assert_eq!(scan.loops.len(), 4);
    assert_eq!(scan.topological_vertices.len(), 4);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let model = &result.ir.model;

    assert_eq!(model.points.len(), 4);
    assert_eq!(model.vertices.len(), 4);
    assert_eq!(model.edges.len(), 6);
    assert_eq!(model.faces.len(), 4);
    assert_eq!(model.loops.len(), 4);
    assert_eq!(model.coedges.len(), 12);
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, cadmpeg_ir::topology::BodyKind::Solid);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn scan_discovers_model_space_datum_planes() {
    let mut datum = vec![4, 0x22, 1, 1, 0, 0];
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let scan = container::scan_bytes(build_prt("c", &[("ActDatums", datum)]));
    assert_eq!(scan.datum_planes.len(), 1);
    assert_eq!(scan.datum_planes[0].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn decode_transfers_exact_datum_plane_carrier() {
    let mut datum = vec![4, 0x22, 1, 1, 0, 0];
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let mut reader = Cursor::new(build_prt("c", &[("ActDatums", datum)]));
    let result = decode::decode(&mut reader, &DecodeOptions::default()).unwrap();
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.surfaces.len(), 1);
}

#[test]
fn decode_annotations_cover_every_emitted_entity() {
    let mut datum = vec![4, 0x22, 1, 1, 0, 0];
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", visibgeom_payload(1, 0)),
            ("NovisGeom", vec![0xaa, 0xbb]),
            ("ActDatums", datum),
        ],
    );
    let datum_offset = container::scan_bytes(data.clone()).datum_planes[0].offset_in_payload as u64;
    let mut reader = Cursor::new(data);
    let result = decode::decode(&mut reader, &DecodeOptions::default()).expect("decode");

    let unknowns = result.ir.native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    for unknown in &unknowns {
        let section_name = unknown
            .id
            .as_str()
            .strip_prefix("creo:")
            .and_then(|suffix| suffix.split_once(":section#"))
            .map(|(name, _)| name)
            .expect("unknown id contains its source section");
        let retained = result
            .source_fidelity
            .retained_records
            .iter()
            .find(|record| record.id == unknown.id.as_str())
            .expect("unknown source record");
        assert_annotation(
            &result.source_fidelity.annotations,
            unknown.id.as_str(),
            &format!("creo:{section_name}"),
            retained.offset,
            "psb_geometry_section",
            Exactness::Unknown,
        );
    }
    for surface in &result.ir.model.surfaces {
        assert_annotation(
            &result.source_fidelity.annotations,
            surface.id.as_str(),
            "creo:ActDatums",
            datum_offset,
            "datum_plane_outline",
            Exactness::Derived,
        );
    }
    let emitted_entity_count = unknowns.len() + result.ir.model.surfaces.len();
    assert_eq!(
        result.source_fidelity.annotations.provenance.len(),
        emitted_entity_count
    );
    assert_eq!(
        result.source_fidelity.annotations.exactness.len(),
        emitted_entity_count
    );
}

#[test]
fn scan_decodes_active_principal_unit() {
    let mut payload = visibgeom_payload(5, 12);
    payload.extend_from_slice(b"_principal_sys_units_id\0\x33");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data);

    assert_eq!(scan.principal_unit.as_deref(), Some("mmNs"));
}

#[test]
fn decode_transfers_mdlstatus_feature_operations_in_history_order() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"noise\0Extrude id 40\0Round id 41\0future id 42\0".to_vec(),
        )],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.feature_operations.len(), 2);
    assert_eq!(scan.feature_operations[0].feature_id, 40);
    assert_eq!(scan.feature_operations[0].kind, "Extrude");
    assert_eq!(scan.feature_operations[1].feature_id, 41);
    assert_eq!(scan.feature_operations[1].kind, "Round");

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(result.ir.model.features.len(), 2);
    assert_eq!(
        result.ir.model.features[0].id.as_str(),
        "creo:mdlstatus:feature#40"
    );
    assert_eq!(result.ir.model.features[0].ordinal, 0);
    assert_eq!(
        result.ir.model.features[1].id.as_str(),
        "creo:mdlstatus:feature#41"
    );
    assert_eq!(result.ir.model.features[1].ordinal, 1);
    assert!(matches!(
        &result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } if kind == "Extrude"
    ));
    assert_annotation(
        &result.source_fidelity.annotations,
        "creo:mdlstatus:feature#40",
        "creo:MdlStatus",
        scan.feature_operations[0].offset as u64,
        "feature_operation_name",
        Exactness::ByteExact,
    );
}

#[test]
fn nd_decoration_selects_nd_layout() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(3, 4))]);
    let scan = container::scan_bytes(data);
    assert_eq!(scan.layout, Layout::Nd);
    // The decorated name is normalized for classification and census.
    assert_eq!(scan.sections[0].name, "VisibGeom");
    assert_eq!(scan.sections[0].raw_name, "ND:0:VisibGeom:1");
    assert_eq!(scan.census.srf_array_count, Some(3));
}

#[test]
fn depdb_data_with_sparse_sections_selects_depdb() {
    let data = build_prt(
        "c",
        &[("VisibGeom", vec![0x00]), ("DEPDB_DATA", vec![0x00, 0x01])],
    );
    let scan = container::scan_bytes(data);
    assert_eq!(scan.layout, Layout::Depdb);
}

#[test]
fn framing_names_are_not_mistaken_for_sections() {
    let data = build_prt("c", &[("VisibGeom", vec![0x00])]);
    let scan = container::scan_bytes(data);
    // Only VisibGeom — the header/TOC framing markers are excluded.
    assert_eq!(scan.sections.len(), 1);
    assert_eq!(scan.sections[0].name, "VisibGeom");
}

#[test]
fn decode_is_honest_geometryless_with_preserved_sections() {
    let mut visible = visibgeom_payload(5, 12);
    visible.extend_from_slice(b"_principal_sys_units_id\0\x33");
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", visible),
            ("NovisGeom", vec![0xaa, 0xbb]),
            ("AllFeatur", vec![0x01]),
        ],
    );
    let mut reader = Cursor::new(data);
    let result = decode::decode(&mut reader, &DecodeOptions::default()).expect("decode");

    assert!(!result.report.geometry_transferred);
    // The two PSB geometry sections are preserved as unknown records.
    let unknowns = result.ir.native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 2);
    assert!(unknowns.iter().any(|u| u.id.0.contains("VisibGeom")));
    assert!(unknowns.iter().any(|u| u.id.0.contains("NovisGeom")));
    // No geometry arenas populated.
    assert!(result.ir.model.surfaces.is_empty());
    assert!(result.ir.model.points.is_empty());
    assert!(result.ir.model.faces.is_empty());
    // Source attributes carry the census.
    let source = result.ir.source.as_ref().expect("source");
    assert_eq!(
        source.attributes.get("srf_array_count").map(String::as_str),
        Some("5")
    );
    assert_eq!(
        source.attributes.get("crv_array_count").map(String::as_str),
        Some("12")
    );
    assert_eq!(
        source.attributes.get("principal_unit").map(String::as_str),
        Some("mmNs")
    );
    // A blocking loss note names the prototype-vs-instance limitation.
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("prototype")));
}

#[test]
fn inspect_summary_has_layout_and_census_notes() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(7, 9))]);
    let mut reader = Cursor::new(data);
    let summary = CreoCodec.inspect(&mut reader).expect("inspect");
    assert_eq!(summary.format, "creo");
    assert_eq!(summary.container_kind, "psb");
    assert!(summary.notes.iter().any(|n| n.contains("layout: ND")));
    assert!(summary.notes.iter().any(|n| n.contains("srf_array=7")));
}
