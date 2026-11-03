// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic PSB byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images. They construct raw bytes only;
//! decode and owner tests own the assertions.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::Exactness;

/// Assemble a minimal PSB file: the `#UGC:2` header, a TOC, then the given
/// `(header_name, payload)` sections joined by the `#\n` terminator rule.
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
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

/// One slot of a synthetic `dim_array`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DimSlot<'a> {
    /// The null slot `e1`.
    Null,
    /// The labeled first element, from its `e3` opener through `bck_value`.
    Prototype(&'a [u8]),
    /// A later element, from its `e3` opener through its last field before
    /// `symbol`.
    Later(&'a [u8]),
}

/// A `dim_array` field with one slot for each of `slots`: `e0 00 dim_array\0`,
/// `token`, `f8 <count> f7 02 fb`, then each slot. Each element ends with the
/// `symbol` field that states its slot index. Returns the bytes and the offset
/// of each slot.
pub(crate) fn dim_array(token: &[u8], slots: &[DimSlot<'_>]) -> (Vec<u8>, Vec<usize>) {
    assert!(slots.len() < 0x80, "one-byte compact slot count");
    let mut bytes = b"\xe0\x00dim_array\0".to_vec();
    bytes.extend_from_slice(token);
    bytes.push(0xf8);
    bytes.push(u8::try_from(slots.len()).unwrap());
    bytes.extend_from_slice(b"\xf7\x02\xfb");
    let mut starts = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        starts.push(bytes.len());
        let index = u8::try_from(index).unwrap();
        match slot {
            DimSlot::Null => bytes.push(0xe1),
            DimSlot::Prototype(fields) => {
                bytes.extend_from_slice(fields);
                bytes.extend_from_slice(
                    b"\xe0\x00symbol\0\xe2\xe0\x01type\0\x01\xe0\x01text(i_val)\0",
                );
                bytes.push(index);
                bytes.push(0xf2);
            }
            DimSlot::Later(fields) => {
                bytes.extend_from_slice(fields);
                bytes.extend_from_slice(&[0xe2, 0x01, index]);
            }
        }
    }
    (bytes, starts)
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
        row.push(u8::try_from(schema_class).expect("fixture value fits u8"));
    } else {
        assert!(schema_class <= 0x3fff);
        row.extend_from_slice(&[
            0x80 | (u8::try_from(schema_class >> 8).expect("fixture value fits u8")),
            u8::try_from(schema_class & 0xff).expect("fixture value fits u8"),
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
            packed[offset / 8] |= (u8::try_from((u16::from(value) >> bit) & 1)
                .expect("fixture value fits u8"))
                << (offset % 8);
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
        assert!(!annotations.exactness().contains_key(id));
    } else {
        assert_eq!(annotations.exactness()[id].entity(), exactness);
        assert!(annotations.exactness()[id].fields().is_empty());
    }
}

pub(crate) fn assert_unknown_visible_surface(surfaces: &[cadmpeg_ir::geometry::Surface], id: u32) {
    let surface = surfaces
        .iter()
        .find(|surface| surface.id.as_str() == format!("creo:visibgeom:surface#{id}"))
        .expect("retained unresolved visible surface");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(_)
        })
    ));
}

/// A complete legacy layout for tests of layout-dependent projection.
pub(crate) fn legacy_layout() -> crate::container::Layout {
    crate::container::scan_bytes_ok(
        b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 12\n#END_OF_P_OBJECT\n#Pro/ENGINEER\n"
            .as_slice(),
    )
    .framing
    .layout
}

/// Stable synthetic byte offset for a legacy object identifier.
pub(crate) fn fixture_offset(id: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hash);
    let word = hash.finish();
    #[cfg(target_pointer_width = "32")]
    let word = word & u64::from(u32::MAX);
    usize::try_from(word).expect("fixture offset word fits pointer width")
}

/// Legacy object record placed at [`fixture_offset`] of `id`.
pub(crate) fn object(
    id: &str,
    name: &str,
    parent: Option<&str>,
    mut payload: crate::legacy::ObjectPayload,
) -> crate::legacy::ObjectRecord {
    if let crate::legacy::ObjectPayload::Array { elements, .. } = &mut payload {
        for element in elements {
            *element = crate::legacy::object_node_id(fixture_offset(element));
        }
    }
    crate::legacy::ObjectRecord {
        name: name.to_string(),
        attribute_id: 0,
        scope_offset: 0,
        parent: parent.map(fixture_offset),
        depth: 0,
        payload,
        offset: fixture_offset(id),
    }
}

