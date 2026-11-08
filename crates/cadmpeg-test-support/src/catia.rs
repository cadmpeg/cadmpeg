// SPDX-License-Identifier: Apache-2.0
//! Generated native records for two planar faces with one shared edge.

/// Build a nested CATPart with two planar faces and one source-shared edge.
/// The roster control and second loop tail allow targeted rejection tests.
#[must_use]
pub fn joined_planar_sheet(roster_control: u8, second_loop_tail: u8) -> Vec<u8> {
    const FIRST_FACE: u8 = 1;
    const FIRST_LOOP: u8 = 3;
    const PLANE: u8 = 5;
    const FIRST_PCURVE: u8 = 6;
    const FIRST_EDGE: u8 = 13;
    const FIRST_VERTEX: u8 = 20;

    fn frame(bytes: &mut Vec<u8>, flag: u8, class: u8, payload: &[u8]) {
        let size = u8::try_from(payload.len()).expect("small fixture payload");
        bytes.extend([0xb2, flag, class, size, 0x05]);
        bytes.extend(payload);
    }
    let child = |from, to| 4 * (to - from - 1) + 3;
    // Include a standalone profile to select the freeform route.
    let mut bytes = Vec::new();
    let mut profile = Vec::new();
    for value in [1.0_f64, 2.0, 3.0, 0.0, 0.6, 0.8, 1.0, -4.0, 9.0] {
        profile.extend(value.to_le_bytes());
    }
    frame(&mut bytes, 0x03, 0x0e, &profile);
    let mut group = vec![0x81, 0x03, 0x05, 0x01];
    for value in [0.0_f32, 2.0, 0.0, 1.0, 0.0, 0.0] {
        group.extend(value.to_le_bytes());
    }
    group.extend([0x08, 0xd6, 0x28]);
    frame(&mut bytes, 0x13, 0x5b, &group);
    frame(&mut bytes, 0x03, 0x61, &[0x82, 3, 7, 0, roster_control, 3]);
    for i in 0..2 {
        let face = FIRST_FACE + i;
        frame(
            &mut bytes,
            0x03,
            0x5f,
            &[0x82, child(face, PLANE), child(face, FIRST_LOOP + i), 3],
        );
    }
    for (index, edges) in [[0, 1, 2, 3], [4, 5, 6, 1]].into_iter().enumerate() {
        let slot = FIRST_LOOP + u8::try_from(index).expect("fixture loop index");
        let mut ring = vec![0x89];
        for edge in edges {
            ring.extend([
                child(slot, FIRST_PCURVE + edge),
                child(slot, FIRST_EDGE + edge),
            ]);
        }
        ring.push(child(slot, PLANE));
        ring.extend([
            0x84,
            0x41,
            0,
            0,
            if index == 0 { 1 } else { second_loop_tail },
        ]);
        frame(&mut bytes, 0x03, 0x62, &ring);
    }

    let mut plane = vec![0x84, 0];
    for value in [
        0.0_f64, 0.0, 0.0, // Origin.
        1.0, 0.0, 0.0, // U direction.
        0.0, 1.0, 0.0, // V direction.
        0.0, 2.0, 0.0, 1.0, // Chart bounds.
    ] {
        plane.extend(value.to_le_bytes());
    }
    frame(&mut bytes, 0x03, 0x27, &plane);

    let vertices = [
        [0.0_f64, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [2.0, 0.0],
        [2.0, 1.0],
    ];
    let edges = [[0_u8, 1], [1, 2], [3, 2], [0, 3], [1, 4], [4, 5], [2, 5]];
    for [start, end] in edges {
        let [x0, y0] = vertices[usize::from(start)];
        let [x1, y1] = vertices[usize::from(end)];
        let (control, values) = if x0 == x1 {
            (1, [x0, y0, y1])
        } else {
            (2, [y0, x0, x1])
        };
        let mut payload = vec![5, control];
        for value in values {
            payload.extend(value.to_le_bytes());
        }
        frame(&mut bytes, 0x03, 0x18, &payload);
    }
    for (index, [start, end]) in edges.into_iter().enumerate() {
        let slot = FIRST_EDGE + u8::try_from(index).expect("fixture edge index");
        frame(
            &mut bytes,
            0x03,
            0x5e,
            &[
                1,
                child(slot, FIRST_VERTEX + start),
                child(slot, FIRST_VERTEX + end),
                1,
                1,
                1,
            ],
        );
    }
    for _ in vertices {
        frame(&mut bytes, 0x03, 0x5d, &[1]);
    }
    wrap_main_stream(&bytes)
}

fn wrap_main_stream(main: &[u8]) -> Vec<u8> {
    const MAGIC: &[u8; 8] = b"V5_CFV2\0";
    const HEADER_LEN: u32 = 16;
    let main_len = u32::try_from(main.len()).expect("small main stream");
    let directory_offset = HEADER_LEN + main_len;
    let mut directory = b"CATIA_V5 CB0001\0".to_vec();
    for (name, offset, length) in [
        ("MainDataStream", HEADER_LEN, main_len),
        ("SurfacicReps", directory_offset, 0),
    ] {
        let mut descriptor = [0; 0x54];
        descriptor[0x0c..0x10].copy_from_slice(&length.to_be_bytes());
        for (slot, unit) in descriptor[0x10..0x50]
            .chunks_exact_mut(2)
            .zip(name.encode_utf16())
        {
            slot.copy_from_slice(&unit.to_le_bytes());
        }
        descriptor[0x50..0x54].copy_from_slice(&1_u32.to_be_bytes());
        directory.extend(descriptor);
        for field in [offset, length, length, 0, 0] {
            directory.extend(field.to_be_bytes());
        }
    }
    directory.extend(b"CB__END");
    let directory_len = u32::try_from(directory.len()).expect("small directory");
    let outer_end = HEADER_LEN + directory_offset + directory_len;
    let mut file = MAGIC.to_vec();
    file.extend(outer_end.to_be_bytes());
    file.extend(0_u32.to_be_bytes());
    file.extend(MAGIC);
    file.extend(directory_offset.to_be_bytes());
    file.extend(directory_len.to_be_bytes());
    file.extend(main);
    file.extend(directory);
    file
}
