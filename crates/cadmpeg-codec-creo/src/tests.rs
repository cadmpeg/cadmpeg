// SPDX-License-Identifier: Apache-2.0
//! Synthetic byte-literal tests for the container framing and honest decode.
//!
//! No external CAD file is used; every fixture is a hand-built PSB byte image
//! exercising the `#UGC:2` framing, the `#\n#<name>\n` section-boundary rule, the
//! ND/DEPDB layout signals, and the `srf_array`/`crv_array` count headers.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::surface::TorusRadius2Encoding;
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
    reversed: bool,
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    origin: [f64; 3],
) {
    payload.extend_from_slice(&[
        surface_id,
        0x22,
        4,
        if reversed { 0xf6 } else { 0x01 },
        0,
        0,
    ]);
    let normal = [
        u_axis[1] * v_axis[2] - u_axis[2] * v_axis[1],
        u_axis[2] * v_axis[0] - u_axis[0] * v_axis[2],
        u_axis[0] * v_axis[1] - u_axis[1] * v_axis[0],
    ];
    let held_axis = (0..3).find(|axis| {
        normal[*axis].abs() > 1e-9
            && (0..3).all(|other| other == *axis || normal[other].abs() <= 1e-9)
    });
    let corners = held_axis.map_or([[0.0; 3]; 2], |axis| {
        let mut corners = [[-1.0, -1.0, -1.0], [1.0, 2.0, 2.0]];
        corners[0][axis] = origin[axis];
        corners[1][axis] = origin[axis];
        corners
    });
    for value in [0.0; 4].into_iter().chain(corners.into_iter().flatten()) {
        push_generated_scalar(payload, value);
    }
    payload.push(0xe3);
    for value in u_axis
        .into_iter()
        .chain(v_axis)
        .chain([0.0; 3])
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

fn push_named_analytic_prototype(payload: &mut Vec<u8>, family: &str, fields: &[(&str, f64)]) {
    payload.extend_from_slice(format!("srf_prim_ptr({family})\0").as_bytes());
    payload.extend_from_slice(b"\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        push_generated_scalar(payload, value);
    }
    payload.push(0x18);
    for (name, value) in fields {
        payload.extend_from_slice(b"\xe0\x01");
        payload.extend_from_slice(name.as_bytes());
        payload.push(0);
        if *name == "half_angle" {
            payload.extend_from_slice(&[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23]);
        } else {
            push_generated_scalar(payload, *value);
        }
    }
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
fn scan_decodes_length_prefixed_native_model_name() {
    let data = b"#UGC:2 PART test \\\n#- CMNM 00bwidget.prt                                      \\\n#-END_OF_UGC_HEADER\n"
        .to_vec();
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.framing.model_name.as_deref(), Some("widget.prt "));
    let model_name_offset = data
        .windows(b"widget.prt ".len())
        .position(|window| window == b"widget.prt ")
        .expect("model name offset");
    assert_eq!(scan.framing.model_name_offset, Some(model_name_offset));
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("model_name"))
            .map(String::as_str),
        Some("widget.prt ")
    );
    let [product] = result.ir.model.products.as_slice() else {
        panic!("one part product");
    };
    assert_eq!(product.id.as_str(), "creo:model:product#root");
    assert_eq!(product.product_id, "widget.prt ");
    assert_eq!(product.name.as_deref(), Some("widget.prt "));
    assert!(product.bodies.is_empty());
    let [occurrence] = result.ir.model.product_occurrences.as_slice() else {
        panic!("one root occurrence");
    };
    assert_eq!(occurrence.product, product.id);
    assert!(matches!(
        occurrence.parent,
        cadmpeg_ir::product::OccurrenceParent::Root
    ));
    assert_eq!(
        occurrence.transform,
        cadmpeg_ir::transform::Transform::identity()
    );
    assert_annotation(
        &result.source_fidelity.annotations,
        product.id.as_str(),
        "creo:archive_header",
        model_name_offset as u64,
        "part_product",
        Exactness::Derived,
    );
    assert_annotation(
        &result.source_fidelity.annotations,
        occurrence.id.as_str(),
        "creo:archive_header",
        model_name_offset as u64,
        "part_product_occurrence",
        Exactness::Derived,
    );
}

#[test]
fn scan_withholds_repeated_native_model_names() {
    let data = b"#UGC:2 PART test \\\n+#- CMNM 00awidget.prt                                      \\\n+#- CMNM 00bwidget2.prt                                     \\\n+#-END_OF_UGC_HEADER\n"
        .to_vec();

    let scan = container::scan_bytes(data);
    assert!(scan.framing.model_name.is_none());
    assert!(scan.framing.model_name_offset.is_none());
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

    assert_eq!(scan.framing.version_line, "#UGC:2 P test");
    assert_eq!(scan.framing.sections.len(), 3);
    assert_eq!(scan.framing.sections[0].name, "VisibGeom");
    assert_eq!(scan.framing.sections[0].role, role::GEOMETRY);
    assert_eq!(scan.framing.sections[1].name, "AllFeatur");
    assert_eq!(scan.framing.sections[1].role, role::MODEL_DATA);
    assert_eq!(scan.framing.sections[2].role, role::THUMBNAIL);
    assert!(container::has_thumbnail(&scan));
}

#[test]
fn scan_enumerates_toc_backed_compound_close_section_boundaries() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n#UGC_TOC\n\
        DEPDB_DATA 1 2 3\nVisibGeom 4 5 6\nAllFeatur 7 8 9\n\
        #END_OF_TOC_HEADER\n#DEPDB_DATA\nopaque"
        .to_vec();
    data.extend_from_slice(b"\xf1#VisibGeom\npacked\xf1#not_in_toc\ninside");
    data.extend_from_slice(b"\xf1#AllFeatur\nfeatures");

    let scan = container::scan_bytes(data);

    assert_eq!(
        scan.framing
            .sections
            .iter()
            .map(|section| section.name.as_str())
            .collect::<Vec<_>>(),
        ["DEPDB_DATA", "VisibGeom", "AllFeatur"]
    );
    assert_eq!(scan.framing.sections[1].role, role::GEOMETRY);
    assert_eq!(scan.framing.sections[2].role, role::MODEL_DATA);
}

#[test]
fn scan_uses_fixed_width_toc_offsets_for_adjacent_sections() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 2 81 17").as_bytes());
    let first_offset = 3 * 81;
    let first = b"#SolidPrimdata\nabc";
    let second_offset = first_offset + first.len();
    let second = b"#VisibGeom\nxyz";
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("SolidPrimdata {first_offset:x} {:x} 0", first.len())
        )
        .as_bytes(),
    );
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("VisibGeom {second_offset:x} {:x} 0", second.len())
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + first_offset);
    data.extend_from_slice(first);
    data.extend_from_slice(second);

    let scan = container::scan_bytes(data);

    assert_eq!(scan.framing.sections.len(), 2);
    assert_eq!(scan.framing.sections[0].name, "SolidPrimdata");
    assert_eq!(scan.framing.sections[0].length, first.len());
    assert_eq!(scan.framing.sections[1].name, "VisibGeom");
    assert_eq!(scan.framing.sections[1].offset, header_base + second_offset);
}

#[test]
fn scan_expands_toc_sized_unix_compress_payload() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 1 81 17").as_bytes());
    let section_offset = 2 * 81;
    let compressed = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
    let section_length = b"#SolidPrimdata\n".len() + compressed.len();
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("SolidPrimdata {section_offset:x} {section_length:x} 3")
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + section_offset);
    data.extend_from_slice(b"#SolidPrimdata\n");
    data.extend_from_slice(&compressed);

    let scan = container::scan_bytes(data);

    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].data, b"ABC");
    let summary = container::summarize(&scan);
    assert_eq!(summary.entries[0].compression, "unix-compress");
    assert_eq!(summary.entries[0].uncompressed_size, 18);
}

#[test]
fn decode_extracts_jpeg_thumbnail_as_native_asset() {
    let data = build_prt("c", &[("THMB_IMG_MAIN", jpeg_payload())]);
    let result = decode::decode(
        &mut Cursor::new(data),
        &DecodeOptions {
            container_only: true,
        },
    )
    .expect("decode thumbnail");

    assert!(!result.report.geometry_transferred);
    let unknowns = result.ir.native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = result
        .source_fidelity
        .retained_records
        .iter()
        .find(|record| record.id == unknowns[0].id.as_str())
        .expect("retained thumbnail");
    assert_eq!(retained.data.as_deref(), Some(jpeg_payload().as_slice()));
    assert_annotation(
        &result.source_fidelity.annotations,
        unknowns[0].id.as_str(),
        "creo:THMB_IMG_MAIN",
        retained.offset,
        "jpeg_thumbnail",
        Exactness::ByteExact,
    );
    let source = result.ir.source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["section_count"], "1");
    assert_eq!(source.attributes["section.0.name"], "THMB_IMG_MAIN");
    assert_eq!(source.attributes["section.0.raw_name"], "THMB_IMG_MAIN");
    assert_eq!(source.attributes["section.0.role"], role::THUMBNAIL);
    assert!(source.attributes["section.0.offset"]
        .parse::<usize>()
        .is_ok());
    assert!(source.attributes["section.0.length"]
        .parse::<usize>()
        .is_ok());
}

#[test]
fn scan_reads_namespace_counts() {
    let data = build_prt("c", &[("VisibGeom", visibgeom_payload(5, 12))]);
    let scan = container::scan_bytes(data);
    assert_eq!(scan.framing.census.srf_array_count, Some(5));
    assert_eq!(scan.framing.census.crv_array_count, Some(12));
}

#[test]
fn scan_sums_concatenated_depdb_surface_namespaces() {
    let mut payload = visibgeom_payload(3, 4);
    payload.extend_from_slice(&visibgeom_payload(5, 6));
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert_eq!(scan.framing.layout, Layout::Depdb);
    assert_eq!(scan.framing.census.srf_array_count, Some(8));
    assert_eq!(scan.framing.census.crv_array_count, Some(10));
}

#[test]
fn scan_does_not_treat_unlabeled_depdb_bytes_as_geometry_rows() {
    let payload = vec![7, 0x22, 4, 0x01, 0, 8, 8, 0x24, 4, 0xf6, 0x01, 0];
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert!(scan.surfaces.rows.is_empty());
    assert!(scan.surfaces.parameters.is_empty());
}

#[test]
fn scan_discovers_typed_surface_rows() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 0x01, 0]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.rows.len(), 2);
    assert_eq!(scan.surfaces.rows[0].id, 7);
    assert_eq!(scan.surfaces.rows[0].type_byte, 0x22);
    assert_eq!(scan.surfaces.rows[1].id, 8);
    assert_eq!(scan.surfaces.rows[1].type_byte, 0x24);
}

#[test]
fn scan_preserves_linear_extrusion_type_variants() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x2a, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[8, 0x2c, 4, 0x01, 0, 0]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.surfaces.rows.len(), 2);
    assert_eq!(
        scan.surfaces.rows[0].kind,
        crate::surface::SurfaceKind::Extrusion
    );
    assert_eq!(scan.surfaces.rows[0].type_byte, 0x2a);
    assert_eq!(
        scan.surfaces.rows[1].kind,
        crate::surface::SurfaceKind::Extrusion
    );
    assert_eq!(scan.surfaces.rows[1].type_byte, 0x2c);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let rows = &result.ir.native.namespace("creo").unwrap().arenas["surface_rows"];
    assert_eq!(rows[0].fields["surface_variant"], "ruled_surface");
    assert_eq!(rows[1].fields["surface_variant"], "tabulated_cylinder");
}

#[test]
fn scan_bounds_tabulated_cylinder_cubic_curve_replay() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        9, 0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7, 32, 0xfb, 0xe2,
        0xf7, 36,
    ]);
    for separator in [
        vec![0x18, 0xf1, 0xf7, 32, 0xe2],
        vec![0x18, 0xe2],
        vec![0x18, 0xe2],
        vec![0x18, 0xf2, 0xf7, 37, 0xf6, 0xe3],
    ] {
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&separator);
    }
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.tabulated_cylinder_replays.len(), 1);
    let replay = &scan.curves.tabulated_cylinder_replays[0];
    assert_eq!(replay.surface_id, 7);
    assert_eq!(replay.curve_id, 9);
    assert_eq!(replay.curve_type, 0x13);
    assert_eq!(replay.degree, 3);
    assert_eq!(replay.parameter_body, [0x18, 0xe6, 0x0f, 0xe6]);
    assert_eq!(replay.control_point_ids, [32, 33, 34, 35]);
    assert_eq!(replay.successor_reference, 36);
    assert_eq!(replay.control_point_bodies[0][0], 0x46);
    assert_eq!(replay.control_point_bodies[3][8], 0x46);
    assert_eq!(replay.control_points, [Some([-3.0, 3.0]); 4]);
    assert_eq!(replay.terminal_reference, 37);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let native =
        &result.ir.native.namespace("creo").unwrap().arenas["tabulated_cylinder_curve_replays"][0];
    assert_eq!(native.fields["surface_id"], 7);
    assert_eq!(native.fields["control_point_ids"][2], 34);
    assert_eq!(native.fields["control_point_bodies"][3][8], 0x46);
    assert_eq!(native.fields["control_points"][2][0], -3.0);
    assert_eq!(
        result.source_fidelity.annotations.provenance[&native.id]
            .tag
            .as_deref(),
        Some("tabulated_cylinder_curve_replay")
    );
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.0 != "creo:visibgeom:surface#7"));
}

#[test]
fn scan_bounds_surface_parameter_bodies_and_decodes_scalars() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 6, 0]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 2);
    assert_eq!(scan.surfaces.parameters[0].surface_id, 7);
    assert_eq!(scan.surfaces.parameters[0].body, vec![0x0f, 0xe4]);
    assert_eq!(scan.surfaces.parameters[0].scalar_values, vec![0.0, 1.0]);
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
    assert_eq!(scan.surfaces.parameters[1].surface_id, 8);
    assert_eq!(scan.surfaces.parameters[1].scalar_values, vec![3.0]);
    assert_eq!(
        scan.surfaces.parameters[1]
            .scalar_tokens
            .iter()
            .map(|token| (token.offset, token.length))
            .collect::<Vec<_>>(),
        [(0, 8)]
    );
    assert_eq!(
        scan.surfaces.parameters[1].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn decode_transfers_positional_line_extrusion_plane() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(&[0x00, 0x0c, 0x9a]);
    for value in [0.0, 0.0, 0.0, 2.0, 0.0, 0.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("extrusion plane");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            normal: cadmpeg_ir::math::Vector3 {
                x: 0.0,
                y: -1.0,
                z: 0.0
            },
            u_axis: cadmpeg_ir::math::Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
        }
    ));
    let carrier_id = surface.id.clone();
    let construction = result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface == carrier_id)
        .expect("extrusion construction");
    assert!(matches!(
        construction.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
            parameter_interval: None,
            direction: cadmpeg_ir::math::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            native_position: None,
            ..
        }
    ));
    let record = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(record.fields["surface_type_byte"], 0x2c);
    assert_eq!(record.fields["extrusion_direction"][0], 0.0);
    assert_eq!(record.fields["extrusion_direction"][1], 0.0);
    assert_eq!(record.fields["extrusion_direction"][2], 1.0);
    assert_eq!(
        result
            .report
            .coverage
            .get("decoded_positional_extrusion_direction_count")
            .copied(),
        Some(1)
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_positional_line_extrusion_for_duplicate_surface_id() {
    let mut extrusion = visibgeom_payload(1, 0);
    extrusion.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 1.0] {
        push_generated_scalar(&mut extrusion, value);
    }
    extrusion.extend_from_slice(&[0x00, 0x0c, 0x9a]);
    for value in [0.0, 0.0, 0.0, 2.0, 0.0, 0.0] {
        push_generated_scalar(&mut extrusion, value);
    }
    extrusion.push(0xe3);

    let mut plane = visibgeom_payload(1, 0);
    plane.extend_from_slice(&[7, 0x26, 5, 0x01, 0, 0, 0xe4, 0xe3]);
    let result = decode::decode(
        &mut Cursor::new(build_prt(
            "c",
            &[("ND:0:VisibGeom:0", extrusion), ("ND:1:VisibGeom:0", plane)],
        )),
        &DecodeOptions::default(),
    )
    .expect("decode");

    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#7"));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .all(|curve| curve.id.as_str() != "creo:visibgeom:surface_directrix#7"));
    assert!(result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface_extrusion#7"));
}

#[test]
fn decode_preserves_type_2c_direction_before_named_record() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0, 0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x00, 0x0c, 0x9a]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    let record = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(record.fields["boundary"], "named_record");
    assert_eq!(record.fields["extrusion_direction"][0], 0.0);
    assert_eq!(record.fields["extrusion_direction"][1], 1.0);
    assert_eq!(record.fields["extrusion_direction"][2], 0.0);
    assert!(result.ir.model.surfaces.is_empty());
}

#[test]
fn torus_family_does_not_shorten_unframed_negative_world_scalar() {
    let mut payload = visibgeom_payload(1, 0);
    let scalar = [0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66, 0x66, 0x66];
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].body, scalar);
    assert_eq!(scan.surfaces.parameters[0].scalar_tokens[0].length, 8);
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn torus_parameter_trailer_retains_typed_outline_frame() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x01, 0x12, 0x50, 0x50, 0x48, 0x68, 0x10, 0x48, 0x14, 0x00, 0x2d, 0x43, 0xff, 0xff, 0xff,
        0xa4, 0x41, 0x99, 0x48, 0x64, 0xf0, 0x48, 0x08, 0x00, 0x2f, 0x4a, 0x40,
    ]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    let frame = scan.surfaces.parameters[0]
        .torus_outline_frame(0x26)
        .expect("typed torus outline frame");
    assert_eq!(
        frame.values,
        [-192.5, -5.0, -39.999_999_957_278_48, -167.5, -3.0, 52.5]
    );
    assert_eq!(frame.selector, 80);
    assert_eq!(frame.offset, 0);
    assert!(scan.surfaces.parameters[0]
        .torus_outline_frame(0x24)
        .is_none());

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let native = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(native.fields["torus_outline_frame"]["selector"], 80);
    assert_eq!(native.fields["torus_outline_frame"]["values"][5], 52.5);
}

#[test]
fn torus_parameter_trailer_retains_tagged_radius_overrides() {
    let cases = [
        (
            vec![
                0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
            ],
            0.249_999_999_951_747_04,
            0.249_999_999_951_747_04,
            TorusRadius2Encoding::Direct,
        ),
        (
            vec![
                0x18, 0x0d, 0x2a, 0xe8, 0x00, 0x00, 0x0e, 0x01, 0x29, 0xdf, 0xff,
            ],
            0.250_000_000_000_000_06,
            0.75,
            TorusRadius2Encoding::OuterRingDifference,
        ),
    ];
    for (body, expected_radius2, stored_radial_scalar, expected_encoding) in cases {
        let mut payload = visibgeom_payload(1, 0);
        payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let data = build_prt("c", &[("VisibGeom", payload)]);
        let scan = container::scan_bytes(data.clone());

        let overrides = scan.surfaces.parameters[0]
            .torus_radius_overrides(0x26)
            .expect("tagged torus radius overrides");
        assert_eq!(overrides.radius1, 0.499_999_999_999_999_94);
        assert_eq!(overrides.radius2, expected_radius2);
        assert_eq!(overrides.radius2_encoding, expected_encoding);
        assert_eq!(overrides.offset, 0);
        assert_eq!(
            scan.surfaces.parameters[0].scalar_values,
            [stored_radial_scalar, 0.499_999_999_999_999_94]
        );
        assert!(scan.surfaces.parameters[0]
            .torus_radius_overrides(0x24)
            .is_none());

        let result =
            decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
        let native = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
        assert_eq!(
            native.fields["torus_radius_overrides"]["radius1"],
            0.499_999_999_999_999_94
        );
        assert_eq!(
            native.fields["torus_radius_overrides"]["radius2"],
            expected_radius2
        );
        assert_eq!(
            native.fields["torus_radius_overrides"]["radius2_encoding"],
            match expected_encoding {
                TorusRadius2Encoding::Direct => "direct",
                TorusRadius2Encoding::OuterRingDifference => "outer_ring_difference",
            }
        );
        assert_eq!(
            result
                .report
                .coverage
                .get("decoded_torus_radius_override_count")
                .copied(),
            Some(1)
        );
        assert_eq!(
            result
                .report
                .coverage
                .get("decoded_torus_outline_extent_count")
                .copied(),
            Some(0)
        );
        assert!(result.report.losses.iter().any(|loss| {
            loss.message
                .contains("Retained 1 tagged type-26 radius override(s)")
        }));
    }
}

#[test]
fn cone_terminal_half_angle_bounds_the_parameter_body() {
    let half_angle = [0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x05];
    let expected = f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x05]);
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x25, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0xe3, 0x18, 0xe4]);
    payload.extend_from_slice(&half_angle);
    payload.push(0xe3);
    payload.extend_from_slice(&[0xfe; 12]);
    payload.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe3]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(
        scan.surfaces.parameters[0].body,
        [&[0xe3, 0x18, 0xe4][..], &half_angle[..]].concat()
    );
    assert_eq!(
        scan.surfaces.parameters[0].scalar_values,
        [0.0, 1.0, expected]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
    let override_value = scan.surfaces.parameters[0]
        .cone_half_angle_override(0x25)
        .expect("terminal cone half-angle");
    assert_eq!(override_value.radians, expected);
    assert_eq!(override_value.offset, 3);
    assert!(scan.surfaces.parameters[0]
        .cone_half_angle_override(0x26)
        .is_none());

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let native = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(
        native.fields["cone_half_angle_override"]["radians"],
        expected
    );
    assert_eq!(native.fields["cone_half_angle_override"]["offset"], 3);
}

