// SPDX-License-Identifier: Apache-2.0
//! Admit source ownership and typed content order from record positions.

use crate::records::{FeatureHistory, FeatureInputLane};
use cadmpeg_core::CodecError;
use std::collections::{BTreeSet, HashMap};

/// `selection_positions` holds the source positions of the body, edge and
/// generated-surface selections, which carry no offset of their own.
pub(super) fn finalize(
    histories: &[FeatureHistory],
    lane: &mut FeatureInputLane,
    selection_positions: &HashMap<String, u64>,
) -> Result<(), CodecError> {
    let features = crate::records::HistoryFeatures::new(histories);
    let names = super::FeatureObjectNames::new(&lane.names);
    let mut starts = features
        .iter()
        .filter_map(|feature| Some((names.of(feature)?.offset, feature.id.clone())))
        .collect::<Vec<_>>();
    starts.sort_unstable_by_key(|(position, _)| *position);
    let owner_at = |position: u64| {
        let end = starts.partition_point(|(start, _)| *start <= position);
        end.checked_sub(1)
            .and_then(|index| starts.get(index))
            .map(|(_, id)| id.clone())
    };
    for name in &mut lane.names {
        name.feature_ref = owner_at(name.offset);
    }
    let names_by_position = lane
        .names
        .iter()
        .map(|name| (name.offset, name))
        .collect::<HashMap<_, _>>();
    for class in &mut lane.classes {
        let direct_name = class
            .offset
            .checked_add(6)
            .and_then(|position| {
                position.checked_add(cadmpeg_core::decode::u64_from_index(class.name.len()))
            })
            .and_then(|position| names_by_position.get(&position).copied());
        class.feature_ref = direct_name
            .and_then(|name| name.feature_ref.clone())
            .or_else(|| owner_at(class.offset));
    }
    let selection_position = |id: &str| {
        selection_positions
            .get(id)
            .copied()
            .ok_or_else(|| CodecError::malformed("SLDPRT selection lacks its source position"))
    };
    let mut positions = BTreeSet::new();
    macro_rules! collect {
        ($($arena:ident),+ $(,)?) => {$(
            for record in &lane.$arena {
                positions.insert(record.offset);
            }
        )+};
    }
    collect!(
        classes,
        names,
        scalars,
        relation_instances,
        surface_selections,
        references,
        relation_bindings,
    );
    for id in lane
        .edge_selections
        .iter()
        .map(|record| &record.id)
        .chain(
            lane.generated_surface_identities
                .iter()
                .map(|record| &record.id),
        )
        .chain(lane.body_selections.iter().map(|record| &record.id))
    {
        positions.insert(selection_position(id)?);
    }
    for marker in &lane.sketch_entities {
        positions.insert(marker.offset());
    }
    let ranks = positions
        .into_iter()
        .enumerate()
        .map(|(rank, position)| {
            u32::try_from(rank)
                .map(|rank| (position, rank))
                .map_err(|_| CodecError::malformed("SLDPRT typed content order exceeds u32"))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    macro_rules! assign {
        ($($arena:ident),+ $(,)?) => {$(
            for record in &mut lane.$arena {
                if let Some(rank) = ranks.get(&record.offset) {
                    record.sequence = *rank;
                }
            }
        )+};
    }
    assign!(
        classes,
        names,
        scalars,
        relation_instances,
        surface_selections,
        references,
        relation_bindings,
    );
    macro_rules! assign_selections {
        ($($arena:ident),+ $(,)?) => {$(
            for record in &mut lane.$arena {
                if let Some(rank) = ranks.get(&selection_position(&record.id)?) {
                    record.sequence = *rank;
                }
            }
        )+};
    }
    assign_selections!(
        edge_selections,
        generated_surface_identities,
        body_selections
    );
    for marker in &mut lane.sketch_entities {
        if let Some(rank) = ranks.get(&marker.offset()) {
            marker.set_sequence(*rank);
        }
    }
    Ok(())
}
