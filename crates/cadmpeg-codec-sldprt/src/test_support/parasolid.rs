// SPDX-License-Identifier: Apache-2.0
//! Synthetic Parasolid stream and B-rep body byte builders for crate tests.
#![allow(clippy::unwrap_used)]

/// A minimal Parasolid stream payload: `PS\0\0`, description, padding, a
/// length-prefixed schema token, then the class-definition record `body`.
pub(crate) fn parasolid_payload(description: &str, schema: &str) -> Vec<u8> {
    parasolid_with_body(description, schema, &[0u8; 8])
}

pub(crate) fn parasolid_with_body(description: &str, schema: &str, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[b'P', b'S', 0x00, 0x00]);
    b.extend_from_slice(&(description.len() as u16).to_be_bytes());
    b.extend_from_slice(description.as_bytes());
    b.extend_from_slice(&[0x00, 0x00]); // padding
    b.push(schema.len() as u8);
    b.extend_from_slice(schema.as_bytes());
    b.extend_from_slice(body);
    b
}

pub(crate) const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

pub(crate) const DIRTY_TERMINAL_KNOT: [u8; 8] = 0x7ff8_0000_0000_0001u64.to_be_bytes();

pub(crate) fn be16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn be32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn bef64(b: &mut Vec<u8>, v: f64) {
    b.extend_from_slice(&v.to_be_bytes());
}

/// A compact analytic plane carrier (tag `00 32`, 9 f64): origin, normal, refdir.
pub(crate) fn plane_carrier(
    attr: u16,
    origin: [f64; 3],
    normal: [f64; 3],
    refdir: [f64; 3],
) -> Vec<u8> {
    let mut b = vec![0x00, 0x32];
    be16(&mut b, attr);
    be32(&mut b, 0); // ordinal
    for _ in 0..5 {
        be16(&mut b, 0); // refs[5]
    }
    b.push(0x2b); // marker
    for v in origin.into_iter().chain(normal).chain(refdir) {
        bef64(&mut b, v);
    }
    b
}

/// A compact type-60 offset surface over one support-surface attribute.
pub(crate) fn offset_surface_carrier(attr: u16, support: u16, distance: f64) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x3c];
    be16(&mut bytes, attr);
    be32(&mut bytes, 0);
    for _ in 0..5 {
        be16(&mut bytes, 0);
    }
    bytes.push(0x2b);
    bytes.push(b'V');
    bytes.push(1);
    be16(&mut bytes, support);
    bef64(&mut bytes, distance);
    bytes
}

/// A compact type-56 constant-radius rolling-ball blend.
pub(crate) fn blend_surface_carrier(
    attr: u16,
    supports: [u16; 2],
    spine: u16,
    signed_radius: f64,
) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x38];
    be16(&mut bytes, attr);
    be32(&mut bytes, 0);
    for _ in 0..5 {
        be16(&mut bytes, 0);
    }
    bytes.push(0x2b);
    bytes.push(b'R');
    for reference in supports.into_iter().chain([spine]) {
        be16(&mut bytes, reference);
    }
    for value in [signed_radius, signed_radius, 1.0, 1.0] {
        bef64(&mut bytes, value);
    }
    bytes
}

