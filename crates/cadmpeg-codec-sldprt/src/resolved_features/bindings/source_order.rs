// SPDX-License-Identifier: Apache-2.0
//! Admit source ownership and typed content order before dropping layout positions.

use crate::records::{FeatureHistory, FeatureInputLane};
use cadmpeg_core::CodecError;
use std::collections::{BTreeSet, HashMap};

pub(super) fn finalize(
    histories: &[FeatureHistory],
    lane: &mut FeatureInputLane,
) -> Result<(), CodecError> {
    let features = crate::records::HistoryFeatures::new(histories);
    let names = super::FeatureObjectNames::new(&lane.names);
    let mut starts = features
        .iter()
        .filter_map(|feature| Some((names.of(feature)?.offset?, feature.id.clone())))
        .collect::<Vec<_>>();
    starts.sort_unstable_by_key(|(position, _)| *position);
    let owner_at = |position: u64| {
        let end = starts.partition_point(|(start, _)| *start <= position);
        end.checked_sub(1)
            .and_then(|index| starts.get(index))
            .map(|(_, id)| id.clone())
    };
    for name in &mut lane.names {
        name.feature_ref = name.offset.and_then(owner_at);
    }
    let names_by_position = lane
        .names
        .iter()
        .filter_map(|name| Some((name.offset?, name)))
        .collect::<HashMap<_, _>>();
    for class in &mut lane.classes {
        let direct_name = class
            .offset
            .and_then(|position| position.checked_add(6))
            .and_then(|position| {
                position.checked_add(cadmpeg_core::decode::u64_from_index(class.name.len()))
            })
            .and_then(|position| names_by_position.get(&position).copied());
        class.feature_ref = direct_name
            .and_then(|name| name.feature_ref.clone())
            .or_else(|| class.offset.and_then(owner_at));
    }
    let mut positions = BTreeSet::new();
    macro_rules! collect {
        ($($arena:ident),+ $(,)?) => {$(
            for record in &lane.$arena {
                if let Some(position) = record.offset { positions.insert(position); }
            }
        )+};
    }
    collect!(
        classes,
        names,
        scalars,
        relation_bindings,
        relation_instances,
        body_selections,
        edge_selections,
        surface_selections,
        generated_surface_identities,
        references
    );
    for marker in &lane.sketch_entities {
        if let Some(position) = marker.offset() {
            positions.insert(position);
        }
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
                if let Some(rank) = record.offset.and_then(|position| ranks.get(&position)) {
                    record.sequence = *rank;
                }
            }
        )+};
    }
    assign!(
        classes,
        names,
        scalars,
        relation_bindings,
        relation_instances,
        body_selections,
        edge_selections,
        surface_selections,
        generated_surface_identities,
        references
    );
    for marker in &mut lane.sketch_entities {
        if let Some(rank) = marker.offset().and_then(|position| ranks.get(&position)) {
            marker.set_sequence(*rank);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::finalize;
    use crate::records::FeatureInputLane;

    #[test]
    fn typed_content_order_survives_without_any_source_positions() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let payload = crate::test_support::history::resolved_feature_classes_with_ids(&[
            ("moExtrusion_c", "Boss1", 10),
            ("moCut_c", "Cut1", 20),
        ]);
        let parent = "lane";
        let classes =
            crate::resolved_features::names::class_declarations(&payload, parent).unwrap();
        let names = crate::resolved_features::names::object_names(&ctx, &payload, parent).unwrap();
        let mut lane = FeatureInputLane {
            id: parent.into(),
            configuration: None,
            native_payload: payload.into(),
            classes,
            names,
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        finalize(&[], &mut lane).unwrap();
        assert_eq!(
            lane.classes
                .iter()
                .map(|class| class.sequence)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(
            lane.names
                .iter()
                .map(|name| name.sequence)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        let wire = serde_json::to_value(&lane).unwrap();
        for arena in ["classes", "names"] {
            for record in wire[arena].as_array().unwrap() {
                assert!(record.get("offset").is_none());
            }
        }
        let restored: FeatureInputLane = serde_json::from_value(wire.clone()).unwrap();
        assert!(restored.native_payload.is_empty());
        assert!(restored.classes.iter().all(|class| class.offset.is_none()));
        assert!(restored.names.iter().all(|name| name.offset.is_none()));
        assert_eq!(serde_json::to_value(restored).unwrap(), wire);
    }
}
