// SPDX-License-Identifier: Apache-2.0
//! Binding of `Public Access` records to `Shape Contents` objects and to the
//! `Brep Data` tree (`ironcad.md` §§5, 6).

use std::collections::HashMap;

use cadmpeg_core::decode::u64_from_index;

use crate::placement::{Role, SceneRecord};
use crate::shape_contents::{ObjectKind, SceneObject};
use crate::tree::Assembly;

/// One part result record of the tree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResultSpan {
    /// The offset where the node before the record starts.
    pub(crate) child_start: usize,
    /// The offset where the record ends.
    pub(crate) end: usize,
    /// The tree assembly that holds the pair.
    pub(crate) assembly: Option<usize>,
}

/// What one record places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// The body of the part result record at this index, in the assembly
    /// of its pair.
    Part(usize),
    /// The tree assembly at this index.
    Assembly(usize),
}

/// How the records of a scene bind.
#[derive(Debug, Default)]
pub(crate) struct Binding {
    /// What each record places, by record index.
    pub(crate) places: Vec<Option<Place>>,
    /// Records that repeat an object that an earlier record binds: members
    /// of an assembly copy, which the copy places.
    pub(crate) repeats: Vec<bool>,
    /// Records whose place comes from a copy path whose assembly identifiers
    /// match no result record of the source identifier.
    pub(crate) unmatched_paths: Vec<bool>,
}

/// Binds each record to its scene object, and each object to a part result
/// record or a tree assembly by its `end`.
///
/// A record matches an unused object of the kind its role prefers, of its
/// name, and of its identifier; when no object of that kind, name, and
/// identifier exists, it matches by name alone. Among several candidates the
/// first after the last match is taken, else the first.
pub(crate) fn bind(
    records: &[SceneRecord],
    objects: &[SceneObject],
    results: &[ResultSpan],
    assemblies: &[Assembly],
) -> Binding {
    let feature = |object: &SceneObject| {
        let next = results.partition_point(|span| u64_from_index(span.end) <= object.end);
        object.kind == ObjectKind::Body
            && results
                .get(next)
                .is_some_and(|span| object.end > u64_from_index(span.child_start))
    };
    let scene: Vec<&SceneObject> = objects
        .iter()
        .filter(|object| object.kind != ObjectKind::Other && !feature(object))
        .collect();
    let mut index = Index::default();
    for (at, object) in scene.iter().enumerate() {
        index
            .by_name
            .entry(object.name.as_str())
            .or_default()
            .push(at);
        index.by_id.entry(object.id).or_default().push(at);
    }
    let mut used = vec![false; scene.len()];
    let mut cursor = None;
    let mut binding = Binding {
        places: vec![None; records.len()],
        repeats: vec![false; records.len()],
        unmatched_paths: vec![false; records.len()],
    };
    let mut assembly_objects: Vec<(usize, usize)> = Vec::new();
    for (position, record) in records.iter().enumerate() {
        let order = if record.role == Role::Assembly && !record.copy {
            [ObjectKind::Assembly, ObjectKind::Body]
        } else {
            [ObjectKind::Body, ObjectKind::Assembly]
        };
        let mut chosen = None;
        for kind in order {
            match candidate(record, &scene, &index, &used, cursor, kind) {
                Match::Found(at) => {
                    chosen = Some(at);
                    break;
                }
                Match::Repeat => {
                    binding.repeats[position] = true;
                    break;
                }
                Match::None => {}
            }
        }
        let Some(at) = chosen else {
            continue;
        };
        used[at] = true;
        cursor = Some(at);
        let object = scene[at];
        binding.places[position] = match object.kind {
            ObjectKind::Body => {
                let (place, unmatched) = part(record, object, &scene, results, assemblies);
                binding.unmatched_paths[position] = unmatched;
                place
            }
            ObjectKind::Assembly => {
                let place = assembly(record, object, &assembly_objects, &scene, assemblies);
                if let (Some(Place::Assembly(tree)), false) = (place, record.copy) {
                    assembly_objects.push((at, tree));
                }
                place
            }
            ObjectKind::Other => None,
        };
    }
    binding
}

/// The scene objects by name and by identifier, in stored order.
#[derive(Default)]
struct Index<'a> {
    by_name: HashMap<&'a str, Vec<usize>>,
    by_id: HashMap<u32, Vec<usize>>,
}

/// The result of a search for the object of one record.
enum Match {
    Found(usize),
    /// Objects of the record's kind, name, and identifier exist and are all
    /// bound.
    Repeat,
    None,
}

