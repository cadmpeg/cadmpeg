// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::sheet_metal::Fixture;
use cadmpeg_ir::{
    features::{FeatureId, GeneratedEdgeRef, PlanarProfileRef},
    ids::IdentityKey,
    sketches::SketchId,
};

fn attached(fixture: &Fixture) -> Feature {
    let values = operands::plate(
        &fixture.plate,
        &fixture.style,
        |r| fixture.property(r),
        Fixture::length,
        Fixture::direction,
        cadmpeg_ir::units::UnitVector3::Z_AXIS,
        cadmpeg_ir::units::UnitVector3::X_AXIS,
    )
    .expect("operands");
    let attachment = EdgeSelection::generated(
        vec![GeneratedEdgeRef {
            feature: FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("test", "design", "feature"),
                IdentityKey::try_new("base").expect("key"),
            ),
            local_id: cadmpeg_ir::features::SelectionReference::new("boundary".to_owned())
                .expect("name"),
        }],
        "inventor:pmdc:feature-property#fixture-10".to_owned(),
    )
    .expect("attachment");
    Feature {
        id: crate::feature::history_feature_id(&fixture.plate.identity).expect("identity"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        native_ref: Some(fixture.plate.id()),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::SheetMetalAttachedPlate {
                profile: PlanarProfileRef::Sketch(SketchId::compose(
                    &cadmpeg_ir::identity_namespace!("test", "design", "sketch"),
                    IdentityKey::try_new("profile").expect("key"),
                )),
                thickness: values.thickness,
                side: values.side,
                attachment,
            },
        )),
    }
}

#[test]
fn bend_owns_the_dimensions_and_names_the_separate_flat_plate() {
    let fixture = Fixture::new();
    let plate = attached(&fixture);
    let operation = bend_definition(
        &plate,
        &fixture.plate,
        &fixture.bend,
        &fixture.values(),
        |r| fixture.property(r),
    )
    .expect("bend");
    let FeatureOperation::SheetMetalBend {
        plate: target,
        attachment,
        inside_radius,
        relief,
        transition,
    } = operation
    else {
        panic!("bend");
    };
    assert_eq!(target, plate.id);
    assert_eq!(inside_radius.get(), 11.0);
    assert_eq!(relief.width.get(), 13.0);
    assert_eq!(transition.radius.get(), 23.0);
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
        attachment: expected,
        ..
    }) = plate.evaluation.definition()
    else {
        panic!("plate");
    };
    assert_eq!(&attachment, expected);
}

#[test]
fn bend_rejects_a_native_plate_or_another_plate_identity() {
    let fixture = Fixture::new();
    let mut plate = attached(&fixture);
    plate.native_ref = Some("inventor:pmdc:feature#other-100".to_owned());
    assert!(bend_definition(
        &plate,
        &fixture.plate,
        &fixture.bend,
        &fixture.values(),
        |r| fixture.property(r)
    )
    .is_none());
    let mut plate = attached(&fixture);
    plate.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("plate".into()),
            parameters: BTreeMap::default(),
        },
    ));
    assert!(bend_definition(
        &plate,
        &fixture.plate,
        &fixture.bend,
        &fixture.values(),
        |r| fixture.property(r)
    )
    .is_none());
}

#[test]
fn document_style_selection_rejects_duplicates_and_other_carriers() {
    let fixture = Fixture::new();
    assert!(style(&[], "fixture").is_none());
    assert!(style(std::slice::from_ref(&fixture.style), "other").is_none());
    assert_eq!(
        style(std::slice::from_ref(&fixture.style), "fixture"),
        Some(&fixture.style)
    );
    assert!(style(&[fixture.style.clone(), fixture.style], "fixture").is_none());
}

#[test]
fn projected_features_each_report_their_retained_controls() {
    let fixture = Fixture::new();
    let plate = attached(&fixture);
    let mut bend = plate.clone();
    bend.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        bend_definition(
            &plate,
            &fixture.plate,
            &fixture.bend,
            &fixture.values(),
            |r| fixture.property(r),
        )
        .expect("bend"),
    ));
    let losses = super::super::retained_control_losses([&plate, &bend]);
    assert_eq!(losses.len(), 3);
    let text = serde_json::to_string(&losses).expect("losses");
    assert!(text.contains("feature.sheet-metal-attachment-controls-retained"));
    assert!(text.contains("feature.sheet-metal-bend-controls-retained"));
    assert!(text.contains("slot-10 Boolean"));
    assert!(text.contains("feature.sheet-metal-style-forms-native"));
}

fn label(
    ordinal: u32,
    owner: u32,
    class: crate::feature::ClassId,
    participants: &[u32],
) -> PmDcFeatureLabel {
    use crate::{
        feature::{PmDcFeatureLabelPayload, PmDcLinkedHeader},
        record_identity::{Located, RecordTypeId},
        test_support::sheet_metal::{list, reference},
    };
    Located::new(
        PmDcFeatureLabelPayload {
            save_version_major: 29,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0, 0],
                flags_extension: Some(0),
                owner: reference(owner + 1),
                parent: reference(0),
                next: reference(0),
            },
            index: ordinal,
            participants: list(participants),
            name: String::new(),
            name_units: None,
            class_id: class,
        },
        RecordTypeId::try_from("00000000000000000000000000000002".to_owned()).expect("type"),
        IdentityKey::try_new("fixture").expect("token"),
        ordinal,
    )
}

