// SPDX-License-Identifier: Apache-2.0
//! Typed ownership nodes read from the node sequence of a partition or plain
//! stream (`parasolid.md` §2).

use super::{valid_resolution, BodyNode, FaceNode, Facts, RegionKind, RegionNode, ShellNode};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_parasolid::records::node_type::{BODY, FACE, REGION, SHELL};
use cadmpeg_parasolid::records::NodeFields;
use cadmpeg_parasolid::records::PrimaryRecords;

/// A typed node whose base fields were read: admitted by the ownership
/// guards of its scanned form, or omitted.
enum Read<T> {
    Admitted(T),
    Omitted,
}

/// The typed BODY, SHELL, REGION, and FACE nodes of a node sequence whose
/// body starts at `body_offset`. Offsets are body-relative. A node that
/// fails the ownership guards of its scanned form is omitted; a node that
/// lacks a base field makes the sequence unusable.
pub(in crate::brep) fn facts(records: &PrimaryRecords<'_>, body_offset: usize) -> Option<Facts> {
    let mut facts = Facts::default();
    for node in records.nodes() {
        match node.kind() {
            BODY => {
                if let Read::Admitted(record) = body(&node, body_offset)? {
                    facts.bodies.push(record);
                }
            }
            SHELL => {
                if let Read::Admitted(record) = shell(&node, body_offset)? {
                    facts.shells.push(record);
                }
            }
            REGION => {
                if let Read::Admitted(record) = region(&node, body_offset)? {
                    facts.regions.push(record);
                }
            }
            FACE => {
                if let Read::Admitted(record) = face(&node, body_offset)? {
                    facts.faces.push(record);
                }
            }
            _ => {}
        }
    }
    Some(facts)
}

/// The body-relative start of `node` and the body-relative end of `last`.
fn span(node: &impl NodeFields, last: &str, body_offset: usize) -> Option<(usize, usize)> {
    Some((
        node.range().start.checked_sub(body_offset)?,
        node.field_range(last)?.end.checked_sub(body_offset)?,
    ))
}

fn body(node: &impl NodeFields, body_offset: usize) -> Option<Read<BodyNode>> {
    let node_id = node.integer("highest_node_id")?;
    let topology_refs = node.pointers([
        "shell",
        "boundary_surface",
        "boundary_curve",
        "boundary_point",
        "region",
        "edge",
        "vertex",
    ])?;
    let size = node.real("res_size")?;
    let linear = node.real("res_linear")?;
    let kind = match node.byte("body_type")? {
        1 => Some(BodyKind::Solid),
        2 => Some(BodyKind::Wire),
        3 => Some(BodyKind::Sheet),
        6 => Some(BodyKind::General),
        _ => None,
    };
    let (offset, end) = span(node, "vertex", body_offset)?;
    let admitted = node.index() > 1
        && node_id != 0
        && node.byte("state")? == 1
        && valid_resolution(size, linear);
    Some(kind.filter(|_| admitted).map_or(Read::Omitted, |kind| {
        Read::Admitted(BodyNode {
            attr: node.index(),
            node_id,
            topology_refs,
            ownership_refs: Vec::new(),
            kind,
            offset,
            end,
        })
    }))
}

fn shell(node: &impl NodeFields, body_offset: usize) -> Option<Read<ShellNode>> {
    let node_id = node.integer("node_id")?;
    let refs = node.pointers([
        "attributes_features",
        "body",
        "next",
        "face",
        "edge",
        "vertex",
        "region",
        "front_face",
    ])?;
    let (offset, end) = span(node, "front_face", body_offset)?;
    let admitted = node.index() > 1 && node_id != 0 && refs[6] > 1;
    Some(if admitted {
        Read::Admitted(ShellNode {
            attr: node.index(),
            node_id,
            refs,
            offset,
            end,
        })
    } else {
        Read::Omitted
    })
}

fn region(node: &impl NodeFields, body_offset: usize) -> Option<Read<RegionNode>> {
    let node_id = node.integer("node_id")?;
    let refs = node.pointers(["attributes_features", "body", "next", "previous", "shell"])?;
    let kind = RegionKind::from_byte(Some(&node.char("type")?));
    let (offset, end) = span(node, "type", body_offset)?;
    let admitted = node.index() > 1 && node_id != 0 && refs[1] > 1;
    Some(kind.filter(|_| admitted).map_or(Read::Omitted, |kind| {
        Read::Admitted(RegionNode {
            attr: node.index(),
            node_id,
            refs,
            kind,
            offset,
            end,
        })
    }))
}

fn face(node: &impl NodeFields, body_offset: usize) -> Option<Read<FaceNode>> {
    let node_id = node.integer("node_id")?;
    let refs = node.pointers(["next", "previous", "loop", "shell", "surface"])?;
    let tolerance = node.real("tolerance")?;
    let sense = match node.char("sense")? {
        b'+' => Some(Sense::Forward),
        b'-' => Some(Sense::Reversed),
        _ => None,
    };
    let (offset, end) = span(node, "sense", body_offset)?;
    let tolerance_admitted =
        node.is_null("tolerance")? || (tolerance.is_finite() && tolerance >= 0.0);
    let admitted = node.index() > 1 && node_id != 0 && refs[3] > 1 && tolerance_admitted;
    Some(sense.filter(|_| admitted).map_or(Read::Omitted, |sense| {
        Read::Admitted(FaceNode {
            attr: node.index(),
            node_id,
            refs,
            sense,
            offset,
            end,
        })
    }))
}

#[cfg(test)]
mod tests;
