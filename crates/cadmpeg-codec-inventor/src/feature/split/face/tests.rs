// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation, SplitFaceTool};

use crate::test_support::split::{
    boolean, face_fixture as fixture, face_item, set_kind, set_marker, set_slot,
};

use crate::{
    feature::{
        history::{History, HistoryNodeKind, HistoryPlacement},
        split,
        tests::{reference, reference_list, test_label},
        work_plane::tests::work_plane,
        PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
        PmDcFeatureReferenceFamily, ProjectionIndex,
    },
    pmdc::{PmDcListMetadata, PmDcReferenceList},
    record_identity::{Located, RecordTypeId},
};

#[test]
fn face_split_preserves_both_controls_and_uses_the_input_body_as_result() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for side in [false, true] {
        let (source, properties) = fixture(side);
        let plane = work_plane(42, [0.0, 0.0, 1.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let plane_id = crate::feature::history_feature_id(&plane.identity).expect("plane id");
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
            work_planes: [((plane.identity.segment_token.as_str(), 42), Some(&plane))]
                .into_iter()
                .collect(),
            ..ProjectionIndex::default()
        };
        let placement = |ordinal, kind| HistoryPlacement {
            ordinal,
            kind,
            below_end_of_part: false,
            model_history: true,
            model_root: 0,
        };
        let history = History {
            placements: [
                (42, placement(0, HistoryNodeKind::WorkPlane)),
                (30, placement(1, HistoryNodeKind::Feature)),
            ]
            .into_iter()
            .collect(),
            ..History::default()
        };
        let mut losses = Vec::new();
        let (feature, result) = split::project(
            &ctx,
            &source,
            &test_label(30, 4, split::CLASS_ID, &[]),
            &index,
            &history,
            &mut losses,
        )
        .expect("projection")
        .expect("feature");
        assert_eq!(
            feature.evaluation.definition(),
            &FeatureDefinition::Operation(FeatureOperation::SplitFace {
                targets: FaceSelection::Native(properties[11].id()),
                tool: SplitFaceTool::Plane {
                    plane: plane_id.clone()
                },
            })
        );
        assert_eq!(feature.dependencies.as_slice(), &[plane_id]);
        assert_eq!(feature.source_tag.as_deref(), Some("split_face"));
        assert_eq!(
            feature.source_properties["property_6_boolean"],
            side.to_string()
        );
        assert_eq!(
            result.native_ref.as_deref(),
            Some(properties[5].id().as_str())
        );
        assert_eq!(
            result
                .bodies()
                .iter()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                .collect::<Vec<_>>(),
            vec![properties[7].id()]
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::InventorLossCode::FeatureSplitFaceSelectionNative
                .note("test")
                .code
        );
    }
}

#[test]
fn face_split_refuses_empty_multi_index_and_untyped_face_items_with_the_pinned_loss() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for kind in [
        face_item(&[]),
        face_item(&[0]),
        face_item(&[7, 8]),
        boolean(false),
    ] {
        let (source, mut properties) = fixture(false);
        set_kind(&mut properties, 12, kind);
        assert!(admit(&source, &properties).is_err());
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
        assert!(split::project(
            &ctx,
            &source,
            &test_label(30, 4, split::CLASS_ID, &[]),
            &index,
            &History::default(),
            &mut losses
        )
        .is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::InventorLossCode::FeatureSplitModeTwoNative
                .note("test")
                .code
        );
    }
}

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
        |_| Some(crate::feature::history_feature_id(&source.identity).expect("synthetic plane")),
    )
}

#[test]
fn face_split_checks_every_slot_and_segment_version() {
    let (source, properties) = fixture(false);
    assert!(admit(&source, &properties).is_ok());
    for slot in 0..10 {
        let mut source = source.clone();
        let value = if source.properties.references()[slot].index() == 0 {
            reference(2)
        } else {
            reference(0)
        };
        set_slot(&mut source, slot, value);
        assert!(admit(&source, &properties).is_err(), "slot {slot}");
    }
    for version in [0, 15, 16, 18, 20, 23, 25, 28, 31, u8::MAX] {
        let mut payload = (*source).clone();
        payload.save_version_major = version;
        let source = Located::new(
            payload,
            source.identity.type_id.clone(),
            source.identity.segment_token.clone(),
            source.identity.record_ordinal,
        );
        assert!(admit(&source, &properties).is_err(), "version {version}");
    }
    assert!(split::definition(
        &source,
        |index| properties
            .iter()
            .find(|p| p.identity.record_ordinal + 1 == index),
        |_| None
    )
    .is_err());
}

