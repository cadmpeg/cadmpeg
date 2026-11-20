// SPDX-License-Identifier: Apache-2.0

use super::{native_stream, single_operand_group};
use crate::loss::F3dLossCode;
use crate::records::entity_header::SegmentType;
use crate::records::feature::path_features::pipe::BODY_GUID;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::report::loss::LossNote;

const WHOLE_BODY_VERSION: u32 = 1;

pub(super) fn project_reverse_normal(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    design_types: &[SegmentType],
    losses: &mut Vec<LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let operation = scope.reverse_normal_operation()?;
    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    let mut references = scope.reference_members().values().copied();
    if group.scope_reference_ordinal != 0
        || references.next() != Some(group.record_index)
        || group.record_index != operation.body_group.value
        || !group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references)
    {
        return None;
    }
    let types = native_stream(&scope.id)
        .and_then(|stream| stream.strip_prefix("f3d:"))
        .and_then(crate::ids::decode_identity_key_component)
        .map(|entry| crate::design::decode::meta::stream_types_by_entity(design_types, &entry))
        .unwrap_or_default();
    let unverified = group
        .members()
        .iter()
        .map(|member| member.value)
        .filter(|member| {
            types
                .get(&u64::from(*member))
                .is_none_or(|(guid, version)| {
                    !guid.eq_ignore_ascii_case(BODY_GUID) || *version != WHOLE_BODY_VERSION
                })
        })
        .collect::<Vec<_>>();
    if !unverified.is_empty() {
        losses.push(F3dLossCode::FeatureReverseNormalBodyUnverified.note(format!(
            "Reverse Normal {} reverses body-group members {unverified:?} as whole bodies; the type table registers no whole-body record type {BODY_GUID} version {WHOLE_BODY_VERSION} for them.",
            scope.id
        )));
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::ReverseShape {
            source: BodySelection::Native(group.id.clone()),
        },
    ))
}

/// The link GUID that a scope shares with the other scopes of one linked
/// timeline item.
pub(super) fn timeline_link(scope: &DesignParameterScope) -> Option<&str> {
    scope
        .reverse_normal_operation()?
        .link
        .as_ref()
        .map(|link| link.value.as_str())
}

/// The linked scopes that are not timeline items, by link GUID, in record
/// order.
pub(super) fn linked_scopes_outside<'a>(
    scopes: &[&'a DesignParameterScope],
    items: &[u64],
) -> std::collections::HashMap<&'a str, Vec<u64>> {
    let items = items
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut linked = std::collections::HashMap::<&str, Vec<u64>>::new();
    for scope in scopes {
        let record = u64::from(scope.record_index);
        if let Some(link) = timeline_link(scope).filter(|_| !items.contains(&record)) {
            linked.entry(link).or_default().push(record);
        }
    }
    for records in linked.values_mut() {
        records.sort_unstable();
    }
    linked
}

/// The link GUIDs that more than one timeline item carries. Such a GUID
/// names no single item, so its other linked scopes take no position.
pub(super) fn shared_timeline_links<'a>(
    scopes_by_record: &std::collections::HashMap<u32, &'a DesignParameterScope>,
    items: &[u64],
) -> std::collections::HashSet<&'a str> {
    let mut seen = std::collections::HashSet::new();
    let mut shared = std::collections::HashSet::new();
    for item in items {
        let Some(link) = u32::try_from(*item)
            .ok()
            .and_then(|record| scopes_by_record.get(&record))
            .and_then(|scope| timeline_link(scope))
        else {
            continue;
        };
        if !seen.insert(link) {
            shared.insert(link);
        }
    }
    shared
}

/// A linked Reverse Normal scope that takes no timeline position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnplacedLinkedScope {
    pub(crate) scope_id: String,
    pub(crate) link: String,
    /// Another scope with the same link takes a position, so several timeline
    /// items carry the link.
    pub(crate) shared: bool,
}

/// The linked Reverse Normal scopes without an authored ordinal, in scope
/// order.
pub(crate) fn unplaced_linked_scopes<'a>(
    scopes: &'a [DesignParameterScope],
    ordinals: &std::collections::HashMap<(&'a str, u32), u64>,
) -> Vec<UnplacedLinkedScope> {
    let stream_of = |scope: &'a DesignParameterScope| {
        native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM)
    };
    let placed_links = scopes
        .iter()
        .filter(|scope| ordinals.contains_key(&(stream_of(scope), scope.record_index)))
        .filter_map(|scope| Some((stream_of(scope), timeline_link(scope)?)))
        .collect::<std::collections::HashSet<_>>();
    scopes
        .iter()
        .filter(|scope| !ordinals.contains_key(&(stream_of(scope), scope.record_index)))
        .filter_map(|scope| {
            let link = timeline_link(scope)?;
            Some(UnplacedLinkedScope {
                scope_id: scope.id.clone(),
                link: link.to_owned(),
                shared: placed_links.contains(&(stream_of(scope), link)),
            })
        })
        .collect()
}
