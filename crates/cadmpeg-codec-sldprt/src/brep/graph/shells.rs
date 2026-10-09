// SPDX-License-Identifier: Apache-2.0
//! Shell membership, connected components and final face assignments.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::ShellId;
use cadmpeg_ir::topology::{Coedge, Face, Loop};

struct ShellMembers<'ctx> {
    values: Vec<usize>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> ShellMembers<'ctx> {
    fn empty(ctx: &'ctx DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        Ok(Self { values: Vec::new(), storage: ctx.reserve_scoped(0, operation)? })
    }

    fn push(&mut self, ctx: &DecodeContext<'_>, face: usize, operation: &'static str) -> Result<(), CodecError> {
        self.storage.with_storage(|| ctx.push_vec(&mut self.values, face, operation))
    }
}

struct FaceShellAssignment<'ctx> {
    face: usize,
    shell: ShellId,
    storage: ScopedReservation<'ctx>,
}

pub(super) struct FaceShellAssignments<'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    values: Vec<FaceShellAssignment<'ctx>>,
    _shell: ScopedReservation<'ctx>,
}

impl FaceShellAssignments<'_, '_> {
    pub(super) fn apply(self, faces: &mut [Face]) -> Result<(), CodecError> {
        for assignment in self.context.admit_iter(self.values, "assign Parasolid face shells")? {
            if let Some(face) = faces.get_mut(assignment.face) {
                face.shell = assignment.storage.commit_value(assignment.shell)?;
            }
        }
        Ok(())
    }
}

pub(super) struct ShellComponents<'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    values: Vec<ShellMembers<'ctx>>,
    _shell: ScopedReservation<'ctx>,
}

