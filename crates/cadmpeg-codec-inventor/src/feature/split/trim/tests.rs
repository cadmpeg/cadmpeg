// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition, FeatureOperation};

use crate::{
    feature::{
        history::History,
        split,
        tests::{reference, reference_list, test_label},
        PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
        ProjectionIndex,
    },
    pmdc::PmDcReference,
    record_identity::RecordTypeId,
    test_support::split::{boolean, set_kind, set_marker, set_slot, set_version, trim_fixture},
};

fn admit(
    source: &PmDcFeature,
    properties: &[PmDcFeatureProperty],
) -> Result<FeatureDefinition, split::NativeReason> {
    split::definition(
        source,
        |index| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == index)
        },
        |_| None,
    )
}

#[test]
fn surface_trim_keeps_its_side_native_and_uses_the_input_body_as_result() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (source, properties) = trim_fixture();
    let index = ProjectionIndex {
        properties: properties
            .iter()
            .map(|p| {
                (
                    (p.identity.segment_token.as_str(), p.identity.record_ordinal),
                    Some(p),
                )
            })
            .collect(),
        ..ProjectionIndex::default()
    };
    let mut losses = Vec::new();
    let (feature, result) = split::project(
        &ctx,
        &source,
        &test_label(30, 4, split::CLASS_ID, &[]),
        &index,
        &History::default(),
        &mut losses,
    )
    .expect("projection")
    .expect("projected trim");
    let FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, keep }) =
        feature.evaluation.definition()
    else {
        panic!("trim bodies")
    };
    assert_eq!(*keep, BodyTrimSide::Unresolved);
    assert_eq!(
        operands.targets(),
        &BodySelection::Native(properties[7].id())
    );
    assert_eq!(
        operands.tools(),
        &BodySelection::Native(properties[10].id())
    );
    assert_eq!(feature.source_tag.as_deref(), Some("trim_bodies"));
    assert!(feature.dependencies.is_empty());
    assert_eq!(
        result.native_ref.as_deref(),
        Some(properties[5].id().as_str())
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.split-trim-side-native"
    );
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureSplitTrimSideNative
            .note("test")
            .code
    );
}

#[test]
fn surface_trim_checks_each_slot_type_and_qualifier() {
    for slot in 0..10 {
        let (mut source, properties) = trim_fixture();
        let old = source.properties.references()[slot];
        let replacement = if old.index() == 0 {
            reference(62)
        } else {
            reference(0)
        };
        set_slot(&mut source, slot, replacement);
        assert!(admit(&source, &properties).is_err(), "slot {slot}");
        let (mut source, properties) = trim_fixture();
        let old = source.properties.references()[slot];
        set_slot(
            &mut source,
            slot,
            PmDcReference::new(old.index(), !old.qualified()).expect("reference"),
        );
        assert!(admit(&source, &properties).is_err(), "qualifier {slot}");
    }
    for position in [0, 1, 2, 3, 4, 5, 7, 10] {
        let (source, mut properties) = trim_fixture();
        properties[position].identity.type_id = RecordTypeId::from_bytes([0; 16]);
        assert!(admit(&source, &properties).is_err(), "type {position}");
    }
    let (mut source, properties) = trim_fixture();
    source.identity.type_id = RecordTypeId::from_bytes([0; 16]);
    assert!(admit(&source, &properties).is_err());
    for version in [18, 20, 23, 31] {
        let (mut source, properties) = trim_fixture();
        set_version(&mut source, version);
        assert!(admit(&source, &properties).is_err());
    }
}

#[test]
fn surface_trim_refuses_unmeasured_booleans_and_body_forms() {
    for position in [1, 4] {
        for kind in [
            boolean(true),
            PmDcFeaturePropertyKind::Boolean {
                name: "control".into(),
                name_value: 0,
                value: false,
            },
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 1,
                value: false,
            },
        ] {
            let (source, mut properties) = trim_fixture();
            set_kind(&mut properties, position, kind);
            assert_eq!(
                admit(&source, &properties)
                    .expect_err("Boolean form")
                    .note()
                    .code
                    .local_code(),
                "feature.split-mode-one-native"
            );
        }
    }
    for position in [7, 10] {
        for body in [reference(0), reference(1)] {
            let (source, mut properties) = trim_fixture();
            set_kind(
                &mut properties,
                position,
                PmDcFeaturePropertyKind::SurfaceBody { body },
            );
            assert!(admit(&source, &properties).is_err());
        }
    }
    for members in [
        Vec::new(),
        vec![reference(69), reference(70)],
        vec![PmDcReference::new(69, false).expect("unqualified")],
    ] {
        let (source, mut properties) = trim_fixture();
        set_kind(
            &mut properties,
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: crate::pmdc::PmDcReferenceList::new(
                    2,
                    (!members.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    members,
                )
                .expect("members"),
            },
        );
        assert!(admit(&source, &properties).is_err());
    }
    let (mut source, properties) = trim_fixture();
    set_slot(&mut source, 8, reference(69));
    assert!(admit(&source, &properties).is_err());
    for position in [3, 5] {
        let (source, mut properties) = trim_fixture();
        set_marker(&mut properties, position);
        assert!(admit(&source, &properties).is_err());
    }
    let (source, mut properties) = trim_fixture();
    set_kind(
        &mut properties,
        3,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BoundaryPatch,
            items: reference_list(&[69]),
        },
    );
    assert!(admit(&source, &properties).is_err());
}
