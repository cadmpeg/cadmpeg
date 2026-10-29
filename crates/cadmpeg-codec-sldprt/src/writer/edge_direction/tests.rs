// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::CadIr;

use super::reversed_compact_edges;

/// Every edge of the cube has one forward and one reversed coedge.
fn cube() -> CadIr {
    cadmpeg_ir::examples::unit_cube().unwrap()
}

/// Make every coedge of edge `index` run against it.
fn reverse_all_coedges(ir: &mut CadIr, index: usize) {
    let edge = ir.model.edges[index].id.clone();
    for coedge in &mut ir.model.coedges {
        if coedge.edge == edge {
            coedge.sense = Sense::Reversed;
        }
    }
}

#[test]
fn an_edge_with_a_forward_coedge_is_stored_along_its_curve() {
    let reversed = reversed_compact_edges(&cube()).unwrap();
    assert!(reversed.edges.is_empty());
    assert!(reversed.curves.is_empty());
}

#[test]
fn an_edge_whose_coedges_all_run_against_it_is_stored_reversed() {
    let mut ir = cube();
    reverse_all_coedges(&mut ir, 0);
    let reversed = reversed_compact_edges(&ir).unwrap();
    let edge = &ir.model.edges[0];
    assert_eq!(reversed.edges.len(), 1);
    assert!(reversed.edges.contains(&edge.id));
    assert_eq!(reversed.curves.len(), 1);
    assert!(reversed.curves.contains(edge.curve().unwrap()));
}

#[test]
fn stored_sense_flips_only_the_coedges_of_reversed_edges() {
    let mut ir = cube();
    reverse_all_coedges(&mut ir, 0);
    let reversed = reversed_compact_edges(&ir).unwrap();
    let flipped = &ir.model.edges[0].id;
    let kept = &ir.model.edges[1].id;
    assert_eq!(
        reversed.stored_sense(flipped, Sense::Reversed),
        Sense::Forward
    );
    assert_eq!(
        reversed.stored_sense(flipped, Sense::Forward),
        Sense::Reversed
    );
    assert_eq!(reversed.stored_sense(kept, Sense::Forward), Sense::Forward);
    assert_eq!(
        reversed.stored_sense(kept, Sense::Reversed),
        Sense::Reversed
    );
}

#[test]
fn a_curve_with_edges_stored_in_both_directions_is_refused() {
    let mut ir = cube();
    reverse_all_coedges(&mut ir, 0);
    let shared = ir.model.edges[0].curve().cloned();
    ir.model.edges[1].set_curve(shared).unwrap();
    let error = reversed_compact_edges(&ir).err().unwrap();
    assert!(
        matches!(&error, CodecError::NotImplemented(message)
            if message.ends_with("carries edges stored in both directions")),
        "{error:?}"
    );
}
