// SPDX-License-Identifier: Apache-2.0
//! The match of a timeline item to its v10 feature.

use std::collections::BTreeSet;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureOperation as LegacyOperation, UnresolvedFamily,
};

use cadmpeg_ir::ids::ProductDefinitionId;

use super::{legacy, tail_flags, timeline, STREAM};
use crate::design_v11::timeline as projection;

const MEMBER: &str =
    "f3d:0574d3e0-934b-4a4e-a78b-7f54f53c2287.f3d@FusionAssetName[Active]/Design1/BulkStream.dat";

/// An unresolved extrude of `stream` stored at scope record `record`, keyed as
/// the v10 projection keys it, with ordinal `ordinal`.
fn keyed(stream: &str, kind: &str, record: u32, ordinal: u64) -> LegacyFeature {
    let mut feature = legacy(
        "unused:0",
        ordinal,
        Some(false),
        LegacyOperation::Unresolved {
            family: UnresolvedFamily::Extrude,
        },
    );
    feature.id = crate::ids::neutral_feature_id_parts(stream, kind, 1, record);
    feature.native_ref = Some(format!("{stream}:design-parameter-scope#{record}"));
    feature
}

/// The features of the document `stream` holds, projected from `features` and
/// one timeline of `items`, with the authored order `order` states and the
/// losses charged.
fn items_ordered(
    stream: &str,
    features: &[LegacyFeature],
    items: &[u64],
    flags: &projection::TailFlags<'_>,
    order: &dyn Fn() -> projection::StreamOrder,
) -> (
    Vec<cadmpeg_ir::design::feature::Feature>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        projection::project_ordered(
            ctx,
            &projection::Sources {
                features,
                timelines: &[timeline(stream, 0, items)],
                types: &[],
            },
            &projection::ScopeInputs {
                records: &[],
                tail_flags: flags,
            },
            &BTreeSet::new(),
            &|_, _| order(),
            &mut losses,
        )
    })
    .unwrap();
    (documents.into_values().next().unwrap().features, losses)
}

/// `items_ordered` with no authored order.
fn items_of(
    stream: &str,
    features: &[LegacyFeature],
    items: &[u64],
    flags: &projection::TailFlags<'_>,
) -> Vec<cadmpeg_ir::design::feature::Feature> {
    items_ordered(
        stream,
        features,
        items,
        flags,
        &projection::StreamOrder::default,
    )
    .0
}

fn by_position(losses: &[cadmpeg_ir::report::loss::LossNote]) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code == crate::loss::F3dLossCode::HistoryItemMatchedByPosition.kind())
        .count()
}

fn is_timeline_item(feature: &cadmpeg_ir::design::feature::Feature) -> bool {
    matches!(
        &feature.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:timeline-item"
    )
}

/// The merge renumbers the ordinals of the features it places, so an item is
/// matched to its feature by record in a member and in the root alike.
#[test]
fn an_item_is_matched_to_the_feature_of_its_record_whatever_its_ordinal() {
    for stream in [MEMBER, STREAM] {
        let features = [
            keyed(stream, "Extrude", 10, 41),
            keyed(stream, "Extrude", 20, 7),
        ];
        let got = items_of(stream, &features, &[10, 20], &projection::TailFlags::new());
        let ids = got.iter().map(|feature| &feature.id).collect::<Vec<_>>();
        assert_eq!(ids, [&features[0].id, &features[1].id], "{stream}");
        assert!(got.iter().all(|feature| !is_timeline_item(feature)));
    }
}

#[test]
fn an_item_with_no_scope_of_its_record_takes_the_feature_at_its_position_and_charges_a_loss() {
    for stream in [MEMBER, STREAM] {
        // The merge places the member's features after those already merged,
        // so the first ordinal of the stream is not zero.
        let feature = keyed(stream, "Extrude", 10, 40);
        let (got, losses) = items_ordered(
            stream,
            std::slice::from_ref(&feature),
            &[20],
            &projection::TailFlags::new(),
            &projection::StreamOrder::default,
        );
        let [item] = got.as_slice() else {
            panic!("one feature")
        };
        assert_eq!(item.id, feature.id, "{stream}");
        assert_eq!(by_position(&losses), 1, "{stream}");
    }
}

#[test]
fn a_feature_of_another_item_is_not_taken_by_position() {
    for stream in [MEMBER, STREAM] {
        let feature = keyed(stream, "Extrude", 10, 0);
        let (got, losses) = items_ordered(
            stream,
            std::slice::from_ref(&feature),
            &[20, 10],
            &projection::TailFlags::new(),
            &projection::StreamOrder::default,
        );
        let [first, second] = got.as_slice() else {
            panic!("two features")
        };
        assert!(is_timeline_item(first), "{stream}");
        assert_eq!(second.id, feature.id);
        assert_eq!(by_position(&losses), 0);
    }
}

/// The item of a paste object takes the feature of the `Base Feature` scope
/// the object owns, and the item of an `Assemble` envelope the feature of the
/// `JointOrigin` it binds: the scope order places the scope at the item's
/// position. The record of the item states neither.
#[test]
fn an_item_takes_the_feature_of_the_scope_placed_at_its_position() {
    for stream in [MEMBER, STREAM] {
        let paste = keyed(stream, "BaseFeature", 363, 90);
        let joint = keyed(stream, "JointOrigin", 501, 92);
        let own = keyed(stream, "Extrude", 10, 89);
        let order = || {
            projection::StreamOrder::from_positions(
                [(10, 0), (362, 1), (500, 2)].into_iter().collect(),
                [(10, 0), (363, 1), (501, 2)],
            )
        };
        let (got, losses) = items_ordered(
            stream,
            &[joint.clone(), paste.clone(), own.clone()],
            &[10, 362, 500],
            &projection::TailFlags::new(),
            &order,
        );
        let ids = got.iter().map(|feature| &feature.id).collect::<Vec<_>>();
        assert_eq!(ids, [&own.id, &paste.id, &joint.id], "{stream}");
        assert!(got.iter().all(|feature| !is_timeline_item(feature)));
        assert_eq!(by_position(&losses), 0, "{stream}");
    }
}