pub(crate) fn blend_triangle_body() -> Vec<u8> {
    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    body.extend(blend_surface_carrier(180, [181, 182], 183, 0.002));
    body.extend(plane_carrier(
        181,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(line_carrier(183, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body
}

/// A compact analytic line carrier (tag `00 1e`, 6 f64): point, direction.
pub(crate) fn line_carrier(attr: u16, point: [f64; 3], dir: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1e];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for v in point.into_iter().chain(dir) {
        bef64(&mut b, v);
    }
    b
}

pub(crate) fn prefixed_line_carrier(attr: u16, point: [f64; 3], dir: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1e];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for reference in [0u16, 0, 0, 0, 0] {
        be16(&mut b, reference);
        b.push(1);
    }
    b.push(0x2b);
    for value in point.into_iter().chain(dir) {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn cylinder_carrier(
    attr: u16,
    origin: [f64; 3],
    axis: [f64; 3],
    radius: f64,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x33];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in origin
        .into_iter()
        .chain(axis)
        .chain([radius, 1.0, 0.0, 0.0])
    {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn cone_carrier(
    attr: u16,
    origin: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    half_angle: f64,
    reference: [f64; 3],
) -> Vec<u8> {
    let mut b = vec![0x00, 0x34];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in origin.into_iter().chain(axis).chain([
        radius,
        half_angle.sin(),
        half_angle.cos(),
        reference[0],
        reference[1],
        reference[2],
    ]) {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn torus_carrier(
    attr: u16,
    center: [f64; 3],
    axis: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
    reference: [f64; 3],
) -> Vec<u8> {
    let mut b = vec![0x00, 0x36];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in center.into_iter().chain(axis).chain([
        major_radius,
        minor_radius,
        reference[0],
        reference[1],
        reference[2],
    ]) {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn sphere_carrier(attr: u16, center: [f64; 3], radius: f64) -> Vec<u8> {
    let mut b = vec![0x00, 0x35];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in center
        .into_iter()
        .chain([radius, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0])
    {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn circle_carrier(attr: u16, center: [f64; 3], axis: [f64; 3], radius: f64) -> Vec<u8> {
    let mut b = vec![0x00, 0x1f];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    let reference = if axis[0].abs() > 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    for value in center
        .into_iter()
        .chain(axis)
        .chain(reference)
        .chain([radius])
    {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn ellipse_carrier(
    attr: u16,
    center: [f64; 3],
    axis: [f64; 3],
    major_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x20];
    be16(&mut bytes, attr);
    be32(&mut bytes, 0);
    for _ in 0..5 {
        be16(&mut bytes, 0);
    }
    bytes.push(0x2b);
    for value in center
        .into_iter()
        .chain(axis)
        .chain(major_direction)
        .chain([major_radius, minor_radius])
    {
        bef64(&mut bytes, value);
    }
    bytes
}

pub(crate) fn closed_cylinder_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(circle_carrier(70, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(circle_carrier(71, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(bridge(10, 20, 100));
    let mut first = loop_head(20, 30, 10);
    first[14..16].copy_from_slice(&21u16.to_be_bytes());
    b.extend(first);
    b.extend(loop_head(21, 31, 10));
    b.extend(coedge(30, 20, 30, 50, 0, 40, false));
    b.extend(coedge(31, 21, 31, 51, 0, 41, false));
    b.extend(edge_use_with_canonical(40, 30, 70));
    b.extend(edge_use_with_canonical(41, 31, 71));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(world_point(60, [-1.0, 0.0, 0.0]));
    b.extend(world_point(61, [-1.0, 0.0, 1.0]));
    b
}

pub(crate) fn sphere_patch_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(sphere_carrier(100, [0.0, 0.0, 0.0], 1.0));
    b.extend(circle_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1.0));
    b.extend(circle_carrier(71, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0));
    b.extend(circle_carrier(72, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 50, 0, 40, false));
    b.extend(coedge(31, 20, 32, 51, 0, 41, false));
    b.extend(coedge(32, 20, 30, 52, 0, 42, false));
    b.extend(edge_use(40, 70));
    b.extend(edge_use(41, 71));
    b.extend(edge_use(42, 72));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(vertex_use(52, 62));
    b.extend(world_point(60, [1.0, 0.0, 0.0]));
    b.extend(world_point(61, [0.0, 1.0, 0.0]));
    b.extend(world_point(62, [0.0, 0.0, 1.0]));
    b
}

pub(crate) fn sphere_existing_seam_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(sphere_carrier(100, [0.0, 0.0, 0.0], 1.0));
    b.extend(circle_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1.0));
    b.extend(circle_carrier(71, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0));
    b.extend(circle_carrier(72, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 51, 0, 40, false));
    b.extend(coedge(31, 20, 32, 52, 0, 41, false));
    b.extend(coedge(32, 20, 33, 53, 0, 42, false));
    b.extend(coedge(33, 20, 30, 51, 0, 43, false));
    b.extend(edge_use(40, 70));
    b.extend(edge_use(41, 71));
    b.extend(edge_use(42, 72));
    b.extend(edge_use(43, 0));
    b.extend(vertex_use(51, 60));
    b.extend(vertex_use(52, 61));
    b.extend(vertex_use(53, 62));
    b.extend(world_point(60, [0.0, 0.0, -1.0]));
    b.extend(world_point(61, [1.0, 0.0, 0.0]));
    b.extend(world_point(62, [0.0, 1.0, 0.0]));
    b
}

pub(crate) fn f64_array(tag: u8, attr: u16, values: &[f64]) -> Vec<u8> {
    let mut b = vec![0x00, tag, 0x2b];
    be32(&mut b, values.len() as u32);
    be16(&mut b, attr);
    for value in values {
        bef64(&mut b, *value);
    }
    b
}

pub(crate) fn u16_array(attr: u16, values: &[u16]) -> Vec<u8> {
    let mut b = vec![0x00, 0x7f, 0x2b];
    be32(&mut b, values.len() as u32);
    be16(&mut b, attr);
    for value in values {
        be16(&mut b, *value);
    }
    b
}

pub(crate) fn remove_array_type_markers(bytes: &mut Vec<u8>) {
    let mut offset = 0;
    while offset + 2 < bytes.len() {
        if bytes[offset] == 0
            && matches!(bytes[offset + 1], 0x2d | 0x7f | 0x80)
            && bytes[offset + 2] == 0x2b
        {
            bytes.remove(offset + 2);
        }
        offset += 1;
    }
}

pub(crate) fn nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let mult_attr = descriptor_attr + 2;
    let knot_attr = descriptor_attr + 3;
    let mut b = vec![0x00, 0x86];
    be16(&mut b, wrapper_attr);
    be16(&mut b, descriptor_attr);
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&[0x00, 0x88]);
    be16(&mut b, descriptor_attr);
    be16(&mut b, 2);
    be32(&mut b, 3);
    be16(&mut b, 3);
    be32(&mut b, 2);
    b.push(0);
    be32(&mut b, 0);
    be16(&mut b, control_attr);
    be16(&mut b, mult_attr);
    be16(&mut b, knot_attr);
    b.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
    ));
    b.extend(u16_array(mult_attr, &[3, 3]));
    b.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
    b
}

pub(crate) fn typed_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let mut bytes = nurbs_curve_carrier(wrapper_attr, descriptor_attr);
    let descriptor = bytes.split_off(14);
    bytes.truncate(4);
    be32(&mut bytes, 0x1a);
    for reference in [
        descriptor_attr + 20,
        descriptor_attr + 21,
        descriptor_attr + 22,
    ] {
        be16(&mut bytes, reference);
    }
    be16(&mut bytes, 1);
    bytes.push(0x2b);
    be16(&mut bytes, descriptor_attr);
    be16(&mut bytes, descriptor_attr + 1);
    bytes.extend(descriptor);
    remove_array_type_markers(&mut bytes);
    bytes
}

pub(crate) fn rational_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let mult_attr = descriptor_attr + 2;
    let knot_attr = descriptor_attr + 3;
    let mut bytes = vec![0x00, 0x86];
    be16(&mut bytes, wrapper_attr);
    be16(&mut bytes, descriptor_attr);
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0x00, 0x88]);
    be16(&mut bytes, descriptor_attr);
    be16(&mut bytes, 2);
    be32(&mut bytes, 3);
    be16(&mut bytes, 4);
    be32(&mut bytes, 2);
    bytes.push(0);
    be32(&mut bytes, 0);
    be16(&mut bytes, control_attr);
    be16(&mut bytes, mult_attr);
    be16(&mut bytes, knot_attr);
    bytes.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 1.0, 0.25, 0.5, 0.0, 0.5, 1.0, 0.0, 0.0, 1.0],
    ));
    bytes.extend(u16_array(mult_attr, &[3, 3]));
    bytes.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
    bytes
}