fn candidate(
    record: &SceneRecord,
    scene: &[&SceneObject],
    index: &Index<'_>,
    used: &[bool],
    cursor: Option<usize>,
    kind: ObjectKind,
) -> Match {
    let of_kind = |object: &SceneObject| object.kind == kind;
    let after = |at: &usize| cursor.is_none_or(|last| *at > last);
    if record.name.is_empty() {
        let by_id = record
            .id
            .and_then(|id| index.by_id.get(&id))
            .map_or(&[][..], Vec::as_slice);
        if let Some(at) = by_id
            .iter()
            .find(|at| after(at) && !used[**at] && of_kind(scene[**at]))
        {
            return Match::Found(*at);
        }
        let opens_assembly = by_id
            .iter()
            .any(|at| !used[*at] && scene[*at].kind == ObjectKind::Assembly);
        if record.copy || (!by_id.is_empty() && !opens_assembly) {
            return Match::Repeat;
        }
        return (0..scene.len())
            .find(|at| after(at) && !used[*at] && of_kind(scene[*at]))
            .map_or(Match::None, Match::Found);
    }
    let pool = index
        .by_name
        .get(record.name.as_str())
        .map_or(&[][..], Vec::as_slice);
    let pick = |matches: &dyn Fn(&SceneObject) -> bool| {
        let open = |at: &&usize| !used[**at] && matches(scene[**at]);
        let first = pool.iter().find(|at| after(at) && open(at));
        first.or_else(|| pool.iter().find(open)).copied()
    };
    if let Some(id) = record.id {
        let same = |object: &SceneObject| of_kind(object) && object.id == id;
        if pool.iter().any(|at| same(scene[*at])) {
            return pick(&same).map_or(Match::Repeat, Match::Found);
        }
    }
    pick(&of_kind).map_or(Match::None, Match::Found)
}

/// The part result record whose end is `end`.
fn result_at(results: &[ResultSpan], end: u64) -> Option<usize> {
    results
        .iter()
        .position(|span| u64_from_index(span.end) == end)
}

/// The identifiers of the assembly objects that hold a pair in tree
/// assembly `tree`, from the root down. Assembly nodes that end at one offset
/// take the assembly objects with that end in the order the nodes end.
fn ancestry(tree: Option<usize>, scene: &[&SceneObject], assemblies: &[Assembly]) -> Vec<u32> {
    let id_of = |index: usize| {
        let end = u64_from_index(assemblies[index].end);
        let rank = (0..index)
            .filter(|earlier| u64_from_index(assemblies[*earlier].end) == end)
            .count();
        scene
            .iter()
            .filter(|object| object.kind == ObjectKind::Assembly && object.end == end)
            .nth(rank)
            .map(|object| object.id)
    };
    let mut ids = Vec::new();
    let mut at = tree;
    while let Some(index) = at {
        ids.extend(id_of(index));
        at = assemblies[index].parent;
    }
    ids.reverse();
    ids
}

/// The place of a record bound to a body object: the result record that ends
/// at the object's end, else the result record of the body object that the
/// first resolvable copy path names. The flag is set when no source result
/// lies in the assemblies that the path names, and the place falls back to a
/// source of the object's name or to the first source.
fn part(
    record: &SceneRecord,
    object: &SceneObject,
    scene: &[&SceneObject],
    results: &[ResultSpan],
    assemblies: &[Assembly],
) -> (Option<Place>, bool) {
    if let Some(result) = result_at(results, object.end) {
        return (Some(Place::Part(result)), false);
    }
    let resolved = record.paths.iter().find_map(|path| {
        let (&source, prefix) = path.split_last()?;
        let sources = || {
            scene
                .iter()
                .filter(|item| item.kind == ObjectKind::Body && item.id == source)
                .filter_map(|item| result_at(results, item.end).map(|result| (item, result)))
        };
        let placed = |result: usize| ancestry(results[result].assembly, scene, assemblies);
        let matched = sources().find(|(_, result)| placed(*result) == prefix);
        let unmatched = matched.is_none();
        matched
            .or_else(|| sources().find(|(item, _)| item.name == object.name))
            .or_else(|| sources().next())
            .map(|(_, result)| (Place::Part(result), unmatched))
    });
    resolved.map_or((None, false), |(place, unmatched)| (Some(place), unmatched))
}

/// The tree assembly of a record bound to an assembly object. Assembly nodes
/// that end at one offset take the assembly objects with that end in the
/// order the nodes end. A copy takes the assembly of the bound object with
/// its name and end.
fn assembly(
    record: &SceneRecord,
    object: &SceneObject,
    bound: &[(usize, usize)],
    scene: &[&SceneObject],
    assemblies: &[Assembly],
) -> Option<Place> {
    let ending: Vec<usize> = (0..assemblies.len())
        .filter(|at| u64_from_index(assemblies[*at].end) == object.end)
        .collect();
    if record.copy {
        let source = bound
            .iter()
            .find(|(at, _)| scene[*at].name == object.name && scene[*at].end == object.end);
        return source
            .map(|(_, tree)| *tree)
            .or_else(|| ending.first().copied())
            .map(Place::Assembly);
    }
    let taken = bound
        .iter()
        .filter(|(at, _)| scene[*at].end == object.end)
        .count();
    ending.get(taken).copied().map(Place::Assembly)
}

#[cfg(test)]
mod tests;
