// SPDX-License-Identifier: Apache-2.0
//! The timeline record that owns a `Derived Base Feature` scope.

use crate::records::identity::Located;
use cadmpeg_core::decode::u64_from_index;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_snapshot, DesignDerivedBaseSnapshot, "snapshot");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignDerivedBaseSource {
    pub(crate) parameter_body: Located<u64>,
    pub(crate) result_body: Located<u64>,
    pub(crate) owner_record: Located<u64>,
    pub(crate) source_body: Located<u64>,
    pub(crate) result_repeat_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_snapshot"
    )]
    pub(crate) snapshot: Option<DesignDerivedBaseSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignDerivedBaseSnapshot {
    pub(crate) history_body: cadmpeg_ir::ids::HistoryBodyId,
    pub(crate) body_slot: i64,
}

impl DesignDerivedBaseSource {
    pub(crate) fn frame_holds(&self, start: u64, end: u64) -> bool {
        use crate::layout::derived_base_source_pair_prefix as prefix;
        use cadmpeg_core::decode::u64_from_index;
        [
            (self.parameter_body.offset, prefix::PARAMETER_BODY_RECORD),
            (self.result_body.offset, prefix::RESULT_BODY_RECORD),
            (self.owner_record.offset, prefix::OWNER_RECORD),
            (self.source_body.offset, prefix::SOURCE_RECORD),
            (self.result_repeat_offset, prefix::REPEAT_RECORD),
        ]
        .into_iter()
        .all(|(actual, relative)| start.checked_add(u64_from_index(relative)) == Some(actual))
            && start
                .checked_add(u64_from_index(prefix::LEN))
                .is_some_and(|prefix_end| prefix_end <= end)
            && self
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.body_slot >= 0)
    }
}

rewrite_native_record!(DesignDerivedBaseSource, []; {parameter_body, result_body, owner_record, source_body, result_repeat_offset, snapshot});
rewrite_native_record!(DesignDerivedBaseSnapshot, []; {history_body, body_slot});

fn owner_record(scope: &super::scope::DesignParameterScope) -> Option<u32> {
    scope
        .derived_base_owner
        .map(|owner| owner.record_index.value)
        .or_else(|| {
            let source = scope.derived_base_source.as_ref()?;
            source.snapshot.as_ref()?;
            u32::try_from(source.owner_record.value).ok()
        })
}

/// A timeline owner confirmed by the repeated owner reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignDerivedBaseOwner {
    /// Owner record index, at the offset of its first reference.
    pub(crate) record_index: Located<u32>,
    /// Byte offset of the repeated reference.
    pub(crate) repeat_offset: u64,
    /// Saved result body named by the mirror-derived scope.
    #[serde(
        default,
        deserialize_with = "deserialize_result_body",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) result_body: Option<Located<u32>>,
}

cadmpeg_core::named_optional_field!(deserialize_result_body, Located<u32>, "result_body");

impl DesignDerivedBaseOwner {
    /// Whether the references are at the positions of an admitted owner form.
    pub(crate) fn frame_holds(&self, byte_offset: u64, paired_byte_offset: u64) -> bool {
        use crate::layout::{
            derived_base_mirror_companion as companion, derived_base_mirror_scope as mirror,
        };

        let first = byte_offset.checked_add(u64_from_index(
            crate::layout::derived_base_owner_reference::RECORD_INDEX,
        ));
        let in_frame = paired_byte_offset.checked_sub(u64_from_index(REPEAT_FROM_END))
            == Some(self.repeat_offset);
        let in_companion = self.record_index.value != 0
            && byte_offset.checked_add(u64_from_index(mirror::PAIRED_HEADER))
                == Some(paired_byte_offset)
            && byte_offset.checked_add(u64_from_index(mirror::LEN + companion::OWNER_REFERENCE))
                == Some(self.repeat_offset);
        first == Some(self.record_index.offset)
            && match self.result_body {
                Some(body) => {
                    in_companion
                        && body.value != 0
                        && byte_offset
                            .checked_add(u64_from_index(mirror::RESULT_BODY_REFERENCE + 1))
                            == Some(body.offset)
                }
                None => in_frame || in_companion,
            }
    }
}

/// Distance from the end of a derived base frame to the repeated owner
/// reference.
pub(crate) const REPEAT_FROM_END: usize = 12;

/// The type GUID of a `Derived Base Feature` scope.
pub(crate) const TYPE_GUID: &str = "B1E5B971-C007-48C9-8FF5-F4CCD91F87A4";

