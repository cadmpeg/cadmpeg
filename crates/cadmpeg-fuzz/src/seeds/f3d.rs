// SPDX-License-Identifier: Apache-2.0
//! Autodesk ASM binary (SMBH) seed builders.

pub const SEED_LINEAR_TOLERANCE: f64 = 1.0e-6;
pub const SEED_ANGULAR_TOLERANCE: f64 = 1.0e-10;

pub fn push_u8_string(b: &mut Vec<u8>, s: &str) {
    b.push(0x07);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}

pub fn push_tagged_f64(b: &mut Vec<u8>, v: f64) {
    b.push(0x06);
    b.extend_from_slice(&v.to_le_bytes());
}

pub fn smbh_header_prefix() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8<");
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&7u64.to_be_bytes());
    b.extend_from_slice(&3u64.to_be_bytes());
    b.extend_from_slice(&[0u8; 7]);
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    push_tagged_f64(&mut b, 60.0);
    push_tagged_f64(&mut b, SEED_LINEAR_TOLERANCE);
    push_tagged_f64(&mut b, SEED_ANGULAR_TOLERANCE);
    b
}

pub fn t_ref(b: &mut Vec<u8>, v: i64) {
    b.push(0x0c);
    b.extend_from_slice(&v.to_le_bytes());
}

pub fn t_long(b: &mut Vec<u8>, v: i64) {
    b.push(0x04);
    b.extend_from_slice(&v.to_le_bytes());
}

pub fn t_pos(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x13);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}

pub fn t_vec(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x14);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}

pub fn t_ident(b: &mut Vec<u8>, s: &str) {
    b.push(0x0d);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}

pub fn t_subident(b: &mut Vec<u8>, s: &str) {
    b.push(0x0e);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}

pub fn t_end(b: &mut Vec<u8>) {
    b.push(0x11);
}

pub fn synthetic_mixed_smbh() -> Vec<u8> {
    let mut r = Vec::new();
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    t_ident(&mut r, "body");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 2);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_end(&mut r);

    t_ident(&mut r, "region");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 3);
    t_ref(&mut r, 1);
    t_end(&mut r);

    t_ident(&mut r, "shell");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 4);
    t_ref(&mut r, -1);
    t_ref(&mut r, 2);
    t_end(&mut r);

    let face = |r: &mut Vec<u8>, next: i64, first_loop: i64, surface: i64| {
        t_ident(r, "face");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, next);
        t_ref(r, first_loop);
        t_ref(r, 3);
        t_ref(r, -1);
        t_ref(r, surface);
        r.push(0x0b);
        r.push(0x0b);
        t_end(r);
    };
    face(&mut r, 5, 6, 8);
    face(&mut r, -1, 7, 9);

    let lp = |r: &mut Vec<u8>, first_coedge: i64, owner_face: i64| {
        t_ident(r, "loop");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, -1);
        t_ref(r, first_coedge);
        t_ref(r, owner_face);
        t_end(r);
    };
    lp(&mut r, 10, 4);
    lp(&mut r, 13, 5);

    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_pos(&mut r, [0.0, 0.0, 0.0]);
    t_vec(&mut r, [0.0, 0.0, 1.0]);
    t_pos(&mut r, [1.0, 0.0, 0.0]);
    r.push(0x0b);
    t_end(&mut r);

    t_subident(&mut r, "spline");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    push_tagged_f64(&mut r, 0.0);
    r.push(0x0b);
    t_end(&mut r);

    let ce =
        |r: &mut Vec<u8>, next: i64, prev: i64, partner: i64, edge: i64, rev: bool, owner: i64| {
            t_ident(r, "coedge");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, next);
            t_ref(r, prev);
            t_ref(r, partner);
            t_ref(r, edge);
            r.push(if rev { 0x0a } else { 0x0b });
            t_ref(r, owner);
            t_long(r, 0);
            t_ref(r, -1);
            t_end(r);
        };
    ce(&mut r, 11, 12, 13, 16, false, 6);
    ce(&mut r, 12, 10, -1, 17, false, 6);
    ce(&mut r, 10, 11, -1, 18, false, 6);
    ce(&mut r, 14, 15, 10, 16, true, 7);
    ce(&mut r, 15, 13, -1, 19, false, 7);
    ce(&mut r, 13, 14, -1, 20, false, 7);

    let edge = |r: &mut Vec<u8>, start: i64, end: i64| {
        t_ident(r, "edge");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, start);
        push_tagged_f64(r, 0.0);
        t_ref(r, end);
        push_tagged_f64(r, 1.0);
        t_ref(r, -1);
        t_ref(r, -1);
        r.push(0x0b);
        push_u8_string(r, "unknown");
        t_end(r);
    };
    edge(&mut r, 21, 22);
    edge(&mut r, 22, 23);
    edge(&mut r, 23, 21);
    edge(&mut r, 21, 24);
    edge(&mut r, 24, 22);

    let vert = |r: &mut Vec<u8>, owning_edge: i64, point: i64| {
        t_ident(r, "vertex");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, owning_edge);
        t_long(r, 0);
        t_ref(r, point);
        t_end(r);
    };
    vert(&mut r, 16, 25);
    vert(&mut r, 16, 26);
    vert(&mut r, 17, 27);
    vert(&mut r, 19, 28);

    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_long(&mut r, 1);
        t_end(&mut r);
    }

    t_ident(&mut r, "delta_state");
    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}
