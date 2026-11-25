// SPDX-License-Identifier: Apache-2.0
//! The document a writer that writes no subdivision cages writes (§5.6).
//!
//! A cage outside a form's `cage` belongs to a body of its own, whose one
//! region holds one shell that lists the cage and nothing else. A writer that
//! leaves the cage out leaves that body out with it: the body is not written
//! as an empty solid or sheet, the export census does not count it, and the
//! loss that names the cage names the body too ([`cage_bodies_clause`]). The
//! cage stays in `subds`, where the writer's coverage of that arena charges
//! its loss.

use std::collections::BTreeSet;

use crate::document::{CadIr, Topology};
use crate::ids::{BodyId, RegionId, ShellId};
use crate::topology::ShellMember;

/// The shells whose members are subdivision cages only and that no face
/// names.
fn cage_shells(geometry: &Topology) -> BTreeSet<&ShellId> {
    let face_shells: BTreeSet<&ShellId> = geometry.faces.iter().map(|face| &face.shell).collect();
    geometry
        .shells
        .iter()
        .filter(|shell| {
            !shell.members.is_empty()
                && shell
                    .members
                    .iter()
                    .all(|member| matches!(member, ShellMember::Subd { .. }))
                && !face_shells.contains(&shell.id)
        })
        .map(|shell| &shell.id)
        .collect()
}

/// The bodies of `ir` every region of which holds only shells of subdivision
/// cages, sorted.
#[must_use]
pub fn cage_bodies(ir: &CadIr) -> Vec<BodyId> {
    let geometry = &ir.geometry;
    let cage_shells = cage_shells(geometry);
    if cage_shells.is_empty() {
        return Vec::new();
    }
    let mut cage_regions: BTreeSet<&BodyId> = BTreeSet::new();
    let mut other_regions: BTreeSet<&BodyId> = BTreeSet::new();
    for region in &geometry.regions {
        let mut shells = region.stated_shells().peekable();
        let only_cages = shells.peek().is_some() && shells.all(|shell| cage_shells.contains(shell));
        if only_cages {
            cage_regions.insert(&region.body);
        } else {
            other_regions.insert(&region.body);
        }
    }
    let mut bodies: Vec<BodyId> = geometry
        .bodies
        .iter()
        .map(|body| &body.id)
        .filter(|body| cage_regions.contains(body) && !other_regions.contains(body))
        .cloned()
        .collect();
    bodies.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
    bodies
}

/// `ir` without the bodies that hold subdivision cages (see
/// [`remove_cage_bodies`]). `None` where no body holds a cage.
#[must_use]
pub fn without_cage_bodies(ir: &CadIr) -> Option<CadIr> {
    if cage_shells(&ir.geometry).is_empty() {
        return None;
    }
    let mut out = ir.clone();
    (!remove_cage_bodies(&mut out).is_empty()).then_some(out)
}

/// Removes each of the [`cage_bodies`], with its regions and their shells,
/// and each reference to the body from a product definition, an instance's
/// bodies, a design configuration, and a design scope's current states and
/// unproduced bodies. The cages stay in `subds`. Returns the removed bodies,
/// sorted.
pub fn remove_cage_bodies(ir: &mut CadIr) -> Vec<BodyId> {
    let bodies = cage_bodies(ir);
    if bodies.is_empty() {
        return bodies;
    }
    let removed: BTreeSet<&BodyId> = bodies.iter().collect();
    let geometry = &ir.geometry;
    let regions: BTreeSet<RegionId> = geometry
        .regions
        .iter()
        .filter(|region| removed.contains(&region.body))
        .map(|region| region.id.clone())
        .collect();
    let shells: BTreeSet<ShellId> = geometry
        .regions
        .iter()
        .filter(|region| removed.contains(&region.body))
        .flat_map(|region| region.stated_shells().cloned())
        .collect();
    let geometry = &mut ir.geometry;
    geometry.bodies.retain(|body| !removed.contains(&body.id));
    geometry
        .regions
        .retain(|region| !regions.contains(&region.id));
    geometry.shells.retain(|shell| !shells.contains(&shell.id));
    let kept = |body: &BodyId| !removed.contains(body);
    for definition in &mut ir.model.product_definitions {
        definition.bodies.retain(kept);
        for instance in &mut definition.instance_bodies {
            instance.bodies.retain(kept);
        }
    }
    for configuration in &mut ir.model.configurations {
        if let Some(members) = &mut configuration.bodies {
            members.retain(kept);
        }
    }
    for scope in &mut ir.design.scopes {
        for state in &mut scope.current {
            state.bodies.retain(kept);
        }
        scope.unproduced.retain(kept);
    }
    bodies
}

/// What a loss that names omitted subdivision cages appends to name
/// `bodies`, the cage bodies left out with them: the count and the sorted
/// ids, or nothing where `bodies` is empty.
#[must_use]
pub fn cage_bodies_clause(bodies: &[BodyId]) -> String {
    if bodies.is_empty() {
        return String::new();
    }
    format!(
        "; {} body record(s) that hold only these cages are not written: {}",
        bodies.len(),
        bodies
            .iter()
            .map(BodyId::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    )
}
