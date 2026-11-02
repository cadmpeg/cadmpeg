// SPDX-License-Identifier: Apache-2.0

pub(crate) fn pointer(out: &mut Vec<u8>, value: u32) {
    if value <= 0x7fff {
        out.extend(u16::try_from(value).unwrap().to_be_bytes());
    } else {
        out.extend((0x8000 | u16::try_from(value & 0x7fff).unwrap()).to_be_bytes());
        out.extend(u16::try_from(value >> 15).unwrap().to_be_bytes());
    }
}

fn node(out: &mut Vec<u8>, tag: u8, attr: u32, sequence: bool, first: bool) {
    out.extend([0, tag]);
    if first {
        out.push(0xff);
    }
    pointer(out, attr);
    if sequence {
        out.extend(attr.to_be_bytes());
    }
}

fn refs(out: &mut Vec<u8>, base: u32, values: &[u32]) {
    for value in values {
        pointer(out, if *value > 1 { base + value } else { *value });
    }
}

pub(crate) fn triangle(base: u32) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x32, base + 100, true, true);
    refs(&mut out, base, &[0; 5]);
    out.push(b'+');
    for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0] {
        out.extend(value.to_be_bytes());
    }
    node(&mut out, 0x0e, base + 10, true, true);
    refs(&mut out, base, &[1]);
    out.extend(super::MAGIC);
    refs(&mut out, base, &[1, 1, 20, 901, 100]);
    out.push(b'+');
    refs(&mut out, base, &[1, 1, 1, 1, 901]);
    node(&mut out, 0x0f, base + 20, true, true);
    refs(&mut out, base, &[1, 30, 10, 1]);
    for (fin, next, previous, vertex, edge) in [
        (30, 31, 32, 51, 40),
        (31, 32, 30, 52, 41),
        (32, 30, 31, 50, 42),
    ] {
        node(&mut out, 0x11, base + fin, false, fin == 30);
        refs(
            &mut out,
            base,
            &[1, 20, next, previous, vertex, 1, edge, 1, 1],
        );
        out.push(b'+');
        node(&mut out, 0x10, base + edge, true, edge == 40);
        refs(&mut out, base, &[1]);
        out.extend(super::MAGIC);
        refs(&mut out, base, &[fin, 1, 1, 0, 1, 1, 900]);
    }
    for (vertex, point, xyz) in [
        (50, 60, [0.0_f64, 0.0, 0.0]),
        (51, 61, [0.001, 0.0, 0.0]),
        (52, 62, [0.0, 0.001, 0.0]),
    ] {
        node(&mut out, 0x12, base + vertex, true, vertex == 50);
        refs(&mut out, base, &[1, 1, 1, 1, point]);
        out.extend(super::MAGIC);
        refs(&mut out, base, &[900]);
        node(&mut out, 0x1d, base + point, true, point == 60);
        refs(&mut out, base, &[1; 4]);
        for value in xyz {
            out.extend(value.to_be_bytes());
        }
    }
    node(&mut out, 0x0c, base + 900, true, true);
    refs(&mut out, base, &[1; 6]);
    out.extend(1000.0_f64.to_be_bytes());
    out.extend(1.0e-8_f64.to_be_bytes());
    refs(&mut out, base, &[1; 3]);
    out.push(1);
    refs(&mut out, base, &[1]);
    out.extend([3, 1]);
    refs(&mut out, base, &[901, 1, 1, 1, 1, 1, 1, 902, 1, 1, 1]);
    node(&mut out, 0x0d, base + 901, true, true);
    refs(&mut out, base, &[1, 900, 1, 1, 1, 1, 902, 1]);
    node(&mut out, 0x13, base + 902, true, true);
    refs(&mut out, base, &[1, 900, 1, 1, 901]);
    out.push(b'S');
    out
}

pub(crate) fn edge(base: u32, owner: u32) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x10, base + 40, true, false);
    pointer(&mut out, 1);
    out.extend(super::MAGIC);
    for reference in [base + 30, 1, 1, base + 100, 1, 1, owner] {
        pointer(&mut out, reference);
    }
    out
}

pub(crate) fn integer_values(key: u32, values: &[u32]) -> Vec<u8> {
    let mut out = vec![0, 0x52];
    out.extend(u32::try_from(values.len()).unwrap().to_be_bytes());
    pointer(&mut out, key);
    for value in values {
        out.extend(value.to_be_bytes());
    }
    out
}

pub(crate) fn attribute(key: u32, definition: u32, slots: &[u32]) -> Vec<u8> {
    let mut out = vec![0, 0x51];
    out.extend(u32::try_from(slots.len() - 5).unwrap().to_be_bytes());
    pointer(&mut out, key);
    out.extend(1_u32.to_be_bytes());
    pointer(&mut out, definition);
    for slot in slots {
        pointer(&mut out, *slot);
    }
    out
}

pub(crate) fn fin(key: u32, references: &[u32; 9]) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x11, key, false, false);
    for reference in references {
        pointer(&mut out, *reference);
    }
    out.push(b'+');
    out
}

pub(crate) fn bounded_curve(key: u32, source: u32, values: &[f64; 8]) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x85, key, true, false);
    for reference in [1, source, key, 0x7fff, 0x1_0000] {
        pointer(&mut out, reference);
    }
    out.push(b'+');
    pointer(&mut out, source);
    for value in values {
        out.extend(value.to_be_bytes());
    }
    out
}

pub(crate) fn line(key: u32, node_id: u32, origin: [f64; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x1e, key, false, false);
    out.extend(node_id.to_be_bytes());
    for reference in [1, 512, 512, 512, 512] {
        pointer(&mut out, reference);
    }
    out.push(b'+');
    for value in origin.into_iter().chain([1.0, 0.0, 0.0]) {
        out.extend(value.to_be_bytes());
    }
    out
}

pub(crate) fn sp_curve(key: u32, surface: u32, b_curve: u32, first: bool) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x89, key, true, first);
    for reference in [1, 0x8000, 0x1_0000, 1, 1] {
        pointer(&mut out, reference);
    }
    out.push(b'+');
    for reference in [surface, b_curve, 1] {
        pointer(&mut out, reference);
    }
    out.extend(super::MAGIC);
    out
}

pub(crate) fn string_node(tag: u8, key: u32, text: &str) -> Vec<u8> {
    let mut out = vec![0, tag];
    out.extend(u32::try_from(text.len()).unwrap().to_be_bytes());
    pointer(&mut out, key);
    out.extend(text.as_bytes());
    out
}

pub(crate) fn curve_wrapper(tag: u8, key: u32, node_id: u32, descriptor: u32) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, tag, key, false, false);
    out.extend(node_id.to_be_bytes());
    for reference in [1, 0x1_0000, 1, 0x7fff, 1] {
        pointer(&mut out, reference);
    }
    out.push(b'+');
    pointer(&mut out, descriptor);
    out
}

pub(crate) fn curve_descriptor(key: u32, arrays: [u32; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    node(&mut out, 0x88, key, false, false);
    out.extend(1_u16.to_be_bytes());
    out.extend(2_u32.to_be_bytes());
    out.extend(3_u16.to_be_bytes());
    out.extend(2_u32.to_be_bytes());
    out.extend([0; 5]);
    for reference in arrays {
        pointer(&mut out, reference);
    }
    out
}

pub(crate) fn float_values(key: u32, values: &[f64]) -> Vec<u8> {
    let mut out = vec![0, 0x2d];
    out.extend(u32::try_from(values.len()).unwrap().to_be_bytes());
    pointer(&mut out, key);
    for value in values {
        out.extend(value.to_be_bytes());
    }
    out
}
