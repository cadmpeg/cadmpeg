// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic PSB byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images. They construct raw bytes only;
//! decode and owner tests own the assertions.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::Exactness;

/// Assemble a minimal PSB file: the `#UGC:2` header, a TOC, then the given
/// `(header_name, payload)` sections joined by the `#\n` terminator rule.
pub(crate) fn build_prt(version: &str, sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let sections = sections
        .iter()
        .map(|(name, payload)| {
            let payload =
                if *name == "DEPDB_DATA" && !payload.starts_with(b"\xe0\x00p_dep_db\0\xe3") {
                    let mut prefixed = b"\xe0\x00p_dep_db\0\xe3".to_vec();
                    prefixed.extend_from_slice(payload);
                    prefixed
                } else {
                    payload.clone()
                };
            (*name, payload)
        })
        .collect::<Vec<_>>();
    build_prt_raw(version, &sections)
}

pub(crate) fn build_prt_raw(version: &str, sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
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
pub(crate) fn visibgeom_payload(srf: u8, crv: u8) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"srf_array\0");
    p.extend_from_slice(&[0xf8, srf]); // f8 <count>
    p.extend_from_slice(&[0xe0, 0x22, b'p', 0]); // some noise resembling a row
    p.extend_from_slice(b"crv_array\0");
    p.extend_from_slice(&[0xf3, 0xf8, crv]); // [f3] f8 <count>
    p
}

/// Build one `AllFeatur` row with the settled fixed root-schema prefix.
pub(crate) fn allfeatur_row(
    feature_id: u8,
    header: [u8; 2],
    schema_class: u32,
    body: &[u8],
) -> Vec<u8> {
    let mut row = vec![
        feature_id, header[0], header[1], 0x00, 0x10, 0x01, 0x80, 0x80, 0x00, 0xe4, 0xe3, 0xf6,
    ];
    if schema_class < 0x80 {
        row.push(schema_class as u8);
    } else {
        assert!(schema_class <= 0x3fff);
        row.extend_from_slice(&[
            0x80 | ((schema_class >> 8) as u8),
            (schema_class & 0xff) as u8,
        ]);
    }
    row.push(0xe1);
    row.extend_from_slice(body);
    row
}

pub(crate) fn push_generated_scalar(bytes: &mut Vec<u8>, value: f64) {
    match value {
        0.0 => bytes.push(0x0f),
        1.0 => bytes.push(0xe4),
        -1.0 => bytes.extend_from_slice(&[0x43, 0xf0, 0x00]),
        2.0 => bytes.extend_from_slice(&[0x2f, 0x00, 0x00]),
        4.0 => bytes.extend_from_slice(&[0x2f, 0x10, 0x00]),
        -2.0 => bytes.extend_from_slice(&[0x48, 0x00, 0x00]),
        0.5 => {
            bytes.push(0x71);
            bytes.extend_from_slice(&value.to_be_bytes()[1..]);
        }
        _ => panic!("generated fixture scalar is not encoded"),
    }
}

pub(crate) fn push_generated_plane_row(
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
        normal[*axis].abs() > 1.0e-9
            && (0..3).all(|other| other == *axis || normal[other].abs() <= 1.0e-9)
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

pub(crate) fn push_generated_topology_row(
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

pub(crate) fn push_named_analytic_prototype(
    payload: &mut Vec<u8>,
    family: &str,
    fields: &[(&str, f64)],
) {
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

pub(crate) fn jpeg_payload() -> Vec<u8> {
    vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]
}

pub(crate) fn unix_compress_literals(payload: &[u8]) -> Vec<u8> {
    let mut stream = vec![0x1f, 0x9d, 0x10];
    let mut packed = vec![0; payload.len().saturating_mul(9).div_ceil(8)];
    for (index, value) in payload.iter().copied().enumerate() {
        for bit in 0..9 {
            let offset = index * 9 + bit;
            packed[offset / 8] |= (((u16::from(value) >> bit) & 1) as u8) << (offset % 8);
        }
    }
    stream.extend_from_slice(&packed);
    stream
}

pub(crate) fn build_toc_section_prt(name: &str, payload: &[u8], expanded_length: usize) -> Vec<u8> {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 1 81 17").as_bytes());
    let section_offset = 2 * 81;
    let section_header = format!("#{name}\n");
    let section_length = section_header.len() + payload.len();
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("{name} {section_offset:x} {section_length:x} {expanded_length:x}")
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + section_offset);
    data.extend_from_slice(section_header.as_bytes());
    data.extend_from_slice(payload);
    data
}

pub(crate) fn assert_annotation(
    annotations: &cadmpeg_ir::Annotations,
    id: &str,
    stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let provenance = &annotations.provenance[id];
    assert_eq!(provenance.stream(), stream);
    assert_eq!(provenance.offset, offset);
    assert_eq!(provenance.tag.as_deref(), Some(tag));
    if exactness == Exactness::ByteExact {
        assert!(!annotations.exactness.contains_key(id));
    } else {
        assert_eq!(annotations.exactness[id].entity, exactness);
        assert!(annotations.exactness[id].fields.is_empty());
    }
}

pub(crate) fn assert_unknown_visible_surface(surfaces: &[cadmpeg_ir::geometry::Surface], id: u32) {
    let surface = surfaces
        .iter()
        .find(|surface| surface.id.as_str() == format!("creo:visibgeom:surface#{id}"))
        .expect("retained unresolved visible surface");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Unknown { record: Some(_) }
    ));
}