#[test]
fn decode_preserves_surface_parameter_slots_in_native_ir() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode surface parameters");

    let records = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fields["surface_id"], 7);
    assert_eq!(records[0].fields["surface_family"], "torus_or_sphere");
    assert_eq!(records[0].fields["boundary"], "compound_close");
    assert_eq!(
        records[0].fields["slots"][0]["value"],
        f64::from_be_bytes([0x3f, 0xe8, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0])
    );
    for (index, expected) in [0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0]
        .into_iter()
        .enumerate()
    {
        assert_eq!(records[0].fields["slots"][0]["raw"][index], expected);
    }
    assert_eq!(records[0].fields["slots"][0]["length"], 7);
    assert_eq!(
        records[0].fields["opaque_spans"].as_array().unwrap().len(),
        0
    );
    assert_eq!(records[0].fields["terminal_scalar_frame"]["offset"], 0);
    assert_eq!(records[0].fields["scalar_frames"][0]["offset"], 0);
    assert_eq!(
        records[0].fields["terminal_scalar_frame"]["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let row = &result.ir.native.namespace("creo").unwrap().arenas["surface_rows"][0];
    assert_eq!(row.fields["surface_id"], 7);
    assert_eq!(row.fields["type_byte"], 0x26);
    assert_eq!(row.fields["surface_family"], "torus_or_sphere");
    assert_eq!(row.fields["feature_id"], 4);
    assert_eq!(row.fields["reversed"], false);
    assert_eq!(row.fields["boundary_type"], 0);
    assert_eq!(row.fields["next_surface"], 0);
    assert_eq!(
        result.source_fidelity.annotations.provenance["creo:visibgeom:surface_row#7"]
            .tag
            .as_deref(),
        Some("surface_namespace_row")
    );
}

#[test]
fn decode_retains_type26_coordinate_envelope_in_native_ir() {
    let body = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode type-26 envelope");
    let record = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    let envelope = &record.fields["type26_five_coordinate_envelope"];
    assert_eq!(envelope["offset"], 7);
    let values = envelope["values"].as_array().expect("coordinate values");
    for (actual, expected) in values.iter().zip([-2.65, -15.0, -2.65, 2.65, -17.65]) {
        assert!((actual.as_f64().expect("finite coordinate") - expected).abs() < 1.0e-12);
    }
    assert!(record.fields["type26_split_coordinate_envelope"].is_null());
    assert_eq!(
        result.report.coverage["decoded_type26_five_coordinate_envelope_count"],
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 five-coordinate envelope(s)")));
}

#[test]
fn decode_places_complete_positional_torus() {
    let body = [
        40, 141, 7, 27, 210, 101, 111, 108, 24, 148, 63, 2, 112, 22, 190, 252, 0, 18, 32, 71, 19,
        204, 70, 49, 61, 112, 163, 215, 10, 62, 71, 19, 204, 46, 19, 204, 70, 48, 189, 112, 163,
        215, 10, 62, 33, 177, 72, 10, 227, 194, 255, 45, 89, 199, 15, 241, 65, 141, 6, 220, 32,
        138, 77, 219, 24, 229, 16, 40, 141, 6, 220, 32, 138, 77, 219, 194, 255, 45, 89, 199, 15,
        241, 24, 228, 70, 48, 189, 112, 163, 215, 10, 62, 24, 46, 17, 204, 14,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend(body);
    payload.push(0xe3);
    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode complete positional torus");

    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("positional torus surface");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if (center.x - 1.0).abs() < 1e-12
            && (center.y - 16.74).abs() < 1e-12
            && center.z.abs() < 1e-12
            && axis.x.abs() < 1e-12
            && axis.y.abs() < 1e-12
            && (axis.z - 1.0).abs() < 1e-12
            && (ref_direction.x + 0.999_899_554_583_406_1).abs() < 1e-12
            && (ref_direction.y - 0.014_173_240_416_574_131).abs() < 1e-12
            && ref_direction.z.abs() < 1e-12
            && (major_radius - 4.45).abs() < 1e-12
            && (minor_radius - 0.5).abs() < 1e-12
    ));
    let record = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert!(
        (record.fields["positional_torus_frame"]["major_radius"]
            .as_f64()
            .expect("major radius")
            - 4.45)
            .abs()
            < 1e-12
    );
    assert_eq!(
        result.report.coverage["transferred_positional_torus_count"],
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 exact positional torus carrier")));
}

#[test]
fn decode_reports_transferred_positional_cylinders() {
    let body = [
        17, 72, 0, 0, 19, 24, 72, 55, 192, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 72, 52,
        64, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    payload.extend(body);
    payload.push(0xe3);
    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode positional cylinder");

    assert_eq!(
        result.report.coverage["transferred_positional_cylinder_count"],
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 exact positional cylinder carrier")));
}

#[test]
fn decode_places_paired_five_coordinate_sphere_envelopes() {
    let lower = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let upper = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb8, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x28, 0xb3, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x2e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xd7, 0x18,
    ];
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&lower);
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&upper);
    payload.push(0xe3);
    payload.extend_from_slice(
        b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xe3",
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode paired sphere envelopes");
    for id in [7, 8] {
        let surface = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == format!("creo:visibgeom:surface#{id}"))
            .expect("paired sphere surface");
        assert!(matches!(
            surface.geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } if center.x == 0.0
                && center.y == 0.0
                && (center.z + 15.0).abs() < 1.0e-12
                && axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                && ref_direction == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && radius == 2.65
        ));
    }
    assert_eq!(
        result.report.coverage["transferred_paired_envelope_sphere_count"],
        2
    );
    assert!(result.report.losses.iter().any(|loss| {
        loss.message
            .contains("Transferred 2 sphere carrier(s) from complementary five-coordinate")
    }));
    assert!(result.report.losses.iter().any(|loss| {
        loss.message
            .contains("Placement-complete paired sphere envelopes additionally transfer")
    }));
}

#[test]
fn decode_retains_split_type26_coordinate_envelope_in_native_ir() {
    let body = [
        0x28, 0x8d, 0x07, 0x1b, 0xd2, 0x65, 0x6f, 0x6c, 0x18, 0x94, 0x3f, 0x02, 0x70, 0x16, 0xbe,
        0xfc, 0x00, 0x12, 0x20, 0x47, 0x13, 0xcc, 0x46, 0x31, 0x3d, 0x70, 0xa3, 0xd7, 0x0a, 0x3e,
        0x3a, 0xb1, 0x47, 0xba, 0x2e, 0x13, 0xcc, 0x46, 0x30, 0xbd, 0x70, 0xa3, 0xd7, 0x0a, 0x3e,
        0x2e, 0x13, 0xcc,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode split type-26 envelope");
    let record = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    let envelope = &record.fields["type26_split_coordinate_envelope"];
    assert_eq!(envelope["offset"], 19);
    let values = envelope["values"].as_array().expect("coordinate values");
    for (actual, expected) in values.iter().zip([-4.95, 17.24, 16.74, 4.95]) {
        assert!((actual.as_f64().expect("finite coordinate") - expected).abs() < 1.0e-12);
    }
    assert!(record.fields["type26_five_coordinate_envelope"].is_null());
    assert_eq!(
        result.report.coverage["decoded_type26_split_coordinate_envelope_count"],
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 split-coordinate envelope(s)")));
}

#[test]
fn decode_preserves_unframed_surface_parameter_spans() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x11, 0xe4, 0x12, 0x13, 0x0d, 0xe3]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode surface parameter spans");

    let record = &result.ir.native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(record.fields["slots"][0]["offset"], 1);
    assert_eq!(record.fields["slots"][1]["offset"], 4);
    assert_eq!(record.fields["opaque_spans"][0]["offset"], 0);
    assert_eq!(record.fields["opaque_spans"][0]["raw"][0], 0x11);
    assert_eq!(record.fields["opaque_spans"][1]["offset"], 2);
    assert_eq!(record.fields["opaque_spans"][1]["length"], 2);
    assert_eq!(record.fields["terminal_scalar_frame"]["offset"], 4);
    let frames = record.fields["scalar_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["offset"], 1);
    assert_eq!(frames[1]["offset"], 4);
    assert_eq!(
        record.fields["terminal_scalar_frame"]["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn surface_parameter_body_ignores_compound_close_inside_scalar() {
    let mut payload = visibgeom_payload(1, 0);
    let scalar = [0x46, 0x08, 0xe3, 0, 0, 0, 0, 0];
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&scalar);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].body, scalar);
    assert_eq!(
        scan.surfaces.parameters[0].scalar_values,
        [f64::from_be_bytes([0x40, 0x08, 0xe3, 0, 0, 0, 0, 0])]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
}

#[test]
fn surface_parameter_body_ignores_invalid_embedded_named_marker() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x2f, 0x43, 0, 0xe0, 0xff, 0x80, 0, 0x0f]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(
        scan.surfaces.parameters[0].body,
        [0x2f, 0x43, 0, 0xe0, 0xff, 0x80, 0, 0x0f]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn surface_parameter_body_ignores_valid_looking_header_inside_scalar() {
    let mut payload = visibgeom_payload(1, 0);
    let scalar = [0x71, 0xe0, 0x01, b'x', 0, 0, 0, 0];
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].body, scalar);
    assert_eq!(
        scan.surfaces.parameters[0].scalar_values,
        [f64::from_be_bytes([0x3f, 0xe0, 0x01, b'x', 0, 0, 0, 0])]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn scan_ignores_surface_header_candidates_inside_a_preceding_header() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0x24]);
    payload.extend_from_slice(&[0x22, 4, 0x01, 0, 0, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].surface_id, 7);
}

#[test]
fn scan_decodes_plane_local_system_support_frame() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 1, 0]);
    for value in [0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 1.0, 1.0, 2.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[
        0x18, 0xe5, // stock first in-plane direction [0, 1, 0]
        0xe4, 0x0f, 0x0f, // second in-plane direction
        0x0f, 0x0f, 0x0f, // structural zero row
    ]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.planes.local_systems.len(), 1);
    let frame = &scan.planes.local_systems[0];
    assert_eq!(frame.surface_id, 7);
    assert_eq!(frame.slots.len(), 12);
    assert_eq!(frame.origin, Some([3.0, 0.0, 1.0]));
    assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal, Some([0.0, 0.0, -1.0]));
    assert_eq!(
        frame.classification,
        crate::surface::LocalSystemClassification::Simple
    );
    assert_eq!(scan.planes.outlines.len(), 1);
    assert_eq!(scan.planes.outlines[0].origin, [0.0, 0.0, 1.0]);
    assert_eq!(scan.planes.outlines[0].normal, [0.0, 0.0, -1.0]);
    assert_eq!(scan.planes.outlines[0].u_axis, [0.0, 1.0, 0.0]);
}

#[test]
fn scan_resolves_section_scalar_cache_in_surface_rows() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0, 0x18, 0x00, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].surface_id, 7);
    assert_eq!(scan.surfaces.parameters[0].scalar_values, vec![3.0]);
}

#[test]
fn decode_transfers_axis_aligned_plane_from_outline() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 1.0, 1.0, 2.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let expected_offset = container::scan_bytes(data.clone()).planes.local_systems[0].offset as u64;
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let namespace = result.ir.native.namespace("creo").unwrap();
    assert_eq!(
        namespace.arenas["plane_local_systems"][0].fields["surface_id"],
        7
    );
    assert_eq!(
        namespace.arenas["plane_envelopes"][0].fields["surface_id"],
        7
    );
    assert_eq!(
        namespace.arenas["plane_envelopes"][0].fields["envelope"]["kind"],
        "standard"
    );
    assert_eq!(
        namespace.arenas["outline_planes"][0].fields["normal"][2],
        -1.0
    );

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
fn decode_transfers_plane_from_shared_rank_two_local_system_image() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    payload.extend_from_slice(&[
        0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe1, 0xe3,
    ]);

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_eq!(
        result.ir.model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
    let coverage = &result.report.coverage;
    assert_eq!(coverage["visible_plane_surface_row_count"], 1);
    assert_eq!(coverage["transferred_visible_plane_surface_row_count"], 1);
    assert_eq!(coverage["untransferred_visible_surface_row_count"], 0);
}

#[test]
fn decode_withholds_axis_aligned_surface_without_parameter_chart() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    assert!(result.ir.model.surfaces.is_empty());
}

#[test]
fn decode_transfers_held_coordinate_plane_with_canonical_chart() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 1.0, 1.0, 2.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_eq!(
        result.ir.model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }
    );
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

    assert_eq!(scan.planes.envelopes.len(), 2);
    let crate::surface::PlaneEnvelope::Standard {
        bounds_2d,
        corners_3d,
    } = &scan.planes.envelopes[0].envelope
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
        &scan.planes.envelopes[1].envelope
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
fn scan_derives_named_surface_plane_from_outline_corners() {
    let mut payload = b"srf_array\0geom_id\0\x05geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\x01next_geom_ptr\0\0\
        outline\0\xf9\x02\x03"
        .to_vec();
    payload.extend_from_slice(&[0xe4, 0x0f, 0x2f, 0, 0, 0x0d, 0x0f, 0x48, 0, 0]);
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert_eq!(scan.planes.envelopes.len(), 1);
    assert_eq!(scan.planes.outlines.len(), 1);
    assert_eq!(scan.planes.outlines[0].surface_id, 5);
    assert_eq!(scan.planes.outlines[0].origin, [0.0, 0.0, 0.0]);
    assert_eq!(scan.planes.outlines[0].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn scan_discovers_labeled_surface_namespace_row() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(
        b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan
        .surfaces
        .rows
        .iter()
        .any(|row| { row.id == 7 && row.feature_id == 4 && row.next_surface == 0 }));
}

#[test]
fn scan_keeps_depdb_cross_section_surfaces_out_of_model_namespace() {
    let visible = b"srf_array\0\xf8\x01geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0".to_vec();
    let cross_section = b"Sld_Xsections\0\xe3\xe0\0xsec_geom\0\xe2srf_array\0\xf8\x01geom_id\0\x09geom_type\0\x24feat_id\0\x08orient\0\x01boundary_type\0\x06next_geom_ptr\0\0".to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", visible), ("Xsections", cross_section)],
    ));

    assert_eq!(scan.surfaces.rows.len(), 1);
    assert_eq!(scan.surfaces.rows[0].id, 7);
    assert_eq!(scan.surfaces.cross_section_rows.len(), 1);
    assert_eq!(scan.surfaces.cross_section_rows[0].id, 9);
    assert_eq!(scan.surfaces.cross_section_rows[0].boundary_type, 0x06);
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
    payload.extend_from_slice(b"\xe0\x01id\0\x0f");
    payload.extend_from_slice(b"\xe0\x01degree\0\x03");
    payload.extend_from_slice(b"\xe0\x02params\0\xf8\x04\x00\x00\x01\x01");
    payload.extend_from_slice(b"\xe0\x01flip\0\xf1\x01");
    payload.extend_from_slice(b"\xe0\x02dum_array\0\xf8\x03\x01\x02\x03\x04");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.surfaces.prototype_records.len(), 1);
    let prototype = &scan.surfaces.prototype_records[0];
    assert_eq!(prototype.declared_family, "cylinder");
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
            tokens: Vec::new(),
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
    assert_eq!(
        prototype.field("id").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(15))
    );
    assert_eq!(
        prototype.field("degree").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(3))
    );
    assert_eq!(
        prototype.field("params").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactIntArray(vec![
            0, 0, 1, 1
        ]))
    );
    assert_eq!(
        prototype.field("flip").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![0xf1, 0x01]))
    );
    assert_eq!(
        prototype.field("dum_array").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![
            0xf8, 0x03, 0x01, 0x02, 0x03, 0x04
        ]))
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let native = &result.ir.native.namespace("creo").unwrap().arenas["surface_prototypes"][0];
    assert_eq!(native.fields["declared_family"], "cylinder");
    assert_eq!(native.fields["family"], "cylinder");
    assert_eq!(native.fields["parameters"][0]["name"], "local_sys");
    assert_eq!(native.fields["parameters"][0]["value_kind"], "scalar_array");
    assert_eq!(native.fields["parameters"][0]["scalar_dimensions"], 4);
    assert_eq!(native.fields["parameters"][0]["scalar_values"][0], 1.0);
    assert_eq!(native.fields["parameters"][1]["name"], "radius");
    assert_eq!(native.fields["parameters"][1]["body"][0], 0xe4);
    assert_eq!(native.fields["parameters"][2]["compact_values"][0], 7);
    assert_eq!(native.fields["parameters"][2]["compact_values"][1], 8);
    assert_eq!(native.fields["parameters"][3]["compact_values"][0], 128);
    assert_eq!(native.fields["parameters"][3]["compact_values"][1], 129);
    assert_eq!(native.fields["parameters"][3]["compact_values"][2], 130);
    assert_eq!(native.fields["parameters"][4]["name"], "id");
    assert_eq!(native.fields["parameters"][4]["compact_values"][0], 15);
    assert_eq!(native.fields["parameters"][5]["name"], "degree");
    assert_eq!(native.fields["parameters"][5]["compact_values"][0], 3);
    assert_eq!(native.fields["parameters"][6]["name"], "params");
    assert_eq!(native.fields["parameters"][6]["compact_values"][2], 1);
    assert_eq!(native.fields["parameters"][7]["name"], "flip");
    assert_eq!(native.fields["parameters"][7]["opaque"][0], 0xf1);
    assert_eq!(native.fields["parameters"][8]["name"], "dum_array");
    assert_eq!(native.fields["parameters"][8]["value_kind"], "opaque");
    assert_eq!(
        result.source_fidelity.annotations.provenance[&native.id]
            .tag
            .as_deref(),
        Some("surface_prototype_record")
    );
}

#[test]
fn scan_decodes_cone_half_angle_in_its_positive_dict_lane() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0");
    payload.extend_from_slice(b"\xe0\x01half_angle\0\x74\x21\xfb\x54\x44\x2d\x23");
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    let prototype = scan
        .surfaces
        .prototype_records
        .iter()
        .find(|record| record.family == crate::surface::SurfacePrototypeFamily::Cone)
        .expect("cone prototype");
    assert_eq!(
        prototype.field("half_angle").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::ScalarSequence(vec![
            f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23]),
        ]))
    );
}

#[test]
fn scan_keeps_out_of_range_cone_half_angle_opaque() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0");
    payload.extend_from_slice(b"\xe0\x01half_angle\0\x8b\0\0\0\0\0\0");
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    let prototype = scan
        .surfaces
        .prototype_records
        .iter()
        .find(|record| record.family == crate::surface::SurfacePrototypeFamily::Cone)
        .expect("cone prototype");
    assert_eq!(
        prototype.field("half_angle").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![
            0x8b, 0, 0, 0, 0, 0, 0,
        ]))
    );
}

#[test]
fn scan_collects_feature_owners_from_rows_and_parent_lists() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"parent_feats\0\xf8\x02\x04\x09");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.features.ids, vec![4, 9]);
}

#[test]
fn scan_binds_allfeatur_mixed_entity_table_to_known_feature() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, // feature row for owner 4
        0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, // two mixed entity references
        7, 0x80, 0xc8, 1, 0, 0xe3, // a materialized surface id
        0xf7, 0x1e, 9, 0x80, 0xc8, 2, 0, 0xe3, // a prefixed non-surface entity id
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Protrusion id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.entity_tables.len(), 1);
    let table = &scan.features.entity_tables[0];
    assert_eq!(table.feature_id, Some(4));
    assert_eq!(table.table_class_id, 29);
    assert_eq!(table.entry_ids, vec![7, 9]);
    assert_eq!(table.entries.len(), 2);
    assert!(!table.entries[0].prefixed);
    assert!(table.entries[1].prefixed);
    assert_eq!(table.entries[0].entity_id, 7);
    assert_eq!(table.entries[1].entity_id, 9);
    assert_eq!(table.entries[0].class_id, 200);
    assert_eq!(table.entries[1].class_id, 200);
    assert_eq!(table.entries[0].source_entity_id, Some(1));
    assert_eq!(table.entries[1].source_entity_id, Some(2));
    assert_eq!(table.entries[0].end_offset, table.entries[1].offset - 2);
    assert_eq!(table.surface_ids, vec![7]);
    assert_eq!(table.non_surface_entity_ids, vec![9]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#4")
        .expect("feature 4");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude { .. }
    ));
    assert_eq!(
        feature.source_properties["native_parameter.generated_entity.7.source_section_entity_id"],
        "1"
    );
    assert_eq!(
        feature.source_properties["native_parameter.generated_entity.7.entry_class"],
        "200"
    );
    assert_eq!(
        feature.source_properties["native_parameter.generated_entity.9.source_section_entity_id"],
        "2"
    );
    let tables = &result.ir.native.namespace("creo").unwrap().arenas["feature_entity_tables"];
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].fields["owner_feature_id"], 4);
    assert_eq!(tables[0].fields["table_class_id"], 29);
    assert_eq!(tables[0].fields["entry_ids"][0], 7);
    assert_eq!(tables[0].fields["entry_ids"][1], 9);
    assert_eq!(tables[0].fields["entries"][0]["class_id"], 200);
    assert_eq!(tables[0].fields["entries"][0]["source_entity_id"], 1);
    assert_eq!(tables[0].fields["entries"][1]["prefixed"], true);
    assert_annotation(
        &result.source_fidelity.annotations,
        &tables[0].id,
        "creo:AllFeatur",
        table.offset as u64,
        "feature_entity_table",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_binds_ordered_visible_surfaces_to_matching_replay_runs() {
    let mut visible = b"srf_array\0\xf8\x02".to_vec();
    visible.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 8, 0xe3]);
    visible.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0, 0xe3]);
    visible.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut nonvisible = b"srf_array\0\xf8\x05".to_vec();
    for (id, kind) in [(9, 0x24), (10, 0x26), (11, 0x22), (12, 0x24), (13, 0x26)] {
        nonvisible.extend_from_slice(&[id, kind, 4, 0x01, 0, 0, 0xe3]);
    }
    nonvisible.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut allfeatur = vec![4, 0xeb, 0x04, 0xf8, 7, 0xf7, 79, 0xfb, 0xe3];
    for (id, class_id) in [
        (7, 254),
        (8, 254),
        (9, 214),
        (10, 214),
        (11, 215),
        (12, 214),
        (13, 214),
    ] {
        allfeatur.extend_from_slice(&[id, 0x80, class_id, 0, 0, 0xe3]);
    }
    let result = decode::decode(
        &mut Cursor::new(build_prt(
            "c",
            &[
                ("VisibGeom", visible),
                ("NovisGeom", nonvisible),
                ("AllFeatur", allfeatur),
                ("MdlStatus", b"Round id 4\0".to_vec()),
            ],
        )),
        &DecodeOptions::default(),
    )
    .expect("decode ordered surface replay");

    let associations =
        &result.ir.native.namespace("creo").unwrap().arenas["feature_surface_replays"];
    assert_eq!(associations.len(), 4);
    for (association, visible_id, replay_id, ordinal) in [
        (&associations[0], 7, 9, 0),
        (&associations[1], 8, 10, 0),
        (&associations[2], 7, 12, 1),
        (&associations[3], 8, 13, 1),
    ] {
        assert_eq!(association.fields["owner_feature_id"], 4);
        assert_eq!(association.fields["visible_surface_id"], visible_id);
        assert_eq!(association.fields["replay_surface_id"], replay_id);
        assert_eq!(association.fields["replay_ordinal"], ordinal);
    }
    assert_eq!(
        result.report.coverage["decoded_feature_surface_replay_association_count"],
        4
    );
}

