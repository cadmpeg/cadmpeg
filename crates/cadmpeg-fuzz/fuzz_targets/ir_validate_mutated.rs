// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for IR validation with mutated documents.
//!
//! Takes a base IR JSON document and applies random mutations to create
//! semantically invalid but structurally valid JSON. Tests that validation
//! catches these issues without panicking.
//! Contract: no input may panic.

#![no_main]

use cadmpeg_ir::validate::validate;
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 100 {
        return;
    }

    // Split data: first byte determines mutation strategy, rest is JSON
    let strategy = data[0];
    let json_bytes = &data[1..];

    let json_str = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut ir = match CadIr::from_json(json_str) {
        Ok(ir) => ir,
        Err(_) => return,
    };

    // Apply mutations based on strategy
    match strategy % 10 {
        0 => {
            // Mutate vertex positions with NaN/infinity
            for point in &mut ir.points {
                point.position.x = f64::NAN;
                point.position.y = f64::INFINITY;
                point.position.z = f64::NEG_INFINITY;
            }
        }
        1 => {
            // Create invalid cross-references
            if !ir.vertices.is_empty() {
                ir.vertices[0].point = cadmpeg_ir::ids::PointId("nonexistent".to_string());
            }
        }
        2 => {
            // Break coedge ring topology
            if ir.coedges.len() >= 2 {
                ir.coedges[0].next = ir.coedges[1].id.clone();
                ir.coedges[1].previous = ir.coedges[0].id.clone();
            }
        }
        3 => {
            // Create inconsistent edge references
            if !ir.edges.is_empty() && ir.vertices.is_empty() {
                ir.edges[0].start = cadmpeg_ir::ids::VertexId("nonexistent".to_string());
            }
        }
        4 => {
            // Mutate surface geometry with degenerate values
            for surface in &mut ir.surfaces {
                if let cadmpeg_ir::geometry::SurfaceGeometry::Plane { normal, .. } =
                    &mut surface.geometry
                {
                    normal.x = 0.0;
                    normal.y = 0.0;
                    normal.z = 0.0;
                }
            }
        }
        5 => {
            // Create empty body (no lumps)
            if !ir.bodies.is_empty() {
                ir.bodies[0].lumps.clear();
            }
        }
        6 => {
            // Create face with no loops
            if !ir.faces.is_empty() {
                ir.faces[0].loops.clear();
            }
        }
        7 => {
            // Create loop with no coedges
            if !ir.loops.is_empty() {
                ir.loops[0].coedges.clear();
            }
        }
        8 => {
            // Mutate tolerances to invalid values
            ir.tolerances.resabs = -1.0;
            ir.tolerances.resnor = f64::NAN;
        }
        9 => {
            // Clear all geometry but keep topology
            ir.points.clear();
            ir.curves.clear();
            ir.surfaces.clear();
        }
        _ => {}
    }

    // Validate - should catch issues without panicking
    let _ = validate(&ir, Vec::new());
});
