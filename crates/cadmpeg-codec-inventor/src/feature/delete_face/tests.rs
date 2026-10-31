// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::tests::{test_feature, test_label};
use crate::pmdc::{PmDcListMetadata, PmDcReferenceList};
use crate::record_identity::Located;

fn placed(sources: &[PmDcFeature]) -> Vec<cadmpeg_ir::features::Feature> {
    use cadmpeg_ir::features::{
        Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
        NativeFeatureKind,
    };
    sources
        .iter()
        .map(|source| Feature {
            id: FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
                cadmpeg_ir::identity_key!("generated"),
            ),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Native {
                    kind: NativeFeatureKind::DeleteFace,
                    parameters: std::collections::BTreeMap::default(),
                },
            )),
            native_ref: Some(source.id()),
            product_definition: None,
        })
        .collect()
}

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
        (source, NativeReason::Controls, "slots 1 and 2 Boolean"),
    ] {
        assert_eq!(admission(&source), Err(reason));
        let features = placed(std::slice::from_ref(&source));
        let inventory = FeatureInventory {
            features: vec![source],
            labels: vec![test_label(0, 1, CLASS_ID, &[])],
            ..Default::default()
        };
        assert!(native_losses(&inventory, &[]).is_empty());
        let losses = native_losses(&inventory, &features);
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
    let source = test_feature(0, 4, &[]);
    let features = placed(std::slice::from_ref(&source));
    let mut inventory = FeatureInventory {
        features: vec![source],
        labels: vec![test_label(1, 1, CLASS_ID, &[])],
        ..Default::default()
    };
    assert!(native_losses(&inventory, &features).is_empty());
    inventory.labels = vec![test_label(0, 1, super::super::THREAD_CLASS_ID, &[])];
    assert!(native_losses(&inventory, &features).is_empty());
    inventory.labels = vec![test_label(0, 1, CLASS_ID, &[])];
    assert_eq!(native_losses(&inventory, &features).len(), 1);
}