pub(crate) fn linear_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let mult_attr = descriptor_attr + 2;
    let knot_attr = descriptor_attr + 3;
    let mut b = vec![0x00, 0x86];
    be16(&mut b, wrapper_attr);
    be16(&mut b, descriptor_attr);
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&[0x00, 0x88]);
    be16(&mut b, descriptor_attr);
    be16(&mut b, 1);
    be32(&mut b, 2);
    be16(&mut b, 3);
    be32(&mut b, 2);
    b.push(0);
    be32(&mut b, 0);
    be16(&mut b, control_attr);
    be16(&mut b, mult_attr);
    be16(&mut b, knot_attr);
    b.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    ));
    b.extend(u16_array(mult_attr, &[2, 2]));
    b.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
    b
}

pub(crate) fn rational_linear_nurbs_curve_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
) -> Vec<u8> {
    let mut bytes = linear_nurbs_curve_carrier(wrapper_attr, descriptor_attr);
    let descriptor = bytes
        .windows(2)
        .position(|window| window == [0x00, 0x88])
        .unwrap();
    bytes[descriptor + 10..descriptor + 12].copy_from_slice(&4u16.to_be_bytes());
    let control = bytes
        .windows(3)
        .position(|window| window == [0x00, 0x2d, 0x2b])
        .unwrap();
    let old_end = control + 9 + 6 * 8;
    let mut replacement = f64_array(
        0x2d,
        descriptor_attr + 1,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.5],
    );
    bytes.splice(control..old_end, replacement.drain(..));
    bytes
}

