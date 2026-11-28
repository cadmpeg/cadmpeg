// SPDX-License-Identifier: Apache-2.0
use std::collections::{HashMap, HashSet};

use cadmpeg_asm::history::{
    historical_record_archive, materialize_record_table, walk_entity_versions,
};
use cadmpeg_core::decode::{unique_index, work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;

use super::{attachment_edges, AttachmentEdges};
use crate::feature::sheet_metal::flange::FlangeKind;
use crate::feature::FeatureInventory;
use crate::feature::{edge_selection::historical, history_states};
use crate::kernel::ActiveCarrierState;
use crate::pmdc::PmDcReference;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};

// resource-cap: MAX_ATTACHMENT_WORK = 4
const MAX_ATTACHMENT_WORK: usize = 1 << 21;

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    document: &crate::rse::RseInventory<'_>,
    inventory: &FeatureInventory,
) -> Result<HashMap<(String, u32), AttachmentEdges>, CodecError> {
    let mut resolved = HashMap::new();
    let plates = inventory
        .features
        .iter()
        .filter(|plate| {
            plate
                .properties
                .references()
                .get(12)
                .is_some_and(|reference| reference.index() != 0)
                && inventory.sheet_metal_flanges.iter().any(|flange| {
                    FlangeKind::of(flange) == Some(FlangeKind::Contour)
                        && flange.identity.segment_token == plate.identity.segment_token
                        && flange
                            .children
                            .references()
                            .first()
                            .and_then(|reference| reference.record_ordinal())
                            == Some(plate.identity.record_ordinal)
                })
        })
        .collect::<Vec<_>>();
    if plates.is_empty() {
        return Ok(resolved);
    }
    let ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(resolved);
    };
    if carrier.family != crate::kernel::KernelFamily::Asm
        || !historical::supported_schema(carrier.schema)
    {
        return Ok(resolved);
    }
    let Ok(header) = carrier.header.as_ref() else {
        return Ok(resolved);
    };
    let mut selectors = HashMap::new();
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry.map(|registry| registry.version_major) else {
            continue;
        };
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let types = unique_index(
            table
                .records
                .iter()
                .map(|record| (record.ordinal, record.type_id)),
        );
        let bodies = unique_index(
            table
                .records
                .iter()
                .filter(|record| record.type_id == crate::pmdc::inventor_id(0x9087_4d47))
                .filter_map(|record| {
                    Some((
                        record.ordinal,
                        historical::source(record.payload, version).ok()?,
                    ))
                }),
        );
        let mut parsed = Vec::new();
        for record in &table.records {
            if record.type_id != crate::pmdc::inventor_id(0x9087_4d53) {
                continue;
            }
            let selector = match historical::parse(ctx, record.payload, version) {
                Ok(Some(selector)) => selector,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => continue,
            };
            if historical::links(
                &selector,
                |ordinal| types.get(&ordinal).copied().flatten(),
                |ordinal| bodies.get(&ordinal).copied().flatten(),
            ) {
                parsed.push(selector);
            }
        }
        selectors.insert(segment.pair.token.key().as_str(), parsed);
    }
    if selectors.len() != 1 {
        return Ok(resolved);
    }
    let bytes = carrier.bytes.window();
    let history_states::Framing::Framed(framed) =
        history_states::frame_chain(ctx, bytes, header.width)?
    else {
        return Ok(resolved);
    };
    let history_states::Prepared::Ready(chain) =
        history_states::prepare(ctx, &framed, bytes, header.width)?
    else {
        return Ok(resolved);
    };
    let history_states::Reading::Read(index) =
        history_states::index_chain(&chain, carrier.schema, history_states::MAX_STATE_SCAN_WORK)
    else {
        return Ok(resolved);
    };
    let history_states::Chain {
        views,
        frames,
        archived_ids,
        active_count,
    } = *chain;
    let Some(archive) = historical_record_archive(&views, frames) else {
        return Ok(resolved);
    };
    let properties = unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            property,
        )
    }));
    let work = WorkBudget::new(MAX_ATTACHMENT_WORK);
    let mut failure = None;
    let mut visited = 0;
    let mut bounded = false;
    let mut signatures = HashMap::new();
    let mut ambiguous = HashSet::new();
    let closed = walk_entity_versions(&views, active_count, &archived_ids, |slot, versions| {
        let ordinal = views
            .len()
            .checked_sub(visited + 1)
            .and_then(|ordinal| u32::try_from(ordinal).ok());
        visited += 1;
        let Some(ordinal) = ordinal else {
            return;
        };
        if failure.is_some() {
            return;
        }
        if !work.charge_by(work_units(versions.len())) {
            bounded = true;
            return;
        }
        let Some(table) = materialize_record_table(versions, |revision| archive.get(&revision))
        else {
            return;
        };
        for plate in &plates {
            if !candidate(
                index.first.get(&plate.header.source_index).copied(),
                ordinal,
            ) {
                continue;
            }
            let token = plate.identity.segment_token.as_str();
            let Some(selectors) = selectors.get(token) else {
                continue;
            };
            let selected = match historical::selected(ctx, &table, selectors, carrier.schema) {
                Ok(selected) => selected,
                Err(error @ CodecError::ResourceLimit(_)) => {
                    failure = Some(error);
                    return;
                }
                Err(_) => continue,
            };
            let Some(mut attachment) = attachment_edges(
                plate,
                |reference| {
                    properties
                        .get(&(token, reference.record_ordinal()?))?
                        .as_ref()
                        .copied()
                },
                |item, source| {
                    let edge = selected.get(&source)?;
                    selected_owner(item, edge)
                },
            ) else {
                continue;
            };
            let key = (token.to_owned(), plate.identity.record_ordinal);
            if ambiguous.contains(&key) {
                continue;
            }
            let signature = attachment
                .edges
                .iter()
                .map(|edge| {
                    Some((
                        *versions.get(&edge.entity_index)?,
                        selected.get(&edge.selector_source)?.geometry.clone(),
                    ))
                })
                .collect::<Option<Vec<_>>>();
            let Some(signature) = signature else {
                continue;
            };
            if signatures
                .get(&key)
                .is_some_and(|previous| previous != &signature)
            {
                resolved.remove(&key);
                ambiguous.insert(key);
                continue;
            }
            signatures.insert(key.clone(), signature);
            for edge in &mut attachment.edges {
                edge.revision = versions.get(&edge.entity_index).copied();
            }
            attachment.state = Some(history_states::StateRef {
                ordinal,
                state_id: views[slot].state_id,
            });
            resolved.insert(key, attachment);
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if bounded || !closed {
        resolved.clear();
    }
    Ok(resolved)
}

pub(super) fn selected_owner(item: PmDcReference, edge: &historical::SelectedEdge) -> Option<i64> {
    (edge.item == item.record_ordinal()? && edge.owners == 1).then_some(edge.owner)
}

fn candidate(first_output: Option<u32>, ordinal: u32) -> bool {
    first_output.is_none_or(|after| after.checked_sub(1) == Some(ordinal))
}

#[cfg(test)]
mod tests {
    #[test]
    fn built_inputs_use_the_predecessor_and_unbuilt_inputs_use_matching_states() {
        assert!(super::candidate(Some(3), 2));
        assert!(!super::candidate(Some(3), 3));
        assert!(!super::candidate(Some(3), 1));
        assert!(!super::candidate(Some(0), 0));
        assert!(super::candidate(None, 0));
        assert!(super::candidate(None, 3));
    }
}
