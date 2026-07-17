// SPDX-License-Identifier: Apache-2.0
//! Parses IR JSON, applies one of 15 deterministic semantic mutations selected
//! by the first input byte, and validates the result. Validation findings are
//! expected; panics are failures.

#![no_main]

use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 100 {
        return;
    }

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
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();

    match strategy % 15 {
        0 => {
            // Mutate vertex positions with NaN/infinity
            for point in &mut ir.model.points {
                point.position.x = f64::NAN;
                point.position.y = f64::INFINITY;
                point.position.z = f64::NEG_INFINITY;
            }
        }
        1 => {
            // Create invalid cross-references
            if !ir.model.vertices.is_empty() {
                ir.model.vertices[0].point = cadmpeg_ir::ids::PointId("nonexistent".to_string());
            }
        }
        2 => {
            // Break coedge ring topology
            if ir.model.coedges.len() >= 2 {
                ir.model.coedges[0].next = ir.model.coedges[1].id.clone();
                ir.model.coedges[1].previous = ir.model.coedges[0].id.clone();
            }
        }
        3 => {
            // Create inconsistent edge references
            if !ir.model.edges.is_empty() {
                ir.model.edges[0].start = cadmpeg_ir::ids::VertexId("nonexistent".to_string());
            }
        }
        4 => {
            // Mutate surface geometry with degenerate values
            for surface in &mut ir.model.surfaces {
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
            // Create empty body (no regions)
            if !ir.model.bodies.is_empty() {
                ir.model.bodies[0].regions.clear();
            }
        }
        6 => {
            // Create face with no loops
            if !ir.model.faces.is_empty() {
                ir.model.faces[0].loops.clear();
            }
        }
        7 => {
            // Create loop with no coedges
            if !ir.model.loops.is_empty() {
                ir.model.loops[0].coedges.clear();
            }
        }
        8 => {
            // Mutate tolerances to invalid values
            ir.tolerances.linear = -1.0;
            ir.tolerances.angular = f64::NAN;
        }
        9 => {
            // Clear all geometry but keep topology
            ir.model.points.clear();
            ir.model.curves.clear();
            ir.model.surfaces.clear();
        }
        10 => {
            // Break a radial ring with an unresolved coedge.
            if let Some(coedge) = ir.model.coedges.first_mut() {
                coedge.radial_next = cadmpeg_ir::ids::CoedgeId("nonexistent".to_string());
            }
        }
        11 => {
            // Violate canonical arena ordering.
            ir.model.coedges.reverse();
        }
        12 => {
            // Put a coedge-owned edge into a shell's wire set.
            if let (Some(shell), Some(edge)) = (ir.model.shells.first_mut(), ir.model.edges.first())
            {
                shell.wire_edges.push(edge.id.clone());
            }
        }
        13 => {
            // Add an annotation for an entity that does not exist.
            source_fidelity.annotations.provenance.insert(
                "nonexistent".to_string(),
                cadmpeg_ir::annotations::Provenance {
                    stream: u32::MAX,
                    offset: u64::MAX,
                    tag: None,
                },
            );
        }
        14 => {
            // Put an invalid range on a canonical curve parameterization.
            if let Some(edge) = ir.model.edges.first_mut() {
                edge.param_range = Some([f64::INFINITY, f64::NEG_INFINITY]);
            }
        }
        _ => {}
    }

    let _ = cadmpeg_ir::validate_with_source_fidelity(&ir, &source_fidelity, Vec::new());
});