#[test]
fn scan_decodes_source_entity_id_whose_compact_tail_is_e3() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, 7, 0x80, 0xc8, 0x80, 0xe3, 0, 0xe3, 8,
        0x80, 0xc8, 3, 0, 0xe3,
    ];
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    let [table] = scan.features.entity_tables.as_slice() else {
        panic!("expected one generated-entity table");
    };
    assert_eq!(table.entry_ids, vec![7, 8]);
    assert_eq!(table.entries[0].class_id, 200);
    assert_eq!(table.entries[0].source_entity_id, Some(227));
    assert_eq!(table.entries[1].source_entity_id, Some(3));
}

#[test]
fn scan_accepts_large_structurally_bounded_feature_entity_tables() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur = vec![
        4, 0xeb, 0x04, // feature row for owner 4
        0xf8, 65, 0xf7, 0x1d, 0xfb, 0xe3,
    ];
    allfeatur.extend_from_slice(&[7, 0x80, 0xc8, 1, 0, 0xe3]);
    for _ in 1..65 {
        allfeatur.extend_from_slice(&[9, 0x80, 0xc8, 1, 0, 0xe3]);
    }
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    let [table] = scan.features.entity_tables.as_slice() else {
        panic!("expected one large generated-entity table");
    };
    assert_eq!(table.feature_id, Some(4));
    assert_eq!(table.entry_ids.len(), 65);
    assert_eq!(table.surface_ids, vec![7]);
    assert_eq!(table.non_surface_entity_ids.len(), 64);
}

#[test]
fn scan_rejects_feature_entity_table_that_crosses_the_next_feature_row() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 9, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0xf8, 2, 0xf7, 0x1d, 0xfb, 0xe3, 7, 0x80, 0xc8, 1, 0, 0xe3,
        // The second declared entry is absent before feature 9 starts.
        9, 0x90, 0x01, 8, 0xe3,
    ];
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", geometry), ("AllFeatur", allfeatur)],
    ));

    assert!(scan.features.entity_tables.is_empty());
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

    assert_eq!(scan.features.rows.len(), 2);
    assert_eq!(scan.features.rows[0].feature_id, 4);
    assert_eq!(scan.features.rows[0].header, [0xeb, 0x04]);
    assert_eq!(scan.features.rows[0].body, vec![0xeb, 0x04, 0xaa, 0xbb]);
    assert_eq!(scan.features.rows[1].feature_id, 9);
    assert_eq!(scan.features.rows[1].body, vec![0x90, 0x01, 0xcc]);
}

#[test]
fn scan_decodes_allfeatur_root_featdefs_schema_class() {
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&[8, 0x22, 9, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 9, 0xeb,
        0x04, 0, 0x10, 1, 0, 0xe5, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            (
                "MdlStatus",
                b"protrevolve\0Revolve id 4\0Round id 9\0".to_vec(),
            ),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.rows[0].root_schema_class, Some(917));
    assert_eq!(scan.features.rows[1].root_schema_class, Some(913));

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(
        result.ir.model.features[0]
            .source_properties
            .get("featdefs_schema_class")
            .map(String::as_str),
        Some("917")
    );
    assert_eq!(
        result.ir.model.features[0]
            .source_properties
            .get("recipe")
            .map(String::as_str),
        Some("protrevolve")
    );
    assert_eq!(
        result.ir.model.features[1]
            .source_properties
            .get("featdefs_schema_class")
            .map(String::as_str),
        Some("913")
    );
}

#[test]
fn decode_types_class_911_as_unresolved_hole() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x8f, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Hole id 4\0".to_vec()),
        ],
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("hole feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: cadmpeg_ir::features::HoleKind::Unresolved { form: None, .. },
            diameter: None,
            extent: None,
            ..
        }
    ));
}

#[test]
fn decode_types_class_914_as_unresolved_chamfer() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x92, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Chamfer id 4\0".to_vec()),
        ],
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("chamfer feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved { form: None },
        }])
    ));
}

#[test]
fn decode_types_class_946_as_unresolved_surface_merge() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0xb2, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("surface merge feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::KnitSurface {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            merge_entities: None,
            create_solid: None,
            gap_tolerance: None,
        }
    ));
    assert_eq!(feature.name.as_deref(), Some("Surface Merge id 4"));
}

#[test]
fn decode_types_row_only_class_927_as_unresolved_draft() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x9f, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("draft feature");
    assert_eq!(feature.name.as_deref(), Some("Draft id 4"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
            pull_direction: None,
            angle: None,
            outward: None,
        }
    ));
}

#[test]
fn decode_types_named_draft_with_unresolved_operands() {
    for name in ["Draft", "Schräge"] {
        let stored_name = format!("{name} id 40\0");
        let data = build_prt("c", &[("MdlStatus", stored_name.into_bytes())]);
        let result =
            decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
        let feature = result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#40")
            .expect("draft feature");

        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Draft {
                faces: cadmpeg_ir::features::FaceSelection::Unresolved,
                neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                pull_direction: None,
                angle: None,
                outward: None,
            }
        ));
    }
}

#[test]
fn decode_types_named_mirror_with_unresolved_operands() {
    let data = build_prt("c", &[("MdlStatus", b"oMirror id 4\0".to_vec())]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("mirror feature");

    assert_eq!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: cadmpeg_ir::features::PatternKind::Unresolved {
                form: Some(cadmpeg_ir::features::PatternForm::Mirror),
            },
        }
    );
    assert_eq!(
        feature
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("o")
    );
}

#[test]
fn decode_types_z_prefixed_round_with_unresolved_operands() {
    let data = build_prt("c", &[("MdlStatus", b"zRound id 4\0".to_vec())]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");

    assert_eq!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
                radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }],
        }
    );
    assert_eq!(
        feature
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("z")
    );
}

#[test]
fn decode_recovers_schema_feature_that_owns_materialized_surfaces() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 9, 0xeb,
        0x04, 0, 0x10, 1, 0, 0xe5, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(result.ir.model.features.len(), 1);
    let feature = &result.ir.model.features[0];
    assert_eq!(feature.id.as_str(), "creo:model:feature#4");
    assert_eq!(feature.name.as_deref(), Some("Protrusion id 4"));
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            ..
        }
    ));
    assert_eq!(
        feature
            .source_properties
            .get("featdefs_schema_class")
            .map(String::as_str),
        Some("917")
    );
    assert!(result
        .ir
        .model
        .features
        .iter()
        .all(|feature| feature.id.as_str() != "creo:model:feature#9"));
}

#[test]
fn decode_types_row_only_class_916_as_subtractive_extrusion() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x94, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("cut feature");

    assert_eq!(feature.name.as_deref(), Some("Cut id 4"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_types_named_base_protrusion_as_new_body() {
    let data = build_prt("c", &[("MdlStatus", b"Protrusion id 4\0".to_vec())]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("protrusion feature");

    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            ..
        }
    ));
}

#[test]
fn decode_types_named_sweeps_without_recipe_or_operands() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"Extrude id 4\0Revolve id 5\0Cut id 6\0".to_vec(),
        )],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = |id| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == id)
            .expect("named sweep feature")
    };

    assert!(matches!(
        feature("creo:model:feature#4").definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        feature("creo:model:feature#5").definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
        }
    ));
    assert!(matches!(
        feature("creo:model:feature#6").definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_types_schema_datum_from_its_unique_plane_carrier() {
    let mut geometry = visibgeom_payload(1, 0);
    push_generated_plane_row(
        &mut geometry,
        7,
        false,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    );
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x9b, 0xe1,
    ];
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(result.ir.model.features.len(), 1);
    assert!(matches!(
        &result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { origin, normal, u_axis }
            if *origin == cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0)
                && *normal == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                && *u_axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn scan_resolves_allfeatur_walker_order_entity_references() {
    let allfeatur =
        b"\xe0\x00Sld_Features\0\xe0\x22first\0\xf7\x02\xe3\xe0\x24second\0\xf7\x01\xe3".to_vec();
    let data = build_prt("c", &[("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.entities.len(), 3);
    assert_eq!(scan.features.entities[0].entity_id, 0);
    assert_eq!(scan.features.entities[0].name, "Sld_Features");
    assert_eq!(scan.features.entities[1].entity_id, 1);
    assert_eq!(scan.features.entities[1].name, "first");
    assert_eq!(scan.features.entity_references.len(), 2);
    assert_eq!(scan.features.entity_references[0].source_entity_id, Some(1));
    assert_eq!(scan.features.entity_references[0].target_entity_id, 2);
    assert!(scan.features.entity_references[0].target_resolved);
    assert_eq!(scan.features.entity_references[1].source_entity_id, Some(2));
    assert_eq!(scan.features.entity_references[1].target_entity_id, 1);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let namespace = result.ir.native.namespace("creo").expect("creo namespace");
    let entities = &namespace.arenas["feature_entities"];
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].id, "creo:allfeatur:entity#0");
    assert_eq!(entities[0].fields["type_byte"], 0);
    assert_eq!(entities[0].fields["name"], "Sld_Features");
    let references = &namespace.arenas["feature_entity_references"];
    assert_eq!(references.len(), 2);
    let forward = references
        .iter()
        .find(|reference| reference.fields["target_entity_id"] == 2)
        .expect("forward reference");
    assert_eq!(forward.fields["source_entity_id"], 1);
    assert_eq!(forward.fields["target_resolved"], true);
    assert_annotation(
        &result.source_fidelity.annotations,
        &entities[0].id,
        "creo:AllFeatur",
        scan.features.entities[0].offset as u64,
        "feature_entity",
        Exactness::ByteExact,
    );
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

    assert_eq!(scan.features.choices.len(), 2);
    assert_eq!(scan.features.choices[0].feature_id, 4);
    assert_eq!(scan.features.choices[0].label, "blend_choice");
    assert_eq!(scan.features.choices[0].type_byte, Some(0x22));
    assert_eq!(scan.features.choices[0].payload, vec![0x11, 0x12]);
    assert_eq!(scan.features.choices[1].label, "depth_choice");
    assert_eq!(scan.features.choices[1].payload, vec![0x07]);
}

#[test]
fn scan_decodes_allfeatur_choice_field_wrappers() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur =
        b"\x04\xeb\x04\xe0\x22blend_choice\0\xe0\x21count\0\x07\xe0\x22refs\0\xf8\x02\x03\x04\xe0\x24depth_choice\0"
            .to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.choice_fields.len(), 2);
    assert_eq!(scan.features.choice_fields[0].name, "count");
    assert_eq!(
        scan.features.choice_fields[0].value,
        crate::feature::FeatureFieldValue::CompactInt(7)
    );
    assert_eq!(scan.features.choice_fields[1].name, "refs");
    assert_eq!(
        scan.features.choice_fields[1].value,
        crate::feature::FeatureFieldValue::CompactIntArray(vec![3, 4])
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = &result.ir.model.features[0];
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. },
            ..
        }])
    ));
    assert_eq!(
        feature.source_properties["native_parameter.choice.blend_choice.count"],
        "7"
    );
    assert_eq!(
        feature.source_properties["native_parameter.choice.blend_choice.refs"],
        "3,4"
    );
}

#[test]
fn decode_types_class_913_without_an_edge_array() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = vec![
        4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
    ];
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. },
            ..
        }])
    ));
}

#[test]
fn torus_only_round_uses_agreeing_tagged_minor_radii() {
    let direct_quarter = [
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff, 0xe3,
    ];
    let round = |second_trailer: &[u8]| {
        let mut geometry = visibgeom_payload(2, 0);
        geometry.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 8]);
        geometry.extend_from_slice(&direct_quarter);
        geometry.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
        geometry.extend_from_slice(second_trailer);
        let allfeatur = vec![
            4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
        ];
        build_prt(
            "c",
            &[
                ("VisibGeom", geometry),
                ("AllFeatur", allfeatur),
                ("MdlStatus", b"Round id 4\0".to_vec()),
            ],
        )
    };
    let data = round(&direct_quarter);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(radius),
            }, ..
        }] if (radius - 0.249_999_999_951_747_04).abs() < 1e-12)
    ));

    let conflicting = round(&[
        0x18, 0x0d, 0x29, 0xdf, 0xff, 0x7b, 0x0e, 0x29, 0xdf, 0xff, 0xe3,
    ]);
    let result =
        decode::decode(&mut Cursor::new(conflicting), &DecodeOptions::default()).expect("decode");
    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. }, ..
        }])
    ));
}

#[test]
fn decode_types_named_german_round_without_a_schema_row() {
    let data = build_prt("c", &[("MdlStatus", b"Rundung id 4\0".to_vec())]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(
        result.ir.model.features[0].name.as_deref(),
        Some("Rundung id 4")
    );
    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. }, ..
        }])
    ));
}

#[test]
fn decode_types_named_annotation_feature_as_a_tree_node() {
    let data = build_prt("c", &[("MdlStatus", b"Annotation Feature id 4\0".to_vec())]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
}

#[test]
fn decode_types_localized_cross_section_nodes() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"Cross Section id 4\0Querschnitt id 5\0".to_vec(),
        )],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(result.ir.model.features.len(), 2);
    assert!(result.ir.model.features.iter().all(|feature| matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::CrossSections,
            ..
        }
    )));
}

#[test]
fn scan_decodes_complete_allfeatur_f9_scalar_slots() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur =
        b"\x04\xeb\x04\xe0\x22blend_choice\0\xe0\x21values\0\xf9\x01\x03\x0f\xe4".to_vec();
    allfeatur.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(
        scan.features.choice_fields[0].value,
        crate::feature::FeatureFieldValue::ScalarArray {
            dimensions: 1,
            count: 3,
            body: vec![0x0f, 0xe4, 0x46, 0x08, 0, 0, 0, 0, 0, 0],
            decoded_values: Some(vec![0.0, 1.0, 3.0]),
        }
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let namespace = result.ir.native.namespace("creo").unwrap();
    let rows = &namespace.arenas["feature_rows"];
    assert_eq!(rows[0].fields["owner_feature_id"], 4);
    assert_eq!(rows[0].fields["header"][0], 0xeb);
    assert_eq!(rows[0].fields["header"][1], 0x04);
    assert_eq!(rows[0].fields["body"][0], 0xeb);
    assert_eq!(rows[0].fields["body"][2], 0xe0);
    let choices = &namespace.arenas["feature_choices"];
    assert_eq!(choices[0].fields["owner_feature_id"], 4);
    assert_eq!(choices[0].fields["label"], "blend_choice");
    let fields = &namespace.arenas["feature_choice_fields"];
    assert_eq!(fields[0].fields["choice_label"], "blend_choice");
    assert_eq!(fields[0].fields["name"], "values");
    assert_eq!(fields[0].fields["value"]["kind"], "scalar_array");
    assert_eq!(fields[0].fields["value"]["decoded_values"][2], 3.0);
}

#[test]
fn scan_decodes_allfeatur_generated_geometry_manifest() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04edg_id_tab_ptr\0\xf1\xf8\x03\xf7\x53\xfb\xe3used_bodies\0\xf8\x01\xf7\x60\xfb\xe2dtm_id_tab\0\xf2\xf8\x02\xf7\x57\xfb\xe2\xe0\x01dtm_id\0\x2a\xe0\x01dtm_id\0\x2b".to_vec();
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.geometry_tables.len(), 3);
    assert_eq!(scan.features.geometry_tables[0].feature_id, 4);
    assert_eq!(
        scan.features.geometry_tables[0].kind,
        crate::feature::FeatureGeometryTableKind::EdgeIds
    );
    assert_eq!(scan.features.geometry_tables[0].count, 3);
    assert_eq!(scan.features.geometry_tables[0].entity_class, 0x53);
    assert_eq!(
        scan.features.geometry_tables[1].kind,
        crate::feature::FeatureGeometryTableKind::UsedBodies
    );
    assert_eq!(
        scan.features.geometry_tables[2].kind,
        crate::feature::FeatureGeometryTableKind::DatumIds
    );
    assert_eq!(
        scan.features.geometry_tables[2].entry_ids,
        Some(vec![42, 43])
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let tables = &result.ir.native.namespace("creo").unwrap().arenas["feature_geometry_tables"];
    assert_eq!(tables.len(), 3);
    assert_eq!(tables[0].fields["owner_feature_id"], 4);
    assert_eq!(tables[0].fields["kind"], "edge_ids");
    assert_eq!(tables[0].fields["declared_count"], 3);
    assert_eq!(tables[0].fields["entity_class_id"], 0x53);
    assert_eq!(tables[2].fields["entry_ids"][0], 42);
    assert_eq!(tables[2].fields["entry_ids"][1], 43);
    assert_annotation(
        &result.source_fidelity.annotations,
        &tables[0].id,
        "creo:AllFeatur",
        scan.features.geometry_tables[0].offset as u64,
        "feature_geometry_table",
        Exactness::ByteExact,
    );
}

#[test]
fn scan_decodes_allfeatur_affected_id_arrays() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\xe0\x21geoms_affected\0\xf8\x03\x07\x80\x80\x09\
        \xe0\x22contours\0\xf8\x01\x2a\xe0\x01parent_table\0\xf8\x02\x01\x03"
        .to_vec();
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.affected_ids.len(), 3);
    assert_eq!(
        scan.features.affected_ids[0].kind,
        crate::feature::AffectedIdKind::Geometry
    );
    assert_eq!(scan.features.affected_ids[0].ids, vec![7, 128, 9]);
    assert_eq!(
        scan.features.affected_ids[1].kind,
        crate::feature::AffectedIdKind::Contours
    );
    assert_eq!(scan.features.affected_ids[1].ids, vec![42]);
    assert_eq!(
        scan.features.affected_ids[2].kind,
        crate::feature::AffectedIdKind::Parents
    );
    assert_eq!(scan.features.affected_ids[2].ids, vec![1, 3]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["feature_affected_ids"];
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].fields["owner_feature_id"], 4);
    assert_eq!(records[0].fields["kind"], "geometry");
    assert_eq!(records[0].fields["ids"][1], 128);
}

#[test]
fn decode_types_round_with_labeled_edge_selection() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1\
        \xe0\x21edgs_affected\0\xf8\x02\x2c\x2d"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");

    assert_eq!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: cadmpeg_ir::features::EdgeSelection::Native(
                    "creo:allfeatur:edgs_affected#4:44,45".to_string()
                ),
                radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }],
        }
    );
    assert_eq!(
        feature
            .source_properties
            .get("native_parameter.affected_edge_ids")
            .map(String::as_str),
        Some("44,45")
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_identifies_variable_round_form_from_differing_complete_envelopes() {
    let geometry = |type_bytes: [u8; 2]| {
        let mut geometry = b"srf_array\0\xf8\x02".to_vec();
        for ((surface_id, next_surface, diameter, extent), type_byte) in
            [(7, 8, 1.0, [1.0, 2.0, 2.0]), (8, 0, 2.0, [2.0, 1.0, 1.0])]
                .into_iter()
                .zip(type_bytes)
        {
            geometry.extend_from_slice(&[surface_id, type_byte, 4, 0x01, 0, next_surface]);
            geometry.push(0x15);
            for value in [0.0, 0.0, diameter, 0.0, 0.0, 0.0]
                .into_iter()
                .chain(extent)
            {
                push_generated_scalar(&mut geometry, value);
            }
            geometry.push(0xe3);
        }
        geometry.extend_from_slice(b"crv_array\0\xf3\xf8\0");
        geometry
    };
    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1".to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry([0x24, 0x24])),
            ("AllFeatur", allfeatur.clone()),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Variable)
            }, ..
        }])
    ));

    let mixed = build_prt(
        "c",
        &[
            ("VisibGeom", geometry([0x24, 0x26])),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let mixed = decode::decode(&mut Cursor::new(mixed), &DecodeOptions::default()).expect("decode");
    assert!(matches!(
        mixed.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None }, ..
        }])
    ));
}

