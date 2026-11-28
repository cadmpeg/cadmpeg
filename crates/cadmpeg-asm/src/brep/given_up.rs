// SPDX-License-Identifier: Apache-2.0
//! The topology records a model decode reads and leaves out of the B-rep.
//!
//! A record the walk reaches but whose emit pass finds a reference it cannot
//! state, and a face whose surface reference resolves to no record, is
//! withheld; the embedding codec gives it up as a part over its own framed
//! bytes (CADIR §3.3). Each part's dropped
//! list names the neutral id the record would carry, so the decode wrapper
//! drops or marks every record that references it, and, for a face, the
//! records reached only through its loops that never reached the B-rep. The
//! shell of a withheld face states unstated members, and the face's part
//! lists it as marked.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{Identity, ShellId};
use cadmpeg_ir::topology::Unstated;
use cadmpeg_ir::unknown::part::{Cause, Part};

use super::attributes::unknown_record_id;
use super::geometry::{
    coedge_pcurve_ref, collect_carrier, is_analytic_curve, is_coedge_record, is_edge_record,
    is_vertex_record, vertex_point_ref,
};
use super::topology::{owned_child, subshell_ancestor_shells};
use super::{id, AsmBrep, Reachable};
use crate::ids::IdFormat;
use crate::sab::Record;

/// A topology record the model decode read and left out of the B-rep: the
/// embedding codec gives it up as a part over its bytes (CADIR §3.3), in the
/// stream that holds the ASM bytes, keeping only a digest where a record it
/// retains already holds the whole stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithheldRecord {
    /// The part kind: the record's topology kind.
    pub kind: &'static str,
    /// The record's offset in the decoded ASM bytes.
    pub offset: usize,
    /// The record's bytes, from `offset`.
    pub bytes: Vec<u8>,
    /// The give-up's cause: [`Cause::Unreadable`] where the record breaks
    /// the topology rules of the format, [`Cause::Undecoded`] where it needs
    /// a record the decode does not state.
    pub cause: Cause,
    /// Why the decode leaves the record out.
    pub reason: &'static str,
    /// The neutral id the record would carry, then the ids of the records
    /// that need it and never reached the B-rep.
    pub dropped: Vec<Identity>,
    /// The emitted records that state unstated members because the record
    /// is left out: the shell of a face.
    pub marked: Vec<Identity>,
}