pub(crate) fn bounded_curve_wrapper(
    attr: u16,
    source_attr: u16,
    start: [f64; 3],
    end: [f64; 3],
    start_parameter: f64,
    end_parameter: f64,
) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x85];
    be16(&mut bytes, attr);
    be32(&mut bytes, 0);
    for _ in 0..5 {
        be16(&mut bytes, 0);
    }
    bytes.push(0x2b);
    be16(&mut bytes, source_attr);
    for value in start
        .into_iter()
        .chain(end)
        .chain([start_parameter, end_parameter])
    {
        bef64(&mut bytes, value);
    }
    bytes
}

pub(crate) fn nurbs_surface_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    nurbs_surface_carrier_with_v_knot_storage(
        wrapper_attr,
        descriptor_attr,
        bridge_attr,
        &[2, 2],
        &[0.0, 1.0],
    )
}

pub(crate) fn compact_f64_array(attr: u16, values: &[f64]) -> Vec<u8> {
    let mut bytes = vec![
        0,
        u8::try_from(values.len()).expect("compact f64 array count"),
    ];
    be16(&mut bytes, attr);
    for value in values {
        bef64(&mut bytes, *value);
    }
    bytes
}

pub(crate) fn compact_u16_array(attr: u16, values: &[u16]) -> Vec<u8> {
    let mut bytes = vec![
        0,
        u8::try_from(values.len()).expect("compact u16 array count"),
    ];
    be16(&mut bytes, attr);
    for value in values {
        be16(&mut bytes, *value);
    }
    bytes
}

pub(crate) fn compact_counted_nurbs_surface_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    let mut bytes = nurbs_surface_carrier(wrapper_attr, descriptor_attr, bridge_attr);
    let arrays = bytes
        .windows(3)
        .position(|window| window == [0x00, 0x2d, 0x2b])
        .expect("first long array");
    bytes.truncate(arrays);
    bytes.extend(compact_f64_array(
        descriptor_attr + 1,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
    ));
    for attr in [descriptor_attr + 2, descriptor_attr + 3] {
        bytes.extend(compact_u16_array(attr, &[2, 2, 0, 0]));
    }
    for attr in [descriptor_attr + 4, descriptor_attr + 5] {
        bytes.extend(compact_f64_array(
            attr,
            &[
                0.0,
                1.0,
                f64::from_bits(0x7ff8_0000_0000_0001),
                f64::from_bits(0x7ff8_0000_0000_0002),
            ],
        ));
    }
    bytes
}

pub(crate) fn nurbs_surface_carrier_with_terminal_knot_slot(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    nurbs_surface_carrier_with_v_knot_storage(
        wrapper_attr,
        descriptor_attr,
        bridge_attr,
        &[2, 2, 0],
        &[0.0, 1.0, f64::from_bits(0x7ff8_0000_0000_0001)],
    )
}

