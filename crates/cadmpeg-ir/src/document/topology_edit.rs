// SPDX-License-Identifier: Apache-2.0
//! Atomic ownership edits keep parent pointers and child lists reciprocal.

use std::collections::BTreeSet;

use super::{CadIr, DimensionEditError, Model};
use crate::ids::{BodyId, CoedgeId, FaceId, LoopId, RegionId, ShellId};
use crate::topology::{FaceLoops, LoopBoundaryRole};

/// An ownership edit cannot preserve the topology's structural invariants.
#[derive(Debug, thiserror::Error)]
pub enum TopologyEditError {
    /// A referenced topology entity does not exist.
    #[error("missing {kind} `{id}`")]
    MissingEntity {
        /// Kind of the missing entity.
        kind: &'static str,
        /// Missing identity.
        id: String,
    },
    /// The edit would lose a required boundary or introduce inconsistent ownership.
    #[error("invalid topology edit: {0}")]
    Invalid(String),
    /// Dependency invalidation could not be completed.
    #[error(transparent)]
    Invalidation(#[from] DimensionEditError),
}

fn missing(kind: &'static str, id: &str) -> TopologyEditError {
    TopologyEditError::MissingEntity {
        kind,
        id: id.to_owned(),
    }
}

fn body_of_shell(model: &Model, id: &ShellId) -> Result<BodyId, TopologyEditError> {
    let shell = model
        .shells
        .iter()
        .find(|row| row.id == *id)
        .ok_or_else(|| missing("shell", id.as_str()))?;
    model
        .regions
        .iter()
        .find(|row| row.id == shell.region)
        .map(|row| row.body.clone())
        .ok_or_else(|| missing("region", shell.region.as_str()))
}
fn body_of_face(model: &Model, id: &FaceId) -> Result<BodyId, TopologyEditError> {
    let face = model
        .faces
        .iter()
        .find(|row| row.id == *id)
        .ok_or_else(|| missing("face", id.as_str()))?;
    body_of_shell(model, &face.shell)
}
fn body_of_loop(model: &Model, id: &LoopId) -> Result<BodyId, TopologyEditError> {
    let boundary = model
        .loops
        .iter()
        .find(|row| row.id == *id)
        .ok_or_else(|| missing("loop", id.as_str()))?;
    body_of_face(model, &boundary.face)
}

impl CadIr {
    fn commit_ownership_edit(
        &mut self,
        candidate: Model,
        bodies: [BodyId; 2],
    ) -> Result<(), TopologyEditError> {
        let mut findings = Vec::new();
        // Admit ownership before either topology or invalidation state changes.
        crate::validate::check_topology_reciprocity(&candidate, &mut findings);
        if let Some(finding) = findings.first() {
            return Err(TopologyEditError::Invalid(finding.message.clone()));
        }
        let bodies: BTreeSet<_> = bodies.into_iter().collect();
        let producers: Vec<_> = candidate
            .features
            .iter()
            .filter(|feature| {
                feature
                    .evaluation
                    .outputs()
                    .iter()
                    .any(|body| bodies.contains(body))
            })
            .map(|feature| feature.id.clone())
            .collect();
        let original = std::mem::replace(&mut self.model, candidate);
        for feature in producers {
            if let Err(error) = self.mark_needs_recompute(&feature) {
                self.model = original;
                return Err(error.into());
            }
        }
        Ok(())
    }

    /// Move a region and update both bodies' ordered region lists.
    pub fn reparent_region(
        &mut self,
        id: &RegionId,
        body: &BodyId,
    ) -> Result<(), TopologyEditError> {
        let mut model = self.model.clone();
        let child = model
            .regions
            .iter()
            .position(|row| row.id == *id)
            .ok_or_else(|| missing("region", id.as_str()))?;
        let old = model.regions[child].body.clone();
        let old_parent = model
            .bodies
            .iter()
            .position(|row| row.id == old)
            .ok_or_else(|| missing("body", old.as_str()))?;
        let new_parent = model
            .bodies
            .iter()
            .position(|row| row.id == *body)
            .ok_or_else(|| missing("body", body.as_str()))?;
        if old == *body {
            return Ok(());
        }
        model.bodies[old_parent]
            .regions
            .retain(|member| member != id);
        model.bodies[new_parent].regions.push(id.clone());
        model.regions[child].body = body.clone();
        self.commit_ownership_edit(model, [old, body.clone()])
    }

    /// Move a shell and update both regions' ordered shell lists.
    pub fn reparent_shell(
        &mut self,
        id: &ShellId,
        region: &RegionId,
    ) -> Result<(), TopologyEditError> {
        let mut model = self.model.clone();
        let child = model
            .shells
            .iter()
            .position(|row| row.id == *id)
            .ok_or_else(|| missing("shell", id.as_str()))?;
        let old = model.shells[child].region.clone();
        let old_parent = model
            .regions
            .iter()
            .position(|row| row.id == old)
            .ok_or_else(|| missing("region", old.as_str()))?;
        let new_parent = model
            .regions
            .iter()
            .position(|row| row.id == *region)
            .ok_or_else(|| missing("region", region.as_str()))?;
        if old == *region {
            return Ok(());
        }
        let bodies = [
            model.regions[old_parent].body.clone(),
            model.regions[new_parent].body.clone(),
        ];
        model.regions[old_parent]
            .shells
            .retain(|member| member != id);
        model.regions[new_parent].shells.push(id.clone());
        model.shells[child].region = region.clone();
        self.commit_ownership_edit(model, bodies)
    }

