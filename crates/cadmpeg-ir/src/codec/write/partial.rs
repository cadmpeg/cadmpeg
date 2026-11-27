// SPDX-License-Identifier: Apache-2.0
//! The geometry a writer of another codec writes where the document holds
//! partial topology (§3.4, Content the target cannot hold).
//!
//! A partial face is not written. A region that is marked, or that holds a
//! marked shell or a partial face, is written as its stated shells and never
//! as a solid. [`without_partial_topology`] gives a writer that writes only
//! complete topology the document in that form: each partial face, its
//! loops, and their coedges are left out; every radial ring that ran through
//! a coedge left out closes past it; a carrier, edge, vertex, point, or
//! pcurve that only the records left out referenced is left out with them;
//! every mark is cleared, a marked region listing all of its stated shells;
//! and a solid body that holds such a region becomes a sheet body, which a
//! writer writes as shells.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value as Json;

use super::partial_topology_omitted;
use crate::document::CadIr;
use crate::ids::{CoedgeId, ShellId};
use crate::report::loss::LossNote;
use crate::topology::BodyKind;
use cadmpeg_core::CodecError;

/// The arenas whose records are left out where only removed records
/// referenced them.
const CARRIER_ARENAS: [&str; 8] = [
    "edges",
    "vertices",
    "points",
    "curves",
    "surfaces",
    "pcurves",
    "procedural_surfaces",
    "procedural_curves",
];

/// `ir` with its partial topology in the form a writer of another codec
/// writes, and the `export:face.partial` and `export:solid.partial` losses
/// that name what it leaves out, `form` saying what the writer writes for a
/// partial region. `None` where every face, shell, region, and body is
/// complete.
///
/// # Errors
///
/// Returns a malformed error where the geometry does not serialize.
pub fn without_partial_topology(
    ir: &CadIr,
    format: &str,
    form: &str,
) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    let geometry = &ir.geometry;
    let lists = geometry.child_lists();
    let partial_faces: BTreeSet<&str> = geometry
        .faces
        .iter()
        .filter(|face| face.loops.is_partial())
        .map(|face| face.id.as_str())
        .collect();
    let partial_bodies: BTreeSet<&str> = geometry
        .regions
        .iter()
        .filter(|region| lists.complete_shells(region).is_none())
        .map(|region| region.body.as_str())
        .collect();
    if partial_faces.is_empty() && partial_bodies.is_empty() {
        return Ok(None);
    }
    let losses = partial_topology_omitted(format, geometry, form);
    let removed_loops: BTreeSet<&str> = geometry
        .loops
        .iter()
        .filter(|loop_| partial_faces.contains(loop_.face.as_str()))
        .map(|loop_| loop_.id.as_str())
        .collect();
    let removed_coedges: BTreeSet<&str> = geometry
        .coedges
        .iter()
        .filter(|coedge| removed_loops.contains(coedge.owner_loop.as_str()))
        .map(|coedge| coedge.id.as_str())
        .collect();
    let mut removed_records = Vec::new();
    for face in geometry
        .faces
        .iter()
        .filter(|face| partial_faces.contains(face.id.as_str()))
    {
        removed_records.push(to_json(face)?);
    }
    for loop_ in geometry
        .loops
        .iter()
        .filter(|loop_| removed_loops.contains(loop_.id.as_str()))
    {
        removed_records.push(to_json(loop_)?);
    }
    for coedge in geometry
        .coedges
        .iter()
        .filter(|coedge| removed_coedges.contains(coedge.id.as_str()))
    {
        removed_records.push(to_json(coedge)?);
    }

    let mut out = ir.clone();
    let written = &mut out.geometry;
    written
        .faces
        .retain(|face| !partial_faces.contains(face.id.as_str()));
    written
        .loops
        .retain(|loop_| !removed_loops.contains(loop_.id.as_str()));
    close_radial_rings(written, geometry, &removed_coedges);
    for region in &mut written.regions {
        if region.unstated.take().is_some() {
            let stated: Vec<ShellId> = region.stated_shells().cloned().collect();
            region.set_shells(stated);
        }
    }
    for shell in &mut written.shells {
        shell.unstated = None;
    }
    for body in &mut written.bodies {
        body.unstated = None;
        if body.kind == BodyKind::Solid && partial_bodies.contains(body.id.as_str()) {
            body.kind = BodyKind::Sheet;
        }
    }
    drop_unreferenced(&mut out, &removed_records)?;
    Ok(Some((out, losses)))
}