impl WithheldRecord {
    /// The part of `codec` over this record's bytes, naming the records it
    /// drops and marks.
    #[must_use]
    pub fn part(&self, codec: &'static str) -> Part<'_> {
        Part::at(
            codec,
            self.kind,
            u64_from_index(self.offset),
            &self.bytes,
            [ArenaClass::Model],
        )
        .dropping(self.dropped.iter().cloned())
        .marking(self.marked.iter().cloned())
    }

    /// The error the part's give-up states.
    #[must_use]
    pub fn error(&self) -> CodecError {
        CodecError::malformed(self.reason)
    }
}

/// Withholds each record of `records` that the walk reached and no emit
/// pass emitted, and each face that names a shell but no surface record. The
/// emitted shell of each withheld face states unstated members. With
/// `bytes`, the stream `records` were framed in, `out.withheld` lists each
/// withheld record with its bytes, and a record that one listed record names
/// is named by no other.
///
/// # Errors
///
/// [`CodecError::Malformed`] when a record index exceeds the `i64` range or
/// a listed record's span leaves `bytes`, and a refusal of the allocation
/// that keeps a record's bytes.
pub(super) fn list_withheld(
    ctx: &DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    bytes: Option<&[u8]>,
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
    format: IdFormat,
) -> Result<(), CodecError> {
    let emitted = out
        .faces
        .iter()
        .map(|record| record.id.as_str())
        .chain(out.loops.iter().map(|record| record.id.as_str()))
        .chain(out.coedges.iter().map(|record| record.id.as_str()))
        .chain(out.edges.iter().map(|record| record.id.as_str()))
        .chain(out.vertices.iter().map(|record| record.id.as_str()))
        .chain(out.points.iter().map(|record| record.id.as_str()))
        .chain(out.curves.iter().map(|record| record.id.as_str()))
        .chain(out.pcurves.iter().map(|record| record.id.as_str()))
        .chain(out.regions.iter().map(|record| record.id.as_str()))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let mut withheld = Vec::new();
    for r in records {
        let i = i64::try_from(r.index)
            .map_err(|_| CodecError::malformed("ASM record index exceeds the i64 range"))?;
        let own = id(format, i);
        if emitted.contains(own.as_str()) {
            continue;
        }
        if let Some((kind, cause, reason)) = withheld_cause(r, i, by_index, reach) {
            withheld.push((r, kind, cause, reason, own));
        }
    }
    let mut named = HashSet::new();
    let mut retained = HashSet::new();
    if bytes.is_some() {
        named.extend(withheld.iter().map(|(.., own)| own.as_str().to_owned()));
        retained.extend(out.unknowns.iter().map(|record| record.id().as_str()));
    }
    let shells = if withheld.iter().any(|(_, kind, ..)| *kind == "face") {
        subshell_ancestor_shells(records, by_index)?
    } else {
        HashMap::new()
    };
    for (r, kind, cause, reason, own) in withheld {
        let mut marked = None;
        if let ("face", Some(owner)) = (kind, r.ref_at(5)) {
            let shell = id(format, shells.get(&owner).copied().unwrap_or(owner));
            let shell_id = ShellId::from(shell.clone());
            if let Some(record) = out.shells.iter_mut().find(|record| record.id == shell_id) {
                record.unstated = Some(Unstated::Members);
                marked = Some(shell);
            }
        }
        let Some(bytes) = bytes else {
            continue;
        };
        let mut dropped = vec![own];
        if kind == "face" {
            for (index, child) in face_children(r, by_index) {
                let child_id = id(format, index);
                if emitted.contains(child_id.as_str())
                    || retained.contains(unknown_record_id(child, format)?.as_str())
                {
                    continue;
                }
                if named.insert(child_id.as_str().to_owned()) {
                    dropped.push(child_id);
                }
            }
        }
        let span = r.bytes_in(bytes)?;
        let mut record_bytes = ctx.alloc_filled(span.len(), 0_u8, "withheld ASM record bytes")?;
        record_bytes.copy_from_slice(span);
        out.withheld.push(WithheldRecord {
            kind,
            offset: r.offset,
            bytes: record_bytes,
            cause,
            reason,
            dropped,
            marked: marked.into_iter().collect(),
        });
    }
    Ok(())
}

/// The part kind of record `r`, the cause of its give-up and why the decode
/// leaves it out, where it does. A null or dangling topology reference
/// breaks the format; a record that needs a record the decode reads and
/// does not state is undecoded.
fn withheld_cause(
    r: &Record,
    i: i64,
    by_index: &HashMap<i64, &Record>,
    reach: &Reachable,
) -> Option<(&'static str, Cause, &'static str)> {
    use Cause::{Undecoded, Unreadable};
    match r.head() {
        // A face with no shell is retained as a source record; a face the
        // face pass kept that no emit states has no other cause.
        "face" if reach.faces.contains(&i) => Some((
            "face",
            Undecoded,
            "the face record names no shell or no surface",
        )),
        "face" if r.ref_at(5).is_none() => None,
        "face" => match r.ref_at(7) {
            None => Some(("face", Unreadable, "the face record states no surface")),
            Some(surface) if !by_index.contains_key(&surface) => Some((
                "face",
                Unreadable,
                "the face record's surface reference names no record",
            )),
            Some(_) => None,
        },
        "loop" if reach.loops.contains(&i) => Some(if r.ref_at(5).is_none() {
            ("loop", Unreadable, "the loop record names no face")
        } else {
            (
                "loop",
                Undecoded,
                "the loop record's coedge ring holds no coedge the loop owns",
            )
        }),
        _ if is_coedge_record(r) && reach.coedges.contains(&i) => {
            let held = |slot: usize, set: &HashSet<i64>| r.ref_at(slot).map(|at| set.contains(&at));
            let (cause, reason) = match (
                held(3, &reach.coedges),
                held(4, &reach.coedges),
                held(6, &reach.edges),
                held(8, &reach.loops),
            ) {
                (None, ..) => (Unreadable, "the coedge record states no next coedge"),
                (_, None, ..) => (Unreadable, "the coedge record states no previous coedge"),
                (_, _, None, _) => (Unreadable, "the coedge record states no edge"),
                (.., None) => (Unreadable, "the coedge record names no loop"),
                (Some(false), ..) => (
                    Unreadable,
                    "the coedge record's next coedge is no coedge of its ring",
                ),
                (_, Some(false), ..) => (
                    Unreadable,
                    "the coedge record's previous coedge is no coedge of its ring",
                ),
                (_, _, Some(false), _) => (
                    Unreadable,
                    "the coedge record's edge reference names no edge record",
                ),
                (.., Some(false)) => (
                    if r.ref_at(8).is_some_and(|at| by_index.contains_key(&at)) {
                        Undecoded
                    } else {
                        Unreadable
                    },
                    "the coedge record's loop is no loop of a kept face",
                ),
                _ => (
                    Undecoded,
                    "the coedge record names a reference the decode does not hold",
                ),
            };
            Some(("coedge", cause, reason))
        }
        _ if is_edge_record(r) && reach.edges.contains(&i) => Some((
            "edge",
            Unreadable,
            if r.ref_at(3).is_none() || r.ref_at(5).is_none() {
                "the edge record states no start or end vertex"
            } else {
                "the edge record's vertex reference names no vertex record"
            },
        )),
        _ if is_vertex_record(r) && reach.vertices.contains(&i) => {
            Some(match vertex_point_ref(r) {
                None => ("vertex", Unreadable, "the vertex record states no point"),
                Some(_) => (
                    "vertex",
                    Undecoded,
                    "the vertex record's point reference names no point the decode holds",
                ),
            })
        }
        "point" if reach.points.contains(&i) && collect_carrier(r).positions.is_empty() => {
            Some(("point", Unreadable, "the point record states no position"))
        }
        "region" | "lump" if r.ref_at(5).is_none() => {
            Some(("region", Unreadable, "the region record names no body"))
        }
        _ => None,
    }
}

/// The loops of `face`'s loop chain that it owns, the coedges of each
/// loop's ring that the loop owns, and each coedge's pcurve, edge, the
/// edge's curve and vertices, and each vertex's point: the records the face
/// reaches, with their indices.
fn face_children<'record>(
    face: &Record,
    by_index: &HashMap<i64, &'record Record>,
) -> Vec<(i64, &'record Record)> {
    let mut children = Children {
        by_index,
        reached: Vec::new(),
        seen: HashSet::new(),
    };
    let mut next_loop = face.ref_at(4);
    while let Some(loop_index) = next_loop {
        let Some(lp) = owned_child(by_index, loop_index, &["loop"], 5, face) else {
            break;
        };
        if !children.first_reach(loop_index, lp) {
            break;
        }
        let mut next_coedge = lp.ref_at(4);
        while let Some(coedge_index) = next_coedge {
            let Some(coedge) = owned_child(by_index, coedge_index, &["coedge", "tcoedge"], 8, lp)
            else {
                break;
            };
            if !children.first_reach(coedge_index, coedge) {
                break;
            }
            children.reach(coedge_pcurve_ref(coedge), |record| {
                record.head() == "pcurve"
            });
            if let Some(edge) = children.reach(coedge.ref_at(6), is_edge_record) {
                children.reach(edge.ref_at(8), |record| {
                    is_analytic_curve(record.head()) || record.head() == "intcurve"
                });
                for slot in [3, 5] {
                    if let Some(vertex) = children.reach(edge.ref_at(slot), is_vertex_record) {
                        children.reach(vertex_point_ref(vertex), |record| record.head() == "point");
                    }
                }
            }
            next_coedge = coedge.ref_at(3);
        }
        next_loop = lp.ref_at(3);
    }
    children.reached
}

/// The records a face walk reaches, each once.
struct Children<'a, 'record> {
    by_index: &'a HashMap<i64, &'record Record>,
    reached: Vec<(i64, &'record Record)>,
    seen: HashSet<i64>,
}

impl<'record> Children<'_, 'record> {
    /// Lists `record` at `index` and returns whether the walk had not
    /// reached it before.
    fn first_reach(&mut self, index: i64, record: &'record Record) -> bool {
        let first = self.seen.insert(index);
        if first {
            self.reached.push((index, record));
        }
        first
    }

    /// Lists the record that `index` names where it is of the `kind` the
    /// reference needs, and returns it.
    fn reach(&mut self, index: Option<i64>, kind: fn(&Record) -> bool) -> Option<&'record Record> {
        let index = index?;
        let record = self
            .by_index
            .get(&index)
            .copied()
            .filter(|record| kind(record))?;
        self.first_reach(index, record);
        Some(record)
    }
}

#[cfg(test)]
mod tests;