pub(crate) fn nurbs_surface_carrier_with_v_knot_storage(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
    v_multiplicities: &[u16],
    v_unique_knots: &[f64],
) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let u_mult_attr = descriptor_attr + 2;
    let v_mult_attr = descriptor_attr + 3;
    let u_knot_attr = descriptor_attr + 4;
    let v_knot_attr = descriptor_attr + 5;
    let mut b = vec![0x00, 0x7c];
    be16(&mut b, wrapper_attr);
    be32(&mut b, 1);
    for reference in [0, bridge_attr, 0, 0, 0] {
        be16(&mut b, reference);
    }
    b.push(0x2b);
    be16(&mut b, descriptor_attr);
    be16(&mut b, 0);
    b.extend_from_slice(&[0x00, 0x7e]);
    be16(&mut b, descriptor_attr);
    b.extend_from_slice(&[0, 0]);
    be16(&mut b, 1);
    be16(&mut b, 1);
    be32(&mut b, 2);
    be32(&mut b, 2);
    b.extend_from_slice(&[1, 1]);
    be32(&mut b, 2);
    be32(&mut b, 2);
    b.push(0);
    b.extend_from_slice(&[0, 0]);
    b.push(0x0c);
    be16(&mut b, 3);
    for reference in [
        control_attr,
        u_mult_attr,
        v_mult_attr,
        u_knot_attr,
        v_knot_attr,
    ] {
        be16(&mut b, reference);
    }
    b.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
    ));
    b.extend(u16_array(u_mult_attr, &[2, 2]));
    b.extend(u16_array(v_mult_attr, v_multiplicities));
    b.extend(f64_array(0x80, u_knot_attr, &[0.0, 1.0]));
    b.extend(f64_array(0x80, v_knot_attr, v_unique_knots));
    b
}

pub(crate) fn rational_nurbs_surface_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    let mut bytes = nurbs_surface_carrier(wrapper_attr, descriptor_attr, bridge_attr);
    let descriptor = bytes
        .windows(2)
        .position(|window| window == [0x00, 0x7e])
        .unwrap();
    bytes[descriptor + 28] = 1;
    bytes[descriptor + 32..descriptor + 34].copy_from_slice(&4u16.to_be_bytes());
    let control = bytes
        .windows(3)
        .position(|window| window == [0x00, 0x2d, 0x2b])
        .unwrap();
    let old_end = control + 9 + 12 * 8;
    let mut replacement = f64_array(
        0x2d,
        descriptor_attr + 1,
        &[
            0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.5, 1.0, 0.0, 0.0, 1.0, 0.5, 0.5, 0.25, 0.5,
        ],
    );
    bytes.splice(control..old_end, replacement.drain(..));
    bytes
}

pub(crate) fn markerless_nurbs_surface_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    let mut bytes = nurbs_surface_carrier(wrapper_attr, descriptor_attr, bridge_attr);
    remove_array_type_markers(&mut bytes);
    bytes
}

/// Bridge `00 0e`: `refs[2]` = loop head, `refs[4]` = surface carrier.
pub(crate) fn bridge(attr: u16, loop_attr: u16, surface_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x0e];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    be16(&mut b, 0); // p+6 ref0
    b.extend_from_slice(&MAGIC); // p+8..16
    let refs = [0u16, 0, loop_attr, 0, surface_attr];
    for r in refs {
        be16(&mut b, r); // p+16..26
    }
    b.push(0x2b); // p+26 marker
    b.extend_from_slice(&[0u8; 10]); // p+27..37 tail
    b
}

pub(crate) fn bridge_owned(attr: u16, loop_attr: u16, surface_attr: u16, owner: u16) -> Vec<u8> {
    let mut b = bridge(attr, loop_attr, surface_attr);
    b[8..10].copy_from_slice(&owner.to_be_bytes());
    b
}

pub(crate) fn entity51(flags: u32, attr: u16, disc: u16, slots: &[u16]) -> Vec<u8> {
    let slot_count = match flags as u8 {
        1 | 3 => 6,
        2 => 7,
        4 => 9,
        flo => panic!("unsupported synthetic entity flo {flo}"),
    };
    assert!(slots.len() <= slot_count, "too many synthetic entity slots");
    let mut b = vec![0x00, 0x51];
    be32(&mut b, flags);
    be16(&mut b, attr);
    be32(&mut b, 1);
    be16(&mut b, disc);
    for slot in slots {
        be16(&mut b, *slot);
    }
    for _ in slots.len()..slot_count {
        be16(&mut b, 1);
    }
    b
}

pub(crate) fn entity53_color(attr: u16, rgb: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x53];
    be32(&mut b, 3);
    be16(&mut b, attr);
    for value in rgb {
        bef64(&mut b, value);
    }
    b
}

/// Loop head `00 0f`: `refs[1]` = first coedge, `refs[2]` = owning bridge.
pub(crate) fn loop_head(attr: u16, first_coedge: u16, bridge_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x0f];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    let refs = [0u16, first_coedge, bridge_attr, 0];
    for r in refs {
        be16(&mut b, r); // p+6..14
    }
    b
}

