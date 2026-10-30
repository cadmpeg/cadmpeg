// SPDX-License-Identifier: Apache-2.0

use super::super::native_losses;
use crate::feature::tests::{reference, reference_list, test_feature, test_label, test_property};
use crate::feature::{
    native_feature, ClassId, FeatureInventory, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    CHAMFER_CLASS_ID, CORNER_ROUND_CLASS_ID, FILLET_CLASS_ID,
};
use crate::pmdc::PmDcU32List;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};

fn fixture(
    ctx: &DecodeContext<'_>,
    class: ClassId,
    lists: &[&[u32]],
) -> (FeatureInventory, Feature) {
    let mut properties = vec![
        test_property(
            0,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FilletEdgeSets,
                items: reference_list(&[2]),
            },
        ),
        test_property(
            1,
            PmDcFeaturePropertyKind::FilletEdgeSet {
                edges: reference(3),
                radius: reference(0),
                selection: reference(0),
                continuity: reference(0),
                option: None,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(
                    &(0..lists.len())
                        .map(|i| 11 + u32::try_from(i).expect("small fixture"))
                        .collect::<Vec<_>>(),
                ),
            },
        ),
    ];
    for (i, values) in lists.iter().enumerate() {
        properties.push(test_property(
            10 + u32::try_from(i).expect("small fixture"),
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(
                    2,
                    (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    values.to_vec(),
                )
                .expect("list metadata"),
                index_reference_value: -1,
                value: 0,
            },
        ));
    }
    let source = test_feature(
        100,
        16,
        &[(0, if class == CHAMFER_CLASS_ID { 2 } else { 0 })],
    );
    let label = test_label(100, 1, class, &[]);
    let feature = native_feature(ctx, &source.identity, Some(&label), source.id())
        .expect("native edge treatment");
    (
        FeatureInventory {
            features: vec![source],
            labels: vec![label],
            properties,
            ..FeatureInventory::default()
        },
        feature,
    )
}

#[test]
fn empty_and_mixed_edge_items_pin_the_native_loss_for_each_edge_treatment() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for class in [FILLET_CLASS_ID, CHAMFER_CLASS_ID] {
        for lists in [&[&[][..]][..], &[&[7][..], &[][..]][..]] {
            let (inventory, feature) = fixture(&ctx, class, lists);
            assert!(matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ));
            let losses = native_losses(&inventory, &[feature]);
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::InventorLossCode::FeatureEdgeSelectionEmptyItemsNative
                    .note("expected")
                    .code
            );
            assert_eq!(
                losses[0].code.local_code(),
                "feature.edge-selection-empty-items-native"
            );
        }
    }
}

#[test]
fn nonempty_missing_and_ambiguous_items_do_not_claim_the_empty_item_cause() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for class in [FILLET_CLASS_ID, CHAMFER_CLASS_ID, CORNER_ROUND_CLASS_ID] {
        for lists in [&[][..], &[&[7][..]][..]] {
            let (inventory, feature) = fixture(&ctx, class, lists);
            assert!(native_losses(&inventory, &[feature]).is_empty());
        }
        let (mut inventory, feature) = fixture(&ctx, class, &[&[]]);
        let item = inventory.properties.pop().expect("edge item");
        assert!(native_losses(&inventory, std::slice::from_ref(&feature)).is_empty());
        inventory.properties.extend([item.clone(), item]);
        assert!(native_losses(&inventory, &[feature]).is_empty());

        let (mut inventory, feature) = fixture(&ctx, class, &[&[]]);
        let root = if class == CHAMFER_CLASS_ID { 2 } else { 0 };
        let PmDcFeaturePropertyKind::References { items, .. } = &inventory.properties[root].kind
        else {
            panic!("selection collection")
        };
        inventory.properties[root] = test_property(
            u32::try_from(root).expect("fixture ordinal"),
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: items.clone(),
            },
        );
        assert!(native_losses(&inventory, &[feature]).is_empty());
    }
}

#[test]
fn a_corner_round_keeps_only_its_sheet_metal_operand_loss() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for lists in [&[&[][..]][..], &[&[7][..], &[][..]][..]] {
        let (inventory, feature) = fixture(&ctx, CORNER_ROUND_CLASS_ID, lists);
        assert!(native_losses(&inventory, &[feature]).is_empty());
    }
}

#[test]
fn only_retained_native_edge_treatments_receive_the_empty_item_loss() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (inventory, feature) = fixture(&ctx, FILLET_CLASS_ID, &[&[]]);
    assert!(native_losses(&inventory, &[]).is_empty());
    let label = test_label(100, 1, crate::feature::EXTRUSION_CLASS_ID, &[]);
    let source = &inventory.features[0];
    let other = native_feature(&ctx, &source.identity, Some(&label), source.id())
        .expect("other native family");
    assert!(native_losses(&inventory, &[other]).is_empty());
    let losses = native_losses(&inventory, &[feature]);
    assert_eq!(losses.len(), 1);
}
