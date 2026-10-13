// SPDX-License-Identifier: Apache-2.0
//! Fixed-step selected reference sources for edge recipe queries.

use crate::records::topology::edge_identity::DesignEdgeOperand;

/// Yield every selected ordinal, including invalid references.
/// Query consumers must admit a source step before advancing this iterator.
pub(super) fn edge_operand_reference_edge_sets(
    operand: &DesignEdgeOperand,
) -> impl ExactSizeIterator<Item = Option<&[i64]>> + '_ {
    let terminal = operand.recipe_reference_contexts.is_empty();
    let source_count = if terminal {
        operand.terminal_reference_edge_slots.len()
    } else {
        operand.recipe_reference_contexts.len()
    };
    let selected_count = operand
        .local_topology_references
        .as_ref()
        .map_or(source_count, Vec::len);
    (0..selected_count).map(move |selected| {
        let source = match &operand.local_topology_references {
            Some(ordinals) => usize::try_from(ordinals.get(selected)?.get())
                .ok()?
                .checked_sub(1)?,
            None => selected,
        };
        if terminal {
            operand
                .terminal_reference_edge_slots
                .get(source)
                .map(Vec::as_slice)
        } else {
            operand
                .recipe_reference_contexts
                .get(source)
                .map(|context| context.changed_reference_edge_slots.as_slice())
        }
    })
}

#[cfg(test)]
mod tests;
