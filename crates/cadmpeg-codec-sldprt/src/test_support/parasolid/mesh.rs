// SPDX-License-Identifier: Apache-2.0
//! Synthetic mesh-support ownership graphs.

use super::sequence::{node_with_program, sequence_of, F, NULL_REAL};

pub(crate) fn mesh_body_sequence(body_count: u16, faces_per_body: u16) -> Vec<u8> {
    use F::{B, C, I, P, R};
    let mut nodes = Vec::new();
    for body in 0..body_count {
        let base = body * 40;
        nodes.push((
            12,
            base + 2,
            vec![
                I(30),
                P(1),
                P(1),
                P(1),
                P(1),
                P(1),
                P(1),
                R(1000.0),
                R(1.0e-8),
                P(1),
                P(1),
                P(1),
                B(1),
                P(1),
                B(1),
                B(0),
                P(base + 4),
                P(1),
                P(1),
                P(1),
                P(base + 3),
                P(1),
                P(1),
            ],
        ));
        nodes.push((
            13,
            base + 4,
            vec![
                I(3),
                P(1),
                P(base + 2),
                P(1),
                P(base + 5),
                P(1),
                P(1),
                P(base + 3),
                P(1),
            ],
        ));
        for face in 0..faces_per_body {
            nodes.push((
                14,
                base + 5 + face,
                vec![
                    I(u32::from(4 + face)),
                    P(1),
                    R(NULL_REAL),
                    P(1),
                    P(1),
                    P(1),
                    P(base + 4),
                    P(base + 20 + face),
                    C(b'+'),
                    P(1),
                    P(1),
                    P(1),
                    P(1),
                    P(1),
                ],
            ));
        }
    }
    let (mut stream, _) = sequence_of(&nodes);
    stream.truncate(stream.len() - 4);
    let mut program = vec![13, 4];
    program.extend(b"MESH");
    program.push(4);
    program.extend(b"Mesh");
    for (name, pointer, data_type) in [
        ("node_id", 0u16, b'd'),
        ("attributes_features", 1019, 0),
        ("owner", 1007, 0),
        ("next", 1006, 0),
        ("previous", 1006, 0),
        ("geometric_owner", 141, 0),
        ("sense", 0, b'c'),
        ("mesh_box", 0, b'b'),
        ("transform", 100, 0),
        ("rcv_key", 1041, 0),
        ("rcv_index", 0, b'd'),
        ("psm_imesh", 189, 0),
        ("pff_imesh", 188, 0),
    ] {
        program.push(u8::try_from(name.len()).unwrap());
        program.extend(name.as_bytes());
        program.extend(pointer.to_be_bytes());
        program.extend(1u16.to_be_bytes());
        if pointer == 0 {
            program.extend([1, data_type]);
        }
    }
    let mut first = true;
    for body in 0..body_count {
        for face in 0..faces_per_body {
            let base = body * 40;
            node_with_program(
                &mut stream,
                201,
                if first { &program } else { &[] },
                base + 20 + face,
                &[
                    I(u32::from(20 + face)),
                    P(1),
                    P(base + 5 + face),
                    P(1),
                    P(1),
                    P(1),
                    C(b'+'),
                    R(0.0),
                    R(1.0),
                    R(0.0),
                    R(1.0),
                    R(0.0),
                    R(1.0),
                    P(1),
                    P(1),
                    I(1),
                    P(1),
                    P(1),
                ],
            );
            first = false;
        }
    }
    stream.extend([0, 1, 0, 1]);
    stream
}
