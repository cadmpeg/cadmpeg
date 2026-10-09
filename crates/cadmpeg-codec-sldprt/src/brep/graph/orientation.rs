// SPDX-License-Identifier: Apache-2.0
//! Solve face sense parity from shared edge uses.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::topology::Sense;
use std::collections::BTreeMap;
use std::collections::HashMap;
use super::Brep;

pub(super) fn solve_face_orientation(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "solve Parasolid face senses";
    let mut workspace = ctx.reserve_scoped(0, "SLDPRT face orientation workspace")?;
    let senses = workspace.with_storage(|| {
        let mut loop_faces = HashMap::new();
        for lp in ctx.admit_iter(&out.loops, "index oriented Parasolid loops")? {
            ctx.insert_hash_map(
                &mut loop_faces,
                &lp.id,
                &lp.face,
                "index oriented Parasolid loops",
            )?;
        }
        // Edges in identity order, so the adjacency lists and the walk below
        // do not depend on table order.
        let mut uses = BTreeMap::<&EdgeId, Vec<(&FaceId, bool)>>::new();
        for coedge in ctx.admit_iter(&out.coedges, "orient Parasolid face uses")? {
            if let Some(&face) = ctx.get_hash_map(
                &loop_faces,
                &coedge.owner_loop,
                "orient Parasolid face uses",
            )? {
                ctx.push_btree_group(
                    &mut uses,
                    &coedge.edge,
                    (face, coedge.sense == Sense::Reversed),
                    "index Parasolid edge face uses",
                    "collect Parasolid edge face uses",
                )?;
            }
        }
        let mut adjacency = HashMap::<&FaceId, Vec<(&FaceId, bool)>>::new();
        for (_, edge_uses) in ctx.admit_iter(&uses, "index Parasolid face adjacency")? {
            let [(a, a_reversed), (b, b_reversed)] = edge_uses.as_slice() else {
                continue;
            };
            let parity = a_reversed == b_reversed;
            for (face, neighbor) in [(*a, *b), (*b, *a)] {
                ctx.push_hash_group(
                    &mut adjacency,
                    face,
                    (neighbor, parity),
                    "index Parasolid face adjacency",
                    "collect Parasolid face adjacency",
                )?;
            }
        }
        let mut initial = HashMap::new();
        for face in ctx.admit_iter(&out.faces, "index initial Parasolid face senses")? {
            ctx.insert_hash_map(
                &mut initial,
                &face.id,
                face.sense == Sense::Reversed,
                "index initial Parasolid face senses",
            )?;
        }
        let mut solved = HashMap::<&FaceId, bool>::new();
        let mut pending = Vec::new();
        for face in ctx.admit_iter(&out.faces, OPERATION)? {
            let root = &face.id;
            if ctx.contains_key_hash_map(&solved, root, OPERATION)? {
                continue;
            }
            let Some(&sense) = ctx.get_hash_map(&initial, root, OPERATION)? else {
                continue;
            };
            ctx.insert_hash_map(
                &mut solved,
                root,
                sense,
                "track solved Parasolid face senses",
            )?;
            ctx.push_vec(&mut pending, root, "walk Parasolid face senses")?;
            loop {
                ctx.charge_work(1, "walk Parasolid face senses")?;
                let Some(face) = pending.pop() else {
                    break;
                };
                let Some(&sense) = ctx.get_hash_map(&solved, face, OPERATION)? else {
                    continue;
                };
                let Some(neighbors) = ctx.get_hash_map(&adjacency, face, OPERATION)? else {
                    continue;
                };
                for &(neighbor, parity) in
                    ctx.admit_iter(neighbors, "walk Parasolid face adjacency")?
                {
                    if !ctx.contains_key_hash_map(&solved, neighbor, OPERATION)? {
                        ctx.insert_hash_map(
                            &mut solved,
                            neighbor,
                            sense ^ parity,
                            "track solved Parasolid face senses",
                        )?;
                        ctx.push_vec(&mut pending, neighbor, "walk Parasolid face senses")?;
                    }
                }
            }
        }
        ctx.try_collect_vec(
            out.faces.iter().map(|face| {
                Ok::<_, cadmpeg_core::CodecError>(
                    ctx.get_hash_map(&solved, &face.id, "assign Parasolid face senses")?
                        .copied()
                        .unwrap_or(false),
                )
            }),
            "assign Parasolid face senses",
        )
    })?;
    for (face, reversed) in ctx
        .admit_iter(&mut out.faces, "assign Parasolid face senses")?
        .zip(senses)
    {
        face.sense = if reversed {
            Sense::Reversed
        } else {
            Sense::Forward
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests;