#[test]
fn decode_transfers_strong_parents_as_ordered_dependencies() {
    let mut datum = vec![4, 0x22, 1, 1, 0, 0];
    datum.extend([0x0f; 4]);
    datum.extend([0x0f; 6]);
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\xe0\x01parent_table\0\xf8\x01\x01\
        \xe0\x21strong_parents\0\xf8\x02\x02\x01"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("ActDatums", datum),
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Datum Plane id 2\0Protrusion id 4\0".to_vec()),
        ],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert!(feature.parent.is_none());
    assert_eq!(
        feature
            .dependencies
            .iter()
            .map(cadmpeg_ir::FeatureId::as_str)
            .collect::<Vec<_>>(),
        vec!["creo:model:feature#1", "creo:model:feature#2"]
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_resolves_feature_dependencies_independently_of_storage_order() {
    let mut datum = vec![4, 0x22, 1, 1, 0, 0];
    datum.extend([0x0f; 4]);
    datum.extend([0x0f; 6]);
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04\xe0\x01parent_table\0\xf8\x01\x01\
        \xe0\x21strong_parents\0\xf8\x02\x02\x01"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("ActDatums", datum),
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Protrusion id 4\0Datum Plane id 2\0".to_vec()),
        ],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert_eq!(
        feature
            .dependencies
            .iter()
            .map(cadmpeg_ir::FeatureId::as_str)
            .collect::<Vec<_>>(),
        vec!["creo:model:feature#1", "creo:model:feature#2"]
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn scan_partitions_allfeatur_positional_round_operands() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let mut allfeatur = b"\x04\xeb\x04\xe3\xf6\x83\x91\xe1\xf1\xf7\x42\xd8\x80\x01\xe3\xf8\x02\x07\x80\x80\xf8\x01\x09".to_vec();
    allfeatur.extend_from_slice(&[0xf5, 0x96, 0x92]);
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.replay_affected_ids.len(), 1);
    assert_eq!(scan.features.replay_affected_ids[0].feature_id, 4);
    assert_eq!(
        scan.features.replay_affected_ids[0].geometry_ids,
        vec![7, 128]
    );
    assert_eq!(scan.features.replay_affected_ids[0].edge_ids, vec![9]);
    assert_eq!(
        scan.features.replay_affected_ids[0].geometry_extent,
        crate::feature::ReplayExtentSource::Explicit
    );
    assert_eq!(
        scan.features.replay_affected_ids[0].edge_extent,
        crate::feature::ReplayExtentSource::Explicit
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert!(matches!(
        &result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. }, ..
        }] if selection == "creo:allfeatur:replay_edgs_affected#4:9")
    ));
    let records =
        &result.ir.native.namespace("creo").unwrap().arenas["feature_replay_affected_ids"];
    assert_eq!(records[0].fields["geometry_extent"], "explicit");
    assert_eq!(records[0].fields["edge_ids"][0], 9);
}

#[test]
fn scan_decodes_allfeatur_loop_restore_direction_compact_integers() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = b"\x04\xeb\x04lo_restore\0\xe0\x01direction\0\x00\
        \xe0\x01direction2\0\x80\xa7\xe0\x01direction\0\x01"
        .to_vec();
    let data = build_prt("c", &[("VisibGeom", geometry), ("AllFeatur", allfeatur)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.loop_restore_directions.len(), 3);
    assert_eq!(scan.features.loop_restore_directions[0].value, 0);
    assert_eq!(scan.features.loop_restore_directions[1].value, 167);
    assert_eq!(scan.features.loop_restore_directions[2].value, 1);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records =
        &result.ir.native.namespace("creo").unwrap().arenas["feature_loop_restore_directions"];
    assert_eq!(records[0].fields["value"], 0);
    assert_eq!(records[1].fields["value"], 167);
    assert_eq!(records[2].fields["value"], 1);
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature");
    let cadmpeg_ir::features::FeatureDefinition::Native { parameters, .. } = &feature.definition
    else {
        panic!("native feature");
    };
    assert_eq!(parameters["loop_restore.direction"], "0");
    assert_eq!(parameters["loop_restore.direction#2"], "1");
    assert_eq!(parameters["loop_restore.direction2"], "167");
}

#[test]
fn decode_types_full_turn_revolution_from_positional_angle_choice() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 40, 0x01, 0, 0]);
    let allfeatur = vec![
        40, 0xeb, 0x04, 0xe3, 0xf6, 0x83, 0x95, 0xe1, 0x02, 0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00,
        0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00, 0x00, 0x00,
    ];
    let mdlstatus = b"\xe3icon\0protrevolve\0Revolve id 40\0".to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", mdlstatus),
        ],
    );
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.revolution_extents.len(), 1);
    assert_eq!(scan.features.revolution_extents[0].feature_id, 40);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("revolution feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: Some(cadmpeg_ir::features::Extent::Angle {
                    angle: cadmpeg_ir::features::Angle(angle)
                }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::NewBody,
        } if (*angle - std::f64::consts::TAU).abs() < 1e-12
    ));
    let records = &result.ir.native.namespace("creo").unwrap().arenas["feature_revolution_extents"];
    assert_eq!(records[0].fields["kind"], "full_turn");
}

#[test]
fn decode_retains_recipe_proven_revolution_with_unresolved_operands() {
    let mdlstatus = b"\xe3icon\0cutrevolve\0K\xc3\xb6rper id 40\0".to_vec();
    let data = build_prt("c", &[("MdlStatus", mdlstatus)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("revolution feature");

    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
        }
    ));
}

#[test]
fn decode_retains_recipe_proven_extrusion_with_unresolved_operands() {
    let mdlstatus = b"\xe3icon\0cutextrude\0K\xc3\xb6rper id 40\0".to_vec();
    let data = build_prt("c", &[("MdlStatus", mdlstatus)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("extrusion feature");

    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_recipe_supplies_reference_backed_extrusion_boolean_effect() {
    let mdlstatus = b"\xe3icon\0cutextrude\0Extrude 1 id 40\0".to_vec();
    let data = build_prt("c", &[("MdlStatus", mdlstatus)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("reference-backed extrusion feature");

    assert_eq!(feature.name.as_deref(), Some("Extrude 1 id 40"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn scan_decodes_featdefs_records_and_parameter_frames() {
    let mut payload = b"feat_defs_40\0local_sys\0\xf9\x04\x03".to_vec();
    for _ in 0..3 {
        payload.extend_from_slice(&[0x0f, 0x18, 0xe5]);
    }
    payload.extend_from_slice(b"\xe0\x21transf\0\xf9\x04\x03");
    payload.extend([0xe4; 12]);
    payload.extend_from_slice(b"feat_defs_81\0opaque");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.definitions.len(), 2);
    assert_eq!(scan.features.definitions[0].id, 40);
    assert_eq!(scan.features.definitions[0].parameter_frames.len(), 2);
    assert_eq!(
        scan.features.definitions[0].parameter_frames[0].kind,
        crate::feature::FeatureParameterFrameKind::LocalSystem
    );
    assert_eq!(
        scan.features.definitions[0].parameter_frames[0].decoded_values,
        Some(vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0
        ])
    );
    assert_eq!(
        scan.features.definitions[0].parameter_frames[1].kind,
        crate::feature::FeatureParameterFrameKind::Transform
    );
    assert_eq!(
        scan.features.definitions[0].parameter_frames[1].decoded_values,
        Some(vec![1.0; 12])
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let definitions = &result.ir.native.namespace("creo").unwrap().arenas["feature_definitions"];
    let frames = definitions[0].fields["parameter_frames"]
        .as_array()
        .expect("parameter frames");
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["kind"], "local_system");
    assert_eq!(frames[0]["decoded_values"].as_array().unwrap().len(), 12);
    assert_eq!(frames[0]["decoded_values"][0], 0.0);
    assert_eq!(frames[0]["decoded_values"][2], 1.0);
    assert_eq!(frames[1]["kind"], "transform");
    assert_eq!(frames[1]["decoded_values"].as_array().unwrap().len(), 12);
    assert_eq!(frames[1]["decoded_values"][0], 1.0);
}

#[test]
fn scan_decodes_rank_two_featdefs_local_system() {
    let mut payload = b"feat_defs_40\0local_sys\0\xf9\x04\x03\x0f\x18\xe5\x18\xe5".to_vec();
    payload.extend_from_slice(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x2d, 0x10, 0, 0, 0, 0, 0, 0]);
    payload.push(0x18);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data);

    assert_eq!(
        scan.features.definitions[0].parameter_frames[0].decoded_values,
        Some(vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -3.0, -4.0, 0.0
        ])
    );
}

#[test]
fn scan_decodes_featdefs_feature_local_outlines() {
    let mut payload = b"feat_defs_40\0\xe0\x00feat_outl_info\0outline\0\xf9\x02\x03".to_vec();
    payload.extend([0x0f; 6]);
    payload.extend_from_slice(b"\xe0\x00post_roll_back\0\xe3\xf7\x01\xf5\x96\x92\x02");
    payload.extend([0xe4; 6]);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let outlines = &scan.features.definitions[0].outlines;
    assert_eq!(outlines.len(), 2);
    assert_eq!(outlines[0].phase, crate::feature::OutlinePhase::PreRollback);
    assert_eq!(outlines[0].local_values, vec![Some(0.0); 6]);
    assert_eq!(
        outlines[1].phase,
        crate::feature::OutlinePhase::PostRollback
    );
    assert_eq!(outlines[1].local_values, vec![Some(1.0); 6]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let definitions = &result.ir.native.namespace("creo").unwrap().arenas["feature_definitions"];
    let outlines = definitions[0].fields["outlines"]
        .as_array()
        .expect("outlines");
    assert_eq!(outlines.len(), 2);
    assert_eq!(outlines[0]["phase"], "pre_rollback");
    assert_eq!(outlines[0]["local_values"].as_array().unwrap().len(), 6);
    assert_eq!(outlines[0]["local_values"][0], 0.0);
    assert_eq!(outlines[1]["phase"], "post_rollback");
    assert_eq!(outlines[1]["local_values"].as_array().unwrap().len(), 6);
    assert_eq!(outlines[1]["local_values"][0], 1.0);
}

#[test]
fn scan_decodes_featdefs_var_arr_section_points() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    payload.extend_from_slice(&[1, 8, 0xe4, 0x0f, 1, 0, 5, 0xe2]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let variables = scan.features.definitions[0]
        .variables
        .as_ref()
        .expect("var_arr");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(1));
    assert_eq!(variables.rows.len(), 2);
    assert_eq!(variables.rows[0].value, Some(1.0));
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(0));
    assert_eq!(variables.rows[0].uvar_id, Some(3));
    assert_eq!(variables.rows[1].value, Some(3.0));
    assert_eq!(variables.rows[1].known, Some(1));
    assert_eq!(variables.rows[1].homogeneity, Some(0));
    assert_eq!(variables.rows[1].uvar_id, Some(4));
    assert_eq!(variables.points.len(), 1);
    assert_eq!(variables.points[0].point_id, 7);
    assert_eq!(variables.points[0].u, Some(1.0));
    assert_eq!(variables.points[0].v, Some(3.0));
}

#[test]
fn scan_decodes_featdefs_var_arr_named_prototype_row() {
    let payload = b"feat_defs_40\0var_arr\0\xf8\x01\xf7\x01\xfb\xe2\
        \xe0\x05type\0\x01\xe0\x08key\0\x07\xe0\x02value\0\xe4\
        \xe0\x02guess\0\x0f\xe0\x06known\0\x01\
        \xe0\x0chomogeneity\0\x02\xe0\x08uvar_id\0\x03\xf1\xf7\x01\xe2"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let variables = scan.features.definitions[0]
        .variables
        .as_ref()
        .expect("var_arr");
    assert_eq!(variables.rows.len(), 1);
    assert_eq!(variables.rows[0].variable_type, 1);
    assert_eq!(variables.rows[0].key, 7);
    assert_eq!(variables.rows[0].value, Some(1.0));
    assert_eq!(variables.rows[0].guess, Some(0.0));
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(2));
    assert_eq!(variables.rows[0].uvar_id, Some(3));
}

#[test]
fn decode_transfers_featdefs_sketch_variables_as_native_design_data() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    let definition_length = payload.len();
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());
    let offset = scan.features.definitions[0].offset as u64;
    let variable_offset = scan.features.definitions[0]
        .variables
        .as_ref()
        .unwrap()
        .rows[0]
        .offset;
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    let namespace = result.ir.native.namespace("creo").expect("creo namespace");
    assert_eq!(namespace.version, 1);
    let definitions = &namespace.arenas["feature_definitions"];
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].id, "creo:featdefs:feature_definition#40");
    assert_eq!(definitions[0].fields["definition_id"], 40);
    assert_eq!(
        definitions[0].fields["body"].as_array().unwrap().len(),
        definition_length
    );
    let sketches = &namespace.arenas["sketches"];
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].id, "creo:featdefs:sketch#40");
    assert_eq!(sketches[0].fields["definition_id"], 40);
    assert!(sketches[0].fields["owner_feature_id"].is_null());
    let headers = sketches[0].fields["table_headers"]
        .as_array()
        .expect("table headers");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0]["kind"], "variables");
    assert_eq!(headers[0]["declared_count"], 2);
    assert_eq!(headers[0]["entity_ref"], 1);
    assert_eq!(headers[0]["row_count"], 2);
    let points = sketches[0].fields["section_points"]
        .as_array()
        .expect("section points");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0]["point_id"], 7);
    assert_eq!(points[0]["u"], 1.0);
    assert_eq!(points[0]["v"], 3.0);
    assert_eq!(points[0]["state"], "resolved");
    let variables = sketches[0].fields["variables"]
        .as_array()
        .expect("variables array");
    assert_eq!(variables.len(), 2);
    assert_eq!(variables[0]["key"], 7);
    assert_eq!(variables[0]["value"], 1.0);
    assert_eq!(variables[0]["known"], 1);
    assert_eq!(variables[0]["homogeneity"], 0);
    assert_eq!(variables[0]["uvar_id"], 3);
    assert_eq!(variables[0]["offset"], variable_offset);
    assert_eq!(variables[1]["value"], 3.0);
    assert_eq!(variables[1]["known"], 1);
    assert_eq!(variables[1]["homogeneity"], 0);
    assert_eq!(variables[1]["uvar_id"], 4);
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
        b"feat_defs_40\0segtab_ptr\0\xf8\x05\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2, 0xe3]);
    payload.extend_from_slice(&[2, 0, 0, 0, 9, 10, 0xf6, 0, 0, 0xf6, 0xf6, 0x80, 0xe3, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0, 0xf6, 0xe2, 0xc0, 0x80]);
    payload.extend_from_slice(&[2, 0, 0, 0, 11, 12, 0xf6, 0, 0, 0xf6, 0xf6, 0, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0, 0xf6, 0xe2]);
    payload.extend_from_slice(&[5, 1, 0, 0xe4, 13, 0xe4, 0xf6, 0, 2, 0xf6, 0xf6, 4, 0xe2]);
    payload.extend_from_slice(b"dimtab_ptr\0");
    payload.extend_from_slice(&[2, 0, 0, 0, 11, 12, 0xf6, 0, 0, 0xf6, 0xf6, 44, 0xe2]);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let segments = scan.features.definitions[0]
        .segments
        .as_ref()
        .expect("segtab");
    assert_eq!(segments.declared_count, 5);
    assert_eq!(segments.rows.len(), 5);
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
    assert_eq!(segments.rows[2].external_id, 227);
    assert_eq!(segments.rows[3].point_ids, [11, 12]);
    assert_eq!(segments.rows[3].external_id, 0);
    assert_eq!(
        segments.rows[4].kind,
        crate::feature::FeatureSegmentKind::Point
    );
    assert_eq!(segments.rows[4].point_ids, [13, 13]);
    assert_eq!(segments.rows[4].external_id, 4);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let sketch = result
        .ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id.0 == "creo:model:sketch#40")
        .expect("neutral unplaced sketch");
    assert_eq!(
        sketch.placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved
    );
    assert_eq!(
        result
            .ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .count(),
        5
    );
}

#[test]
fn decode_retains_repeated_sketch_snapshots_with_offset_identities() {
    let mut definition =
        b"feat_defs_40\0segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    definition.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    definition.extend_from_slice(&[25, 0, 0, 0, 8, 9, 0xf6, 0, 0, 0xf6, 0xf6, 43, 0xe2, 0xe3]);
    definition.extend_from_slice(
        b"dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
          \xe0\x01type\0\x02\xe0\x01value\0\xe4\
          \xe0\x01direct\0\x00\xe0\x01aux_value\0\x0f\
          \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0",
    );
    let mut payload = definition.clone();
    payload.extend_from_slice(&definition);
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(result.ir.model.sketches.len(), 2);
    assert_eq!(result.ir.model.features.len(), 2);
    assert!(result
        .ir
        .model
        .sketches
        .iter()
        .all(|sketch| sketch.id.0.starts_with("creo:model:sketch#offset:")));
    for sketch in &result.ir.model.sketches {
        let expected_native_ref =
            sketch
                .id
                .0
                .replacen("creo:model:sketch#", "creo:featdefs:sketch#", 1);
        let identity_scope = sketch
            .id
            .0
            .strip_prefix("creo:model:sketch#")
            .expect("Creo sketch identity");
        assert_eq!(
            sketch.native_ref.as_deref(),
            Some(expected_native_ref.as_str())
        );
        assert_eq!(
            result
                .ir
                .model
                .sketch_entities
                .iter()
                .filter(|entity| entity.sketch == sketch.id)
                .count(),
            2
        );
        assert!(result
            .ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .all(|entity| entity.id.0.contains(&format!("#{identity_scope}:"))));
        let parameters = result
            .ir
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.native_ref.as_deref() == sketch.native_ref.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(parameters.len(), 1);
        assert_eq!(
            parameters[0].owner,
            Some(cadmpeg_ir::features::FeatureId(format!(
                "creo:model:sketch_feature#{identity_scope}"
            )))
        );
        assert!(parameters[0].id.0.contains(&format!("#{identity_scope}:")));
    }
    assert_eq!(
        result
            .ir
            .model
            .sketch_entities
            .iter()
            .map(|entity| &entity.id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn resolved_section_points_propagate_orientation_and_signed_dimensions() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureVariableRow {
                variable_type: 3,
                key: 6,
                value: None,
                guess: None,
                known: None,
                homogeneity: None,
                uvar_id: None,
                dimension_driven: true,
                offset: 0,
            }],
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 1,
                    u: Some(2.0),
                    v: Some(3.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 3,
                    u: Some(7.0),
                    v: Some(11.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 4,
                    u: Some(5.0),
                    v: Some(20.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 5,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 6,
                    u: Some(20.0),
                    v: Some(30.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 7,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 8,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 9,
                    u: Some(20.0),
                    v: Some(40.0),
                },
            ],
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 5,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 1,
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [6, 7],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 4,
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [Some(1), None, None],
                    point_ids: [8, 9],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 5,
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [4, 5],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(1),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 3,
                    offset: 0,
                },
                crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [2, 3],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(0),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 2,
                    offset: 0,
                },
            ],
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureDimension {
                    dimension_type: 2,
                    value: Some(12.0),
                    unresolved_value_token: None,
                    value_unit: crate::feature::DimensionUnit::Millimeters,
                    direction_byte: 0,
                    auxiliary_value: Some(0.0),
                    external_id: 1,
                    offset: 0,
                },
                crate::feature::FeatureDimension {
                    dimension_type: 3,
                    value: Some(4.0),
                    unresolved_value_token: None,
                    value_unit: crate::feature::DimensionUnit::Millimeters,
                    direction_byte: 0,
                    auxiliary_value: Some(0.0),
                    external_id: 2,
                    offset: 0,
                },
            ],
            offset: 0,
        }),
        relations: Some(crate::feature::FeatureRelationTable {
            declared_count: 6,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureRelation {
                    relation_id: 1,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(4), Some(5), None, Some(1)],
                        [Some(1), Some(1), Some(0), Some(1)],
                        [Some(15), Some(16), Some(15), Some(1)],
                    ]),
                    sign: 1,
                    dimension_id: 0,
                    relation_type: 0,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureRelation {
                    relation_id: 3,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(6), Some(7), None, Some(1)],
                        [Some(1), Some(1), Some(0), Some(1)],
                        [Some(15), Some(16), Some(15), Some(1)],
                    ]),
                    sign: 0,
                    dimension_id: 0,
                    relation_type: 0,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureRelation {
                    relation_id: 4,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(8), Some(9), None, Some(1)],
                        [Some(1), Some(1), Some(0), Some(1)],
                        [Some(15), Some(16), Some(15), Some(1)],
                    ]),
                    sign: 0,
                    dimension_id: 0,
                    relation_type: 0,
                    body: Vec::new(),
                    offset: 0,
                },
                crate::feature::FeatureRelation {
                    relation_id: 2,
                    used: 0,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(6), Some(0), Some(0), Some(0)],
                        [Some(0); 4],
                        [Some(15), Some(0), Some(0), Some(0)],
                    ]),
                    sign: 1,
                    dimension_id: 1,
                    relation_type: 14,
                    body: Vec::new(),
                    offset: 0,
                },
            ],
            skamps: Vec::new(),
            skamp_header: None,
            triples: Vec::new(),
            triples_header: None,
            offset: 0,
        }),
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&2),
        Some(&[7.0, 3.0])
    );
    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&5),
        Some(&[17.0, 20.0])
    );
    assert_eq!(
        crate::decode::resolved_section_radii(&definition).get(&6),
        Some(&4.0)
    );
    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&7),
        Some(&[8.0, 30.0])
    );
    assert_eq!(
        crate::decode::resolved_section_points(&definition).get(&8),
        Some(&[8.0, 40.0])
    );
    let mut incomplete_variables = definition.clone();
    incomplete_variables
        .variables
        .as_mut()
        .expect("variables")
        .declared_count = 2;
    assert!(crate::decode::resolved_section_points(&incomplete_variables).is_empty());

    let mut incomplete_segments = definition;
    incomplete_segments
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 6;
    assert!(!crate::decode::resolved_section_points(&incomplete_segments).contains_key(&2));
}

#[test]
fn scan_includes_named_segtab_prototype_as_data() {
    let payload = b"feat_defs_40\0segtab_ptr\0\xf8\x01\xf7\x01\xfb\xe2\
        type\0\x02dir\0\xf8\x03\xf6\x00\xe4pointid\0\xf8\x02\x00\x01\
        cntrid\0\xf6arcorient\0\x00verhor\0\x01radius\0\xf6radius2\0\xf6\
        ext_id\0\x04\xf2\xf7\x01\xe2order_table\0";
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload.to_vec())]));
    let segments = scan.features.definitions[0]
        .segments
        .as_ref()
        .expect("segtab");

    assert_eq!(segments.rows.len(), 1);
    assert_eq!(segments.rows[0].external_id, 4);
    assert_eq!(segments.rows[0].point_ids, [0, 1]);
    assert_eq!(segments.rows[0].vertical_horizontal, Some(1));
}

