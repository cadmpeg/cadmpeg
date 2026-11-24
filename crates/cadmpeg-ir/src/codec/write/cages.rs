// SPDX-License-Identifier: Apache-2.0
//! The document a writer that writes no subdivision cages writes (§5.6).
//!
//! A cage outside a form's `cage` belongs to a body of its own, whose one
//! region holds one shell that lists the cage and nothing else. A writer that
//! leaves the cage out leaves that body out with it, so the body is neither
//! written as an empty solid or sheet nor charged twice: the cage stays in
//! `subds`, where the writer's coverage of that arena charges its loss.

use std::collections::BTreeSet;

use crate::document::CadIr;
use crate::ids::{BodyId, RegionId, ShellId};

/// `ir` without the bodies that hold subdivision cages (see
/// [`remove_cage_bodies`]). `None` where no body holds a cage.
#[must_use]
pub fn without_cage_bodies(ir: &CadIr) -> Option<CadIr> {
    if !ir
        .geometry
        .shells
        .iter()
        .any(|shell| shell.subds().next().is_some())
    {
        return None;
    }
    let mut out = ir.clone();
    remove_cage_bodies(&mut out).then_some(out)
}

/// Removes each body every region of which holds only shells that hold a
/// subdivision cage, with those regions and shells, and each reference to
/// the body from a product definition, an instance's bodies, a design
/// configuration, and a design scope's current states and unproduced bodies.
/// The cages stay in `subds`. Returns whether a body was removed.
pub fn remove_cage_bodies(ir: &mut CadIr) -> bool {
    let geometry = &ir.geometry;
    let cage_shells: BTreeSet<&ShellId> = geometry
        .shells
        .iter()
        .filter(|shell| shell.subds().next().is_some())
        .map(|shell| &shell.id)
        .collect();
    if cage_shells.is_empty() {
        return false;
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
    let bodies: BTreeSet<BodyId> = cage_regions
        .difference(&other_regions)
        .map(|&body| body.clone())
        .collect();
    if bodies.is_empty() {
        return false;
    }
    let regions: BTreeSet<RegionId> = geometry
        .regions
        .iter()
        .filter(|region| bodies.contains(&region.body))
        .map(|region| region.id.clone())
        .collect();
    let shells: BTreeSet<ShellId> = geometry
        .regions
        .iter()
        .filter(|region| bodies.contains(&region.body))
        .flat_map(|region| region.stated_shells().cloned())
        .collect();
    let geometry = &mut ir.geometry;
    geometry.bodies.retain(|body| !bodies.contains(&body.id));
    geometry
        .regions
        .retain(|region| !regions.contains(&region.id));
    geometry.shells.retain(|shell| !shells.contains(&shell.id));
    let kept = |body: &BodyId| !bodies.contains(body);
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
    true
}
