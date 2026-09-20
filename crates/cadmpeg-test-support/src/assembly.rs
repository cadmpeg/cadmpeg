// SPDX-License-Identifier: Apache-2.0
//! Queries over validated assembly fixtures.

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{AssemblyGraph, OccurrenceParent};
use cadmpeg_ir::transform::Transform;

/// Compose an occurrence's placement through its validated ancestor chain.
/// Returns `None` if the fixture does not contain the occurrence.
pub fn resolved_transform(graph: &AssemblyGraph<'_>, id: &OccurrenceId) -> Option<Transform> {
    let mut occurrence = graph.occurrence(id)?;
    let mut chain = vec![occurrence.effective_transform().ok()?];
    while let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
        occurrence = graph.occurrence(parent)?;
        chain.push(occurrence.effective_transform().ok()?);
    }
    chain
        .into_iter()
        .rev()
        .try_fold(Transform::identity(), |parent, local| {
            parent.compose(local).ok()
        })
}
