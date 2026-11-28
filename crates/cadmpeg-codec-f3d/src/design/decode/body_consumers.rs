// SPDX-License-Identifier: Apache-2.0
//! Bodies that a `Combine`, Remove, or join `Extrude` scope names as consumed.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::take_reference;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::body_consumer_browser_node_prefix as node_prefix;
use crate::layout::current_extrude_operation_fields as operation_fields;
use crate::layout::sketch_extrude_boolean_scope_frame as boolean_frame;
use crate::metastream::{primary_record_frames, MetaStream, PrimaryRecordFrame};

pub(crate) const COMBINE_TYPE_GUID: &str = "2A94257F-2020-4B19-9A68-103A2672F1B7";
pub(crate) const REMOVE_BODY_TYPE_GUID: &str = "4A782808-FBDD-4351-A10A-F7AE5693D330";
pub(crate) const EXTRUDE_TYPE_GUID: &str = "DD405BC2-D673-44F0-8833-5CB2A1C186C7";
const EXTRUDE_TYPE_VERSION: u32 = 8;
const LOCAL_REFERENCE_LEN: usize = 11;

/// Design entity suffixes of the bodies whose hidden browser nodes a consuming
/// scope selects.
pub(super) fn consumed_body_entities(
    root: View<'_>,
    meta: &MetaStream,
) -> Result<HashSet<u64>, CodecError> {
    let mut consumers = entities_of(meta, |guid| {
        [COMBINE_TYPE_GUID, REMOVE_BODY_TYPE_GUID]
            .iter()
            .any(|consumer| consumer.eq_ignore_ascii_case(guid))
    });
    let extrudes: HashSet<u64> = meta
        .types
        .iter()
        .filter(|kind| {
            kind.version == EXTRUDE_TYPE_VERSION
                && kind
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(EXTRUDE_TYPE_GUID)
        })
        .flat_map(|kind| kind.entities.values().copied())
        .collect();
    if consumers.is_empty() && extrudes.is_empty() {
        return Ok(HashSet::new());
    }
    let frames: HashMap<_, _> = primary_record_frames(meta, root.remaining())?
        .into_iter()
        .map(|frame| (frame.entity_id, frame))
        .collect();
    let bytes = root.window();
    consumers.extend(extrudes.into_iter().filter(|entity| {
        frames
            .get(entity)
            .is_some_and(|frame| is_join_extrude(bytes, *frame))
    }));
    if consumers.is_empty() {
        return Ok(HashSet::new());
    }
    let nodes = entities_of(meta, |guid| {
        crate::design::presentation::BROWSER_NODE_TYPE_GUID.eq_ignore_ascii_case(guid)
    });
    let mut consumed = HashSet::new();
    super::body_reference_tails::for_each_selection(root, meta, &frames, |frame, selected| {
        if !owner_is(bytes, frame, &consumers) {
            return;
        }
        for node in selected {
            if !nodes.contains(node) {
                continue;
            }
            if let Some(entity) = frames
                .get(node)
                .and_then(|node| hidden_node_entity(bytes, *node))
            {
                consumed.insert(entity);
            }
        }
    });
    Ok(consumed)
}

/// A join or cut `Extrude` scope prologue whose result operation is join.
fn is_join_extrude(bytes: &[u8], frame: PrimaryRecordFrame) -> bool {
    let Some(record) = bytes.get(frame.start..frame.member_end) else {
        return false;
    };
    let zeros = |at: usize, len: usize| {
        record
            .get(at..at + len)
            .is_some_and(|run| run.iter().all(|byte| *byte == 0))
    };
    zeros(
        boolean_frame::ZERO_RUN_9,
        boolean_frame::FLAG - boolean_frame::ZERO_RUN_9,
    ) && record.get(boolean_frame::FLAG) == Some(&boolean_frame::FLAG_VALUE)
        && zeros(
            boolean_frame::ZERO_RUN_6,
            boolean_frame::PROLOGUE_BYTE - boolean_frame::ZERO_RUN_6,
        )
        && matches!(record.get(boolean_frame::PROLOGUE_BYTE), Some(0 | 1))
        && crate::design::decode::scopes::shared_frames::extrude_operation_at(
            record,
            boolean_frame::OPERATION_FIELDS + operation_fields::OPERATION,
        ) == Some(crate::records::feature::extrude::DesignExtrudeOperation::Join)
}

fn entities_of(meta: &MetaStream, selected: impl Fn(&str) -> bool) -> HashSet<u64> {
    meta.types
        .iter()
        .filter(|kind| selected(kind.type_guid.as_str()))
        .flat_map(|kind| kind.entities.values().copied())
        .collect()
}

/// A selection record ends with a local reference to the scope that owns it.
fn owner_is(bytes: &[u8], frame: PrimaryRecordFrame, owners: &HashSet<u64>) -> bool {
    let Some(mut cursor) = frame.member_end.checked_sub(LOCAL_REFERENCE_LEN) else {
        return false;
    };
    take_reference(bytes, &mut cursor)
        .and_then(crate::bytes::Reference::into_local)
        .is_some_and(|(owner, _)| owners.contains(&owner) && cursor == frame.member_end)
}

fn hidden_node_entity(bytes: &[u8], frame: PrimaryRecordFrame) -> Option<u64> {
    let record = bytes.get(frame.start..frame.member_end)?;
    let guid_end = fixed_guid_end(record, node_prefix::GUID_CODE_UNIT_COUNT)?;
    let mut cursor = guid_end.checked_add(2)?;
    if !matches!(record.get(guid_end..cursor)?, [1, 0 | 1]) {
        return None;
    }
    Some(take_reference(record, &mut cursor)?.into_local()?.0)
}

#[cfg(test)]
mod tests;
