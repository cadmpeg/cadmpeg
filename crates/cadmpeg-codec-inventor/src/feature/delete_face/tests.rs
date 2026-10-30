// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::tests::{test_feature, test_label};
use crate::pmdc::{PmDcListMetadata, PmDcReferenceList};
use crate::record_identity::Located;

#[test]
fn delete_face_refusals_emit_the_pinned_loss_and_first_check() {
    let source = test_feature(0, 4, &[]);
    let mut payload = (*source).clone();
    payload.properties = PmDcReferenceList::new(
        8,
        Some(PmDcListMetadata::U16([4, 0])),
        source.properties.references().to_vec(),
    )
    .expect("synthetic type-8 list");
    let marker = Located::new(
        payload,
        source.identity.type_id.clone(),
        source.identity.segment_token.clone(),
        source.identity.record_ordinal,
    );
    for (source, reason, detail) in [
        (marker, NativeReason::Marker, "marker differs from type 2"),
        (
            test_feature(0, 3, &[]),
            NativeReason::Slots,
            "four-slot form",
        ),
        (source, NativeReason::Controls, "slot 1 Boolean"),
    ] {
        assert_eq!(admission(&source), Err(reason));
        let inventory = FeatureInventory {
            features: vec![source],
            labels: vec![test_label(0, 1, CLASS_ID, &[])],
            ..Default::default()
        };
        let losses = native_losses(&inventory);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.delete-face-operands-native"
        );
        assert_eq!(
            losses[0].code.local_code(),
            InventorLossCode::FeatureDeleteFaceOperandsNative.code()
        );
        assert!(losses[0].message.contains(detail));
    }
}

#[test]
fn delete_face_loss_uses_the_label_class_and_record_owner() {
    let mut inventory = FeatureInventory {
        features: vec![test_feature(0, 4, &[])],
        labels: vec![test_label(1, 1, CLASS_ID, &[])],
        ..Default::default()
    };
    assert!(native_losses(&inventory).is_empty());
    inventory.labels = vec![test_label(0, 1, super::super::THREAD_CLASS_ID, &[])];
    assert!(native_losses(&inventory).is_empty());
    inventory.labels = vec![test_label(0, 1, CLASS_ID, &[])];
    assert_eq!(native_losses(&inventory).len(), 1);
}
