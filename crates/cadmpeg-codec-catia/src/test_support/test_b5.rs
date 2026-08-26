// SPDX-License-Identifier: Apache-2.0
//! b5-family synthetic topology stream builders.

#![allow(clippy::unwrap_used)]
use super::{a8_surface_stream, a8_surface_tail, le_f32, le_f64};

pub(crate) fn append_b5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

pub(crate) fn b5_linear_pcurve_payload(surface: u16, start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
    b5_linear_pcurve_payload_with_knots(surface, [0.0, 1.0], start, end)
}

pub(crate) fn b5_linear_pcurve_payload_with_knots(
    surface: u16,
    knots: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> Vec<u8> {
    let [knot0, knot1] = knots;
    assert!(knot0.is_finite() && knot0 < knot1 && knot1.is_finite());
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.extend_from_slice(&[0x01, 5, 1, 1, 9, 1]);
    payload.extend_from_slice(&le_f64(knot0));
    payload.extend_from_slice(&le_f64(knot1));
    payload.extend_from_slice(&[9, 9]);
    for uv in [start, end] {
        payload.extend_from_slice(&le_f64(uv[0]));
        payload.extend_from_slice(&le_f64(uv[1]));
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [0.0, knot1 - knot0, 1.0, 0.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    payload
}

pub(crate) fn b5_analytic_line_pcurve_payload(
    surface: u16,
    origin: [f64; 2],
    direction: [f64; 2],
    interval: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.push(0x01);
    for value in [
        origin[0],
        origin[1],
        direction[0],
        direction[1],
        interval[0],
        interval[1],
    ] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn b5_isoparametric_line_pcurve_payload(
    surface: u16,
    constant_u: f64,
    interval_v: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.push(0x05);
    for value in [constant_u, interval_v[0], interval_v[1]] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn b5_transverse_isoparametric_line_pcurve_payload(
    surface: u16,
    constant_v: f64,
    interval_u: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.push(0x09);
    for value in [constant_v, interval_u[0], interval_u[1]] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn b5_plane_payload(origin: [f64; 3]) -> Vec<u8> {
    let mut plane = vec![0; 121];
    plane[0] = 0x80;
    for (offset, value) in [
        (1usize, origin[0]),
        (9, origin[1]),
        (17, origin[2]),
        (25, 1.0),
        (33, 0.0),
        (41, 0.0),
        (49, 0.0),
        (57, 1.0),
        (65, 0.0),
        (73, 1.0),
        (81, 1.0),
        (89, -10_000_000.0),
        (97, 10_000_000.0),
        (105, -10_000_000.0),
        (113, 10_000_000.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    plane
}

/// Encode one `0x18` object reference: the lead byte, then the id as a
/// little-endian `u16`. See `wire::object_ref`.
pub(crate) fn b5_object_ref(id: u32) -> [u8; 3] {
    let [low, high] = u16::try_from(id)
        .expect("object id fits a `0x18` reference")
        .to_le_bytes();
    [0x18, low, high]
}

pub(crate) fn b5_closed_triangle_stream() -> Vec<u8> {
    b5_closed_triangle_stream_over_edges([300, 301, 302])
}

/// Build the closed-triangle object stream over caller-chosen edge object ids.
///
/// The `62` loop payload interleaves one pcurve reference and one edge reference
/// per member and closes with the support-surface reference, so an edge id
/// appears both in its `5e` allocation and in the loop member that uses it.
pub(crate) fn b5_closed_triangle_stream_over_edges(edges: [u32; 3]) -> Vec<u8> {
    const SURFACE: u32 = 100;
    const LOOP: u32 = 400;
    const FACE: u32 = 500;
    let pcurves = [
        (200u32, [0.0, 0.0], [1.0, 0.0]),
        (201, [1.0, 0.0], [0.0, 1.0]),
        (202, [0.0, 1.0], [0.0, 0.0]),
    ];

    let mut bytes = Vec::new();
    let plane = b5_plane_payload([0.0; 3]);
    append_b5_record(&mut bytes, 0x27, SURFACE, &plane);
    for (id, start, end) in pcurves {
        append_b5_record(
            &mut bytes,
            0x21,
            id,
            &b5_linear_pcurve_payload(
                u16::try_from(SURFACE).expect("support surface id fits a `u16`"),
                start,
                end,
            ),
        );
    }
    for id in edges {
        append_b5_record(&mut bytes, 0x5e, id, &[]);
    }

    let mut loop_payload = vec![0x87];
    for ((pcurve, _, _), edge) in pcurves.into_iter().zip(edges) {
        loop_payload.extend_from_slice(&b5_object_ref(pcurve));
        loop_payload.extend_from_slice(&b5_object_ref(edge));
    }
    loop_payload.extend_from_slice(&b5_object_ref(SURFACE));
    loop_payload.extend_from_slice(&[0x83, 0x05, 0x05, 0x03]);
    for _ in 0..pcurves.len() {
        loop_payload.extend_from_slice(&[0x01, 0x00, 0xff, 0xff, 0x01, 0x00]);
    }
    loop_payload.push(0x01);
    append_b5_record(&mut bytes, 0x62, LOOP, &loop_payload);

    let mut face_payload = vec![0x82];
    face_payload.extend_from_slice(&b5_object_ref(SURFACE));
    face_payload.extend_from_slice(&b5_object_ref(LOOP));
    face_payload.push(0x05);
    append_b5_record(&mut bytes, 0x5f, FACE, &face_payload);

    for point in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

/// Build the closed-triangle object stream with the complete native endpoint
/// chain: `5e` edges reference `5d` vertices and `06` endpoint incidences;
/// each `5d` points through one `05` roster to the two incident `06` records.
pub(crate) fn b5_closed_triangle_stream_with_native_vertex_chain() -> Vec<u8> {
    const SURFACE: u32 = 100;
    let pcurves = [
        (200u32, [0.0, 0.0], [1.0, 0.0]),
        (201, [1.0, 0.0], [0.0, 1.0]),
        (202, [0.0, 1.0], [0.0, 0.0]),
    ];

    let mut bytes = Vec::new();
    append_b5_record(&mut bytes, 0x27, SURFACE, &b5_plane_payload([0.0; 3]));
    for (id, start, end) in pcurves {
        append_b5_record(
            &mut bytes,
            0x21,
            id,
            &b5_linear_pcurve_payload(
                u16::try_from(SURFACE).expect("support surface id fits a `u16`"),
                start,
                end,
            ),
        );
    }
    append_b5_closed_triangle_native_vertex_chain(
        &mut bytes,
        SURFACE,
        pcurves.map(|(id, _, _)| id),
    );
    bytes
}

/// Append the native endpoint and face chain shared by analytic and freeform
/// support carriers in the object-stream topology fixtures.
fn append_b5_closed_triangle_native_vertex_chain(
    bytes: &mut Vec<u8>,
    surface: u32,
    pcurves: [u32; 3],
) {
    const LOOP: u32 = 400;
    const FACE: u32 = 500;
    const VERTICES: [u32; 3] = [600, 601, 602];
    const ROSTERS: [u32; 3] = [800, 801, 802];
    const INCIDENCES: [u32; 3] = [700, 701, 702];
    let edges = [
        (
            300u32,
            pcurves[0],
            VERTICES[0],
            VERTICES[1],
            INCIDENCES[0],
            INCIDENCES[1],
        ),
        (
            301,
            pcurves[1],
            VERTICES[1],
            VERTICES[2],
            INCIDENCES[1],
            INCIDENCES[2],
        ),
        (
            302,
            pcurves[2],
            VERTICES[2],
            VERTICES[0],
            INCIDENCES[2],
            INCIDENCES[0],
        ),
    ];

    for (id, pcurve, start_vertex, end_vertex, start_incidence, end_incidence) in edges {
        let mut payload = vec![0x85];
        for reference in [
            pcurve,
            start_vertex,
            end_vertex,
            start_incidence,
            end_incidence,
        ] {
            payload.extend_from_slice(&b5_object_ref(reference));
        }
        payload.push(0x2a);
        append_b5_record(bytes, 0x5e, id, &payload);
    }

    let mut loop_payload = vec![0x87];
    for (pcurve, (edge, _, _, _, _, _)) in pcurves.into_iter().zip(edges) {
        loop_payload.extend_from_slice(&b5_object_ref(pcurve));
        loop_payload.extend_from_slice(&b5_object_ref(edge));
    }
    loop_payload.extend_from_slice(&b5_object_ref(surface));
    loop_payload.extend_from_slice(&[0x83, 0x05, 0x05, 0x03]);
    for _ in 0..pcurves.len() {
        loop_payload.extend_from_slice(&[0x01, 0x00, 0xff, 0xff, 0x01, 0x00]);
    }
    loop_payload.push(0x01);
    append_b5_record(bytes, 0x62, LOOP, &loop_payload);

    let mut face_payload = vec![0x82];
    face_payload.extend_from_slice(&b5_object_ref(surface));
    face_payload.extend_from_slice(&b5_object_ref(LOOP));
    face_payload.push(0x05);
    append_b5_record(bytes, 0x5f, FACE, &face_payload);

    for (vertex, roster) in VERTICES.into_iter().zip(ROSTERS) {
        let mut payload = vec![0x81];
        payload.extend_from_slice(&b5_object_ref(roster));
        payload.push(0x04);
        append_b5_record(bytes, 0x5d, vertex, &payload);
    }
    for (roster, incidence_ids) in ROSTERS.into_iter().zip([
        [INCIDENCES[0], INCIDENCES[2]],
        [INCIDENCES[1], INCIDENCES[0]],
        [INCIDENCES[2], INCIDENCES[1]],
    ]) {
        let mut payload = vec![0x82];
        for incidence in incidence_ids {
            payload.extend_from_slice(&b5_object_ref(incidence));
        }
        append_b5_record(bytes, 0x05, roster, &payload);
    }
    for ((id, curves), parameters) in INCIDENCES
        .into_iter()
        .zip([
            [pcurves[0], pcurves[2]],
            [pcurves[0], pcurves[1]],
            [pcurves[1], pcurves[2]],
        ])
        .zip([[0.0, 1.0], [1.0, 0.0], [1.0, 0.0]])
    {
        let mut payload = vec![0x82];
        for curve in curves {
            payload.extend_from_slice(&b5_object_ref(curve));
        }
        payload.push(0x82);
        for parameter in parameters {
            payload.extend_from_slice(&le_f64(parameter));
            payload.push(0x81);
        }
        append_b5_record(bytes, 0x06, id, &payload);
    }

    for point in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
}

/// Build an elided-pole freeform surface with one external grid allocation and
/// the complete native endpoint chain required to transfer its face.
pub(crate) fn a8_elided_surface_stream_with_native_vertex_chain() -> Vec<u8> {
    const SURFACE: u32 = 100;

    let mut bytes = a8_surface_stream();
    bytes.truncate(59);
    bytes[7..11].copy_from_slice(&SURFACE.to_le_bytes());
    bytes.extend_from_slice(&a8_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 11).expect("small A8 payload");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    append_b5_record(
        &mut bytes,
        0x21,
        200,
        &b5_linear_pcurve_payload(
            u16::try_from(SURFACE).expect("support surface id fits a `u16`"),
            [0.0, 0.0],
            [1.0, 0.0],
        ),
    );
    for u in 0..3 {
        for v in 0..3 {
            for coordinate in [f64::from(u) * 0.5, f64::from(v) * 0.5, 0.0] {
                bytes.extend_from_slice(&le_f64(coordinate));
            }
        }
    }
    append_b5_record(
        &mut bytes,
        0x18,
        201,
        &b5_analytic_line_pcurve_payload(
            u16::try_from(SURFACE).expect("support surface id fits a `u16`"),
            [1.0, 0.0],
            [-1.0, 1.0],
            [0.0, 1.0],
        ),
    );
    append_b5_record(
        &mut bytes,
        0x18,
        202,
        &b5_analytic_line_pcurve_payload(
            u16::try_from(SURFACE).expect("support surface id fits a `u16`"),
            [0.0, 1.0],
            [0.0, -1.0],
            [0.0, 1.0],
        ),
    );
    append_b5_closed_triangle_native_vertex_chain(&mut bytes, SURFACE, [200, 201, 202]);
    bytes
}
