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

/// Check every retained boundary on the route at one byte below its next need.
/// Each run owns a fresh caller context; the service run keeps the caller's assertions.
pub(crate) fn assert_retained_boundaries<T>(
    operations: &[&str],
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut seen = std::collections::BTreeSet::new();
    let mut cap = 0;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, ResourceDimension::RetainedBytes);
                let need = resource
                    .used
                    .checked_add(resource.additional)
                    .expect("byte need fits");
                assert!(need > cap);
                if operations.contains(&resource.operation) {
                    policy.limits.max_retained_bytes = need - 1;
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    assert!(
                        matches!(run(&ctx), Err(CodecError::ResourceLimit(ref below))
                        if below.dimension == ResourceDimension::RetainedBytes
                            && below.operation == resource.operation)
                    );
                    seen.insert(resource.operation);
                }
                cap = need;
            }
            Err(error) => panic!("unexpected route refusal: {error:?}"),
            Ok(_) => {
                assert!(
                    operations.iter().all(|operation| seen.contains(operation)),
                    "missing retained boundary: {operations:?} vs {seen:?}"
                );
                return crate::decode::with_test_decode_ctx(|ctx| run(ctx)).expect("service route");
            }
        }
    }
    panic!("retained route did not finish within boundary bound");
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
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::topology::Loop::new(ctx, face_id, half_edges, &graph)
    })
    .expect("ring admission")
    .expect("valid closed ring fixture")
}

/// Check work refusal propagation at each named boundary of an owner route.
pub(crate) fn assert_work_boundaries<T>(
    operations: &[&str],
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut seen = std::collections::BTreeSet::new();
    let mut cap = 0;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, ResourceDimension::WorkUnits);
                assert_eq!(ctx.resource_refusal().as_ref(), Some(&resource));
                let need = resource
                    .used
                    .checked_add(resource.additional)
                    .expect("work need fits");
                assert!(need > cap);
                if operations.contains(&resource.operation) {
                    policy.limits.max_work_units = need - 1;
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    assert!(
                        matches!(run(&ctx), Err(CodecError::ResourceLimit(ref below))
                        if below.dimension == ResourceDimension::WorkUnits
                            && below.operation == resource.operation)
                    );
                    seen.insert(resource.operation);
                }
                cap = need;
            }
            Err(error) => panic!("unexpected route refusal: {error:?}"),
            Ok(_) => {
                assert!(
                    operations.iter().all(|operation| seen.contains(operation)),
                    "missing work boundary: {operations:?} vs {seen:?}"
                );
                return crate::decode::with_test_decode_ctx(|ctx| run(ctx)).expect("service route");
            }
        }
    }
    panic!("work route did not finish within boundary bound");
}

/// Find the last named refusal after admitting each preceding resource boundary.
pub(crate) fn last_refusal_at<T>(
    input: &[u8],
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut cap = 0;
    let mut last = None;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            _ => panic!("unsupported test boundary dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).expect("root");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, dimension);
                assert_eq!(ctx.resource_refusal().as_ref(), Some(&resource));
                let need = resource
                    .used
                    .checked_add(resource.additional)
                    .expect("resource need");
                assert!(need > cap);
                if resource.operation == operation {
                    match dimension {
                        ResourceDimension::WorkUnits => policy.limits.max_work_units = need - 1,
                        ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = need - 1;
                        }
                        ResourceDimension::RetainedBytes => {
                            policy.limits.max_retained_bytes = need - 1;
                        }
                        ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = need - 1;
                        }
                        _ => panic!("unsupported test boundary dimension"),
                    }
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(input, &arena, &policy).expect("root");
                    match run(&ctx) {
                        Err(CodecError::ResourceLimit(below)) => {
                            assert_eq!(below.dimension, dimension);
                            assert_eq!(below.operation, operation);
                            assert_eq!(ctx.resource_refusal().as_ref(), Some(&below));
                            last = Some(CodecError::ResourceLimit(below));
                        }
                        _ => panic!("named boundary must refuse one unit below its need"),
                    }
                }
                cap = need;
            }
            Err(error) => panic!("unexpected route refusal: {error:?}"),
            Ok(_) => return last.expect("route reaches the named resource boundary"),
        }
    }
    panic!("route did not finish within boundary bound");
}

/// Admit preceding element-storage charges and select the named refusal limit.
/// With no operation, return the first limit that admits the unchanged fixture.
pub(crate) fn allocation_limit_at<T>(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: Option<&str>,
    run: impl Fn(u64) -> Result<T, cadmpeg_core::CodecError>,
) -> u64 {
    use cadmpeg_core::CodecError;
    let mut cap = 0;
    for _ in 0..4096 {
        match run(cap) {
            Err(CodecError::ResourceLimit(resource)) => {
                assert_eq!(resource.dimension, dimension);
                let need = resource
                    .used
                    .checked_add(resource.additional)
                    .expect("resource need");
                assert!(need > cap);
                if operation == Some(resource.operation) {
                    let below = need - 1;
                    assert!(
                        matches!(run(below), Err(CodecError::ResourceLimit(ref refusal))
                        if refusal.dimension == dimension && refusal.operation == resource.operation
                            && refusal.used.checked_add(refusal.additional) == Some(need))
                    );
                    return below;
                }
                cap = need;
            }
            Err(error) => panic!("unexpected fixture refusal: {error:?}"),
            Ok(_) => {
                assert!(operation.is_none(), "fixture did not reach {operation:?}");
                return cap;
            }
        }
    }
    panic!("fixture did not finish within the resource boundary bound");
}