/// Coedge `00 11`: `refs[1]` owner loop, `refs[3]` next, `refs[4]` start
/// vertex-use, `refs[5]` twin, `refs[6]` edge-use; marker is the local sense.
#[allow(clippy::too_many_arguments)]
pub(crate) fn coedge(
    attr: u16,
    owner_loop: u16,
    next: u16,
    start_vuse: u16,
    twin: u16,
    edge_use: u16,
    reversed: bool,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x11];
    be16(&mut b, attr); // p+0
    let refs = [0u16, owner_loop, 0, next, start_vuse, twin, edge_use, 0, 0];
    for r in refs {
        be16(&mut b, r); // p+2..20
    }
    b.push(if reversed { 0x2d } else { 0x2b }); // p+20 marker
    b
}

pub(crate) fn tripled_coedge(
    attr: u16,
    owner_loop: u16,
    next: u16,
    start_vuse: u16,
    edge_use: u16,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x11];
    be16(&mut b, attr);
    for reference in [0, owner_loop, 0, next, start_vuse, 0, edge_use, 0, 0] {
        be16(&mut b, reference);
        b.push(1);
    }
    b.push(0x2b);
    b
}

/// Edge-use `00 10`: `refs[3]` = support curve carrier (0 = none).
pub(crate) fn edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
    edge_use_with_canonical(attr, 0, curve_attr)
}

/// Bare edge-use `refs[0]` names the forward coedge that stores the edge
/// direction. A zero canonical reference is reserved for compact fixtures
/// whose unique forward coedge supplies the same relation.
pub(crate) fn edge_use_with_canonical(
    attr: u16,
    canonical_coedge: u16,
    curve_attr: u16,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x10];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    be16(&mut b, 0); // p+6 ref0
    b.extend_from_slice(&MAGIC); // p+8..16
    let refs = [canonical_coedge, 0, 0, curve_attr, 0, 0];
    for r in refs {
        be16(&mut b, r); // p+16..28
    }
    b
}

pub(crate) fn prefixed_edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x10];
    be16(&mut b, attr);
    be32(&mut b, 0);
    be16(&mut b, 0);
    b.extend_from_slice(&[1, 0, 0]);
    b.extend_from_slice(&MAGIC);
    for reference in [0u16, 0, curve_attr] {
        b.push(1);
        be16(&mut b, reference);
    }
    b
}

pub(crate) fn suffix_prefixed_edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x10];
    be16(&mut b, attr);
    be32(&mut b, 0);
    be16(&mut b, 0);
    b.extend_from_slice(&[1, 0, 0]);
    b.extend_from_slice(&MAGIC);
    for reference in [0x0101, 0x0102, curve_attr] {
        be16(&mut b, reference);
        b.push(1);
    }
    b
}

/// Vertex-use `00 12`: `refs[4]` = world-point attr; magic at body+16.
pub(crate) fn vertex_use(attr: u16, point_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x12];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    let refs = [0u16, 0, 0, 0, point_attr];
    for r in refs {
        be16(&mut b, r); // p+6..16
    }
    b.extend_from_slice(&MAGIC); // p+16..24
    b
}

pub(crate) fn tripled_vertex_use(attr: u16, point_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x12];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for reference in [0u16, 0, 0, 0, point_attr] {
        be16(&mut b, reference);
        b.push(1);
    }
    b.extend_from_slice(&MAGIC);
    b
}

/// World point `00 1d`: xyz f64 BE (metres) at body+14.
pub(crate) fn world_point(attr: u16, xyz: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1d];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    for _ in 0..4 {
        be16(&mut b, 0); // p+6..14 refs[4]
    }
    for v in xyz {
        bef64(&mut b, v); // p+14..38
    }
    b
}

pub(crate) fn tripled_world_point(attr: u16, xyz: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1d];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for reference in [0u16, 0, 0, 0] {
        be16(&mut b, reference);
        b.push(1);
    }
    for value in xyz {
        bef64(&mut b, value);
    }
    b
}