/// `ir` without the faces `faces` and the bodies `bodies`, as a writer writes
/// a document from which it drops them (§3.4, Content the target cannot
/// hold): each face is left out with its loops and their coedges, and each
/// body with its regions, their shells, and those shells' faces. Every radial
/// ring that ran through a coedge left out closes past it. A shell left with
/// no face, wire edge, or free vertex is left out, then a region left with no
/// shell, then a body left with no region. A solid body that loses a face
/// becomes a sheet body, which a writer writes as shells. A carrier, edge,
/// vertex, point, or pcurve that only the records left out referenced is left
/// out with them.
///
/// # Errors
///
/// Returns a malformed error where the geometry does not serialize.
pub fn without_faces_and_bodies(
    ir: &CadIr,
    faces: &BTreeSet<&str>,
    bodies: &BTreeSet<&str>,
) -> Result<CadIr, CodecError> {
    let geometry = &ir.geometry;
    let removed_regions: BTreeSet<&str> = geometry
        .regions
        .iter()
        .filter(|region| bodies.contains(region.body.as_str()))
        .map(|region| region.id.as_str())
        .collect();
    let mut removed_shells: BTreeSet<&str> = geometry
        .shells
        .iter()
        .filter(|shell| removed_regions.contains(shell.region.as_str()))
        .map(|shell| shell.id.as_str())
        .collect();
    let removed_faces: BTreeSet<&str> = geometry
        .faces
        .iter()
        .filter(|face| {
            faces.contains(face.id.as_str()) || removed_shells.contains(face.shell.as_str())
        })
        .map(|face| face.id.as_str())
        .collect();
    let opened_shells: BTreeSet<&str> = geometry
        .faces
        .iter()
        .filter(|face| removed_faces.contains(face.id.as_str()))
        .map(|face| face.shell.as_str())
        .filter(|shell| !removed_shells.contains(shell))
        .collect();
    let kept_face_shells: BTreeSet<&str> = geometry
        .faces
        .iter()
        .filter(|face| !removed_faces.contains(face.id.as_str()))
        .map(|face| face.shell.as_str())
        .collect();
    removed_shells.extend(geometry.shells.iter().filter_map(|shell| {
        let id = shell.id.as_str();
        (opened_shells.contains(id) && !kept_face_shells.contains(id) && shell.members.is_empty())
            .then_some(id)
    }));
    let opened_regions: BTreeSet<&str> = geometry
        .shells
        .iter()
        .filter(|shell| opened_shells.contains(shell.id.as_str()))
        .map(|shell| shell.region.as_str())
        .collect();
    let mut removed_regions = removed_regions;
    removed_regions.extend(geometry.regions.iter().filter_map(|region| {
        let id = region.id.as_str();
        (opened_regions.contains(id)
            && region
                .stated_shells()
                .all(|shell| removed_shells.contains(shell.as_str())))
        .then_some(id)
    }));
    let opened_bodies: BTreeSet<&str> = geometry
        .regions
        .iter()
        .filter(|region| opened_regions.contains(region.id.as_str()))
        .map(|region| region.body.as_str())
        .collect();
    let mut removed_bodies: BTreeSet<&str> = bodies.clone();
    removed_bodies.extend(geometry.bodies.iter().filter_map(|body| {
        let id = body.id.as_str();
        (opened_bodies.contains(id)
            && geometry
                .regions
                .iter()
                .filter(|region| region.body == body.id)
                .all(|region| removed_regions.contains(region.id.as_str())))
        .then_some(id)
    }));
    let removed_loops: BTreeSet<&str> = geometry
        .loops
        .iter()
        .filter(|loop_| removed_faces.contains(loop_.face.as_str()))
        .map(|loop_| loop_.id.as_str())
        .collect();
    let removed_coedges: BTreeSet<&str> = geometry
        .coedges
        .iter()
        .filter(|coedge| removed_loops.contains(coedge.owner_loop.as_str()))
        .map(|coedge| coedge.id.as_str())
        .collect();
    let mut removed_records = Vec::new();
    macro_rules! record_removed {
        ($($arena:ident: $set:ident),*) => {$(
            for record in geometry
                .$arena
                .iter()
                .filter(|record| $set.contains(record.id.as_str()))
            {
                removed_records.push(to_json(record)?);
            }
        )*};
    }
    record_removed!(
        bodies: removed_bodies,
        regions: removed_regions,
        shells: removed_shells,
        faces: removed_faces,
        loops: removed_loops,
        coedges: removed_coedges
    );
    let mut out = ir.clone();
    let written = &mut out.geometry;
    written
        .bodies
        .retain(|body| !removed_bodies.contains(body.id.as_str()));
    written
        .regions
        .retain(|region| !removed_regions.contains(region.id.as_str()));
    written
        .shells
        .retain(|shell| !removed_shells.contains(shell.id.as_str()));
    written
        .faces
        .retain(|face| !removed_faces.contains(face.id.as_str()));
    written
        .loops
        .retain(|loop_| !removed_loops.contains(loop_.id.as_str()));
    close_radial_rings(written, geometry, &removed_coedges);
    for region in &mut written.regions {
        region.retain_stated_shells(|shell| !removed_shells.contains(shell.as_str()));
    }
    for body in &mut written.bodies {
        if body.kind == BodyKind::Solid && opened_bodies.contains(body.id.as_str()) {
            body.kind = BodyKind::Sheet;
        }
    }
    drop_unreferenced(&mut out, &removed_records)?;
    Ok(out)
}