/// Append an `FC05` world-token scalar to a generated payload.
pub(crate) fn world(payload: &mut Vec<u8>, value: f64) {
    let raw = value.to_be_bytes();
    payload.push(match raw[0] {
        0x40 => 0x46,
        0xc0 => 0x2d,
        _ => panic!("generated FC05 value must use a world-token exponent"),
    });
    payload.extend_from_slice(&raw[1..]);
}

/// Build a closed graph for the ring used by a synthetic geometry fixture.
pub(crate) fn closed_loop(
    face_id: Option<std::num::NonZeroU32>,
    half_edges: Vec<crate::topology::HalfEdgeId>,
) -> crate::topology::Loop {
    let graph = half_edges
        .iter()
        .zip(half_edges.iter().cycle().skip(1))
        .map(|(id, next)| crate::topology::HalfEdge {
            id: *id,
            face_id,
            next: Some(*next),
        })
        .collect::<Vec<_>>();
    { crate::topology::Loop::new(face_id, half_edges, &graph) }.expect("ring admission")
}

/// An admitted empty container for owner tests that supply their own rows.
pub(crate) fn empty_container_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(build_prt("test", &[]));
    scan.framing.data = Vec::new().into();
    scan
}

/// Decodes native payload strings for exact byte assertions.
pub(crate) fn native_bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::from_value::<cadmpeg_ir::native::bytes::NativeBytes>(value.clone())
        .expect("native hexadecimal byte string")
        .into_inner()
}

/// A component table: a sub-assembly at the identity,
/// part `BRACKET` with x along x, z along negative y, and origin
/// `(340, 408, 0)`, and part `BRACKET` turned a quarter turn about z with
/// origin `(-355, 2.5, 619)`.
pub(crate) fn members_table() -> Vec<u8> {
    let mut table = b"\xe0\x00members\0\xf1\xf8\x03\xf7\x44\xfb\xe3".to_vec();
    table.extend_from_slice(b"\xe0\x01id\0\x2a\xe0\x01type\0\x01");
    table.extend_from_slice(b"\xe0\x0aname\0SUBASM\0\xe0\x0afamily_name\0SUBASM\0");
    table.extend_from_slice(b"\xe0\x00tha_ref\0\xe1");
    table.extend_from_slice(b"\xe0\x02e1\0\xf8\x03\x0f\x18\xe4");
    table.extend_from_slice(b"\xe0\x02e2\0\xf8\x03\x18\x0f\x18");
    table.extend_from_slice(b"\xe0\x02e3\0\xf8\x03\x18\xe4\x0f");
    table.extend_from_slice(b"\xe0\x02origin\0\xf8\x03\x18\xe5");
    table.extend_from_slice(b"\xf1\xf7\x44\xe3");
    table.extend_from_slice(b"\x8a\x0f\x02BRACKET\0BRACKET\0\xe1");
    table.extend_from_slice(b"\x0f\x18\xe4\x18\xe4\x0f\x18\x10\x18");
    table.extend_from_slice(b"\x2d\x75\x40\x00\x00\x00\x00\x00\x2f\x79\x80\x18\xe3");
    table.extend_from_slice(b"\x8a\x10\x02BRACKET\0BRACKET\0\xe1");
    table.extend_from_slice(b"\x18\x0f\x18\x10\x18\xe4\x18\xe4\x0f");
    table.extend_from_slice(b"\x48\x76\x30\x8f\x00\x00\x00\x00\x00\x00\x2f\x83\x58\xf4");
    table
}

/// A `NeuAsmSld` payload holding `table`.
pub(crate) fn neu_asm_payload(table: Vec<u8>) -> Vec<u8> {
    let mut payload = b"\xe0\x00Sld_NeutAssem\0\xe3".to_vec();
    payload.extend(table);
    payload.push(0x04);
    payload
}

/// A minimal assembly file: an `ASSEMBLY` header, model name `TOP_ASSY`, and
/// the [`members_table`] component table.
pub(crate) fn build_asm() -> Vec<u8> {
    build_asm_with(members_table())
}

/// [`build_asm`] with `table` as its component table.
pub(crate) fn build_asm_with(table: Vec<u8>) -> Vec<u8> {
    let part = build_prt_raw(
        "test",
        &[
            ("BasBasData", b"\xe0\x0amodel_name\0TOP_ASSY\0".to_vec()),
            ("NeuAsmSld", neu_asm_payload(table)),
        ],
    );
    let header_end = part
        .iter()
        .position(|&byte| byte == b'\n')
        .expect("header line");
    let mut assembly = b"#UGC:2 ASSEMBLY 2562 2120".to_vec();
    assembly.extend_from_slice(&part[header_end..]);
    assembly
}
