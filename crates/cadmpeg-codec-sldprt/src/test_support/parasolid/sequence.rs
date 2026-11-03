// SPDX-License-Identifier: Apache-2.0
//! A complete base-schema node sequence with one node of each topology type.

/// One field value in base-schema order.
#[derive(Clone)]
pub(crate) enum F {
    I(u32),
    P(u16),
    R(f64),
    C(u8),
    B(u8),
    V([f64; 3]),
    /// A short.
    S(u16),
    /// A logical.
    L(bool),
    /// A variable real field; its count precedes the node index.
    Reals(Vec<f64>),
    /// A variable short field; its count precedes the node index.
    Shorts(Vec<u16>),
}

/// The null-real sentinel.
pub(crate) const NULL_REAL: f64 = -31_415_800_000_000.0;

fn node(out: &mut Vec<u8>, kind: u16, index: u16, fields: &[F]) {
    node_with_program(out, kind, &[0xff], index, fields);
}

/// A node whose first-use schema information is `program`: `ff` for the
/// base field list, or a field count and an edit program.
fn node_with_program(out: &mut Vec<u8>, kind: u16, program: &[u8], index: u16, fields: &[F]) {
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(program);
    let count = fields.iter().find_map(|field| match field {
        F::Reals(values) => Some(values.len()),
        F::Shorts(values) => Some(values.len()),
        _ => None,
    });
    if let Some(count) = count {
        out.extend_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
    }
    out.extend_from_slice(&index.to_be_bytes());
    for field in fields {
        match field {
            F::I(value) => out.extend_from_slice(&value.to_be_bytes()),
            F::P(value) | F::S(value) => out.extend_from_slice(&value.to_be_bytes()),
            F::R(value) => out.extend_from_slice(&value.to_be_bytes()),
            F::C(value) | F::B(value) => out.push(*value),
            F::L(value) => out.push(u8::from(*value)),
            F::V(values) => values
                .iter()
                .for_each(|value| out.extend_from_slice(&value.to_be_bytes())),
            F::Reals(values) => values
                .iter()
                .for_each(|value| out.extend_from_slice(&value.to_be_bytes())),
            F::Shorts(values) => values
                .iter()
                .for_each(|value| out.extend_from_slice(&value.to_be_bytes())),
        }
    }
}

/// A partition stream holding BODY 2, REGION 3, SHELL 4, FACE 5, LOOP 6,
/// FIN 7, EDGE 8, VERTEX 9, and POINT 10, and the offset where its nodes start.
pub(crate) fn topology_sequence() -> (Vec<u8>, usize) {
    topology_sequence_with_face(None)
}

/// The base-schema FACE 5 fields.
pub(crate) fn face_fields() -> Vec<F> {
    use F::{C, I, P, R};
    #[rustfmt::skip]
    let fields = vec![
        I(4), P(1), R(NULL_REAL), P(1), P(1), P(6), P(4), P(20), C(b'+'),
        P(1), P(1), P(1), P(1), P(1),
    ];
    fields
}

/// `topology_sequence` with FACE 5 written as `(program, fields)`.
/// A partition transmit header with an embedded 13006 schema.
fn header() -> Vec<u8> {
    let description = b": TRANSMIT FILE (partition) node sequence";
    let schema = b"SCH_999_999_13006";
    let mut out = b"PS".to_vec();
    out.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    out.extend_from_slice(description);
    out.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    out.extend_from_slice(schema);
    out.extend_from_slice(&250u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out
}

/// A partition stream of `nodes`, each `(kind, index, fields)`, and the
/// offset where its nodes start. The first node of each type carries base
/// first-use information.
pub(crate) fn sequence_of(nodes: &[(u16, u16, Vec<F>)]) -> (Vec<u8>, usize) {
    let mut out = header();
    let body_offset = out.len();
    let mut kinds = std::collections::HashSet::new();
    for (kind, index, fields) in nodes {
        let program: &[u8] = if kinds.insert(*kind) { &[0xff] } else { &[] };
        node_with_program(&mut out, *kind, program, *index, fields);
    }
    out.extend_from_slice(&[0, 1, 0, 1]);
    (out, body_offset)
}

pub(crate) fn topology_sequence_with_face(face: Option<(&[u8], &[F])>) -> (Vec<u8>, usize) {
    use F::{B, C, I, P, R, V};
    let mut out = header();
    let body_offset = out.len();
    #[rustfmt::skip]
    node(&mut out, 12, 2, &[
        I(10), P(1), P(1), P(1), P(1), P(1), P(1), R(1000.0), R(1.0e-8), P(1), P(1), P(1),
        B(1), P(1), B(1), B(0), P(4), P(1), P(1), P(1), P(3), P(1), P(1),
    ]);
    node(
        &mut out,
        19,
        3,
        &[I(2), P(1), P(2), P(1), P(1), P(4), C(b'S')],
    );
    node(
        &mut out,
        13,
        4,
        &[I(3), P(1), P(2), P(1), P(5), P(1), P(1), P(3), P(1)],
    );
    let base_face = face_fields();
    let (program, fields) = face.unwrap_or((&[0xff], &base_face));
    node_with_program(&mut out, 14, program, 5, fields);
    node(&mut out, 15, 6, &[I(5), P(1), P(7), P(5), P(1)]);
    #[rustfmt::skip]
    node(&mut out, 17, 7, &[
        P(1), P(6), P(7), P(7), P(9), P(1), P(8), P(1), P(1), C(b'-'),
    ]);
    #[rustfmt::skip]
    node(&mut out, 16, 8, &[
        I(6), P(1), R(NULL_REAL), P(7), P(1), P(1), P(21), P(1), P(1), P(2),
    ]);
    node(
        &mut out,
        18,
        9,
        &[I(7), P(1), P(7), P(1), P(1), P(10), R(2.0e-8), P(2)],
    );
    node(
        &mut out,
        29,
        10,
        &[I(8), P(1), P(9), P(1), P(1), V([0.001, 0.002, 0.003])],
    );
    out.extend_from_slice(&[0, 1, 0, 1]);
    (out, body_offset)
}