    /// Move a face while preserving the nonempty shell invariant.
    pub fn reparent_face(&mut self, id: &FaceId, shell: &ShellId) -> Result<(), TopologyEditError> {
        let mut model = self.model.clone();
        let child = model
            .faces
            .iter()
            .position(|row| row.id == *id)
            .ok_or_else(|| missing("face", id.as_str()))?;
        let old = model.faces[child].shell.clone();
        let old_parent = model
            .shells
            .iter()
            .position(|row| row.id == old)
            .ok_or_else(|| missing("shell", old.as_str()))?;
        let new_parent = model
            .shells
            .iter()
            .position(|row| row.id == *shell)
            .ok_or_else(|| missing("shell", shell.as_str()))?;
        if old == *shell {
            return Ok(());
        }
        let bodies = [body_of_shell(&model, &old)?, body_of_shell(&model, shell)?];
        let faces = model.shells[old_parent]
            .faces()
            .iter()
            .filter(|member| *member != id)
            .cloned()
            .collect();
        model.shells[old_parent]
            .replace_faces(faces)
            .map_err(|error| TopologyEditError::Invalid(error.to_string()))?;
        model.shells[new_parent].add_face(id.clone());
        model.faces[child].shell = shell.clone();
        self.commit_ownership_edit(model, bodies)
    }

    /// Move a loop with an explicit classification at the destination.
    ///
    /// Moving a classified outer loop with remaining inner loops requires a
    /// separate explicit replacement of the old outer boundary.
    pub fn reparent_loop(
        &mut self,
        id: &LoopId,
        face: &FaceId,
        role: LoopBoundaryRole,
    ) -> Result<(), TopologyEditError> {
        let mut model = self.model.clone();
        let child = model
            .loops
            .iter()
            .position(|row| row.id == *id)
            .ok_or_else(|| missing("loop", id.as_str()))?;
        let old = model.loops[child].face.clone();
        let old_parent = model
            .faces
            .iter()
            .position(|row| row.id == old)
            .ok_or_else(|| missing("face", old.as_str()))?;
        let new_parent = model
            .faces
            .iter()
            .position(|row| row.id == *face)
            .ok_or_else(|| missing("face", face.as_str()))?;
        if old == *face {
            return Ok(());
        }
        let bodies = [body_of_face(&model, &old)?, body_of_face(&model, face)?];
        remove_loop(&mut model.faces[old_parent].loops, id)?;
        add_loop(&mut model.faces[new_parent].loops, id, role)?;
        model.loops[child].face = face.clone();
        self.commit_ownership_edit(model, bodies)
    }

    /// Move a coedge and its anchored vertex uses between nonempty rings.
    pub fn reparent_coedge(
        &mut self,
        id: &CoedgeId,
        boundary: &LoopId,
    ) -> Result<(), TopologyEditError> {
        let mut model = self.model.clone();
        let child = model
            .coedges
            .iter()
            .position(|row| row.id == *id)
            .ok_or_else(|| missing("coedge", id.as_str()))?;
        let old = model.coedges[child].owner_loop.clone();
        let old_parent = model
            .loops
            .iter()
            .position(|row| row.id == old)
            .ok_or_else(|| missing("loop", old.as_str()))?;
        let new_parent = model
            .loops
            .iter()
            .position(|row| row.id == *boundary)
            .ok_or_else(|| missing("loop", boundary.as_str()))?;
        if old == *boundary {
            return Ok(());
        }
        let bodies = [body_of_loop(&model, &old)?, body_of_loop(&model, boundary)?];
        let edges = model.loops[old_parent]
            .coedges()
            .iter()
            .filter(|member| *member != id)
            .cloned()
            .collect();
        let (moved, kept): (Vec<_>, Vec<_>) = model.loops[old_parent]
            .anchored_vertex_uses()
            .iter()
            .cloned()
            .partition(|use_| use_.after == *id);
        model.loops[old_parent]
            .replace_ring(edges, kept)
            .map_err(|error| TopologyEditError::Invalid(error.to_string()))?;
        let mut edges = model.loops[new_parent].coedges().to_vec();
        edges.push(id.clone());
        let mut uses = model.loops[new_parent].anchored_vertex_uses().to_vec();
        uses.extend(moved);
        model.loops[new_parent]
            .replace_ring(edges, uses)
            .map_err(|error| TopologyEditError::Invalid(error.to_string()))?;
        model.coedges[child].owner_loop = boundary.clone();
        self.commit_ownership_edit(model, bodies)
    }
}

fn remove_loop(loops: &mut FaceLoops, id: &LoopId) -> Result<(), TopologyEditError> {
    match loops {
        FaceLoops::Unspecified { loops } => loops.retain(|member| member != id),
        FaceLoops::Classified { outer, inner } if outer == id => {
            if !inner.is_empty() {
                return Err(TopologyEditError::Invalid(
                    "moving an outer loop requires an explicit replacement outer boundary".into(),
                ));
            }
            *loops = FaceLoops::unspecified(Vec::new());
        }
        FaceLoops::Classified { inner, .. } => inner.retain(|member| member != id),
    }
    Ok(())
}
fn add_loop(
    loops: &mut FaceLoops,
    id: &LoopId,
    role: LoopBoundaryRole,
) -> Result<(), TopologyEditError> {
    match (loops, role) {
        (FaceLoops::Unspecified { loops }, LoopBoundaryRole::Unspecified) => loops.push(id.clone()),
        (value @ FaceLoops::Unspecified { .. }, LoopBoundaryRole::Outer) if value.is_empty() => {
            *value = FaceLoops::classified(id.clone(), Vec::new())
        }
        (FaceLoops::Classified { inner, .. }, LoopBoundaryRole::Inner) => inner.push(id.clone()),
        _ => {
            return Err(TopologyEditError::Invalid(
                "destination loop classification requires an explicit boundary edit".into(),
            ))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