#[test]
fn scan_decodes_featdefs_ent_tab_trimmed_entities() {
    let mut payload =
        b"feat_defs_40\0ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3".to_vec();
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 101, 102, 103, 0, 0xe3]);
    payload.extend_from_slice(&[0x80, 0xe3, 0, 102, 104, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let entities = scan.features.definitions[0]
        .trim_entities
        .as_ref()
        .expect("ent_tab");
    assert_eq!(entities.rows.len(), 3);
    assert_eq!(entities.rows[0].external_id, 42);
    assert_eq!(entities.rows[0].vertices, [100, 101]);
    assert_eq!(entities.rows[0].center_vertex, None);
    assert_eq!(entities.rows[0].kind, crate::feature::TrimEntityKind::Line);
    assert_eq!(entities.rows[1].kind, crate::feature::TrimEntityKind::Arc);
    assert_eq!(entities.rows[2].external_id, 227);
    assert_eq!(entities.solved_external_ids, vec![42, 43, 227]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let trim_entities =
        &result.ir.native.namespace("creo").unwrap().arenas["sketches"][0].fields["trim_entities"];
    assert_eq!(
        trim_entities.as_array().expect("trim entity array").len(),
        3
    );
    assert_eq!(trim_entities[0]["kind"], "line");
    assert_eq!(trim_entities[1]["kind"], "arc");
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
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let vertices = scan.features.definitions[0]
        .trim_vertices
        .as_ref()
        .expect("vert_tab");
    assert_eq!(vertices.rows.len(), 1);
    assert_eq!(vertices.rows[0].vertex_id, 100);
    assert_eq!(vertices.rows[0].entities, [42, 43]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let trim_vertices =
        &result.ir.native.namespace("creo").unwrap().arenas["sketches"][0].fields["trim_vertices"];
    assert_eq!(
        trim_vertices.as_array().expect("trim vertex array").len(),
        1
    );
    assert_eq!(trim_vertices[0]["vertex_id"], 100);
    assert_eq!(trim_vertices[0]["entities"][0], 42);
    assert_eq!(trim_vertices[0]["entities"][1], 43);
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
    let vertex = &scan.features.definitions[0]
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
        \xf1\xf7\x81\x02\xe2\x81\x1b\x08\x00\xe2\x81\x36\x0c\x01\xe0\x01next_field\0"
        .to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let order = scan.features.definitions[0]
        .order_table
        .as_ref()
        .expect("order_table");
    assert_eq!(order.declared_count, 2);
    assert!(!order.has_prototype);
    assert!(order.is_complete());
    assert_eq!(order.entity_ref, Some(258));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.rows[0].external_id, 283);
    assert_eq!(order.rows[0].internal_id, 8);
    assert_eq!(order.rows[0].bitmask, 0);
    assert_eq!(order.external_id(12), Some(310));
    assert_eq!(order.internal_id(283), Some(8));

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let order_rows =
        &result.ir.native.namespace("creo").unwrap().arenas["sketches"][0].fields["order_rows"];
    assert_eq!(order_rows.as_array().expect("order row array").len(), 2);
    assert_eq!(order_rows[0]["external_id"], 283);
    assert_eq!(order_rows[1]["internal_id"], 12);
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
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let section = scan.features.definitions[0]
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

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let sketches = &result.ir.native.namespace("creo").unwrap().arenas["sketches"];
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].fields["source_section"], "FeatDefs");
    let placement = &sketches[0].fields["section_3d"];
    assert_eq!(placement["sketch_plane_entity_id"], 769);
    assert_eq!(placement["sketch_plane_flip"], true);
    assert_eq!(placement["reference_plane_entity_ids"][0], 5);
    assert_eq!(placement["reference_plane_entity_ids"][1], 256);
    assert_eq!(placement["reference_plane_datum_geometry_id"], 9);
    assert_eq!(placement["orientation"]["section_flip"], true);
    assert_eq!(placement["orientation"]["reference_type"], 2);
    assert_eq!(placement["orientation"]["segment_id"], 300);
    assert_eq!(placement["orientation"]["reference_flip"], false);
    assert_eq!(placement["dimension_ids"][0], 7);
    assert_eq!(placement["dimension_ids"][1], 257);
}

#[test]
fn scan_decodes_featdefs_dimension_prototype_and_replay() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x03\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a"
        .to_vec();
    payload.extend_from_slice(b"\xf3\xf7\x81\x02\xe2");
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x81\x02\xe2");
    payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
    payload.extend_from_slice(b"\xe0\x00relat_ptr\0");
    let expressions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02angle=d42\0length=d43+2[mm]\0"
        .to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("FeatDefs", payload), ("DEPDB_DATA", expressions)],
    ));

    let dimensions = scan.features.definitions[0]
        .dimensions
        .as_ref()
        .expect("dimtab");
    assert_eq!(dimensions.declared_count, 3);
    assert_eq!(dimensions.entity_ref, Some(258));
    assert_eq!(dimensions.rows.len(), 3);
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
    assert_eq!(
        dimensions.rows[1].value_unit,
        crate::feature::DimensionUnit::Millimeters
    );
    assert_eq!(dimensions.rows[1].auxiliary_value, Some(0.0));
    assert_eq!(dimensions.rows[1].external_id, 43);
    assert_eq!(
        dimensions.rows[2].value,
        Some(f64::from_be_bytes([
            0x3f, 0xd5, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f
        ]))
    );
    assert_eq!(dimensions.rows[2].external_id, 44);
    assert_eq!(scan.curves.expressions.len(), 1);
    assert_eq!(
        scan.curves.expressions[0].assignments[0].value,
        Some(crate::curve::CurveExpressionValue::Angle(
            1.0f64.to_degrees()
        ))
    );
    assert_eq!(
        scan.curves.expressions[0].assignments[1].value,
        Some(crate::curve::CurveExpressionValue::Length(5.0))
    );
}

#[test]
fn decode_transfers_feature_dimensions_as_owned_parameters() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x02\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xf3\xf7\x81\x02\xe2\
        \x0a\xe4\x01\x18\x2a\xe0\x00relat_ptr\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
            (
                "DEPDB_DATA",
                b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
                    \xe0\x0aexpression\0\xf8\x01result=d42+1[deg]\0"
                    .to_vec(),
            ),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.definitions[0].id, 917);
    assert_eq!(scan.features.definitions[0].owner_feature_id, Some(40));
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(result.ir.model.parameters.len(), 3);
    let parameter = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d917_42_1")
        .expect("first repeated dimension");
    let repeated = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d917_42_2")
        .expect("second repeated dimension");
    let relation = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "result")
        .expect("relation parameter");
    assert_eq!(
        parameter.owner.as_ref().unwrap().as_str(),
        "creo:model:sketch_feature#917"
    );
    assert_eq!(parameter.name, "d917_42_1");
    assert_eq!(repeated.name, "d917_42_2");
    assert_ne!(parameter.id, repeated.id);
    assert_eq!(parameter.expression, "1");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(1.0)
        ))
    );
    assert!(relation.dependencies.is_empty());
    assert_eq!(relation.properties["external_dependencies"], "d42");
    assert_eq!(
        relation.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(1.0 + 1.0f64.to_radians())
        ))
    );
    let model_feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#40")
        .expect("model feature");
    assert!(matches!(
        &model_feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            ..
        }
    ));
    assert_eq!(
        model_feature.source_properties["native_parameter.dimension_count"],
        "2"
    );
    let sketch_feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:sketch_feature#917")
        .expect("sketch feature");
    assert_eq!(
        sketch_feature.source_content,
        [
            cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone()),
            cadmpeg_ir::features::FeatureSourceContent::Parameter(repeated.id.clone()),
        ]
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_transfers_decoded_dimensions_from_an_incomplete_table() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x03\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xf3\xf7\x81\x02\xe2\
        \x02\x46\x08\x00\x00\x00\x00\x00\x00\x00\x00\x18\x2b\xe0\x00relat_ptr\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    let dimensions = scan.features.definitions[0]
        .dimensions
        .as_ref()
        .expect("dimension table");
    assert_eq!(dimensions.declared_count, 3);
    assert_eq!(dimensions.rows.len(), 2);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode incomplete dimension table");

    assert_eq!(result.ir.model.parameters.len(), 2);
    assert!(result.ir.model.parameters.iter().all(|parameter| parameter
        .owner
        .as_ref()
        .unwrap()
        .as_str()
        == "creo:model:sketch_feature#917"));
    let coverage = &result.report.coverage;
    assert_eq!(coverage["decoded_feature_dimension_count"], 2);
    assert_eq!(coverage["transferred_feature_dimension_parameter_count"], 2);
    assert_eq!(coverage["resolved_feature_dimension_value_count"], 2);
}

#[test]
fn decode_retains_bounded_unresolved_dimension_value_tokens() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x03\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x01\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x00\xe0\x01aux_value\0\x18\
        \xe0\x01ext_id\0\x2a\xf3\xf7\x81\x02\xe2\
        \x01\x00\x04\xa6\x00\x18\x2b\xf3\xf7\x81\x02\xe2\
        \x01\x01\x04\xfe\xf2\x00\x18\x2c\xe0\x00relat_ptr\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
        ],
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode dimensions");

    let parameters = &result.ir.model.parameters;
    assert_eq!(parameters.len(), 3);
    assert_eq!(parameters[1].properties["value_state"], "unresolved");
    assert_eq!(
        parameters[1].properties["value_encoding"],
        "three_byte_placeholder"
    );
    assert_eq!(parameters[1].properties["value_token"], "0004a6");
    assert_eq!(
        parameters[2].properties["value_encoding"],
        "four_byte_placeholder"
    );
    assert_eq!(parameters[2].properties["value_token"], "0104fef2");

    let sketches = &result.ir.native.namespace("creo").unwrap().arenas["sketches"];
    let dimensions = sketches[0].fields["dimensions"]
        .as_array()
        .expect("native dimensions");
    assert_eq!(dimensions[1]["unresolved_value_token"][0], 0);
    assert_eq!(dimensions[1]["unresolved_value_token"][1], 4);
    assert_eq!(dimensions[1]["unresolved_value_token"][2], 166);
    assert_eq!(dimensions[2]["unresolved_value_token"][0], 1);
    assert_eq!(dimensions[2]["unresolved_value_token"][1], 4);
    assert_eq!(dimensions[2]["unresolved_value_token"][2], 254);
    assert_eq!(dimensions[2]["unresolved_value_token"][3], 242);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_dimensions_from_repeated_feature_definition_ids() {
    let definition = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
        \xe0\x01type\0\x02\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x00\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(definition);
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.definitions.len(), 2);
    assert!(scan
        .features
        .definitions
        .iter()
        .all(|definition| definition.id == 917));

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let namespace = result.ir.native.namespace("creo").expect("creo namespace");
    let definition_ids = namespace.arenas["feature_definitions"]
        .iter()
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    let sketch_ids = namespace.arenas["sketches"]
        .iter()
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(definition_ids.len(), 2);
    assert_eq!(sketch_ids.len(), 2);
    assert!(definition_ids
        .iter()
        .all(|id| id.starts_with("creo:featdefs:feature_definition#offset:")));
    assert!(sketch_ids
        .iter()
        .all(|id| id.starts_with("creo:featdefs:sketch#offset:")));

    assert_eq!(result.ir.model.parameters.len(), 2);
    assert_ne!(
        result.ir.model.parameters[0].id,
        result.ir.model.parameters[1].id
    );
    assert_ne!(
        result.ir.model.parameters[0].native_ref,
        result.ir.model.parameters[1].native_ref
    );
    assert!(result.ir.model.parameters.iter().all(|parameter| {
        parameter.value
            == Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(1.0),
            ))
    }));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn scan_decodes_counted_featdefs_constraint_relations() {
    let mut payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x04\xf7\x6a\xfb\xe2\
        \xe0\x01id\0\xe0\x01used\0\xe0\x01type\0\xf1\xf7\x6a\xe2\
        \x34\x00\x05\x01\xf6\xe4\x00\xe6\x0f\x10\x0f\xe4\x00\x00\x00\xe2\
        \x35\x01\x07\x29\x32\xf6\x00\xe6\x0f\x10\x0f\xe4\x01\x2a\x03\xe2"
        .to_vec();
    payload.extend_from_slice(
        b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
          \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
          \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
          \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
          \xf3\xf7\x6b\xe2\
          triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
          \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\xe0\x01skamp_id\0\x05\
          \xf1\xf7\x6d\xe2\xf6\x09\x05\xe2",
    );
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let relations = scan.features.definitions[0]
        .relations
        .as_ref()
        .expect("relat_ptr");
    assert_eq!(relations.declared_count, 4);
    assert_eq!(relations.entity_ref, Some(106));
    assert_eq!(relations.rows.len(), 2);
    assert_eq!(relations.rows[0].relation_id, 52);
    assert_eq!(relations.rows[0].used, 0);
    assert_eq!(
        relations.rows[0].operands,
        [0x05, 0x01, 0xf6, 0xe4, 0x00, 0xe6, 0x0f, 0x10, 0x0f, 0xe4]
    );
    assert_eq!(
        relations.rows[0].operand_vectors,
        Some([
            [Some(5), Some(1), None, Some(1)],
            [Some(0), Some(0), Some(0), Some(0)],
            [Some(15), Some(16), Some(15), Some(1)],
        ])
    );
    assert_eq!(relations.rows[0].sign, 0);
    assert_eq!(relations.rows[0].dimension_id, 0);
    assert_eq!(relations.rows[0].relation_type, 0);
    assert_eq!(relations.rows[1].relation_id, 53);
    assert_eq!(relations.rows[1].used, 1);
    assert_eq!(relations.rows[1].dimension_id, 42);
    assert_eq!(relations.rows[1].relation_type, 3);
    assert_eq!(relations.skamps.len(), 1);
    assert_eq!(relations.skamps[0].id, 5);
    assert_eq!(relations.skamps[0].kind, 2);
    assert_eq!(relations.skamps[0].items[0].entity_id, 42);
    assert_eq!(relations.skamps[0].items[0].sense, 1);
    let skamp_header = relations.skamp_header.as_ref().expect("skamp header");
    assert_eq!(skamp_header.declared_count, 1);
    assert_eq!(skamp_header.entity_ref, 107);
    assert_eq!(relations.triples.len(), 2);
    assert_eq!(relations.triples[0].relation_id, Some(7));
    assert_eq!(relations.triples[0].equation_id, Some(8));
    assert_eq!(relations.triples[0].skamp_id, Some(5));
    assert_eq!(relations.triples[1].relation_id, None);
    assert_eq!(relations.triples[1].equation_id, Some(9));
    let triples_header = relations.triples_header.as_ref().expect("triples header");
    assert_eq!(triples_header.declared_count, 2);
    assert_eq!(triples_header.entity_ref, 109);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let headers = result.ir.native.namespace("creo").unwrap().arenas["sketches"][0].fields
        ["table_headers"]
        .as_array()
        .expect("table headers");
    let solver = headers
        .iter()
        .find(|header| header["kind"] == "solver_incidences")
        .expect("solver-incidence header");
    assert_eq!(solver["declared_count"], 1);
    assert_eq!(solver["entity_ref"], 107);
    assert_eq!(solver["row_count"], 1);
    let triples = headers
        .iter()
        .find(|header| header["kind"] == "relation_triples")
        .expect("relation-triple header");
    assert_eq!(triples["declared_count"], 2);
    assert_eq!(triples["entity_ref"], 109);
    assert_eq!(triples["row_count"], 2);
}

#[test]
fn scan_decodes_extended_solver_incidences() {
    let payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x02\xf7\x6a\xfb\xe2\
        schema\xf1\xf7\x6a\xe2\
        skamp_ptr\0\xf4\x05\xf8\x02\xf7\x6b\xfb\xe2\
        \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
        \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
        \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
        \xf3\xf7\x6b\xe2\
        \xc0\x40\x01\x0e\xc0\x40\x00\x22\xf8\x03\xf7\x6c\xfb\xe2\
        \xf7\x6d\x09\x03\xf1\xf7\x6c\xe2\x0a\x02\xe2\x0b\x03\
        \xe0\x00triples_ptr\0"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));
    let relations = scan.features.definitions[0]
        .relations
        .as_ref()
        .expect("relat_ptr");

    assert_eq!(relations.skamps.len(), 2);
    assert_eq!(relations.skamps[1].id, 0x4001);
    assert_eq!(relations.skamps[1].kind, 14);
    assert_eq!(relations.skamps[1].flags, 0x4000);
    assert_eq!(relations.skamps[1].status, 34);
    assert_eq!(
        relations.skamps[1]
            .items
            .iter()
            .map(|item| (item.entity_id, item.sense))
            .collect::<Vec<_>>(),
        [(9, 3), (10, 2), (11, 3)]
    );
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

    let saved = scan.features.definitions[0]
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
          \xe0\x02center\0\xf8\x03\x18\xe5\
          \xe0\x02radius\0\xe4",
    );
    payload.extend_from_slice(b"\xe0\x00entity(dummy_ent)\0\xe0\x01id\0\x2e");
    payload.extend_from_slice(b"\xe0\x02local_sys\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let entities = &scan.features.definitions[0]
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
    assert_eq!(circle.center, [Some(0.0), Some(1.0), Some(0.0)]);
    let crate::feature::FeatureSavedEntity::Dummy(dummy) = &entities[2] else {
        panic!("saved dummy");
    };
    assert_eq!(dummy.entity_id, Some(46));

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let saved =
        &result.ir.native.namespace("creo").unwrap().arenas["sketches"][0].fields["saved_entities"];
    assert_eq!(saved.as_array().expect("saved entity array").len(), 3);
    assert_eq!(saved[0]["kind"], "arc");
    assert_eq!(saved[1]["kind"], "circle");
    assert_eq!(saved[2]["kind"], "dummy");
}

#[test]
fn decode_transfers_equation_verified_model_reference_circles() {
    let payload = b"ent_list(arc_z)\0\xe2\x2d\xe3\x2d\x0f\xe2\x01\
        \xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe0\x00ent_list(line3d)\0"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.references.circles.len(), 1);
    assert_eq!(scan.references.circles[0].center, [0.0; 3]);
    assert_eq!(scan.references.circles[0].radius, 1.0);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle { radius: 1.0, .. }
    )));
    let circle = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:mdl_ref_info:arc_z#45")
        .expect("canonically identified arc_z");
    assert_eq!(
        circle.source_object.as_ref().unwrap().object_id,
        "MdlRefInfo:arc_z:45"
    );
    let record = &result.ir.native.namespace("creo").unwrap().arenas["reference_circles"][0];
    assert_eq!(record.fields["entity_id"], 45);
    assert_eq!(record.fields["center_source"], "endpoint_midpoint");
    assert_annotation(
        &result.source_fidelity.annotations,
        &record.id,
        "creo:MdlRefInfo",
        scan.references.circles[0].offset as u64,
        "reference_circle_record",
        Exactness::Derived,
    );
}

#[test]
fn decode_retains_line3d_original_length() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\xe4\x0f\x0f\xe4"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let scan = container::scan_bytes(data.clone());
    let [line] = scan.references.lines.as_slice() else {
        panic!("one line3d");
    };
    assert_eq!(
        line.kind,
        crate::reference::ReferenceLineKind::Line3d {
            entity_id: 35,
            original_length: 1.0
        }
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let record = &result.ir.native.namespace("creo").unwrap().arenas["reference_lines"][0];
    assert_eq!(record.fields["family"], "line3d");
    assert_eq!(record.fields["entity_id"], 35);
    assert_eq!(record.fields["original_length"], 1.0);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:mdl_ref_info:line3d#35")
        .expect("canonically identified line3d");
    assert_eq!(
        curve.source_object.as_ref().unwrap().object_id,
        "MdlRefInfo:line3d:35"
    );
}

#[test]
fn decode_disambiguates_repeated_line3d_entity_ids() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\xe4\x0f\x0f\xe4\
        \x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\x43\xf0\x00\x0f\x0f\xe4"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let ids = result
        .ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .as_str()
                .starts_with("creo:mdl_ref_info:line3d#35@")
        })
        .map(|curve| curve.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn decode_reports_and_retains_invariant_complete_reference_ellipses() {
    let payload = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
        \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
        \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\x0f\xe4\x43\xf0\x00\xe4\
        \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\
        \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.references.conics.len(), 1);
    assert_eq!(scan.references.ellipses.len(), 1);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Ellipse {
            major_radius: 1.0,
            minor_radius: 1.0,
            ..
        }
    )));
    let record = &result.ir.native.namespace("creo").unwrap().arenas["reference_ellipses"][0];
    assert_eq!(record.fields["source_entity_id"], 43);
    assert_eq!(record.fields["major_radius"], 1.0);
    assert_eq!(record.fields["minor_radius"], 1.0);
    assert_eq!(
        result.report.coverage["transferred_reference_ellipse_count"],
        1
    );
    let ellipse = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:mdl_ref_info:conic#43")
        .expect("canonically identified conic");
    assert_eq!(
        ellipse.source_object.as_ref().unwrap().object_id,
        "MdlRefInfo:conic:43"
    );
    assert!(result.report.losses.iter().any(|loss| {
        loss.message
            .contains("Transferred 1 elliptical reference carrier")
    }));
    assert_annotation(
        &result.source_fidelity.annotations,
        &record.id,
        "creo:MdlRefInfo",
        scan.references.ellipses[0].offset as u64,
        "reference_ellipse_carrier",
        Exactness::Derived,
    );
}

#[test]
fn scan_reads_declared_geomlists_body_count() {
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("Geomlists", b"n_bodies\0\x83\x01".to_vec())],
    ));

    assert_eq!(scan.framing.declared_body_count, Some(769));
}

#[test]
fn scan_reads_geomlists_first_quilt_discriminator() {
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("Geomlists", b"first_quilt_ptr\0\x00".to_vec())],
    ));

    assert_eq!(scan.framing.first_quilt_ptr, Some(0));
}

#[test]
fn scan_discovers_labeled_curve_prototypes() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.prototypes.len(), 1);
    assert_eq!(scan.curves.prototypes[0].id, 7);
    assert_eq!(scan.curves.prototypes[0].type_byte, 8);
    assert_eq!(scan.curves.prototypes[0].feature_id, Some(4));
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["curve_prototypes"];
    assert_eq!(records[0].fields["curve_id"], 7);
    assert_eq!(records[0].fields["type_byte"], 8);
    assert_eq!(records[0].fields["generating_feature_id"], 4);
}

