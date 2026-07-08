// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for STEP writer with degenerate IR geometry.
//!
//! Constructs CadIr documents with NaN, infinity, zero-length vectors,
//! and other degenerate geometry, then exports to STEP.
//! Contract: no input may panic.

#![no_main]

use std::io::Cursor;

use cadmpeg_ir::{CadIr, Part, Body, Face, Loop_, Edge, Vertex, SurfaceGeometry, CurveGeometry, Point3, Vector3};
use cadmpeg_step::{write_step, StepWriteOptions};
use libfuzzer_sys::fuzz_target;

fn arbitrary_f64(data: &[u8], offset: usize) -> f64 {
    if offset + 8 > data.len() {
        return 0.0;
    }
    let bytes: [u8; 8] = data[offset..offset + 8].try_into().unwrap_or([0; 8]);
    let val = f64::from_le_bytes(bytes);
    match data.get(offset).copied().unwrap_or(0) % 5 {
        0 => val,
        1 => f64::NAN,
        2 => f64::INFINITY,
        3 => f64::NEG_INFINITY,
        4 => 0.0,
        _ => val,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 256 {
        return;
    }

    let mut offset = 0;
    let consume_f64 = |data: &[u8], offset: &mut usize| -> f64 {
        let val = arbitrary_f64(data, *offset);
        *offset += 8;
        val
    };

    let origin = Point3::new(
        consume_f64(data, &mut offset),
        consume_f64(data, &mut offset),
        consume_f64(data, &mut offset),
    );
    let normal = Vector3::new(
        consume_f64(data, &mut offset),
        consume_f64(data, &mut offset),
        consume_f64(data, &mut offset),
    );
    let axis = Vector3::new(
        consume_f64(data, &mut offset),
        consume_f64(data, &mut offset),
        consume_f64(data, &mut offset),
    );
    let radius = consume_f64(data, &mut offset);

    let surface = SurfaceGeometry::Plane { origin, normal };
    let curve = CurveGeometry::Line { origin, direction: axis };

    let vertex = Vertex {
        point: origin,
        tolerance: 1e-6,
    };

    let edge = Edge {
        curve: curve,
        start: 0,
        end: 0,
        reversed: false,
    };

    let loop_ = Loop_ {
        edges: vec![edge],
    };

    let face = Face {
        surface: surface,
        loops: vec![loop_],
        reversed: false,
    };

    let body = Body {
        faces: vec![face],
        closed: true,
    };

    let part = Part {
        name: "test".to_string(),
        bodies: vec![body],
    };

    let ir = CadIr {
        version: "1.0".to_string(),
        parts: vec![part],
    };

    let mut out = Cursor::new(Vec::new());
    let _ = write_step(&ir, &mut out, &StepWriteOptions::default());
});
