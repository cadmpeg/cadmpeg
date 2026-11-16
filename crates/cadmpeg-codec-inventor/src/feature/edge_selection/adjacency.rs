// SPDX-License-Identifier: Apache-2.0
//! The two faces that use a stored edge revision.

use std::collections::HashMap;

use cadmpeg_asm::sab::{Record, Token};

/// Coedge slots that the walk reads: the partner coedge, the edge, and the loop.
const COEDGE_PARTNER: usize = 5;
const COEDGE_EDGE: usize = 6;
const COEDGE_LOOP: usize = 8;
/// Edge slot of the first coedge.
const EDGE_COEDGE: usize = 7;
/// Loop slot of the owning face.
const LOOP_FACE: usize = 5;

/// Whether the record is a `kind` record, directly or as the first record of
/// the history snapshot that the `End-of-ASM-History-Section` name chain wraps.
fn is_kind(record: &Record, kinds: &[&str]) -> bool {
    kinds.contains(&record.head())
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.first(),
                Some(Token::Ident(name)) if kinds.contains(&name.as_str())))
}

/// The entity indexes of the two distinct faces whose loops hold the two
/// coedges of the edge revision `edge`, in coedge order.
///
/// The partner ring of the first coedge holds exactly two coedges. Each names
/// the edge as its edge and a loop whose face is a `face` record. Any other
/// ring, a missing record, or one face on both sides gives `None`.
pub(super) fn faces(edge: i64, by_index: &HashMap<i64, &Record>) -> Option<[i64; 2]> {
    let edge_record = by_index.get(&edge)?;
    if !is_kind(edge_record, &["edge", "tedge"]) {
        return None;
    }
    let first = edge_record.ref_at(EDGE_COEDGE)?;
    let mut ring = Vec::with_capacity(2);
    let mut current = first;
    loop {
        let coedge = by_index.get(&current)?;
        if !is_kind(coedge, &["coedge", "tcoedge"]) || coedge.ref_at(COEDGE_EDGE)? != edge {
            return None;
        }
        let owner = by_index.get(&coedge.ref_at(COEDGE_LOOP)?)?;
        if !is_kind(owner, &["loop"]) {
            return None;
        }
        let face = owner.ref_at(LOOP_FACE)?;
        if !is_kind(by_index.get(&face)?, &["face"]) {
            return None;
        }
        ring.push(face);
        current = coedge.ref_at(COEDGE_PARTNER)?;
        if current == first {
            break;
        }
        if ring.len() == 2 {
            return None;
        }
    }
    match ring.as_slice() {
        [a, b] if a != b => Some([*a, *b]),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
