// SPDX-License-Identifier: Apache-2.0
//! The timeline record that owns a `Derived Base Feature` scope.

use crate::records::identity::Located;
use serde::{Deserialize, Serialize};

/// The owner record that a derived base frame names at its owner-reference
/// offset and again twelve bytes before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignDerivedBaseOwner {
    /// Owner record index, at the offset of its first reference.
    pub(crate) record_index: Located<u32>,
    /// Byte offset of the repeated reference.
    pub(crate) repeat_offset: u64,
}

impl DesignDerivedBaseOwner {
    /// Whether the two references sit at their offsets in the frame from
    /// `byte_offset` to `paired_byte_offset`.
    pub(crate) fn frame_holds(&self, byte_offset: u64, paired_byte_offset: u64) -> bool {
        byte_offset.checked_add(cadmpeg_core::decode::u64_from_index(
            crate::layout::derived_base_owner_reference::RECORD_INDEX,
        )) == Some(self.record_index.offset)
            && paired_byte_offset.checked_sub(cadmpeg_core::decode::u64_from_index(REPEAT_FROM_END))
                == Some(self.repeat_offset)
    }
}

/// Distance from the end of a derived base frame to the repeated owner
/// reference.
pub(crate) const REPEAT_FROM_END: usize = 12;

/// Whether `scope` is a `Derived Base Feature` scope.
pub(crate) fn is_derived_base(scope: &super::scope::DesignParameterScope) -> bool {
    matches!(
        scope.kind_name(),
        "Derived Base Feature" | "DerivedBaseFeature"
    )
}

rewrite_native_record!(DesignDerivedBaseOwner, []; {record_index, repeat_offset});

/// The derived base scopes among `scopes` that name one owner, keyed by
/// stream and owner record index, in record-index order. A group holds only
/// when each scope after the first takes the history state of the scope
/// before it as its previous history state, and no derived base scope that
/// names no owner continues the group or precedes it.
pub(crate) fn derived_base_groups<'a>(
    scopes: impl IntoIterator<Item = &'a super::scope::DesignParameterScope>,
) -> std::collections::HashMap<(&'a str, u32), Vec<&'a super::scope::DesignParameterScope>> {
    let mut groups = std::collections::HashMap::<_, Vec<_>>::new();
    let mut ownerless = Vec::new();
    for scope in scopes.into_iter().filter(|scope| is_derived_base(scope)) {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(owner) = scope.derived_base_owner else {
            ownerless.push((stream, scope));
            continue;
        };
        groups
            .entry((stream, owner.record_index.value))
            .or_default()
            .push(scope);
    }
    groups.retain(|&(stream, _), group| {
        group.sort_by_key(|scope| scope.record_index);
        let chained = group.windows(2).all(|pair| {
            pair[0].history_state_id().is_some()
                && pair[1].previous_history_state_id() == pair[0].history_state_id()
        });
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            return false;
        };
        let adjoined = ownerless.iter().any(|&(other_stream, other)| {
            other_stream == stream
                && ((other.previous_history_state_id().is_some()
                    && other.previous_history_state_id() == last.history_state_id())
                    || (other.history_state_id().is_some()
                        && other.history_state_id() == first.previous_history_state_id()))
        });
        chained && !adjoined
    });
    groups
}

/// The holding derived base group among `scopes` that contains `scope`.
pub(crate) fn derived_base_group<'a>(
    scope: &super::scope::DesignParameterScope,
    scopes: &'a [super::scope::DesignParameterScope],
) -> Option<Vec<&'a super::scope::DesignParameterScope>> {
    let owner = scope.derived_base_owner?.record_index.value;
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    derived_base_groups(scopes).remove(&(stream, owner))
}

/// Source property of a feature projected at a derive owner's timeline
/// position: the record indices of its derived base scopes, in record-index
/// order, separated by commas.
pub(crate) fn derived_base_scopes_property() -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::nonblank_literal!("derived_base_scopes")
}

/// The derived base scopes of `feature` when it is projected at a derive
/// owner's timeline position, in record-index order.
pub(crate) fn feature_derived_base_scopes<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [super::scope::DesignParameterScope],
) -> Option<Vec<&'a super::scope::DesignParameterScope>> {
    let records = feature
        .source_properties
        .get(&derived_base_scopes_property())?;
    let stream = feature
        .native_ref
        .as_deref()
        .and_then(crate::ids::native_stream)?;
    records
        .split(',')
        .map(|record| {
            let record = record.parse::<u32>().ok()?;
            scopes.iter().find(|scope| {
                scope.record_index == record && crate::ids::native_stream(&scope.id) == Some(stream)
            })
        })
        .collect()
}

/// The outputs of a derived base group feature in the record-index order of
/// its scopes, each scope contributing the outputs of its own history state;
/// every other feature keeps `outputs`.
pub(crate) fn derived_base_output_order(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &[super::scope::DesignParameterScope],
    history_index: usize,
    state_outputs: &std::collections::HashMap<(usize, i64), Option<Vec<i64>>>,
    outputs: Vec<i64>,
) -> Vec<i64> {
    let Some(group) = feature_derived_base_scopes(feature, scopes) else {
        return outputs;
    };
    let mut ordered = Vec::new();
    for scope in group {
        let Some(Some(slots)) = scope
            .history_state_id()
            .and_then(|state| state_outputs.get(&(history_index, state)))
        else {
            continue;
        };
        for slot in slots {
            if outputs.contains(slot) && !ordered.contains(slot) {
                ordered.push(*slot);
            }
        }
    }
    for slot in outputs {
        if !ordered.contains(&slot) {
            ordered.push(slot);
        }
    }
    ordered
}

#[cfg(test)]
mod tests;