/// The family a `Derived Base Feature` scope takes from its type GUID.
pub(crate) const FAMILY: &str = "Derived Base Feature";

/// Whether `scope` is a `Derived Base Feature` scope.
pub(crate) fn is_derived_base(scope: &super::scope::DesignParameterScope) -> bool {
    matches!(scope.kind_name(), FAMILY | "DerivedBaseFeature")
}

rewrite_native_record!(DesignDerivedBaseOwner, []; {record_index, repeat_offset, result_body});

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
        let Some(owner) = owner_record(scope) else {
            ownerless.push((stream, scope));
            continue;
        };
        groups.entry((stream, owner)).or_default().push(scope);
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
    let owner = owner_record(scope)?;
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

/// Gives the first scope of each derived base group of one stream the
/// timeline position of the group's owner in `ordinals`. A group takes the
/// position when its owner is a timeline item that forms no scope, the
/// position holds no other scope, and no scope of the group holds a position.
pub(crate) fn take_owner_positions<'a>(
    stream: &'a str,
    scopes: &[&'a super::scope::DesignParameterScope],
    item_ordinals: &std::collections::HashMap<u64, u64>,
    ordinals: &mut std::collections::HashMap<(&'a str, u32), u64>,
) {
    let claimed = ordinals
        .values()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    for ((_, owner), group) in derived_base_groups(scopes.iter().copied()) {
        let (Some(ordinal), Some(first)) =
            (item_ordinals.get(&u64::from(owner)).copied(), group.first())
        else {
            continue;
        };
        if scopes.iter().any(|scope| scope.record_index == owner)
            || (group.len() != 1
                && group
                    .iter()
                    .any(|scope| scope.derived_base_source.is_some()))
            || claimed.contains(&ordinal)
            || group
                .iter()
                .any(|scope| ordinals.contains_key(&(stream, scope.record_index)))
        {
            continue;
        }
        // discarded-value: no scope of the group holds an ordinal yet.
        let _ = ordinals.insert((stream, first.record_index), ordinal);
    }
}

/// The derived base group that `scope` leads when the group took its
/// owner's timeline position: the owner is a timeline item of the stream,
/// the scope holds an ordinal and is no timeline item itself, and no other
/// scope of the group holds an ordinal.
fn projected_derived_base_group<'a>(
    scope: &super::scope::DesignParameterScope,
    scopes: &'a [super::scope::DesignParameterScope],
    ordinals: &std::collections::HashMap<(&str, u32), u64>,
    timeline_items: &std::collections::HashSet<(&str, u64)>,
) -> Option<Vec<&'a super::scope::DesignParameterScope>> {
    let group = derived_base_group(scope, scopes)?;
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let (first, rest) = group.split_first()?;
    let owner = owner_record(scope)?;
    (first.record_index == scope.record_index
        && timeline_items.contains(&(stream, u64::from(owner)))
        && ordinals.contains_key(&(stream, scope.record_index))
        && !timeline_items.contains(&(stream, u64::from(scope.record_index)))
        && rest
            .iter()
            .all(|member| !ordinals.contains_key(&(stream, member.record_index))))
    .then_some(group)
}

/// Lists the scopes of each derived base group on the feature projected at
/// the position of the group's owner, under [`derived_base_scopes_property`].
pub(crate) fn mark_group_features(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[super::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    ordinals: &std::collections::HashMap<(&str, u32), u64>,
) {
    let timeline_items = timelines
        .iter()
        .flat_map(|timeline| {
            let stream =
                crate::ids::native_stream(timeline.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
            timeline
                .frame()
                .items()
                .iter()
                .map(move |item| (stream, item.value))
        })
        .collect::<std::collections::HashSet<_>>();
    for feature in features {
        let Some(group) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
            .and_then(|scope| {
                projected_derived_base_group(scope, scopes, ordinals, &timeline_items)
            })
        else {
            continue;
        };
        // discarded-value: the projection sets this property only here.
        let _ = feature.source_properties.insert(
            derived_base_scopes_property(),
            group
                .iter()
                .map(|member| member.record_index.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
    }
}

/// The feature among `features` that lists `scope` in its derived base group.
pub(crate) fn group_feature<'a>(
    features: &'a [cadmpeg_ir::features::Feature],
    scopes: &[super::scope::DesignParameterScope],
    scope: &super::scope::DesignParameterScope,
) -> Option<&'a cadmpeg_ir::features::Feature> {
    features.iter().find(|feature| {
        feature_derived_base_scopes(feature, scopes)
            .is_some_and(|group| group.iter().any(|member| member.id == scope.id))
    })
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