#[test]
fn face_split_refuses_adjacent_control_and_selection_forms() {
    for (position, kind) in [
        (1, boolean(true)),
        (
            2,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 4,
                value: 3,
            },
        ),
        (
            3,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(&[69]),
            },
        ),
        (
            4,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 3,
                value: 2,
            },
        ),
        (
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(&[]),
            },
        ),
        (
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(&[69, 70]),
            },
        ),
        (7, boolean(false)),
        (
            11,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[]),
            },
        ),
        (
            11,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[74, 74]),
            },
        ),
        (
            11,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[74]),
            },
        ),
    ] {
        let (source, mut properties) = fixture(false);
        set_kind(&mut properties, position, kind);
        assert!(admit(&source, &properties).is_err(), "property {position}");
    }
}

#[test]
fn face_split_requires_each_operand_record_type() {
    let (mut source, properties) = fixture(false);
    source.identity.type_id = RecordTypeId::from_bytes([0; 16]);
    assert!(admit(&source, &properties).is_err());
    for index in [0, 1, 2, 3, 4, 5, 7, 11, 12] {
        let (source, mut properties) = fixture(false);
        assert!(admit(&source, &properties).is_ok());
        properties[index].identity.type_id = RecordTypeId::from_bytes([0; 16]);
        assert!(admit(&source, &properties).is_err(), "property {index}");
    }
}

#[test]
fn face_split_refuses_boolean_names_and_name_values() {
    for side in [false, true] {
        for position in [1, 4] {
            for named in [false, true] {
                let (source, mut properties) = fixture(side);
                let mut kind = properties[position].kind.clone();
                let PmDcFeaturePropertyKind::Boolean {
                    name, name_value, ..
                } = &mut kind
                else {
                    panic!("Boolean fixture");
                };
                if named {
                    *name = "control".into();
                } else {
                    *name_value = 1;
                }
                set_kind(&mut properties, position, kind);
                assert_eq!(
                    admit(&source, &properties)
                        .expect_err("Boolean metadata")
                        .note()
                        .code
                        .local_code(),
                    "feature.split-mode-two-native"
                );
            }
        }
    }
}

#[test]
fn face_split_requires_qualified_record_references_and_unqualified_nulls() {
    for slot in 0..10 {
        let (mut source, properties) = fixture(false);
        let reference = source.properties.references()[slot];
        set_slot(
            &mut source,
            slot,
            crate::pmdc::PmDcReference::new(reference.index(), !reference.qualified())
                .expect("reference"),
        );
        assert!(admit(&source, &properties).is_err(), "slot {slot}");
    }
    for (position, family, index) in [
        (5, PmDcFeatureReferenceFamily::BodySelection, 69),
        (11, PmDcFeatureReferenceFamily::FaceCollection, 74),
    ] {
        let (source, mut properties) = fixture(false);
        set_kind(
            &mut properties,
            position,
            PmDcFeaturePropertyKind::References {
                family,
                items: PmDcReferenceList::new(
                    2,
                    Some(PmDcListMetadata::U32([1, 0])),
                    vec![crate::pmdc::PmDcReference::new(index, false).expect("reference")],
                )
                .expect("one member"),
            },
        );
        assert!(admit(&source, &properties).is_err());
    }
}

#[test]
fn face_split_requires_type_two_operand_lists() {
    for position in [3, 5, 11, 12] {
        let (source, mut properties) = fixture(false);
        assert!(admit(&source, &properties).is_ok());
        set_marker(&mut properties, position);
        assert!(admit(&source, &properties).is_err(), "property {position}");
    }
}