impl ShellComponents<'_, '_> {
    pub(super) fn visit(self, mut visit: impl FnMut(usize, &[usize]) -> Result<(), CodecError>) -> Result<(), CodecError> {
        for (ordinal, members) in self.context.admit_iter(self.values, "emit Parasolid shell components")?.enumerate() {
            visit(ordinal, &members.values)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn into_retained(self) -> Result<Vec<Vec<usize>>, CodecError> {
        let mut output = Vec::new();
        for members in self.context.admit_iter(self.values, "emit Parasolid shell components")? {
            let values = members.storage.commit_value(members.values)?;
            self.context.push_vec(&mut output, values, "group Parasolid shell components")?;
        }
        Ok(output)
    }
}

/// The table shell remains bounded as a whole through removals. Each removed
/// member vector has a separate lifetime; no per-node refund is inferred.
pub(super) struct ShellFaceIndex<'a, 'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    faces: &'a [Face],
    shells: BTreeMap<&'a str, ShellMembers<'ctx>>,
    neighbors: BTreeMap<usize, BTreeSet<usize>>,
    assignments: Vec<FaceShellAssignment<'ctx>>,
    _shell_storage: ScopedReservation<'ctx>,
    _neighbor_storage: ScopedReservation<'ctx>,
    assignment_storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx, 'arena> ShellFaceIndex<'a, 'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>, faces: &'a [Face], loops: &[Loop], coedges: &[Coedge]) -> Result<Self, CodecError> {
        let mut neighbor_storage = ctx.reserve_scoped(0, "index Parasolid face neighbors")?;
        let mut neighbors = BTreeMap::<usize, BTreeSet<usize>>::new();
        let (_, build_storage) = ctx.with_scoped_storage("index Parasolid shell faces", || {
            let face_indices = ctx.collect_hash_map(faces.iter().enumerate().map(|(index, face)| (face.id.as_str(), index)), "index Parasolid shell faces")?;
            let mut loop_faces = HashMap::new();
            for loop_ in ctx.admit_iter(loops, "index Parasolid shell loops")? {
                if let Some(&face) = ctx.get_hash_map(&face_indices, loop_.face.as_str(), "index Parasolid shell loops")? {
                    ctx.insert_hash_map(&mut loop_faces, loop_.id.as_str(), face, "index Parasolid shell loops")?;
                }
            }
            let mut faces_by_edge = BTreeMap::<(&str, &str), BTreeSet<usize>>::new();
            for coedge in ctx.admit_iter(coedges, "index Parasolid shell edges")? {
                if let Some(&face) = ctx.get_hash_map(&loop_faces, coedge.owner_loop.as_str(), "index Parasolid shell edges")? {
                    ctx.insert_btree_group_set(&mut faces_by_edge, (coedge.edge.as_str(), faces[face].shell.as_str()), face, "index Parasolid shell edges", "index Parasolid edge faces")?;
                }
            }
            for (_, edge_faces) in ctx.admit_iter(&faces_by_edge, "index Parasolid face neighbors")? {
                let mut edge_faces = ctx.admit_iter(edge_faces, "index Parasolid face neighbors")?;
                let Some(&root) = edge_faces.next() else { continue; };
                for &other in edge_faces {
                    for (face, neighbor) in [(root, other), (other, root)] {
                        neighbor_storage.with_storage(|| ctx.insert_btree_group_set(&mut neighbors, face, neighbor, "index Parasolid face neighbors", "index Parasolid face neighbors"))?;
                    }
                }
            }
            Ok::<_, CodecError>(())
        })?;
        drop(build_storage);
        let mut shell_storage = ctx.reserve_scoped(0, "group Parasolid shell faces")?;
        let mut shells = BTreeMap::<&str, ShellMembers<'ctx>>::new();
        for (index, face) in ctx.admit_iter(faces, "group Parasolid shell faces")?.enumerate() {
            let key = face.shell.as_str();
            match shell_storage.with_storage(|| ctx.entry_btree_map(&mut shells, key, "group Parasolid shell faces"))? {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().push(ctx, index, "collect Parasolid shell faces")?;
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let (values, storage) = ctx.with_scoped_storage("collect Parasolid shell faces", || {
                        let mut values = ctx.collection_vec(1, "collect Parasolid shell faces")?;
                        values.push(index);
                        Ok::<_, CodecError>(values)
                    })?;
                    entry.insert(ShellMembers { values, storage });
                }
            }
        }
        Ok(Self { context: ctx, faces, shells, neighbors, assignments: Vec::new(), _shell_storage: shell_storage, _neighbor_storage: neighbor_storage, assignment_storage: ctx.reserve_scoped(0, "collect Parasolid face shells")? })
    }

    pub(super) fn faces(&self) -> &'a [Face] { self.faces }

    #[cfg(test)]
    pub(super) fn neighbors(&self) -> &BTreeMap<usize, BTreeSet<usize>> { &self.neighbors }

    pub(super) fn assign(&mut self, face: usize, shell: &ShellId) -> Result<(), CodecError> {
        let ctx = self.context;
        let (shell, storage) = ctx.with_scoped_storage("hold Parasolid face shell identity", || shell.try_clone_for_decode(ctx, "SLDPRT decoded identity copy"))?;
        let assignment = FaceShellAssignment { face, shell, storage };
        self.assignment_storage.with_storage(|| ctx.push_vec(&mut self.assignments, assignment, "collect Parasolid face shells"))
    }

    pub(super) fn into_assignments(self) -> FaceShellAssignments<'ctx, 'arena> {
        FaceShellAssignments { context: self.context, values: self.assignments, _shell: self.assignment_storage }
    }

    pub(super) fn take_components(&mut self, shell: &str) -> Result<ShellComponents<'ctx, 'arena>, CodecError> {
        const WALK: &str = "walk Parasolid shell components";
        let ctx = self.context;
        let mut component_shell = ctx.reserve_scoped(0, WALK)?;
        let mut components = Vec::new();
        let Some(candidates) = ctx.remove_btree_map(&mut self.shells, shell, "select Parasolid shell faces")? else {
            return Ok(ShellComponents { context: ctx, values: components, _shell: component_shell });
        };
        let (_, workspace) = ctx.with_scoped_storage(WALK, || {
            let members = ctx.collect_btree_set(candidates.values.iter().copied(), WALK)?;
            let mut assigned = BTreeSet::new();
            let mut pending = Vec::new();
            for &face in ctx.admit_iter(&candidates.values, WALK)? {
                if !ctx.insert_btree_set(&mut assigned, face, WALK)? { continue; }
                let mut component = ShellMembers::empty(ctx, "group Parasolid shell components")?;
                ctx.push_vec(&mut pending, face, WALK)?;
                loop {
                    ctx.charge_work(1, WALK)?;
                    let Some(current) = pending.pop() else { break; };
                    component.push(ctx, current, "group Parasolid shell components")?;
                    let Some(values) = ctx.get_btree_map(&self.neighbors, &current, WALK)? else { continue; };
                    for &neighbor in ctx.admit_iter(values, "walk Parasolid shell component neighbors")? {
                        if ctx.contains_btree_set(&members, &neighbor, WALK)? && ctx.insert_btree_set(&mut assigned, neighbor, WALK)? {
                            ctx.push_vec(&mut pending, neighbor, WALK)?;
                        }
                    }
                }
                ctx.stable_sort_by_key(&mut component.values, |index: &usize| self.faces[*index].id.as_str(), Ord::cmp, "sort Parasolid shell faces")?;
                component_shell.with_storage(|| ctx.push_vec(&mut components, component, "group Parasolid shell components"))?;
            }
            Ok::<_, CodecError>(())
        })?;
        drop(workspace);
        drop(candidates);
        Ok(ShellComponents { context: ctx, values: components, _shell: component_shell })
    }
}

#[cfg(test)]
mod tests;
