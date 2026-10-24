// SPDX-License-Identifier: Apache-2.0
//! Face rings of decoded exact shapes run in traversal order.

use crate::FcstdCodec;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::topology::{LoopBoundary, Sense};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

/// Rings whose coedges, with their senses applied, do not each end at the
/// vertex where the next one starts.
fn discontinuous_rings(ir: &CadIr) -> Vec<String> {
    let model = &ir.model;
    let edges: HashMap<_, _> = model.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let coedges: HashMap<_, _> = model
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect();
    let mut discontinuous = Vec::new();
    for face_loop in &model.loops {
        let LoopBoundary::Ring(ring) = &face_loop.boundary else {
            continue;
        };
        let ends: Vec<(&VertexId, &VertexId)> = ring
            .coedges()
            .iter()
            .map(|id| {
                let coedge = coedges[id];
                let edge = edges[&coedge.edge];
                match coedge.sense {
                    Sense::Forward => (&edge.start, &edge.end),
                    Sense::Reversed => (&edge.end, &edge.start),
                }
            })
            .collect();
        let continuous = ends
            .iter()
            .zip(ends.iter().cycle().skip(1))
            .all(|(current, next)| current.1 == next.0);
        if !continuous {
            discontinuous.push(face_loop.id.as_str().to_owned());
        }
    }
    discontinuous
}

#[test]
fn every_corpus_face_ring_runs_in_traversal_order() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("FreeCAD corpus fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "FCStd")
        })
        .collect();
    paths.sort();
    assert!(paths
        .iter()
        .any(|path| path.ends_with("geometry_topology.FCStd")));
    for path in paths {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let result = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let discontinuous = discontinuous_rings(result.ir());
        assert!(
            discontinuous.is_empty(),
            "{}: rings out of traversal order: {discontinuous:?}",
            path.display()
        );
    }
}