#[test]
fn decode_preserves_counted_curve_expression_programs() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x89\x4c\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xe0\x0aexpression\0\xf8\x04r=5\0w=1\0theta=w*t*360\0z=71*t\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.curves.expressions.len(), 1);
    assert_eq!(scan.curves.expressions[0].entity_id, 0x094c);
    assert_eq!(scan.curves.expressions[0].lines.len(), 4);
    let local_system = scan.curves.expressions[0]
        .local_system
        .as_ref()
        .expect("curve local system");
    assert_eq!((local_system.dimensions, local_system.count), (4, 3));
    assert_eq!(
        local_system.body,
        [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6]
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["curve_expressions"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fields["entity_id"], 0x094c);
    assert_eq!(records[0].fields["lines"][2]["text"], "theta=w*t*360");
    assert_eq!(records[0].fields["assignments"][2]["name"], "theta");
    assert_eq!(records[0].fields["assignments"][2]["dependencies"][0], "w");
    assert_eq!(records[0].fields["assignments"][0]["value"], 5.0);
    assert_eq!(records[0].fields["local_system"]["dimensions"], 4);
    assert_eq!(result.ir.model.features.len(), 1);
    assert!(matches!(
        &result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::HelixNativeAxis {
            axial_rise: cadmpeg_ir::features::Length(71.0),
            pitch: cadmpeg_ir::features::Length(71.0),
            revolutions: 1.0,
            start_angle: cadmpeg_ir::features::Angle(0.0),
            clockwise: false,
            ..
        }
    ));
    assert_eq!(result.ir.model.parameters.len(), 4);
    assert_eq!(result.ir.model.parameters[0].name, "r");
    assert_eq!(
        result.ir.model.parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(result.ir.model.parameters[2].name, "theta");
    assert_eq!(
        result.ir.model.parameters[2].dependencies,
        [result.ir.model.parameters[1].id.clone()]
    );
    assert_eq!(
        result.ir.model.parameters[2].properties["independent_variables"],
        "t"
    );
    assert!(!result.ir.model.parameters[2]
        .properties
        .contains_key("external_dependencies"));
    assert_eq!(
        result.ir.model.features[0].source_content,
        result
            .ir
            .model
            .parameters
            .iter()
            .map(
                |parameter| cadmpeg_ir::features::FeatureSourceContent::Parameter(
                    parameter.id.clone()
                )
            )
            .collect::<Vec<_>>()
    );
    assert_annotation(
        &result.source_fidelity.annotations,
        &records[0].id,
        "creo:DEPDB_DATA",
        scan.curves.expressions[0].expression_offset as u64,
        "curve_expression_program",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_binds_unique_forward_curve_expression_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=A\0a=5\0theta=T*360\0z=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let [r, a, theta, _] = result.ir.model.parameters.as_slice() else {
        panic!("four curve-expression parameters");
    };

    assert_eq!(r.name, "r");
    assert_eq!(r.ordinal, 1);
    assert_eq!(r.value, None);
    assert_eq!(r.dependencies, std::slice::from_ref(&a.id));
    assert_eq!(a.ordinal, 0);
    assert!(!r.properties.contains_key("external_dependencies"));
    assert_eq!(theta.properties["independent_variables"], "T");
    assert_eq!(
        result.ir.model.features[0].source_content,
        result
            .ir
            .model
            .parameters
            .iter()
            .map(
                |parameter| cadmpeg_ir::features::FeatureSourceContent::Parameter(
                    parameter.id.clone()
                )
            )
            .collect::<Vec<_>>()
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_complete_scoped_curve_expression_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01value=d1:2+PARAM:FID_20+PI\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let [parameter] = result.ir.model.parameters.as_slice() else {
        panic!("one curve-expression parameter");
    };

    assert_eq!(
        parameter.properties["external_dependencies"],
        "d1:2,PARAM:FID_20"
    );
    assert!(!parameter.properties.contains_key("ambiguous_dependencies"));
    let coverage = &result.report.coverage;
    assert_eq!(
        coverage["decoded_active_curve_expression_assignment_count"],
        1
    );
    assert_eq!(coverage["transferred_curve_expression_parameter_count"], 1);
    assert_eq!(
        coverage["evaluated_active_curve_expression_assignment_count"],
        0
    );
}

#[test]
fn decode_binds_curve_expression_dependencies_to_unique_dimensions() {
    let featdefs = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x01\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0"
        .to_vec();
    let expressions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01result=d42+1[deg]\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", featdefs),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
            ("DEPDB_DATA", expressions),
        ],
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let dimension = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d42")
        .expect("dimension parameter");
    let relation = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "result")
        .expect("relation parameter");

    assert_eq!(relation.dependencies, [dimension.id.clone()]);
    assert!(!relation.properties.contains_key("external_dependencies"));
    assert_eq!(
        relation.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(1.0 + 1.0f64.to_radians())
        ))
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_prohibited_curve_expression_strings_without_values() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06material='steel'\0label=material+'-'+itos(2)\0\
        length=string_length(label)\0match=label=='steel-2'\0formatted=rtos(123.456,2)\0\
        kind=rel_model_type()\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let parameters = &result.ir.model.parameters;

    assert!(parameters.iter().all(|parameter| parameter.value.is_none()));
    let native = &result
        .ir
        .native
        .namespace("creo")
        .expect("Creo native data")
        .arenas["curve_expressions"][0];
    assert_eq!(native.fields["prohibited_constructs"][0], "itos");
    let coverage = &result.report.coverage;
    assert_eq!(
        coverage["prohibited_active_curve_expression_record_count"],
        1
    );
    assert_eq!(coverage["prohibited_active_curve_expression_kind_count"], 1);
    assert!(result.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Attribute
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 active curve-equation record(s) containing prohibited datum-curve constructs \
                 were not evaluated",
            )
    }));
    assert!(result.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Attribute
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 prohibited datum-curve construct(s) across active curve-equation records were \
                 not evaluated",
            )
    }));
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "1 active curve-equation record(s) containing prohibited datum-curve constructs"
        )));
    assert_eq!(
        native.fields["assignments"][4]["expression"],
        "rtos(123.456,2)"
    );
    assert!(native.fields["assignments"][5]["value"].is_null());
    assert_eq!(parameters[4].expression, "rtos(123.456,2)");
    assert_eq!(parameters[5].expression, "rel_model_type()");
    assert_eq!(parameters[5].value, None);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_evaluates_relation_model_name_from_unique_counted_header() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01name=rel_model_name()\0"
        .to_vec();
    let mut data = build_prt("c", &[("DEPDB_DATA", payload)]);
    let header_end = data
        .windows(b"#-END_OF_UGC_HEADER\n".len())
        .position(|window| window == b"#-END_OF_UGC_HEADER\n")
        .expect("header end");
    data.splice(
        header_end..header_end,
        b"#- CMNM 00bwidget.prt \n".iter().copied(),
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let [parameter] = result.ir.model.parameters.as_slice() else {
        panic!("one curve-expression parameter")
    };
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "widget".to_owned()
        ))
    );
}

#[test]
fn decode_transfers_new_relation_parameter_unit_declarations() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x05span[inch]=2\0copy=span+25.4[mm]\0\
        stress[N/mm^2]=2\0angle=atan2(span,25.4[mm])\0freezing[C]=0\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let parameters = &result.ir.model.parameters;
    assert_eq!(parameters.len(), 5);
    assert_eq!(parameters[0].name, "span");
    assert_eq!(parameters[0].properties["declared_unit"], "inch");
    assert_eq!(
        parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(50.8)
        ))
    );
    let Some(cadmpeg_ir::features::ParameterValue::Length(copy)) = &parameters[1].value else {
        panic!("dimensioned copy");
    };
    assert!((copy.0 - 76.2).abs() < 1e-12);
    let native = &result.ir.native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(native.fields["assignments"][0]["name"], "span");
    assert_eq!(native.fields["assignments"][0]["declared_unit"], "inch");
    assert_eq!(parameters[2].properties["declared_unit"], "N/mm^2");
    assert_eq!(
        parameters[2].properties["evaluated_canonical_value"],
        "2000"
    );
    assert_eq!(
        parameters[2].properties["evaluated_dimension"],
        "length:-1,mass:1,time:-2,angle:0,temperature:0"
    );
    assert_eq!(parameters[2].value, None);
    assert_eq!(native.fields["assignments"][2]["value"]["value"], 2_000.0);
    assert_eq!(native.fields["assignments"][2]["value"]["length_power"], -1);
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = &parameters[3].value else {
        panic!("angle parameter");
    };
    assert!((angle.0 - 2.0f64.atan()).abs() < 1e-12);
    assert_eq!(parameters[4].properties["declared_unit"], "C");
    assert_eq!(
        parameters[4].properties["evaluated_dimension"],
        "length:0,mass:0,time:0,angle:0,temperature:1"
    );
    assert_eq!(
        parameters[4].properties["evaluated_canonical_value"],
        "273.15"
    );
    assert_eq!(parameters[4].value, None);
}

#[test]
fn decode_transfers_curve_expression_conditional_activation() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x07a=YES\0IF a\0value=5\0ELSE\0value=9\0ENDIF\0z=value\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let parameters = &result.ir.model.parameters;
    assert_eq!(parameters.len(), 4);
    assert_eq!(parameters[0].properties["activation"], "active");
    assert_eq!(parameters[1].properties["activation"], "active");
    assert_eq!(parameters[2].properties["activation"], "inactive");
    assert_eq!(parameters[3].properties["activation"], "active");
    assert_eq!(parameters[3].value, None);
    assert_eq!(parameters[3].dependencies, [parameters[1].id.clone()]);
    assert!(!parameters[3]
        .properties
        .contains_key("ambiguous_dependencies"));
    let native_assignments = result
        .ir
        .native
        .namespace("creo")
        .expect("Creo native data")
        .arenas["curve_expressions"][0]
        .fields["assignments"]
        .as_array()
        .expect("assignments");
    assert_eq!(native_assignments[2]["activation"], "inactive");
    let prohibited = result
        .ir
        .native
        .namespace("creo")
        .expect("Creo native data")
        .arenas["curve_expressions"][0]
        .fields["prohibited_constructs"]
        .as_array()
        .expect("prohibited constructs");
    assert_eq!(prohibited.len(), 3);
    assert_eq!(prohibited[0], "else");
    assert_eq!(prohibited[1], "endif");
    assert_eq!(prohibited[2], "if");
    let coverage = &result.report.coverage;
    assert_eq!(coverage["active_curve_expression_assignment_count"], 3);
    assert_eq!(coverage["inactive_curve_expression_assignment_count"], 1);
    assert_eq!(coverage["conditional_curve_expression_assignment_count"], 0);
}

#[test]
fn decode_resolves_positive_local_exists_before_declaration() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06IF exists('later')\0value=5\0ELSE\0\
        value=9\0ENDIF\0later=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let parameters = &result.ir.model.parameters;
    assert_eq!(parameters.len(), 3);
    assert_eq!(parameters[0].properties["activation"], "active");
    assert_eq!(parameters[0].value, None);
    assert_eq!(parameters[1].properties["activation"], "inactive");
    assert_eq!(parameters[1].value, None);
    assert_eq!(parameters[2].value, None);
}

#[test]
fn decode_retains_cyclic_curve_expression_dependencies_without_invalid_edges() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=a\0a=r\0theta=t*360\0z=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let [r, a, _, _] = result.ir.model.parameters.as_slice() else {
        panic!("four curve-expression parameters");
    };

    assert!(r.dependencies.is_empty());
    assert_eq!(r.properties["cyclic_dependencies"], "a");
    assert!(a.dependencies.is_empty());
    assert_eq!(a.properties["cyclic_dependencies"], "r");
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_transfers_reassigned_curve_expression_names_without_identity_collisions() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=1\0R=2\0theta=t*360\0z=r\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");

    assert_eq!(
        result
            .ir
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.ordinal))
            .collect::<Vec<_>>(),
        [("r#1", 0), ("R#2", 1), ("theta", 2), ("z", 3)]
    );
    assert_eq!(result.ir.model.parameters[0].properties["source_name"], "r");
    assert_eq!(
        result.ir.model.parameters[0].properties["source_assignment_ordinal"],
        "0"
    );
    assert_eq!(result.ir.model.parameters[1].properties["source_name"], "R");
    assert_eq!(
        result.ir.model.parameters[3].properties["ambiguous_dependencies"],
        "r"
    );
    assert!(result.ir.model.parameters[3].dependencies.is_empty());
    assert!(!result.ir.model.parameters[3]
        .properties
        .contains_key("external_dependencies"));
    assert_eq!(
        result.ir.model.features[0].source_text.as_deref(),
        Some("r=1\nR=2\ntheta=t*360\nz=r")
    );
    assert_eq!(
        result
            .ir
            .native
            .namespace("creo")
            .expect("Creo native data")
            .arenas["curve_expressions"][0]
            .fields["assignments"]
            .as_array()
            .expect("assignments")
            .len(),
        4
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_places_helix_from_complete_curve_expression_frame() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=0-t*360\0z=-2+10*t\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("placed helix");
    };
    assert_eq!(*angle_range, [0.0, std::f64::consts::TAU]);
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, -2.0));
    assert_eq!(*major, cadmpeg_ir::math::Vector3::new(5.0, 0.0, 0.0));
    assert_eq!(*minor, cadmpeg_ir::math::Vector3::new(0.0, -5.0, 0.0));
    assert_eq!(*pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 10.0));
    assert_eq!(*apex_factor, 0.0);
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn decode_places_helix_from_rank_two_curve_expression_frame() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=t*360\0z=10*t\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        center,
        major,
        minor,
        pitch,
        axis,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("placed helix");
    };
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*major, cadmpeg_ir::math::Vector3::new(0.0, 5.0, 0.0));
    assert_eq!(*minor, cadmpeg_ir::math::Vector3::new(5.0, 0.0, 0.0));
    assert_eq!(*pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, -10.0));
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0));
}

#[test]
fn scan_discovers_curve_halfedge_topology() {
    let mut payload = visibgeom_payload(0, 1);
    payload
        .extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.topology_rows.len(), 1);
    assert_eq!(scan.curves.topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curves.topology_rows[0].next_edges, [7, 7]);
    assert_eq!(scan.topology.half_edges.len(), 2);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let row = &result.ir.native.namespace("creo").unwrap().arenas["curve_topology_rows"][0];
    assert_eq!(row.fields["curve_id"], 7);
    assert_eq!(row.fields["type_byte"], 8);
    assert_eq!(row.fields["feature_id"], 4);
    assert_eq!(row.fields["directions"][0], 1);
    assert_eq!(row.fields["directions"][1], 0xf6);
    assert_eq!(row.fields["faces"][0], 10);
    assert_eq!(row.fields["faces"][1], 11);
    assert_eq!(row.fields["next_edges"][0], 7);
    assert_eq!(row.fields["next_edges"][1], 7);
    assert_eq!(
        result.source_fidelity.annotations.provenance["creo:visibgeom:curve_topology#7"]
            .tag
            .as_deref(),
        Some("curve_topology_row")
    );
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

    assert_eq!(scan.curves.topology_rows.len(), 2);
    assert_eq!(scan.curves.topology_rows[0].id, 7);
    assert_eq!(scan.curves.topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curves.topology_rows[1].id, 8);
    assert_eq!(scan.curves.topology_rows[1].faces, [12, 13]);
}

#[test]
fn scan_bounds_curve_parameter_body_before_topology_suffix() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4, 0xf7, 0x81, 0x00]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.parameters.len(), 1);
    let parameters = &scan.curves.parameters[0];
    assert_eq!(parameters.curve_id, 7);
    assert_eq!(parameters.type_byte, 8);
    assert_eq!(parameters.scalar_values, vec![0.0, 1.0, 3.0]);
    assert_eq!(parameters.scalar_tokens[2].offset, 5);
    assert_eq!(parameters.scalar_tokens[2].length, 8);
    assert_eq!(parameters.scalar_tokens[2].raw[0], 0x46);
    assert_eq!(parameters.skipped_references, vec![256]);
    assert_eq!(parameters.references[0].entity_id, 256);
    assert_eq!(parameters.references[0].offset, 2);
    assert_eq!(parameters.references[0].length, 3);
    assert_eq!(parameters.opaque_spans.len(), 1);
    assert_eq!(parameters.opaque_spans[0].offset, 13);
    assert_eq!(parameters.opaque_spans[0].raw, [0xff]);
    assert_eq!(parameters.suffix, crate::curve::CurveSuffixStatus::Unique);
    assert_eq!(parameters.body.last(), Some(&0xff));
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let record = &result.ir.native.namespace("creo").unwrap().arenas["curve_parameters"][0];
    assert_eq!(record.fields["curve_id"], 7);
    assert_eq!(record.fields["type_byte"], 8);
    assert_eq!(
        record.fields["body"].as_array().unwrap().len(),
        parameters.body.len()
    );
    assert_eq!(record.fields["scalar_values"][2], 3.0);
    assert_eq!(record.fields["scalar_tokens"][2]["offset"], 5);
    assert_eq!(record.fields["scalar_tokens"][2]["raw"][0], 0x46);
    assert_eq!(record.fields["skipped_references"][0], 256);
    assert_eq!(record.fields["references"][0]["entity_id"], 256);
    assert_eq!(record.fields["references"][0]["offset"], 2);
    assert_eq!(record.fields["opaque_spans"][0]["offset"], 13);
    assert_eq!(record.fields["opaque_spans"][0]["raw"][0], 0xff);
    assert_eq!(record.fields["suffix"], "unique");
    assert!(record.fields["suffix_candidate_count"].is_null());
    assert_eq!(
        result.source_fidelity.annotations.provenance["creo:visibgeom:curve_parameter#7"]
            .tag
            .as_deref(),
        Some("curve_parameter_record")
    );
}

#[test]
fn scan_resolves_section_scalar_cache_in_curve_rows() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x18, 0x00, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.parameters[0].scalar_values, vec![3.0]);
}

#[test]
fn scan_decodes_pcurve_endpoints_in_both_face_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.push(0xe4);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.pcurves.len(), 1);
    let pcurve = &scan.curves.pcurves[0];
    assert_eq!(pcurve.curve_id, 7);
    assert_eq!(pcurve.faces, [10, 11]);
    assert_eq!(pcurve.face_0_endpoints, [[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(pcurve.face_1_endpoints, [[3.0, 0.0], [3.0, 1.0]]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["pcurve_endpoints"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "creo:visibgeom:pcurve_endpoints#7");
    assert_eq!(records[0].fields["faces"][0], 10);
    assert_eq!(records[0].fields["faces"][1], 11);
    assert_eq!(records[0].fields["source_form"], "positional");

    let mut mismatched_topology = scan.curves.topology_rows.clone();
    mismatched_topology[0].type_byte = 1;
    assert!(
        crate::curve::pcurve_endpoints(&scan.curves.parameters, &mismatched_topology).is_empty()
    );
}

#[test]
fn scan_decodes_standalone_zero_slots_in_pcurve_endpoint_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.parameters[0].scalar_tokens.len(), 5);
    assert_eq!(scan.curves.parameters[0].opaque_spans.len(), 3);
    assert!(scan.curves.parameters[0]
        .opaque_spans
        .iter()
        .all(|span| span.raw == [0x12]));
    assert_eq!(scan.curves.pcurves.len(), 1);
    assert_eq!(
        scan.curves.pcurves[0].face_0_endpoints,
        [[0.0, 1.0], [1.0, 0.0]]
    );
    assert_eq!(
        scan.curves.pcurves[0].face_1_endpoints,
        [[3.0, 0.0], [3.0, 1.0]]
    );
}

#[test]
fn scan_withholds_nine_slot_pcurve_endpoint_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4, 0x12]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.pcurves.is_empty());
}

#[test]
fn scan_withholds_pcurve_endpoints_with_unclaimed_body_bytes() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend([0x0f; 8]);
    payload.push(0xff);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.parameters[0].scalar_tokens.len(), 8);
    assert_eq!(scan.curves.parameters[0].opaque_spans[0].raw, [0xff]);
    assert!(scan.curves.pcurves.is_empty());
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
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.fc_coordinates.len(), 1);
    let coordinates = &scan.curves.fc_coordinates[0];
    assert_eq!(coordinates.curve_id, 7);
    assert_eq!(coordinates.subtype, 8);
    assert_eq!(coordinates.body, scan.curves.parameters[0].body);
    assert_eq!(coordinates.values_mm, vec![3.0, -3.0, 2.0, -2.0]);
    assert_eq!(coordinates.tokens[0].offset, 2);
    assert_eq!(coordinates.tokens[0].length, 8);
    assert_eq!(coordinates.tokens[0].raw, [0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    assert_eq!(coordinates.tokens[1].offset, 10);
    assert_eq!(coordinates.opaque_spans[0].offset, 0);
    assert_eq!(coordinates.opaque_spans[0].raw, [0xfc, 0x08]);
    assert_eq!(coordinates.opaque_spans[1].raw, [0xff]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["fc_curve_coordinates"];
    assert_eq!(records[0].fields["curve_id"], 7);
    assert_eq!(records[0].fields["values_mm"][1], -3.0);
    assert_eq!(records[0].fields["tokens"][1]["offset"], 10);
    assert_eq!(records[0].fields["tokens"][1]["length"], 8);
    assert_eq!(records[0].fields["opaque_spans"][1]["raw"][0], 0xff);
}

#[test]
fn decode_withholds_unplaced_cylinder_prototype_frame() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0");
    push_generated_scalar(&mut payload, 1.0);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#7"));
}

#[test]
fn decode_places_first_cylinder_instance_from_complete_named_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "cylinder", &[("radius", 1.0)]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");
    let cylinder = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first cylinder instance");

    assert_eq!(
        cylinder.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            radius: 1.0,
        }
    );
    assert!(result.report.losses.iter().any(|loss| {
        loss.message.contains(
            "first-instance ND plane, cylinder, cone, torus, or interpolation-spline carrier",
        )
    }));
}