pub(crate) fn tripled_triangle_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
    ));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(tripled_coedge(30, 20, 31, 50, 40));
    b.extend(tripled_coedge(31, 20, 32, 51, 41));
    b.extend(tripled_coedge(32, 20, 30, 52, 42));
    b.extend(edge_use(40, 0));
    b.extend(edge_use(41, 0));
    b.extend(edge_use(42, 0));
    b.extend(tripled_vertex_use(50, 60));
    b.extend(tripled_vertex_use(51, 61));
    b.extend(tripled_vertex_use(52, 62));
    b.extend(tripled_world_point(60, [0.0, 0.0, 0.0]));
    b.extend(tripled_world_point(61, [1.0, 0.0, 0.0]));
    b.extend(tripled_world_point(62, [0.0, 1.0, 0.0]));
    typed_single_face_ownership(&mut b, 900, 901, 902, 10, 100, 1);
    b
}

pub(crate) fn prefixed_edge_triangle_body() -> Vec<u8> {
    let mut b = tripled_triangle_body();
    b.extend(prefixed_line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    b.extend(prefixed_edge_use(40, 70));
    b
}

pub(crate) fn suffix_prefixed_edge_triangle_body() -> Vec<u8> {
    let mut b = tripled_triangle_body();
    b.extend(prefixed_line_carrier(
        0x0103,
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ));
    b.extend(suffix_prefixed_edge_use(40, 0x0103));
    b
}

fn typed_ref(bytes: &mut Vec<u8>, value: u32) {
    assert!(value <= 0x7ffe, "synthetic typed reference must fit u16");
    be16(bytes, value as u16);
}

fn typed_prefix(bytes: &mut Vec<u8>, tag: [u8; 2], attr: u16, node_id: u32) {
    bytes.extend_from_slice(&tag);
    bytes.push(0xff);
    be16(bytes, attr);
    be32(bytes, node_id);
}

/// Append one validated XT ownership graph for a compact face bridge.
pub(crate) fn typed_single_face_ownership(
    bytes: &mut Vec<u8>,
    body_attr: u16,
    shell_attr: u16,
    region_attr: u16,
    face_attr: u16,
    surface_attr: u16,
    body_type: u8,
) {
    typed_prefix(
        bytes,
        [0x00, 0x0c],
        body_attr,
        100_000 + u32::from(body_attr),
    );
    for value in [5, 6, 1, 1, 1, 1] {
        typed_ref(bytes, value);
    }
    bef64(bytes, 1000.0);
    bef64(bytes, 1.0e-8);
    for value in [1, 1, 1] {
        typed_ref(bytes, value);
    }
    bytes.push(1);
    typed_ref(bytes, 2);
    bytes.push(body_type);
    bytes.push(1);
    for value in [u32::from(shell_attr), 1, 1, 1, 1, 1, 1] {
        typed_ref(bytes, value);
    }
    typed_ref(bytes, u32::from(region_attr));
    typed_ref(bytes, 1);
    typed_ref(bytes, 1);
    typed_ref(bytes, 1);

    typed_prefix(
        bytes,
        [0x00, 0x0d],
        shell_attr,
        200_000 + u32::from(shell_attr),
    );
    for value in [
        1,
        u32::from(body_attr),
        1,
        1,
        1,
        1,
        u32::from(region_attr),
        1,
    ] {
        typed_ref(bytes, value);
    }

    typed_prefix(
        bytes,
        [0x00, 0x13],
        region_attr,
        300_000 + u32::from(region_attr),
    );
    for value in [1, u32::from(body_attr), 1, 1, u32::from(shell_attr)] {
        typed_ref(bytes, value);
    }
    bytes.push(b'S');

    typed_prefix(
        bytes,
        [0x00, 0x0e],
        face_attr,
        400_000 + u32::from(face_attr),
    );
    typed_ref(bytes, 1);
    bytes.extend_from_slice(&MAGIC);
    for value in [1, 1, 0, u32::from(shell_attr), u32::from(surface_attr)] {
        typed_ref(bytes, value);
    }
    bytes.push(0x2b);
}

/// One triangular planar face: a plane carrier, a bridge, a loop, three coedges
/// forming a closed ring, three edge-uses, three vertex-uses, and three points.
pub(crate) fn triangle_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 50, 0, 40, false));
    b.extend(coedge(31, 20, 32, 51, 0, 41, false));
    b.extend(coedge(32, 20, 30, 52, 0, 42, false));
    b.extend(edge_use(40, 0));
    b.extend(edge_use(41, 0));
    b.extend(edge_use(42, 0));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(vertex_use(52, 62));
    b.extend(world_point(60, [0.0, 0.0, 0.0]));
    b.extend(world_point(61, [1.0, 0.0, 0.0]));
    b.extend(world_point(62, [0.0, 1.0, 0.0]));
    typed_single_face_ownership(&mut b, 900, 901, 902, 10, 100, 1);
    b
}

