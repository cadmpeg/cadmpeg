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

rewrite_native_record!(DesignDerivedBaseOwner, []; {record_index, repeat_offset});

/// The derived base scopes among `scopes` that name one owner, keyed by
/// stream and owner record index, in record-index order. A group holds only
/// when each scope after the first takes the history state of the scope
/// before it as its previous history state.
pub(crate) fn derived_base_groups<'a>(
    scopes: impl IntoIterator<Item = &'a super::scope::DesignParameterScope>,
) -> std::collections::HashMap<(&'a str, u32), Vec<&'a super::scope::DesignParameterScope>> {
    let mut groups = std::collections::HashMap::<_, Vec<_>>::new();
    for scope in scopes {
        let Some(owner) = scope.derived_base_owner else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        groups
            .entry((stream, owner.record_index.value))
            .or_default()
            .push(scope);
    }
    groups.retain(|_, group| {
        group.sort_by_key(|scope| scope.record_index);
        group.windows(2).all(|pair| {
            pair[0].history_state_id().is_some()
                && pair[1].previous_history_state_id() == pair[0].history_state_id()
        })
    });
    groups
}

/// The holding derived base group among `scopes` that contains `scope`.
pub(crate) fn derived_base_group<'a>(
    scope: &super::scope::DesignParameterScope,
    scopes: &'a [super::scope::DesignParameterScope],
) -> Option<Vec<&'a super::scope::DesignParameterScope>> {
    let owner = scope.derived_base_owner?.record_index.value;
    let stream = crate::ids::native_stream(&scope.id);
    derived_base_groups(scopes.iter().filter(|candidate| {
        crate::ids::native_stream(&candidate.id) == stream
            && candidate
                .derived_base_owner
                .is_some_and(|other| other.record_index.value == owner)
    }))
    .into_values()
    .next()
}
