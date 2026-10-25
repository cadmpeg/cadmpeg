// SPDX-License-Identifier: Apache-2.0

use super::test_bytes::{put_f64, put_ref, put_vec3, record};

pub(crate) fn primary_partition() -> Vec<u8> {
    let mut partition = b"PS".to_vec();
    for text in [
        b": TRANSMIT FILE (partition) created by blend history fixture".as_slice(),
        b"SCH_TEST_13006".as_slice(),
    ] {
        partition.extend_from_slice(&u32::try_from(text.len()).unwrap().to_be_bytes());
        partition.extend_from_slice(text);
    }
    partition.extend_from_slice(&0_u32.to_be_bytes());
    let mut body = record(12, 61);
    put_ref(&mut body, 2, 2);
    body[4..8].copy_from_slice(&9000_u32.to_be_bytes());
    put_f64(&mut body, 20, 1e-07);
    put_f64(&mut body, 28, 1e-07);
    for (at, reference) in [
        (8, 1),
        (10, 1),
        (12, 12),
        (14, 9),
        (16, 11),
        (18, 1),
        (36, 1),
        (38, 1),
        (40, 1),
        (43, 1),
        (47, 3),
        (49, 1),
        (51, 1),
        (53, 1),
        (55, 13),
        (57, 8),
        (59, 10),
    ] {
        put_ref(&mut body, at, reference);
    }
    partition.extend(body);
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    shell[4..8].copy_from_slice(&3_u32.to_be_bytes());
    for (at, reference) in [
        (8, 1),
        (10, 2),
        (12, 1),
        (14, 4),
        (16, 1),
        (18, 1),
        (20, 13),
        (22, 1),
    ] {
        put_ref(&mut shell, at, reference);
    }
    partition.extend(shell);
    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    face[4..8].copy_from_slice(&4_u32.to_be_bytes());
    put_f64(&mut face, 10, 0.0002);
    face[28] = b'+';
    for (at, reference) in [
        (8, 710),
        (18, 1),
        (20, 1),
        (22, 5),
        (24, 3),
        (26, 12),
        (29, 1),
        (31, 1),
        (33, 1),
        (35, 1),
        (37, 1),
    ] {
        put_ref(&mut face, at, reference);
    }
    partition.extend(face);
    let mut loop_record = record(15, 16);
    put_ref(&mut loop_record, 2, 5);
    loop_record[4..8].copy_from_slice(&5_u32.to_be_bytes());
    for (at, reference) in [(8, 1), (10, 7), (12, 4), (14, 1)] {
        put_ref(&mut loop_record, at, reference);
    }
    partition.extend(loop_record);
    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 7);
    fin[22] = b'+';
    for (at, reference) in [
        (4, 1),
        (6, 5),
        (8, 7),
        (10, 7),
        (12, 10),
        (14, 1),
        (16, 8),
        (18, 9),
        (20, 1),
    ] {
        put_ref(&mut fin, at, reference);
    }
    partition.extend(fin);
    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 8);
    edge[4..8].copy_from_slice(&8_u32.to_be_bytes());
    put_f64(&mut edge, 10, 0.0003);
    for (at, reference) in [
        (8, 1),
        (18, 7),
        (20, 1),
        (22, 1),
        (24, 9),
        (26, 1),
        (28, 1),
        (30, 1),
    ] {
        put_ref(&mut edge, at, reference);
    }
    partition.extend(edge);
    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 6);
    plane[4..8].copy_from_slice(&6_u32.to_be_bytes());
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    for (at, reference) in [(8, 1), (10, 1), (12, 1), (14, 1), (16, 1)] {
        put_ref(&mut plane, at, reference);
    }
    partition.extend(plane);
    let mut line = record(30, 67);
    put_ref(&mut line, 2, 9);
    line[4..8].copy_from_slice(&9_u32.to_be_bytes());
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
    for (at, reference) in [(8, 1), (10, 1), (12, 1), (14, 1), (16, 1)] {
        put_ref(&mut line, at, reference);
    }
    partition.extend(line);
    let mut vertex = record(18, 28);
    put_ref(&mut vertex, 2, 10);
    vertex[4..8].copy_from_slice(&10_u32.to_be_bytes());
    put_f64(&mut vertex, 18, 0.0001);
    for (at, reference) in [(8, 1), (10, 1), (12, 1), (14, 1), (16, 11), (26, 1)] {
        put_ref(&mut vertex, at, reference);
    }
    partition.extend(vertex);
    let mut region = record(19, 19);
    put_ref(&mut region, 2, 13);
    region[4..8].copy_from_slice(&13_u32.to_be_bytes());
    region[18] = b'+';
    for (at, reference) in [(8, 1), (10, 2), (12, 1), (14, 1), (16, 3)] {
        put_ref(&mut region, at, reference);
    }
    partition.extend(region);
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 11);
    point[4..8].copy_from_slice(&11_u32.to_be_bytes());
    put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
    for (at, reference) in [(8, 1), (10, 1), (12, 1), (14, 1)] {
        put_ref(&mut point, at, reference);
    }
    partition.extend(point);
    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[4..8].copy_from_slice(&12_u32.to_be_bytes());
    blend[18] = b'+';
    blend[19] = b'R';
    put_f64(&mut blend, 26, -0.003);
    put_f64(&mut blend, 34, 0.003);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for (at, reference) in [
        (8, 1),
        (10, 1),
        (12, 1),
        (14, 1),
        (16, 1),
        (20, 6),
        (22, 6),
        (24, 1),
        (58, 1),
        (60, 1),
        (62, 1),
        (64, 1),
    ] {
        put_ref(&mut blend, at, reference);
    }
    partition.extend(blend);
    partition
}
