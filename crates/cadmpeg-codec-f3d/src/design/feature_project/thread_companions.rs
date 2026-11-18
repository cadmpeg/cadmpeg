// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use cadmpeg_core::CodecError;

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

pub(super) fn insert_positions<'a>(
    stream: &'a str,
    scopes_by_record: &HashMap<u32, &DesignParameterScope>,
    ordinals: &mut HashMap<(&'a str, u32), u64>,
) -> Result<(), CodecError> {
    let mut companions = HashMap::new();
    for thread in scopes_by_record.values() {
        let Some(construction) = thread.thread_construction() else {
            continue;
        };
        let Some(owner) = construction.form.hole_reference() else {
            continue;
        };
        let owner = owner.value.get();
        let (Some(hole), Some(&ordinal)) =
            (scopes_by_record.get(&owner), ordinals.get(&(stream, owner)))
        else {
            continue;
        };
        if hole.kind() != DesignFeatureKind::Hole
            || hole.record_index >= thread.record_index
            || hole.history_state_id() != thread.previous_history_state_id()
            || match hole.history_state_id() {
                Some(state) => thread
                    .history_state_id()
                    .is_none_or(|result| result == state),
                None => {
                    hole.previous_history_state_id().is_some()
                        || thread.history_state_id().is_some()
                }
            }
        {
            continue;
        }
        companions
            .entry(ordinal)
            .and_modify(|thread| *thread = None)
            .or_insert(Some(thread.record_index));
    }

    let mut insertions = Vec::new();
    insertions.extend(companions.into_iter().filter_map(|(ordinal, thread)| {
        let thread = thread?;
        (!ordinals.contains_key(&(stream, thread))).then_some((ordinal, thread))
    }));
    if insertions.is_empty() {
        return Ok(());
    }
    insertions.sort_unstable();
    let shifted_ordinal = |ordinal: u64, displacement: usize| {
        u64::try_from(displacement)
            .ok()
            .and_then(|displacement| ordinal.checked_add(displacement))
            .ok_or_else(|| CodecError::Malformed("Design feature ordinal exceeds u64".into()))
    };
    for ordinal in ordinals.values_mut() {
        let original = *ordinal;
        *ordinal = shifted_ordinal(
            original,
            insertions.partition_point(|(owner, _)| *owner < original),
        )?;
    }
    for (index, (owner, thread)) in insertions.into_iter().enumerate() {
        // discarded-value: each insertion names a previously unlisted scope.
        let _ = ordinals.insert((stream, thread), shifted_ordinal(owner, index + 1)?);
    }
    Ok(())
}
