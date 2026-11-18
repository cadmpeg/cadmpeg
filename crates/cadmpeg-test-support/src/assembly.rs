// SPDX-License-Identifier: Apache-2.0
//! Queries over assembly fixtures.

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

/// The occurrences from a root definition's occurrence down to `id`, when
/// each owning definition on the way is placed by exactly one occurrence.
pub fn chain<'ir>(ir: &'ir CadIr, id: &OccurrenceId) -> Option<Vec<&'ir OccurrenceId>> {
    let model = &ir.model;
    let mut chain = vec![model.occurrences.iter().find(|row| &row.id == id)?];
    while chain.len() <= model.occurrences.len() {
        let current = chain[chain.len() - 1];
        let owner = model
            .product_definitions
            .iter()
            .find(|definition| definition.occurrences.contains(&current.id))?;
        let mut placements = model
            .occurrences
            .iter()
            .filter(|row| row.prototype == owner.id);
        let Some(parent) = placements.next() else {
            chain.reverse();
            return Some(chain.into_iter().map(|row| &row.id).collect());
        };
        if placements.next().is_some() {
            return None;
        }
        chain.push(parent);
    }
    None
}

/// Compose an occurrence's placement through its chain from the root.
/// Returns `None` if the fixture does not contain the occurrence, a chain
/// member states no placement, or an owner on the way is placed more than
/// once.
pub fn resolved_transform(ir: &CadIr, id: &OccurrenceId) -> Option<Transform> {
    chain(ir, id)?
        .into_iter()
        .try_fold(Transform::identity(), |parent, member| {
            let occurrence = ir.model.occurrences.iter().find(|row| &row.id == member)?;
            parent.compose(occurrence.transform?).ok()
        })
}
