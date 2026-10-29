//! Variable fillet record and lane-object index.

use crate::records::{Feature, FeatureHistory, FeatureInputLane, FeatureInputName};
use crate::records::{FeatureSource, ObjectId};
use crate::resolved_features::projections::VariableFilletObjects;
use crate::resolved_features::scalars::FeatureObjectNames;
use std::collections::BTreeMap;

#[test]
fn variable_fillet_objects_match_the_history_and_lane_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((state >> 33) % bound).unwrap()
    };
    let feature = |id: u32, ordinal: u32, source: u32, fillet: bool| Feature {
        id: format!("native-{id}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal,
        name: format!("Feature{id}"),
        kind: if fillet { "VarFillet" } else { "Fillet" }.into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut ends = 0;
    for _ in 0..32 {
        let histories = (0..2)
            .map(|history| FeatureHistory {
                id: format!("history-{history}"),
                part_name: None,
                properties: BTreeMap::new(),
                content: Vec::new(),
                configurations: Vec::new(),
                features: (0..16)
                    .map(|ordinal| feature(next(20), ordinal, next(24), next(2) == 0))
                    .collect(),
            })
            .collect::<Vec<_>>();
        let lanes = (0..3)
            .map(|lane| FeatureInputLane {
                id: format!("lane-{lane}"),
                configuration: None,
                native_payload: vec![0; 64].into(),
                classes: Vec::new(),
                names: (0..20)
                    .map(|ordinal| FeatureInputName {
                        feature_ref: None,
                        id: format!("name-{lane}-{ordinal}"),
                        parent: format!("lane-{lane}"),
                        ordinal,
                        sequence: ordinal,
                        offset: u64::from(next(12)) * 4,
                        object_id: ObjectId::from_value(next(24)),
                        value: format!("Feature{}", next(20)),
                    })
                    .collect(),
                scalars: Vec::new(),
                relation_bindings: Vec::new(),
                relation_instances: Vec::new(),
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                generated_surface_identities: Vec::new(),
                references: Vec::new(),
                sketch_entities: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut objects = VariableFilletObjects::new(&histories, &lanes);
        for feature_ref in (0..22).map(|id| format!("native-{id}")) {
            let scanned = histories
                .iter()
                .position(|history| {
                    history
                        .features
                        .iter()
                        .any(|feature| feature.id == feature_ref)
                })
                .and_then(|history| {
                    histories[history]
                        .features
                        .iter()
                        .find(|feature| {
                            feature.id == feature_ref
                                && feature.kind.eq_ignore_ascii_case("VarFillet")
                        })
                        .map(|feature| (history, feature))
                });
            assert!(match (objects.fillet(&feature_ref), scanned) {
                (Some((left, indexed)), Some((right, scanned))) =>
                    left == right && std::ptr::eq(indexed, scanned),
                (indexed, scanned) => indexed.is_none() && scanned.is_none(),
            });
            for (history_index, history) in histories.iter().enumerate() {
                let scanned = lanes
                    .iter()
                    .filter_map(|lane| {
                        let names = FeatureObjectNames::new(&lane.names);
                        let mut starts = history
                            .features
                            .iter()
                            .filter_map(|candidate| Some((names.of(candidate)?.offset, candidate)))
                            .collect::<Vec<_>>();
                        starts.sort_unstable_by_key(|(left, _)| *left);
                        let index = starts
                            .iter()
                            .position(|(_, candidate)| candidate.id == feature_ref)?;
                        let end = starts
                            .get(index + 1)
                            .and_then(|(offset, _)| usize::try_from(*offset).ok())
                            .unwrap_or(lane.native_payload.len());
                        Some((lane.id.as_str(), end))
                    })
                    .collect::<Vec<_>>();
                let indexed = objects
                    .object_ends(history_index, &feature_ref)
                    .into_iter()
                    .map(|(lane, end)| (lane.id.as_str(), end))
                    .collect::<Vec<_>>();
                assert_eq!(indexed, scanned);
                ends += scanned.len();
            }
        }
    }
    assert!(ends > 0);
}