#[test]
fn decode_withholds_complete_cylinder_prototype_without_positive_radius() {
    for fields in [Vec::new(), vec![("radius", -1.0)]] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
        push_named_analytic_prototype(&mut payload, "cylinder", &fields);
        payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

        let result = decode::decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#7"));
    }
}

#[test]
fn decode_places_direct_two_direction_named_prototype_frame() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(torus)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, -2.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(b"\xe0\x01radius1\0\xe4\xe0\x01radius2\0\xe4");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");
    let torus = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first torus instance");

    assert_eq!(
        torus.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center: cadmpeg_ir::math::Point3::new(2.0, 0.0, -2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            major_radius: 1.0,
            minor_radius: 1.0,
        }
    );
}

#[test]
fn decode_does_not_promote_untyped_terminal_torus_scalars() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    push_generated_scalar(&mut payload, 2.0);
    push_generated_scalar(&mut payload, 1.0);
    payload.push(0xe3);
    payload.extend_from_slice(b"srf_prim_ptr(torus)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, -2.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(b"\xe0\x01radius1\0\xe4\xe0\x01radius2\0\xe4");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");
    let torus = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first torus instance");

    assert_eq!(
        torus.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center: cadmpeg_ir::math::Point3::new(2.0, 0.0, -2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            major_radius: 1.0,
            minor_radius: 1.0,
        }
    );
}

#[test]
fn decode_places_first_plane_instance_from_named_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");
    let plane = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first plane instance");

    assert_eq!(
        plane.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
    assert_eq!(
        result.report.coverage["transferred_first_instance_prototype_surface_count"],
        1
    );
}

#[test]
fn decode_places_named_prototype_before_its_surface_row() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.push(0xe3);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    let plane = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("following first plane instance");
    assert!(matches!(
        plane.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
}

#[test]
fn decode_binds_prototype_between_same_family_rows_to_the_preceding_instance() {
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "creo:visibgeom:surface#7"));
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#8"));
    assert_eq!(
        result.report.coverage["transferred_first_instance_prototype_surface_count"],
        1
    );
}

#[test]
fn decode_withholds_competing_named_prototypes_for_one_surface_row() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");

    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#7"));
}

#[test]
fn decode_places_first_interpolation_spline_instance_from_named_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x28, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(splsrf)\0\xe0\x02i_points\0\xf9\x04\x03");
    for point in [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 2.0],
    ] {
        for value in point {
            push_generated_scalar(&mut payload, value);
        }
    }
    payload.extend_from_slice(b"\xe0\x02end_u_tangts\0\xf9\x04\x03");
    for _ in 0..4 {
        for value in [1.0, 0.0, 1.0] {
            push_generated_scalar(&mut payload, value);
        }
    }
    payload.extend_from_slice(b"\xe0\x02end_v_tangts\0\xf9\x04\x03");
    for _ in 0..4 {
        for value in [0.0, 1.0, 1.0] {
            push_generated_scalar(&mut payload, value);
        }
    }
    payload.extend_from_slice(b"\xe0\x02end_uv_deriv\0\xf9\x04\x03");
    for _ in 0..12 {
        push_generated_scalar(&mut payload, 0.0);
    }
    for name in ["u_params", "v_params"] {
        payload.extend_from_slice(&[0xe0, 0x02]);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&[0, 0xf8, 0x02]);
        push_generated_scalar(&mut payload, 0.0);
        push_generated_scalar(&mut payload, 1.0);
    }
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("c", &[("ND:0:VisibGeom:0", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.surfaces.rows.len(), 1);
    assert_eq!(scan.surfaces.prototype_records.len(), 1);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first interpolation spline instance");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &surface.geometry else {
        panic!("expected NURBS surface");
    };

    assert_eq!((nurbs.u_degree, nurbs.v_degree), (3, 3));
    assert_eq!((nurbs.u_count, nurbs.v_count), (4, 4));
    assert_eq!(
        nurbs.control_points[0],
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        nurbs.control_points[15],
        cadmpeg_ir::math::Point3::new(1.0, 1.0, 2.0)
    );
}

#[test]
fn decode_places_first_sphere_and_torus_instances_from_named_prototypes() {
    let cases = [
        (
            0x26,
            "torus",
            vec![("radius1", 0.0), ("radius2", 1.0)],
            cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                radius: 1.0,
            },
        ),
        (
            0x26,
            "torus",
            vec![("radius1", 2.0), ("radius2", 1.0)],
            cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                major_radius: 2.0,
                minor_radius: 1.0,
            },
        ),
    ];

    for (kind, family, fields, expected) in cases {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, kind, 4, 0x01, 0, 0]);
        push_named_analytic_prototype(&mut payload, family, &fields);
        payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
        let result = decode::decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
        let surface = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
            .unwrap_or_else(|| panic!("first {family} instance"));
        assert_eq!(surface.geometry, expected);
    }
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
        [3.0, 4.0, 2.0 + std::f64::consts::FRAC_PI_2, 2.0],
        [2.0, 3.0, 2.0 + std::f64::consts::PI, 2.0],
        [3.0, 2.0, 2.0 + 3.0 * std::f64::consts::FRAC_PI_2, 2.0],
    ] {
        world(&mut payload, x);
        world(&mut payload, z);
        world(&mut payload, t);
        world(&mut payload, y);
    }
    payload.push(0xff);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.fc05_circles.len(), 1);
    let circle = &scan.curves.fc05_circles[0];
    assert_eq!(circle.curve_id, 7);
    assert_eq!(circle.center_row_frame, [3.0, 3.0]);
    assert_eq!(circle.radius_mm, 1.0);
    assert_eq!(circle.cap_ordinate_row_frame, Some(2.0));
    assert_eq!(circle.point_count, 4);
    assert_eq!(circle.max_residual, 0.0);
    assert!(circle.angle_parameter_consistent);
    assert_eq!(circle.parameter_sign, Some(1));
    let direction = circle
        .reference_direction_row_frame
        .expect("unique parameter-zero direction");
    assert!((direction[0] - (-2.0_f64).cos()).abs() < 1e-12);
    assert!((direction[1] - (-2.0_f64).sin()).abs() < 1e-12);
    let mut unknown_parameter = scan.curves.parameters[0].clone();
    unknown_parameter.body.splice(114..122, [0x39, 0x29, 0x00]);
    let carriers = crate::curve::fc05_circles(&[unknown_parameter]);
    let [carrier] = carriers.as_slice() else {
        panic!("circle geometry is independent of an unresolved parameter token");
    };
    assert_eq!(carrier.center_row_frame, [3.0, 3.0]);
    assert_eq!(carrier.radius_mm, 1.0);
    assert!(!carrier.angle_parameter_consistent);
    assert_eq!(carrier.parameter_sign, None);
    assert_eq!(carrier.reference_direction_row_frame, None);
    assert_eq!(carrier.sample_direction_row_frame, [1.0, 0.0]);
    let mut trailing = scan.curves.parameters[0].clone();
    trailing.body.push(0xfe);
    assert!(crate::curve::fc05_circles(&[trailing]).is_empty());
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["fc05_circles"];
    assert_eq!(records[0].fields["curve_id"], 7);
    assert_eq!(records[0].fields["radius_mm"], 1.0);
    assert_eq!(records[0].fields["sample_direction_row_frame"][0], 1.0);
    assert_eq!(records[0].fields["parameter_sign"], 1);
}

#[test]
fn decode_places_x_axis_cylinder_from_outline_bound_cap_pair() {
    fn world(payload: &mut Vec<u8>, value: f64) {
        let raw = value.to_be_bytes();
        payload.push(match raw[0] {
            0x40 => 0x46,
            0xc0 => 0x2d,
            _ => panic!("generated FC05 value must use a world-token exponent"),
        });
        payload.extend_from_slice(&raw[1..]);
    }
    fn plane_row(payload: &mut Vec<u8>, id: u8, next: u8, x: f64) {
        payload.extend_from_slice(&[id, 0x22, 4, 0x01, 0, next]);
        for value in [0.0, 1.0, 0.0, 1.0, x, -1.0, -1.0, x, 1.0, 2.0] {
            push_generated_scalar(payload, value);
        }
        payload.push(0xe3);
    }
    fn circle_row(
        payload: &mut Vec<u8>,
        curve: u8,
        plane: u8,
        ordinate: f64,
        preserve_parameters: bool,
    ) {
        payload.extend_from_slice(&[curve, 0x09, 4, 0x01, 0xf6, 0xfc, 0x05]);
        for [a, b, parameter] in [
            [4.0, 5.0, 2.0],
            [3.0, 6.0, 2.0 + std::f64::consts::FRAC_PI_2],
            [2.0, 5.0, 2.0 + std::f64::consts::PI],
            [3.0, 4.0, 2.0 + 3.0 * std::f64::consts::FRAC_PI_2],
        ] {
            world(payload, a);
            world(payload, b);
            world(payload, if preserve_parameters { parameter } else { 2.0 });
            world(payload, ordinate);
        }
        payload.push(0xff);
        payload.extend_from_slice(&[10, plane, curve, curve, 0, 0, 0xe3]);
        payload.extend_from_slice(&[0xe1, 0xf5, 0x05, 0xf6, 0xe3]);
    }

    let mut payload = b"srf_array\0\xf8\x03".to_vec();
    payload.extend_from_slice(&[10, 0x24, 4, 0x01, 0, 11]);
    plane_row(&mut payload, 11, 12, 2.0);
    plane_row(&mut payload, 12, 0, -2.0);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x02topol_ref_data\0");
    let mut one_cap_payload = payload.clone();
    circle_row(&mut one_cap_payload, 20, 11, -5.0, true);
    let one_cap = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", one_cap_payload)])),
        &DecodeOptions::default(),
    )
    .expect("one-cap decode");
    let one_cap_cylinder = one_cap
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#10")
        .expect("placed one-cap cylinder");
    assert!(matches!(
        one_cap_cylinder.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3 {
                x: 2.0,
                y: 5.0,
                z: 3.0
            },
            radius: 1.0,
            ..
        }
    ));
    let one_cap_circle = one_cap
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:visibgeom:curve#20")
        .expect("placed one-cap circle");
    assert!(matches!(
        one_cap_circle.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center: cadmpeg_ir::math::Point3 {
                x: 2.0,
                y: 5.0,
                z: 3.0
            },
            axis: cadmpeg_ir::math::Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            radius: 1.0,
            ..
        }
    ));
    let mut neutral_chart_payload = payload.clone();
    circle_row(&mut neutral_chart_payload, 22, 11, -5.0, false);
    let neutral_chart = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", neutral_chart_payload)])),
        &DecodeOptions::default(),
    )
    .expect("neutral-chart decode");
    let neutral_circle = neutral_chart
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:visibgeom:curve#22")
        .expect("circle with neutral sample chart");
    assert!(matches!(
        neutral_circle.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            ref_direction: cadmpeg_ir::math::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            ..
        }
    ));

    circle_row(&mut payload, 20, 11, 2.0, true);
    circle_row(&mut payload, 21, 12, -2.0, true);
    let result = decode::decode(
        &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
        &DecodeOptions::default(),
    )
    .expect("decode");
    let cap_pairs = &result.ir.native.namespace("creo").unwrap().arenas["fc05_cylinder_cap_pairs"];
    assert_eq!(cap_pairs.len(), 1);
    assert_eq!(cap_pairs[0].fields["surface_id"], 10);
    assert_eq!(cap_pairs[0].fields["curve_ids"][0], 20);
    assert_eq!(cap_pairs[0].fields["curve_ids"][1], 21);
    assert_eq!(cap_pairs[0].fields["radius_mm"], 1.0);
    let cylinder = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#10")
        .expect("placed cylinder");
    assert_eq!(
        cylinder.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3::new(2.0, 5.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(0.0, (-2.0_f64).sin(), (-2.0_f64).cos(),),
            radius: 1.0,
        }
    );
    assert_eq!(result.ir.model.curves.len(), 2);
    assert!(result.ir.model.curves.iter().all(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            axis: cadmpeg_ir::math::Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            radius: 1.0,
            ..
        }
    )));
}

#[test]
fn scan_decodes_labeled_prototype_pcurve_uvs() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.prototype_pcurves.len(), 1);
    let prototype = &scan.curves.prototype_pcurves[0];
    assert_eq!(prototype.curve_id, 44);
    assert_eq!(prototype.face_0_endpoints, [[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(prototype.face_1_endpoints, [[3.0, 0.0], [3.0, 1.0]]);
}

#[test]
fn scan_withholds_non_exact_labeled_prototype_pcurve_arrays() {
    for tail in [
        vec![0xff],
        vec![0xe4, 0x12],
        vec![0x18, 0xe7, 0x04, 0x2f, 0x08, 0x00, 0xe4, 0x18],
    ] {
        let mut payload = visibgeom_payload(0, 0);
        payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
        payload.extend_from_slice(&[0x12, 0xe4]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&tail);
        payload.extend_from_slice(b"topol_ref_data\0");
        let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

        assert!(scan.curves.prototype_pcurves.is_empty());
    }
}

#[test]
fn scan_withholds_displaced_labeled_prototype_pcurve_wrapper() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0junk\xf9\x02\x04");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.prototype_pcurves.is_empty());
}

#[test]
fn scan_withholds_duplicate_labeled_prototype_pcurve_arrays() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
    payload.extend([0x0f; 8]);
    payload.extend_from_slice(b"crv_pnt_arr\0\xf9\x02\x04");
    payload.extend([0x0f; 8]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.prototype_pcurves.is_empty());
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
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.prototype_topology.len(), 1);
    assert_eq!(scan.curves.prototype_topology[0].curve_id, 44);
    assert_eq!(scan.curves.prototype_topology[0].faces, [10, 11]);
    assert_eq!(scan.curves.prototype_topology[0].next_edges, [44, 44]);
    assert_eq!(scan.curves.bound_prototype_pcurves.len(), 1);
    assert_eq!(scan.curves.bound_prototype_pcurves[0].faces, [10, 11]);
    assert_eq!(
        scan.curves.bound_prototype_pcurves[0].face_0_endpoints,
        [[0.0, 1.0], [1.0, 0.0]]
    );
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let namespace = result.ir.native.namespace("creo").unwrap();
    assert_eq!(
        namespace.arenas["prototype_pcurves"][0].fields["curve_id"],
        44
    );
    assert_eq!(
        namespace.arenas["curve_prototype_topology"][0].fields["faces"][1],
        11
    );
}

#[test]
fn scan_withholds_duplicate_labeled_prototype_topology_fields() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00");
    payload.extend_from_slice(
        b"crv_hdr_geom_ptr[0]\0\x0a crv_hdr_geom_ptr[0]\0\x0a \
          crv_hdr_geom_ptr[1]\0\x0b next_crv_hdr_ptr[0]\0\x2c next_crv_hdr_ptr[1]\0\x2c",
    );
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.prototype_topology.is_empty());
}

#[test]
fn prototype_pcurve_binding_requires_unique_native_identity() {
    let pcurve = crate::curve::PrototypePcurveEndpoints {
        curve_id: 44,
        face_0_endpoints: [[0.0, 1.0], [1.0, 0.0]],
        face_1_endpoints: [[3.0, 0.0], [3.0, 1.0]],
        offset: 10,
    };
    let topology = crate::curve::CurvePrototypeTopology {
        curve_id: 44,
        faces: [10, 11],
        next_edges: [44, 44],
        offset: 20,
    };

    assert!(crate::curve::bind_prototype_pcurves(
        &[pcurve.clone(), pcurve.clone()],
        std::slice::from_ref(&topology),
    )
    .is_empty());
    assert!(crate::curve::bind_prototype_pcurves(
        std::slice::from_ref(&pcurve),
        &[topology.clone(), topology],
    )
    .is_empty());
}

#[test]
fn scan_groups_connected_nonzero_face_references() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3\x08\x08\x04\x01\xf6\x0b\x0c\x08\x08\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.face_components.len(), 1);
    assert_eq!(scan.topology.face_components[0].face_ids, vec![10, 11, 12]);
    assert_eq!(scan.topology.face_components[0].curve_ids, vec![7, 8]);
}

#[test]
fn scan_builds_topological_vertex_orbits_and_incidence() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x08\x08\0\0\xe3\xe1\xe3\
          \x08\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.vertices.len(), 2);
    assert_eq!(
        scan.topology.vertices[0].half_edges,
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
        .topology
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
        true,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        2,
        false,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        3,
        false,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        4,
        false,
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

    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1\
        \xe0\x21geoms_affected\0\xf8\x01\x63\
        \xe0\x21edgs_affected\0\xf8\x02\x0a\x0b"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", payload),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.planes.local_systems.len(), 4);
    assert_eq!(scan.curves.topology_rows.len(), 6);
    assert!(
        scan.features.affected_ids.iter().any(|record| {
            record.feature_id == 4
                && record.kind == crate::feature::AffectedIdKind::Edges
                && record.ids == [10, 11]
        }),
        "affected ids: {:#?}",
        scan.features.affected_ids
    );
    assert_eq!(scan.topology.loops.len(), 4);
    assert_eq!(scan.topology.vertices.len(), 4);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let model = &result.ir.model;
    let namespace = result.ir.native.namespace("creo").unwrap();
    assert_eq!(namespace.arenas["half_edges"].len(), 12);
    assert_eq!(namespace.arenas["loops"].len(), 4);
    assert_eq!(namespace.arenas["topological_vertices"].len(), 4);
    assert_eq!(namespace.arenas["half_edge_vertex_incidence"].len(), 12);
    assert_eq!(namespace.arenas["face_components"].len(), 1);
    assert_eq!(namespace.arenas["half_edges"][0].fields["curve_id"], 10);
    assert_eq!(namespace.arenas["half_edges"][0].fields["side"], 0);

    assert_eq!(model.points.len(), 4);
    assert_eq!(model.vertices.len(), 4);
    assert_eq!(model.edges.len(), 6);
    assert_eq!(model.curves.len(), 6);
    assert!(model.edges.iter().all(|edge| edge.curve.is_some()));
    assert!(model.edges.iter().all(|edge| edge.param_range.is_some()));
    for edge in &model.edges {
        let [start_parameter, end_parameter] = edge.param_range.expect("line edge range");
        assert_eq!(start_parameter, 0.0);
        assert!(end_parameter > 0.0);
        let curve = model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve.as_ref())
            .expect("edge curve");
        let cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } = curve.geometry else {
            panic!("edge line: {curve:#?}");
        };
        let start = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge start point")
            .position;
        let end = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge end point")
            .position;
        assert_eq!(origin, start);
        let evaluated = [
            origin.x + direction.x * end_parameter,
            origin.y + direction.y * end_parameter,
            origin.z + direction.z * end_parameter,
        ];
        assert!(evaluated
            .into_iter()
            .zip([end.x, end.y, end.z])
            .all(|(evaluated, expected)| (evaluated - expected).abs() < 1e-10));
    }
    assert_eq!(model.faces.len(), 4);
    assert_eq!(
        model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "creo:visibgeom:face#1")
            .expect("reversed face")
            .sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "creo:visibgeom:face#2")
            .expect("forward face")
            .sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert_eq!(model.loops.len(), 4);
    assert!(model
        .loops
        .iter()
        .all(|lp| lp.boundary_role == cadmpeg_ir::topology::LoopBoundaryRole::Outer));
    assert_eq!(model.coedges.len(), 12);
    assert_eq!(model.pcurves.len(), 12);
    assert!(model.coedges.iter().all(|coedge| coedge.pcurves.len() == 1));
    for coedge in &model.coedges {
        let pcurve = model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
            .expect("projected plane pcurve");
        assert!(matches!(
            pcurve.geometry,
            cadmpeg_ir::geometry::PcurveGeometry::Line { .. }
        ));
        let edge = model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("pcurve edge");
        assert_eq!(pcurve.parameter_range, edge.param_range);
    }
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, cadmpeg_ir::topology::BodyKind::Solid);
    let feature = model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert_eq!(feature.outputs, vec![model.bodies[0].id.clone()]);
    let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } = &feature.definition else {
        panic!("round definition: {:#?}", feature.definition);
    };
    let [cadmpeg_ir::features::FilletGroup { edges, .. }] = groups.as_slice() else {
        panic!("round groups: {groups:#?}");
    };
    let cadmpeg_ir::features::EdgeSelection::Resolved { edges, native } = edges else {
        panic!("round edges: {edges:#?}");
    };
    assert_eq!(
        edges,
        &[
            cadmpeg_ir::ids::EdgeId("creo:visibgeom:edge#10".to_string()),
            cadmpeg_ir::ids::EdgeId("creo:visibgeom:edge#11".to_string()),
        ]
    );
    assert_eq!(native, "creo:allfeatur:edgs_affected#4:10,11");
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
    assert_eq!(scan.planes.datums.len(), 1);
    assert_eq!(scan.planes.datums[0].normal, [0.0, 1.0, 0.0]);
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
    let records = &result.ir.native.namespace("creo").unwrap().arenas["datum_planes"];
    assert_eq!(records[0].fields["datum_id"], 4);
    assert_eq!(records[0].fields["owner_feature_id"], 1);
    assert_eq!(records[0].fields["normal"][1], 1.0);
    assert_eq!(records[0].fields["plane_offset"], 0.0);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_eq!(result.ir.model.features.len(), 1);
    let feature = &result.ir.model.features[0];
    assert_eq!(feature.id.as_str(), "creo:model:feature#1");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { .. }
    ));
}

#[test]
fn decode_merges_datum_geometry_and_operation_history_by_feature_id() {
    let mut datum = vec![4, 0x22, 4, 1, 0, 0];
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
            ("ActDatums", datum),
            ("MdlStatus", b"Round id 3\0Datum Plane id 4\0".to_vec()),
        ],
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(result.ir.model.features.len(), 2);
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("datum feature");
    assert_eq!(feature.id.as_str(), "creo:model:feature#4");
    assert_eq!(feature.ordinal, 1);
    assert_eq!(feature.name.as_deref(), Some("Datum Plane id 4"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { .. }
    ));
    assert_eq!(
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#3")
            .expect("preceding round")
            .ordinal,
        0
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_competing_standalone_datum_planes() {
    let mut row = vec![4, 0x22, 4, 1, 0, 0];
    row.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            row.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            row.extend(bytes);
        }
    }
    let mut datum = row.clone();
    datum.extend(row);
    let data = build_prt("c", &[("ActDatums", datum)]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    assert_eq!(result.ir.model.features.len(), 1);
    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlaneUnresolved
    ));
    assert_eq!(
        result.ir.native.namespace("creo").unwrap().arenas["datum_planes"].len(),
        2
    );
}

