// SPDX-License-Identifier: Apache-2.0
//! Synthetic byte-literal tests for the container framing and honest decode.
//!
//! No external CAD file is used; every fixture is a hand-built PSB byte image
//! exercising the `#UGC:2` framing, the `#\n#<name>\n` section-boundary rule, the
//! ND/DEPDB layout signals, and the `srf_array`/`crv_array` count headers.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

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

fn jpeg_payload() -> Vec<u8> {
    vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]
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
    assert_eq!(result.ir.surfaces.len(), 1);
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
    assert_eq!(result.ir.unknowns.len(), 2);
    assert!(result
        .ir
        .unknowns
        .iter()
        .any(|u| u.id.0.contains("VisibGeom")));
    assert!(result
        .ir
        .unknowns
        .iter()
        .any(|u| u.id.0.contains("NovisGeom")));
    // No geometry arenas populated.
    assert!(result.ir.surfaces.is_empty());
    assert!(result.ir.points.is_empty());
    assert!(result.ir.faces.is_empty());
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
