// SPDX-License-Identifier: Apache-2.0
//! Parses IR JSON, applies one of 13 deterministic semantic mutations selected
//! by the first input byte, and validates the result. Validation findings are
//! expected; panics are failures.

#![no_main]

use cadmpeg_ir::annotations::StreamHandle;
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
    let mut source_fidelity = cadmpeg_ir::source_fidelity::SourceFidelity::new(
        cadmpeg_ir::hash::digest::Sha256Digest::digest(json_str.as_bytes()),
    );

    match strategy % 13 {
        0 => {
            // Drive vertex positions to the finite extremes; a point refuses anything else.
            let Some(extreme) = cadmpeg_ir::features::geometry::FinitePoint3::new(
                cadmpeg_ir::math::Point3::new(f64::MAX, f64::MIN, f64::MIN_POSITIVE),
            ) else {
                return;
            };
            for point in &mut ir.geometry.points {
                point.set_position(extreme);
            }
        }
        1 => {
            // Create invalid cross-references
            if !ir.geometry.vertices.is_empty() {
                ir.geometry.vertices[0].point = cadmpeg_ir::ids::PointId::mint("fuzz:missing:point#0")
                    .expect("identity grammar");
            }
        }
        2 => {
            if let Some(loop_) = ir
                .geometry
                .loops
                .iter_mut()
                .find(|loop_| !loop_.coedges().is_empty())
            {
                let mut coedges = loop_.coedges().to_vec();
                if coedges.len() >= 2 {
                    coedges.swap(0, 1);
                    let vertex_uses = loop_.anchored_vertex_uses().to_vec();
                    // A refused ring leaves the loop unchanged.
                    let _ = loop_.replace_ring(coedges, vertex_uses);
                }
            }
        }
        3 => {
            // Create inconsistent edge references
            if let Some(ends) = ir.geometry.edges.first_mut().and_then(|edge| edge.ends.as_mut()) {
                ends.start = cadmpeg_ir::ids::VertexId::mint("fuzz:missing:vertex#0")
                    .expect("identity grammar");
            }
        }
        4 => {
            // Create empty body (no regions)
            if let Some(body) = ir.geometry.bodies.first().map(|body| body.id.clone()) {
                ir.geometry.regions.retain(|region| region.body != body);
            }
        }
        5 => {
            // Create face with no loops
            if !ir.geometry.faces.is_empty() {
                ir.geometry.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
            }
        }
        6 => {
            // Duplicate a point identity in the document arena.
            if let Some(point) = ir.geometry.points.first().cloned() {
                ir.geometry.points.push(point);
            }
        }
        7 => {
            // Clear all geometry but keep topology
            ir.geometry.points.clear();
            ir.geometry.curves.clear();
            ir.geometry.surfaces.clear();
        }
        8 => {
            // Break a radial ring with an unresolved coedge.
            if let Some(coedge) = ir.geometry.coedges.first_mut() {
                coedge.radial_next = cadmpeg_ir::ids::CoedgeId::mint("fuzz:missing:coedge#0")
                    .expect("identity grammar");
            }
        }
        9 => {
            // Violate canonical arena ordering.
            ir.geometry.coedges.reverse();
        }
        10 => {
            // Put a coedge-owned edge into a shell's wire set.
            if let (Some(shell), Some(edge)) = (ir.geometry.shells.first_mut(), ir.geometry.edges.first())
            {
                shell.add_wire_edge(edge.id.clone());
            }
        }
        11 => {
            // Add an annotation for an entity that does not exist.
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            let stream = StreamHandle::new(cadmpeg_ir::stream_name!("fuzz:nonexistent"));
            annotations.note("nonexistent", &stream, u64::MAX, None);
            source_fidelity
                .annotations
                .append(annotations.build())
                .expect("annotation arena append");
        }
        12 => {
            if let Some(edge) = ir.geometry.edges.first().cloned() {
                ir.geometry.edges.push(edge);
            }
        }
        _ => {}
    }

    let _ = cadmpeg_ir::validate::validate_neutral_with_source_fidelity(
        &ir,
        &source_fidelity,
        Vec::new(),
    );
});