#[test]
fn decode_types_schema_less_datum_plane_names() {
    for name in ["Datum Plane", "Bezugsebene"] {
        let mut payload = b"srf_array\0geom_id\0\x05geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\x01next_geom_ptr\0\0\
            outline\0\xf9\x02\x03"
            .to_vec();
        payload.extend_from_slice(&[0xe4, 0x0f, 0x2f, 0, 0, 0x0d, 0x0f, 0x48, 0, 0]);
        payload.extend_from_slice(b"\xe0\x00srf_prim_ptr(plane)\0\xe3");
        let stored_name = format!("{name} id 4\0");
        let data = build_prt(
            "c",
            &[
                ("VisibGeom", payload),
                ("MdlStatus", stored_name.as_bytes().to_vec()),
            ],
        );

        let result =
            decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
        let feature = result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#4")
            .expect("named datum feature");
        assert_eq!(
            feature.name.as_deref(),
            Some(format!("{name} id 4").as_str())
        );
        assert!(matches!(
            feature.definition,
            cadmpeg_ir::features::FeatureDefinition::DatumPlane { .. }
        ));
    }
}

#[test]
fn decode_retains_named_datum_plane_with_unresolved_placement() {
    let data = build_prt("c", &[("MdlStatus", b"Datum Plane id 4\0".to_vec())]);
    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("named datum feature");

    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlaneUnresolved
    ));
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
    let datum_offset =
        container::scan_bytes(data.clone()).planes.datums[0].offset_in_payload as u64;
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
    let emitted_entity_count =
        unknowns.len() + result.ir.model.surfaces.len() + result.ir.model.features.len();
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

    assert_eq!(scan.framing.principal_unit.as_deref(), Some("mmNs"));
}

#[test]
fn decode_transfers_mdlstatus_feature_operations_in_history_order() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"noise\0xProtrusion id 40\0Round id 41\0Future Feature id 42\0Datum Plane id 43\0Draft id 44\0Hole id 40\0ySurface id 45\0"
                .to_vec(),
        )],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.operation_states.len(), 7);
    assert_eq!(scan.features.operation_states[0].feature_id, 40);
    assert_eq!(scan.features.operation_states[0].kind, "Protrusion");
    assert_eq!(
        scan.features.operation_states[0].stored_name.as_deref(),
        Some("xProtrusion id 40")
    );
    assert_eq!(
        scan.features.operation_states[0]
            .identifier_keyword
            .as_deref(),
        Some("id")
    );
    assert_eq!(
        scan.features.operation_states[0].state_offset + 1,
        scan.features.operation_states[0].offset
    );
    assert_eq!(scan.features.operation_states[5].feature_id, 40);
    assert_eq!(scan.features.operation_states[5].kind, "Hole");
    assert_eq!(scan.features.operations.len(), 6);
    assert_eq!(scan.features.operations[0].feature_id, 41);
    assert_eq!(scan.features.operations[0].kind, "Round");
    assert_eq!(scan.features.operations[1].kind, "Future Feature");
    assert_eq!(scan.features.operations[2].kind, "Datum Plane");
    assert_eq!(scan.features.operations[3].kind, "Draft");
    assert_eq!(scan.features.operations[4].feature_id, 40);
    assert_eq!(scan.features.operations[4].kind, "Hole");
    assert_eq!(scan.features.operations[4].stored_name_prefix, None);
    assert_eq!(scan.features.operations[5].kind, "Surface");
    assert_eq!(scan.features.operations[5].stored_name_prefix, Some(b'y'));

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let states = &result.ir.native.namespace("creo").unwrap().arenas["feature_operation_states"];
    assert_eq!(states.len(), 7);
    let feature_40 = states
        .iter()
        .filter(|state| state.fields["feature_id"] == 40)
        .collect::<Vec<_>>();
    assert_eq!(feature_40.len(), 2);
    assert_eq!(feature_40[0].fields["state_ordinal"], 0);
    assert_eq!(feature_40[0].fields["current"], false);
    assert_eq!(feature_40[0].fields["stored_name"], "xProtrusion id 40");
    assert_eq!(
        feature_40[0].fields["stored_name_bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|byte| byte.as_u64().unwrap() as u8)
            .collect::<Vec<_>>(),
        b"xProtrusion id 40"
    );
    assert_eq!(feature_40[0].fields["identifier_keyword"], "id");
    assert_eq!(feature_40[1].fields["state_ordinal"], 1);
    assert_eq!(feature_40[1].fields["current"], true);
    assert_eq!(result.ir.model.features.len(), 6);
    assert_eq!(
        result.ir.model.features[0].id.as_str(),
        "creo:model:feature#40"
    );
    assert_eq!(result.ir.model.features[0].ordinal, 4);
    assert_eq!(
        result.ir.model.features[1].id.as_str(),
        "creo:model:feature#41"
    );
    assert_eq!(result.ir.model.features[1].ordinal, 0);
    assert!(matches!(
        &result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            diameter: None,
            extent: None,
            ..
        }
    ));
    assert!(matches!(
        &result.ir.model.features[1].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { .. }, ..
        }])
    ));
    assert_eq!(
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#45")
            .expect("state-prefixed feature")
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("y")
    );
    assert_annotation(
        &result.source_fidelity.annotations,
        "creo:model:feature#41",
        "creo:MdlStatus",
        scan.features.operations[0].offset as u64,
        "feature_operation_name",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_preserves_stored_feature_identifier_keyword() {
    let data = build_prt("c", &[("MdlStatus", b"ySurface ID 45\0".to_vec())]);

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#45")
        .expect("surface feature");

    assert_eq!(feature.name.as_deref(), Some("Surface ID 45"));
    assert_eq!(
        feature
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("y")
    );
}

#[test]
fn nd_decoration_selects_nd_layout() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(3, 4))]);
    let scan = container::scan_bytes(data);
    assert_eq!(scan.framing.layout, Layout::Nd);
    // The decorated name is normalized for classification and census.
    assert_eq!(scan.framing.sections[0].name, "VisibGeom");
    assert_eq!(scan.framing.sections[0].raw_name, "ND:0:VisibGeom:1");
    assert_eq!(scan.framing.census.srf_array_count, Some(3));
}

#[test]
fn visible_geometry_namespace_excludes_invisible_and_depdb_rows() {
    let mut visible = visibgeom_payload(1, 0);
    visible.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0xe4, 0xe3]);
    visible.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
    visible
        .extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let mut invisible = visibgeom_payload(1, 0);
    invisible.extend_from_slice(&[8, 0x26, 5, 0x01, 0, 0, 0xe4, 0xe3]);
    invisible.extend_from_slice(b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0\xe4");
    invisible.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x09feat_id\0\x05");
    invisible
        .extend_from_slice(b"topol_ref_data\0\x07\x09\x05\x01\xf6\x0c\x0d\x07\x07\0\0\xe3\xe1\xe3");
    let mut depdb = visibgeom_payload(1, 0);
    depdb.extend_from_slice(&[9, 0x26, 6, 0x01, 0, 0, 0xe4, 0xe3]);

    let scan = container::scan_bytes(build_prt(
        "c",
        &[
            ("VisibGeom", visible),
            ("NovisGeom", invisible),
            ("DEPDB_DATA", depdb),
        ],
    ));

    assert_eq!(
        scan.surfaces
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(
        scan.surfaces
            .parameters
            .iter()
            .map(|record| record.surface_id)
            .collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(
        scan.surfaces
            .nonvisible_rows
            .iter()
            .map(|row| (row.id, row.feature_id))
            .collect::<Vec<_>>(),
        [(8, 5)]
    );
    assert_eq!(scan.curves.prototypes.len(), 1);
    assert_eq!(scan.surfaces.nonvisible_parameters.len(), 1);
    assert_eq!(scan.surfaces.nonvisible_parameters[0].surface_id, 8);
    assert_eq!(scan.surfaces.nonvisible_parameters[0].scalar_values, [1.0]);
    assert_eq!(scan.surfaces.nonvisible_prototype_records.len(), 1);
    assert_eq!(
        scan.surfaces.nonvisible_prototype_records[0].declared_family,
        "cylinder"
    );
    assert_eq!(scan.curves.nonvisible_prototypes.len(), 1);
    assert_eq!(scan.curves.nonvisible_prototypes[0].feature_id, Some(5));
    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.nonvisible_parameters.len(), 1);
    assert_eq!(scan.curves.topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curves.nonvisible_topology_rows[0].faces, [12, 13]);
    assert_eq!(scan.topology.half_edges.len(), 2);

    let result = decode::decode(
        &mut Cursor::new(scan.framing.data.clone()),
        &DecodeOptions::default(),
    )
    .expect("decode");
    let rows = &result.ir.native.namespace("creo").unwrap().arenas["nonvisible_surface_rows"];
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "creo:novisgeom:surface_row#8");
    assert_eq!(rows[0].fields["source_section"], "NovisGeom");
    let namespace = result.ir.native.namespace("creo").unwrap();
    let surface_parameters = &namespace.arenas["nonvisible_surface_parameters"];
    assert_eq!(
        surface_parameters[0].id,
        "creo:novisgeom:surface_parameter#8"
    );
    assert_eq!(surface_parameters[0].fields["slots"][0]["value"], 1.0);
    let surface_prototypes = &namespace.arenas["nonvisible_surface_prototypes"];
    assert!(surface_prototypes[0]
        .id
        .starts_with("creo:novisgeom:surface_prototype#"));
    assert_eq!(surface_prototypes[0].fields["source_section"], "NovisGeom");
    let prototypes = &namespace.arenas["nonvisible_curve_prototypes"];
    assert_eq!(prototypes[0].fields["curve_id"], 7);
    assert_eq!(prototypes[0].fields["source_section"], "NovisGeom");
    let parameters = &namespace.arenas["nonvisible_curve_parameters"];
    assert_eq!(parameters[0].id, "creo:novisgeom:curve_parameter#7");
    let topology = &namespace.arenas["nonvisible_curve_topology_rows"];
    assert_eq!(topology[0].id, "creo:novisgeom:curve_topology#7");
    assert_eq!(topology[0].fields["faces"][0], 12);
}

#[test]
fn depdb_data_with_sparse_sections_selects_depdb() {
    let depdb = b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0feat_defs_12\0protrevolve\0Revolve id 17\0".to_vec();
    let data = build_prt("c", &[("VisibGeom", vec![0x00]), ("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data);
    assert_eq!(scan.framing.layout, Layout::Depdb);
    assert!(scan
        .surfaces
        .rows
        .iter()
        .any(|row| row.id == 7 && row.feature_id == 4));
    assert!(scan
        .features
        .definitions
        .iter()
        .any(|definition| definition.id == 12));
    assert_eq!(scan.features.operations.len(), 1);
    assert_eq!(scan.features.operations[0].feature_id, 17);
    assert_eq!(
        scan.features.operations[0].recipe,
        Some(crate::feature::FeatureRecipe::ProtrudeRevolve)
    );
}

#[test]
fn decode_promotes_unnamed_depdb_recipe_into_feature_history() {
    let depdb = b"\xe3K\xc3\xb6rper ID 8051\0\xe3\
        \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.operations.len(), 2);
    assert_eq!(scan.features.depdb_recipe_rows.len(), 1);
    assert_eq!(scan.features.depdb_recipe_rows[0].feature_id, 8053);
    let operation = scan
        .features
        .operations
        .iter()
        .find(|operation| operation.feature_id == 8053)
        .expect("recipe operation");

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#8053")
        .expect("recipe feature");
    let rows = &result.ir.native.namespace("creo").unwrap().arenas["depdb_recipe_rows"];
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].fields["owner_feature_id"], 8053);
    assert_eq!(rows[0].fields["header"][0], 0);
    assert_eq!(
        rows[0].fields["body"].as_array().map(Vec::len),
        Some(scan.features.depdb_recipe_rows[0].body.len())
    );
    assert_eq!(feature.name, None);
    assert_eq!(
        feature
            .parent
            .as_ref()
            .map(cadmpeg_ir::features::FeatureId::as_str),
        Some("creo:model:feature#8051")
    );
    assert_eq!(
        feature
            .dependencies
            .iter()
            .map(cadmpeg_ir::features::FeatureId::as_str)
            .collect::<Vec<_>>(),
        ["creo:model:feature#8051"]
    );
    assert_eq!(feature.source_tag.as_deref(), Some("protextrude"));
    assert_eq!(
        feature.source_properties.get("recipe").map(String::as_str),
        Some("protextrude")
    );
    assert_annotation(
        &result.source_fidelity.annotations,
        "creo:model:feature#8053",
        "creo:DEPDB_DATA",
        operation.offset as u64,
        "feature_recipe",
        Exactness::ByteExact,
    );
}

#[test]
fn scan_partitions_multiple_depdb_recipe_rows() {
    let depdb = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
        \xf7\x50\x9f\x77\x83\x94\xf6\x9f\x75Profile 2\0\xf6\0cutextrude\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data);

    assert_eq!(scan.features.depdb_recipe_rows.len(), 2);
    assert_eq!(scan.features.depdb_recipe_rows[0].feature_id, 8053);
    assert_eq!(
        scan.features.depdb_recipe_rows[0].root_schema_class,
        Some(917)
    );
    assert_eq!(scan.features.depdb_recipe_rows[1].feature_id, 8055);
    assert_eq!(
        scan.features.depdb_recipe_rows[1].root_schema_class,
        Some(916)
    );
    assert!(scan.features.depdb_recipe_rows[0].offset < scan.features.depdb_recipe_rows[1].offset);
    assert!(
        scan.features.depdb_recipe_rows[0].body_offset <= scan.features.depdb_recipe_rows[0].offset
    );
}

#[test]
fn decode_retains_recipe_history_and_projects_the_final_state() {
    let depdb = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
        \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protrevolve\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.operation_states.len(), 2);
    assert_eq!(scan.features.operations.len(), 1);
    assert_eq!(
        scan.features.operations[0].recipe,
        Some(crate::feature::FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(scan.features.depdb_recipe_rows.len(), 2);
    assert!(scan
        .features
        .depdb_recipe_rows
        .iter()
        .all(|row| row.feature_id == 8053));
    assert_eq!(
        scan.features
            .depdb_recipe_rows
            .iter()
            .filter_map(|row| row.root_schema_class)
            .collect::<Vec<_>>(),
        [917, 917]
    );

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#8053")
        .expect("revolution feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::NewBody,
        }
    ));
    assert_eq!(
        feature
            .source_properties
            .get("featdefs_row_schema_classes")
            .map(String::as_str),
        Some("917")
    );
    assert_eq!(
        feature.source_properties.get("recipe").map(String::as_str),
        Some("protrevolve")
    );
    assert_eq!(feature.source_tag.as_deref(), Some("protrevolve"));
}

#[test]
fn scan_binds_standalone_depdb_section_to_its_recipe_owner() {
    let mut depdb = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
        var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2"
        .to_vec();
    depdb.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    depdb.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    depdb.extend_from_slice(
        b"\xe3Body ID 17\0\xe3\
        \xf7\x3b\x11\x83\x95\xf6\x04Profile 1\0\xf6\0protextrude\0",
    );
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.definitions.len(), 1);
    let definition = &scan.features.definitions[0];
    assert_eq!(definition.id, 2);
    assert_eq!(definition.owner_feature_id, Some(17));
    let variables = definition.variables.as_ref().expect("var_arr");
    assert_eq!(variables.points.len(), 1);
    assert_eq!(variables.points[0].point_id, 7);
    assert_eq!(variables.points[0].u, Some(1.0));
    assert_eq!(variables.points[0].v, Some(3.0));

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["feature_definitions"];
    assert_eq!(records[0].fields["source_section"], "DEPDB_DATA");
    assert_annotation(
        &result.source_fidelity.annotations,
        "creo:featdefs:feature_definition#2",
        "creo:DEPDB_DATA",
        definition.offset as u64,
        "feature_definition_record",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_preserves_unowned_depdb_section_instances_with_unique_native_ids() {
    let depdb = b"feat_defs_917\0template\xe3S2D0004\0first\xe3S2D0004\0second".to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());
    let positional = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.body.starts_with(b"\xe3S2D"))
        .collect::<Vec<_>>();

    assert_eq!(positional.len(), 2);
    assert!(positional
        .iter()
        .all(|definition| definition.owner_feature_id.is_none()));
    let expected_positional_ids = positional
        .iter()
        .map(|definition| {
            format!(
                "creo:featdefs:feature_definition#offset:{}",
                definition.offset
            )
        })
        .collect::<BTreeSet<_>>();

    let result = decode::decode(&mut Cursor::new(data), &DecodeOptions::default()).expect("decode");
    let records = &result.ir.native.namespace("creo").unwrap().arenas["feature_definitions"];
    let positional_ids = records
        .iter()
        .filter(|record| expected_positional_ids.contains(record.id.as_str()))
        .map(|record| record.id.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(positional_ids, expected_positional_ids);
    assert!(positional_ids
        .iter()
        .all(|id| id.starts_with("creo:featdefs:feature_definition#offset:")));
    assert!(result.ir.model.features.is_empty());
}

#[test]
fn scan_binds_standalone_depdb_datum_and_parent_tables_to_recipe_owner() {
    let depdb = b"nested dtm_id_tab\0\xe1\
        \xe0\x01dtm_id_tab\0\xf8\x01\xf7\x24\xe2\xe0\x01dtm_id\0\x29\
        \xe0\x01parent_table\0\xf8\x02\x03\x05\xf7\x24\xe3\
        Body ID 17\0\xe3\xf7\x3b\x11\x83\x95\xf6\x04Profile 1\0\xf6\0protextrude\0"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", depdb)]));

    let datum_table = scan
        .features
        .geometry_tables
        .iter()
        .find(|table| table.kind == crate::feature::FeatureGeometryTableKind::DatumIds)
        .expect("datum table");
    assert_eq!(datum_table.feature_id, 17);
    assert_eq!(datum_table.entry_ids.as_deref(), Some(&[41][..]));

    let parents = scan
        .features
        .affected_ids
        .iter()
        .find(|record| record.kind == crate::feature::AffectedIdKind::Parents)
        .expect("parent table");
    assert_eq!(parents.feature_id, 17);
    assert_eq!(parents.ids, [3, 5]);
}

#[test]
fn scan_distinguishes_null_and_referenced_family_tables() {
    let null_data = build_prt(
        "c",
        &[(
            "FamilyInf",
            b"Sld_FamilyInfo\0drv_tbl_ptr\0\xe1\xf1".to_vec(),
        )],
    );
    let null = container::scan_bytes(null_data.clone());
    assert_eq!(
        null.framing.family_table.unwrap().pointer,
        crate::container::FamilyTablePointer::Null
    );
    let decoded = decode::decode(&mut Cursor::new(null_data), &DecodeOptions::default())
        .expect("decode null family table");
    let configuration = &decoded.ir.native.namespace("creo").unwrap().arenas["configuration"];
    assert_eq!(configuration.len(), 1);
    assert_eq!(configuration[0].id, "creo:family_info:driver_table#root");
    assert_eq!(configuration[0].fields["pointer_kind"], "null");
    assert!(configuration[0].fields["table_entity_id"].is_null());
    assert_eq!(
        decoded.ir.source.as_ref().unwrap().attributes["configuration_state"],
        "none"
    );
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("configuration")));
    let referenced_data = build_prt(
        "c",
        &[(
            "FamilyInf",
            b"Sld_FamilyInfo\0drv_tbl_ptr\0\xf7\x81\x23\xf1".to_vec(),
        )],
    );
    let referenced = container::scan_bytes(referenced_data.clone());
    assert_eq!(
        referenced.framing.family_table.unwrap().pointer,
        crate::container::FamilyTablePointer::Entity(0x0123)
    );
    let decoded = decode::decode(&mut Cursor::new(referenced_data), &DecodeOptions::default())
        .expect("decode referenced family table");
    let configuration = &decoded.ir.native.namespace("creo").unwrap().arenas["configuration"];
    assert_eq!(configuration[0].fields["pointer_kind"], "entity_reference");
    assert_eq!(configuration[0].fields["table_entity_id"], 0x0123);
    assert_eq!(
        decoded.ir.source.as_ref().unwrap().attributes["configuration_state"],
        "driver_table_unresolved"
    );
}

#[test]
fn decode_reports_only_unimplemented_relation_function_namespaces() {
    let data = build_prt("c", &[]);
    let decoded = decode::decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode empty part");

    let relation_gap = decoded
        .report
        .losses
        .iter()
        .find(|loss| loss.message.contains("cross-model relation functions"))
        .expect("precise remaining relation gap");
    assert!(relation_gap.message.contains("graph, case-study, cabling"));
    assert!(!relation_gap.message.contains("pattern-matching"));
}

#[test]
fn framing_names_are_not_mistaken_for_sections() {
    let data = build_prt("c", &[("VisibGeom", vec![0x00])]);
    let scan = container::scan_bytes(data);
    // Only VisibGeom — the header/TOC framing markers are excluded.
    assert_eq!(scan.framing.sections.len(), 1);
    assert_eq!(scan.framing.sections[0].name, "VisibGeom");
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
fn container_only_preserves_sections_without_transferring_entities() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("MdlStatus", b"Datum Plane id 4\0".to_vec()),
        ],
    );
    let result = decode::decode(
        &mut Cursor::new(data),
        &DecodeOptions {
            container_only: true,
        },
    )
    .expect("container decode");

    assert!(result.report.container_only);
    assert!(!result.report.geometry_transferred);
    assert!(result.ir.model.surfaces.is_empty());
    assert!(result.ir.model.features.is_empty());
    assert_eq!(result.ir.native_unknowns("creo").unwrap().len(), 1);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.starts_with("Transferred ")));
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