fn flange() -> Flange {
    use crate::{
        record_identity::{Located, RecordTypeId},
        test_support::test_fixtures::{content_at, parse},
    };
    let mut bytes = content_at(401, 29, &[]);
    bytes.extend_from_slice(&(-1_i32).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
    for value in [3_u32, 1, 0, 101, 201, 301, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Located::new(
        parse(&bytes, |ctx, view| {
            super::super::flange::parse(ctx, view, 29)
        })
        .expect("flange"),
        RecordTypeId::from_bytes(super::super::flange::TYPE),
        IdentityKey::try_new("fixture").expect("token"),
        400,
    )
}

#[test]
fn flange_profile_join_requires_unique_typed_children_and_matching_labels() {
    let fixture = Fixture::new();
    let features = vec![
        fixture.plate,
        fixture.bend,
        crate::test_support::sheet_metal::feature(300, &[]),
    ];
    let mut labels = vec![
        label(
            500,
            400,
            crate::feature::PLATE_CLASS_ID,
            &[51, 101, 201, 301],
        ),
        label(501, 100, crate::feature::PLATE_CLASS_ID, &[]),
        label(502, 200, super::super::BEND_CLASS, &[]),
        label(503, 300, super::super::CORNER_CLASS, &[]),
    ];
    assert_eq!(
        members(&flange(), &features, &labels, |ordinal| ordinal == 50),
        Some(Members {
            sketch: 50,
            plate: 100,
            bend: 200,
            corner: 300
        })
    );
    assert!(members(&flange(), &features, &labels, |_| false).is_none());
    labels.push(labels[1].clone());
    assert!(members(&flange(), &features, &labels, |_| true).is_none());
    labels.pop();
    labels[2] = label(502, 200, crate::feature::PLATE_CLASS_ID, &[]);
    assert!(members(&flange(), &features, &labels, |_| true).is_none());
}

#[test]
fn a_generated_attachment_requires_a_preceding_projected_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let fixture = Fixture::new();
    let plate = attached(&fixture);
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
        attachment: EdgeSelection::Generated { edges, .. },
        ..
    }) = plate.evaluation.definition()
    else {
        panic!("attachment");
    };
    let mut producer = plate.clone();
    producer.id = edges[0].feature.clone();
    producer.ordinal = 0;
    producer.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("base".into()),
            parameters: BTreeMap::default(),
        },
    ));
    let mut items = vec![
        HistoryItem {
            record: 90,
            kind: crate::feature::history::HistoryNodeKind::Feature,
            flags: 0,
            label: None,
            feature: producer,
            result: None,
            neutral: false,
        },
        HistoryItem {
            record: 100,
            kind: crate::feature::history::HistoryNodeKind::Feature,
            flags: 0,
            label: None,
            feature: plate,
            result: None,
            neutral: true,
        },
    ];
    let inventory = FeatureInventory {
        features: vec![fixture.plate],
        ..FeatureInventory::default()
    };
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert_eq!(
        reject_unresolved_inputs(&ctx, &mut items, &inventory).expect("native fallback"),
        1
    );
    assert!(!items[1].neutral);
    assert_eq!(items[1].feature.ordinal, 1);
    assert!(matches!(
        items[1].feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}

#[test]
fn plate_and_bend_projection_is_all_or_nothing_in_history_order() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let fixture = Fixture::new();
    let plate = attached(&fixture);
    let mut bend = plate.clone();
    bend.id = crate::feature::history_feature_id(&fixture.bend.identity).expect("bend");
    bend.native_ref = Some(fixture.bend.id());
    bend.ordinal = 2;
    bend.dependencies.insert(plate.id.clone());
    bend.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        bend_definition(
            &plate,
            &fixture.plate,
            &fixture.bend,
            &fixture.values(),
            |r| fixture.property(r),
        )
        .expect("bend"),
    ));
    let item = |feature: Feature, record| HistoryItem {
        record,
        kind: crate::feature::history::HistoryNodeKind::Feature,
        flags: 0,
        label: None,
        feature,
        result: None,
        neutral: true,
    };
    let inventory = FeatureInventory {
        features: vec![fixture.plate, fixture.bend],
        labels: vec![
            label(501, 100, crate::feature::PLATE_CLASS_ID, &[]),
            label(502, 200, super::super::BEND_CLASS, &[]),
        ],
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let complete = || vec![item(plate.clone(), 100), item(bend.clone(), 200)];
    let mut items = complete();
    assert_eq!(
        reject_incomplete_pairs(&ctx, &mut items, &inventory).expect("pair"),
        0
    );
    for missing in [0, 1] {
        let mut items = complete();
        retain_native(&ctx, &mut items[missing], &inventory).expect("native");
        assert_eq!(
            reject_incomplete_pairs(&ctx, &mut items, &inventory).expect("pair"),
            1
        );
        assert!(items.iter().all(|item| !item.neutral
            && matches!(
                item.feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            )));
        let losses = super::super::native_kind_losses(
            &inventory,
            &items.iter().map(|i| i.feature.clone()).collect::<Vec<_>>(),
        );
        assert_eq!(losses.len(), 2);
        let wire = serde_json::to_string(&losses).expect("losses");
        assert!(wire.contains("feature.sheet-metal-attachment-controls-native"));
        assert!(wire.contains("feature.sheet-metal-bend-controls-native"));
    }
    let mut items = complete();
    items[1].feature.ordinal = 0;
    assert_eq!(
        reject_incomplete_pairs(&ctx, &mut items, &inventory).expect("order"),
        2
    );
}
