// SPDX-License-Identifier: Apache-2.0
//! Transient current topology -> emitted TopExp indices; never persisted backlinks.
use super::{Shape, ShapeUse};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{transform::Transform, CadIr};
use std::collections::{BTreeSet, HashMap, HashSet};

pub(in crate::writer) struct EmittedOccurrence {
    body: Option<String>,
    topology_id: String,
    indexed_name: &'static str,
    output_index: usize,
}

const KINDS: [(&str, &str); 8] = [
    ("Ve", "Vertex"),
    ("Ed", "Edge"),
    ("Wi", "Wire"),
    ("Fa", "Face"),
    ("Sh", "Shell"),
    ("So", "Solid"),
    ("CS", "CompSolid"),
    ("Co", "Compound"),
];

fn placement_key(transform: Transform) -> Vec<u64> {
    transform
        .rows()
        .into_iter()
        .flatten()
        .map(|value| {
            if value == 0.0 {
                0.0_f64.to_bits()
            } else {
                value.to_bits()
            }
        })
        .collect()
}

/// The graph and root ordering here are precisely those serialized by `brep`.
/// Each kind has its own first-encounter numbering, with TopLoc equality exact.
pub(super) fn emitted(
    ir: &CadIr,
    shapes: &[Shape],
    identities: &HashMap<&str, ShapeUse>,
    body_roots: &[(usize, String, Transform)],
    roots: &[ShapeUse],
    locations: &[Transform],
) -> Result<Vec<EmittedOccurrence>, CodecError> {
    let location = |index: usize| {
        if index == 0 {
            Transform::identity()
        } else {
            locations[index - 1]
        }
    };
    let row_owners = ir
        .native
        .namespace("fcstd")
        .map(crate::brep::records::row_owners)
        .transpose()?
        .unwrap_or_default();
    let members = body_roots
        .iter()
        .map(|(_, id, _)| {
            let body = ir.model.bodies.iter().find(|body| body.id.as_str() == id);
            body.map(|body| super::reachable::Reach::bodies(ir, &[body], &row_owners))
        })
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for (target, indexed_name) in KINDS {
        let mut ordinals = HashMap::new();
        for (ordinal, root) in roots.iter().enumerate() {
            let context = body_roots.get(ordinal);
            let body_frame =
                context.map_or_else(Transform::identity, |(_, _, placement)| *placement);
            let mut stack = vec![(root.index, location(root.location))];
            let mut visited = HashSet::new();
            while let Some((index, transform)) = stack.pop() {
                let placement = placement_key(transform);
                if !visited.insert((index, placement.clone())) {
                    continue;
                }
                let shape = &shapes[index];
                if shape.kind == target {
                    let next = ordinals.len() + 1;
                    let output_index = *ordinals.entry((index, placement.clone())).or_insert(next);
                    for (id, usage) in identities.iter().filter(|(_, usage)| usage.index == index) {
                        let expected = body_frame
                            .compose(location(usage.location))
                            .map_err(CodecError::malformed)?;
                        if placement_key(expected) != placement {
                            continue;
                        }
                        if let (Some((_, body, _)), Some(Some(member))) =
                            (context, members.get(ordinal))
                        {
                            let belongs = match indexed_name {
                                "Vertex" => member.vertices.contains(*id),
                                "Edge" => member.edges.contains(*id),
                                "Wire" => {
                                    member.loops.contains(*id)
                                        || body_has_shell(ir, body.as_str(), id)
                                }
                                "Face" => member.faces.contains(*id),
                                "Shell" => body_has_shell(ir, body.as_str(), id),
                                "Solid" => ir
                                    .model
                                    .bodies
                                    .iter()
                                    .find(|value| value.id.as_str() == body.as_str())
                                    .is_some_and(|body| {
                                        body.regions.iter().any(|region| region.as_str() == *id)
                                    }),
                                _ => context.is_some_and(|(_, body, _)| body.as_str() == *id),
                            };
                            if !belongs {
                                continue;
                            }
                        }
                        out.push(EmittedOccurrence {
                            body: context.map(|(_, body, _)| body.clone()),
                            topology_id: (*id).to_owned(),
                            indexed_name,
                            output_index,
                        });
                    }
                    continue;
                }
                if rank(shape.kind) < rank(target) {
                    for child in shape.children.iter().rev() {
                        stack.push((
                            child.index,
                            transform
                                .compose(location(child.location))
                                .map_err(CodecError::malformed)?,
                        ));
                    }
                }
            }
        }
    }
    Ok(out)
}

fn body_has_shell(ir: &CadIr, body: &str, shell: &str) -> bool {
    ir.model
        .bodies
        .iter()
        .find(|value| value.id.as_str() == body)
        .is_some_and(|body| {
            body.regions.iter().any(|id| {
                ir.model
                    .regions
                    .iter()
                    .find(|region| region.id == *id)
                    .is_some_and(|region| region.shells.iter().any(|id| id.as_str() == shell))
            })
        })
}

fn rank(kind: &str) -> usize {
    KINDS
        .iter()
        .position(|(code, _)| *code == kind)
        .map_or(8, |index| 7 - index)
}

pub(in crate::writer) fn assign_names(
    emitted: &[EmittedOccurrence],
    body_names: &HashMap<String, String>,
    names: &mut HashMap<String, String>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let mut selections = HashMap::<&str, BTreeSet<(&str, &str, usize)>>::new();
    for occurrence in emitted {
        if !matches!(occurrence.indexed_name, "Face" | "Edge" | "Vertex") {
            continue;
        }
        let Some(owner) = occurrence
            .body
            .as_ref()
            .and_then(|body| body_names.get(body))
        else {
            continue;
        };
        selections
            .entry(&occurrence.topology_id)
            .or_default()
            .insert((owner, occurrence.indexed_name, occurrence.output_index));
    }
    for (id, targets) in selections {
        let key = format!("{id}:subelement");
        if let Some((owner, kind, index)) = targets.first().filter(|_| targets.len() == 1) {
            names.insert(id.to_owned(), (*owner).to_owned());
            names.insert(key, format!("{kind}{index}"));
        } else {
            names.remove(&key);
            losses.push(
                crate::loss::FreecadLossCode::ExportPresentationNotTransferred.note(format!(
                    "topology {id} has several emitted selection targets; its ambiguous selection is omitted"
                )),
            );
        }
    }
}

#[cfg(test)]
mod tests;