/// Leaves out the coedges in `removed` from `written`, the geometry of
/// `source` without them, and closes every radial ring that ran through one
/// past it.
fn close_radial_rings(
    written: &mut crate::document::Topology,
    source: &crate::document::Topology,
    removed: &BTreeSet<&str>,
) {
    let radial_next: HashMap<&str, &CoedgeId> = source
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), &coedge.radial_next))
        .collect();
    written
        .coedges
        .retain(|coedge| !removed.contains(coedge.id.as_str()));
    for coedge in &mut written.coedges {
        let mut next = coedge.radial_next.clone();
        for _ in 0..removed.len() {
            if !removed.contains(next.as_str()) {
                break;
            }
            next = radial_next[next.as_str()].clone();
        }
        coedge.radial_next = next;
    }
}

/// Leaves out of `out` each carrier that only `removed` referenced, keeping
/// those a record outside the geometry names.
fn drop_unreferenced(out: &mut CadIr, removed: &[Json]) -> Result<(), CodecError> {
    let mut outside = out.clone();
    outside.geometry = crate::document::Topology::default();
    let outside = to_json(&outside)?;
    let mut named_outside = Vec::new();
    strings(&outside, &mut named_outside);
    let named_outside: BTreeSet<String> = named_outside.into_iter().map(str::to_owned).collect();
    remove_unreferenced(&mut out.geometry, removed, &named_outside)
}

fn to_json<T: serde::Serialize>(record: &T) -> Result<Json, CodecError> {
    cadmpeg_core::serde_erase::to_json_value(record).map_err(CodecError::malformed)
}

/// Every string a JSON value holds.
fn strings<'a>(value: &'a Json, into: &mut Vec<&'a str>) {
    match value {
        Json::String(text) => into.push(text),
        Json::Array(items) => items.iter().for_each(|item| strings(item, into)),
        Json::Object(fields) => fields.values().for_each(|item| strings(item, into)),
        Json::Null | Json::Bool(_) | Json::Number(_) => {}
    }
}

/// Leaves out each record of a carrier arena that only `removed` referenced,
/// and in turn each that only records left out referenced. A record that a
/// record outside the geometry names, in `named_outside`, stays.
fn remove_unreferenced(
    geometry: &mut crate::document::Topology,
    removed: &[Json],
    named_outside: &BTreeSet<String>,
) -> Result<(), CodecError> {
    let value = to_json(geometry)?;
    let Json::Object(arenas) = &value else {
        return Ok(());
    };
    let mut arena_of: BTreeMap<&str, &str> = BTreeMap::new();
    let mut records: Vec<(&str, &Json)> = Vec::new();
    for (arena, items) in arenas {
        for record in items.as_array().into_iter().flatten() {
            if let Some(id) = record.get("id").and_then(Json::as_str) {
                arena_of.insert(id, arena.as_str());
                records.push((id, record));
            }
        }
    }
    let references = |record: &'_ Json, own: Option<&str>| -> BTreeSet<String> {
        let mut found = Vec::new();
        strings(record, &mut found);
        found
            .into_iter()
            .filter(|id| Some(*id) != own && arena_of.contains_key(id))
            .map(str::to_owned)
            .collect()
    };
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (id, record) in &records {
        let targets = references(record, Some(id));
        for target in &targets {
            *uses.entry(target.clone()).or_default() += 1;
        }
        outgoing.insert(id, targets);
    }
    let mut pending: Vec<String> = removed
        .iter()
        .flat_map(|record| references(record, None))
        .collect();
    let mut left_out: BTreeSet<String> = BTreeSet::new();
    while let Some(id) = pending.pop() {
        let carrier = arena_of
            .get(id.as_str())
            .is_some_and(|arena| CARRIER_ARENAS.contains(arena));
        if !carrier
            || left_out.contains(&id)
            || named_outside.contains(&id)
            || uses.get(&id).copied().unwrap_or(0) != 0
        {
            continue;
        }
        left_out.insert(id.clone());
        for target in outgoing.get(id.as_str()).into_iter().flatten() {
            if let Some(count) = uses.get_mut(target) {
                *count = count.checked_sub(1).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "{target} is referenced by fewer records than are left out"
                    ))
                })?;
            }
            pending.push(target.clone());
        }
    }
    let keep = |id: &str| !left_out.contains(id);
    geometry.edges.retain(|record| keep(record.id.as_str()));
    geometry.vertices.retain(|record| keep(record.id.as_str()));
    geometry.points.retain(|record| keep(record.id.as_str()));
    geometry.curves.retain(|record| keep(record.id.as_str()));
    geometry.surfaces.retain(|record| keep(record.id.as_str()));
    geometry.pcurves.retain(|record| keep(record.id.as_str()));
    geometry
        .procedural_surfaces
        .retain(|record| keep(record.id.as_str()));
    geometry
        .procedural_curves
        .retain(|record| keep(record.id.as_str()));
    Ok(())
}

#[cfg(test)]
mod tests;