#[test]
fn a_scope_that_is_no_item_stands_before_the_item_that_follows_it() {
    for stream in [MEMBER, STREAM] {
        let features = [
            keyed(stream, "Extrude", 10, 0),
            keyed(stream, "ReverseNormal", 20, 1),
            keyed(stream, "Extrude", 30, 2),
            keyed(stream, "ReverseNormal", 40, 3),
        ];
        let order = || {
            projection::StreamOrder::from_positions(
                [(10, 0), (20, 1), (30, 2), (40, 3)].into_iter().collect(),
                [(10, 0), (20, 1), (30, 2), (40, 3)],
            )
        };
        let (got, _) = items_ordered(
            stream,
            &features,
            &[10, 30],
            &projection::TailFlags::new(),
            &order,
        );
        let ids = got.iter().map(|feature| &feature.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                &features[0].id,
                &features[1].id,
                &features[2].id,
                &features[3].id
            ],
            "{stream}"
        );
    }
}

#[test]
fn an_item_with_no_feature_of_its_record_is_a_timeline_item() {
    let got = items_of(STREAM, &[], &[10], &projection::TailFlags::new());
    assert!(got.iter().all(is_timeline_item));
}

#[test]
fn a_record_is_matched_whole_and_not_by_its_suffix() {
    let features = [keyed(MEMBER, "Extrude", 202, 0)];
    let got = items_of(MEMBER, &features, &[20, 202], &projection::TailFlags::new());
    let [short, whole] = got.as_slice() else {
        panic!("two features")
    };
    assert!(is_timeline_item(short));
    assert_eq!(whole.id, features[0].id);
}

#[test]
fn features_of_one_record_go_to_the_item_in_id_order() {
    // A timeline frame names a record once, so one item meets every feature
    // of its record and takes the one with the smallest id.
    let a = keyed(MEMBER, "Extrude", 10, 0);
    let b = keyed(MEMBER, "Fillet", 10, 1);
    let first = if a.id.as_str() < b.id.as_str() {
        a.id.clone()
    } else {
        b.id.clone()
    };
    for features in [[a.clone(), b.clone()], [b.clone(), a.clone()]] {
        let one = items_of(MEMBER, &features, &[10], &projection::TailFlags::new());
        assert_eq!(one[0].id, first);
    }
}

#[test]
fn a_matched_feature_with_no_stored_suppression_takes_the_items_state() {
    let mut feature = keyed(MEMBER, "Extrude", 10, 0);
    feature.suppressed = None;
    let flags: projection::TailFlags<'_> = [((MEMBER, 10), tail_flags([true, false], true, false))]
        .into_iter()
        .collect();
    let got = items_of(MEMBER, std::slice::from_ref(&feature), &[10], &flags);
    assert_eq!(got[0].id, feature.id);
    assert_eq!(got[0].suppressed, Some(Dimension::Literal(true)));
    let mut stored = feature.clone();
    stored.suppressed = Some(false);
    let got = items_of(MEMBER, std::slice::from_ref(&stored), &[10], &flags);
    assert_eq!(got[0].suppressed, Some(Dimension::Literal(false)));
}

#[test]
fn only_a_feature_of_the_root_document_names_its_owner_as_component() {
    let own =
        ProductDefinitionId::mint("f3d:0574d3e0-934b-4a4e-a78b-7f54f53c2287.f3d@model:component#c")
            .unwrap();
    let root = ProductDefinitionId::mint("f3d:model:component#a").unwrap();
    let flags = projection::TailFlags::new();
    let member = items_of(MEMBER, &[keyed(MEMBER, "Extrude", 10, 0)], &[10], &flags).remove(0);
    assert_eq!(crate::design_v11::member_component(&member, &own), None);
    assert_eq!(crate::design_v11::member_component(&member, &root), None);
    let hosted = items_of(STREAM, &[keyed(STREAM, "Extrude", 10, 0)], &[10], &flags).remove(0);
    assert_eq!(
        crate::design_v11::member_component(&hosted, &root),
        Some(root.clone())
    );
}

#[test]
fn timeline_position_fallback_uses_the_stream_ordinal_with_followers() {
    for stream in [MEMBER, STREAM] {
        for follower in [false, true] {
            let ordinal = if follower { 2 } else { 1 };
            let mut features = vec![keyed(stream, "Extrude", 10, 40)];
            if follower {
                features.push(keyed(stream, "ReverseNormal", 20, 41));
            }
            let target = keyed(stream, "Extrude", 90, 40 + ordinal);
            features.push(target.clone());
            let order = || {
                let mut records = [(10, 0), (30, ordinal)]
                    .into_iter()
                    .collect::<std::collections::HashMap<_, _>>();
                if follower {
                    records.insert(20, 1);
                }
                projection::StreamOrder::from_positions(records, [(80, ordinal)])
            };
            let (got, losses) = items_ordered(
                stream,
                &features,
                &[10, 30],
                &projection::TailFlags::new(),
                &order,
            );
            assert_eq!(got.last().unwrap().id, target.id, "{stream}, {follower}");
            assert_eq!(got.len(), features.len());
            assert_eq!(by_position(&losses), 1);
        }
    }
}