pub(crate) fn triangle_body_with_overlapping_point() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    let mut face_bridge = bridge(10, 20, 100);
    face_bridge.splice(31..31, world_point(60, [0.0, 0.0, 0.0]));
    b.extend(face_bridge);
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 50, 0, 40, false));
    b.extend(coedge(31, 20, 32, 51, 0, 41, false));
    b.extend(coedge(32, 20, 30, 52, 0, 42, false));
    b.extend(edge_use(40, 0));
    b.extend(edge_use(41, 0));
    b.extend(edge_use(42, 0));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(vertex_use(52, 62));
    b.extend(world_point(61, [1.0, 0.0, 0.0]));
    b.extend(world_point(62, [0.0, 1.0, 0.0]));
    typed_single_face_ownership(&mut b, 900, 901, 902, 10, 100, 1);
    b
}

pub(crate) fn owned_triangle(base: u16, owner: u16, x: f64) -> Vec<u8> {
    owned_triangle_with_kind(base, owner, x, 1)
}

pub(crate) fn owned_triangle_with_kind(base: u16, owner: u16, x: f64, body_type: u8) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        base + 100,
        [x, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    b.extend(bridge_owned(base + 10, base + 20, base + 100, owner));
    b.extend(loop_head(base + 20, base + 30, base + 10));
    b.extend(coedge(
        base + 30,
        base + 20,
        base + 31,
        base + 50,
        0,
        base + 40,
        false,
    ));
    b.extend(coedge(
        base + 31,
        base + 20,
        base + 32,
        base + 51,
        0,
        base + 41,
        false,
    ));
    b.extend(coedge(
        base + 32,
        base + 20,
        base + 30,
        base + 52,
        0,
        base + 42,
        false,
    ));
    b.extend(edge_use(base + 40, 0));
    b.extend(edge_use(base + 41, 0));
    b.extend(edge_use(base + 42, 0));
    b.extend(vertex_use(base + 50, base + 60));
    b.extend(vertex_use(base + 51, base + 61));
    b.extend(vertex_use(base + 52, base + 62));
    b.extend(world_point(base + 60, [x, 0.0, 0.0]));
    b.extend(world_point(base + 61, [x + 1.0, 0.0, 0.0]));
    b.extend(world_point(base + 62, [x, 1.0, 0.0]));
    typed_single_face_ownership(
        &mut b,
        owner,
        base + 70,
        base + 80,
        base + 10,
        base + 100,
        body_type,
    );
    b
}

pub(crate) fn untyped_triangle(x: f64) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 999));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [x, 0.0, 0.0]));
    body.extend(world_point(61, [x + 1.0, 0.0, 0.0]));
    body.extend(world_point(62, [x, 1.0, 0.0]));
    typed_single_face_ownership(&mut body, 900, 901, 902, 10, 999, 1);
    body
}

pub(crate) fn circular_sketch_body() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(70, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 70));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [1.0, 0.0, 0.0]));
    typed_single_face_ownership(&mut body, 900, 901, 902, 10, 100, 1);
    body
}

pub(crate) fn arc_sketch_body() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(70, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 70));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [1.0, 0.0, 0.0]));
    body.extend(world_point(61, [0.0, 1.0, 0.0]));
    body.extend(world_point(62, [0.0, 0.0, 0.0]));
    typed_single_face_ownership(&mut body, 900, 901, 902, 10, 100, 1);
    body
}

pub(crate) fn count_entity51_family(payload: &[u8], flags: u32, disc: u16) -> usize {
    use cadmpeg_core::decode::View;
    payload
        .windows(14)
        .filter(|window| {
            window[0..2] == [0x00, 0x51]
                && View::u32_be_at(window, 2) == Some(flags)
                && View::u16_be_at(window, 12) == Some(disc)
        })
        .count()
}

pub(crate) fn nurbs_sketch_body(rational: bool) -> Vec<u8> {
    let mut body = triangle_body();
    body.extend(if rational {
        rational_nurbs_curve_carrier(70, 80)
    } else {
        nurbs_curve_carrier(70, 80)
    });
    body.extend(edge_use(40, 70));
    body
}
