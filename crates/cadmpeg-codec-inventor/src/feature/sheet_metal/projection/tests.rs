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
        Some(&fixture.style),
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
        product_definition: None,
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
    let mut refusals = Refusals::default();
    assert_eq!(
        reject_unresolved_inputs(&ctx, &mut items, &inventory, &mut refusals)
            .expect("native fallback"),
        1
    );
    // The producer fixture shares the plate's source record; the note is for the plate.
    let notes = refusals.losses(&[items[1].feature.clone()]);
    assert_eq!(notes.len(), 1);
    let wire = serde_json::to_string(&notes).expect("notes");
    assert!(wire.contains("feature.sheet-metal-plate-refused"));
    assert!(wire.contains("input-producer"));
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
    let mut refusals = Refusals::default();
    assert_eq!(
        reject_incomplete_pairs(&ctx, &mut items, &inventory, &mut refusals).expect("pair"),
        0
    );
    assert!(refusals
        .losses(&items.iter().map(|i| i.feature.clone()).collect::<Vec<_>>())
        .is_empty());
    for missing in [0, 1] {
        let mut items = complete();
        retain_native(&ctx, &mut items[missing], &inventory).expect("native");
        let mut refusals = Refusals::default();
        assert_eq!(
            reject_incomplete_pairs(&ctx, &mut items, &inventory, &mut refusals).expect("pair"),
            1
        );
        // Only a plate carries a refusal; a refused bend is counted by its own loss.
        let notes = refusals.losses(&items.iter().map(|i| i.feature.clone()).collect::<Vec<_>>());
        assert_eq!(notes.len(), missing);
        if missing == 1 {
            assert!(serde_json::to_string(&notes)
                .expect("notes")
                .contains("bend-pair"));
        }
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
    let mut refusals = Refusals::default();
    assert_eq!(
        reject_incomplete_pairs(&ctx, &mut items, &inventory, &mut refusals).expect("order"),
        2
    );
    assert_eq!(
        refusals
            .losses(&items.iter().map(|i| i.feature.clone()).collect::<Vec<_>>())
            .len(),
        1
    );
}

#[test]
fn the_first_refusal_of_a_plate_is_kept_and_other_operations_have_none() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let fixture = Fixture::new();
    let plate = attached(&fixture);
    let mut refusals = Refusals::default();
    refusals.record(&plate, PlateRefusal::InputProducer);
    refusals.record(&plate, PlateRefusal::BendPair);
    let mut other = plate.clone();
    other.native_ref = Some("other".to_owned());
    other.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("base".into()),
            parameters: BTreeMap::default(),
        },
    ));
    refusals.record(&other, PlateRefusal::Style);
    let inventory = FeatureInventory {
        features: vec![fixture.plate.clone()],
        ..FeatureInventory::default()
    };
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut item = HistoryItem {
        record: 100,
        kind: crate::feature::history::HistoryNodeKind::Feature,
        flags: 0,
        label: None,
        feature: plate,
        result: None,
        neutral: true,
    };
    // A projected plate has no note; the same plate retained natively has one.
    assert!(refusals.losses(&[item.feature.clone()]).is_empty());
    retain_native(&ctx, &mut item, &inventory).expect("native");
    let notes = refusals.losses(&[item.feature.clone(), other]);
    assert_eq!(notes.len(), 1);
    let wire = serde_json::to_string(&notes).expect("notes");
    assert!(wire.contains("feature.sheet-metal-plate-refused"));
    assert!(wire.contains("input-producer"));
    assert!(!wire.contains("bend-pair"));
    assert!(!wire.contains("style"));
}

#[test]
fn the_result_slot_follows_the_plate_form() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let source = |fixture: &Fixture, ordinal: u32| {
        fixture
            .property(crate::test_support::sheet_metal::reference(ordinal))
            .expect("property")
            .id()
    };
    for (fixture, plate_result_property, bend_result_property) in
        [(Fixture::body(), 46, 46), (Fixture::new(), 13, 13)]
    {
        let index = crate::feature::tests::test_projection_index(
            &fixture.properties,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let form = operands::PlateForm::of(&fixture.plate).expect("form");
        let (plate_id, plate) = form_result(&ctx, &fixture.plate, form, false, &index)
            .expect("plate result")
            .expect("charge");
        assert_eq!(plate.output_of, plate_id);
        assert_eq!(
            plate.native_ref.as_deref(),
            Some(source(&fixture, plate_result_property).as_str())
        );
        let (bend_id, bend) = form_result(&ctx, &fixture.bend, form, true, &index)
            .expect("bend result")
            .expect("charge");
        assert_eq!(bend.output_of, bend_id);
        assert_ne!(bend_id, plate_id);
        assert_eq!(
            bend.native_ref.as_deref(),
            Some(source(&fixture, bend_result_property).as_str())
        );
    }
    // A form reads only its own result slot.
    let body = Fixture::body();
    let index = crate::feature::tests::test_projection_index(
        &body.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(form_result(
        &ctx,
        &body.plate,
        operands::PlateForm::Collection,
        false,
        &index
    )
    .is_none());
    let collection = Fixture::new();
    let index = crate::feature::tests::test_projection_index(
        &collection.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(form_result(
        &ctx,
        &collection.plate,
        operands::PlateForm::Body,
        false,
        &index
    )
    .is_none());
}

fn selecting_fixture() -> Fixture {
    use crate::feature::{PmDcFeaturePropertyKind as Kind, PmDcFeatureReferenceFamily as Family};
    use crate::test_support::sheet_metal::{list, property, reference};
    let mut fixture = Fixture::new();
    fixture.properties.extend([
        property(
            11,
            &Kind::References {
                family: Family::SheetMetalAttachmentSelections,
                items: list(&[51]),
            },
        ),
        property(
            51,
            &Kind::SheetMetal {
                definition: super::super::SheetMetalProperty::EdgeSelection {
                    edges: reference(52),
                },
            },
        ),
        property(
            52,
            &Kind::References {
                family: Family::EdgeCollection,
                items: list(&[53]),
            },
        ),
        property(
            53,
            &Kind::EdgeItem {
                index_references: crate::pmdc::PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![61],
                )
                .expect("indexes"),
                index_reference_value: -1,
                value: 0,
            },
        ),
    ]);
    fixture
}

fn named_edge() -> cadmpeg_ir::features::GeneratedEdgeRef {
    GeneratedEdgeRef {
        feature: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("test", "design", "feature"),
            IdentityKey::try_new("base").expect("key"),
        ),
        local_id: cadmpeg_ir::features::SelectionReference::new("boundary".to_owned())
            .expect("name"),
    }
}

#[test]
fn the_attachment_names_a_missing_list_before_a_missing_edge_name() {
    // Property 11, the plate's slot-12 selection list, is absent.
    let fixture = Fixture::new();
    let index = crate::feature::tests::test_projection_index(
        &fixture.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        attachment(&fixture.plate, &index),
        Err(PlateRefusal::SelectionList)
    );
    // The list resolves, but the edge source has no generated name.
    let fixture = selecting_fixture();
    let mut index = crate::feature::tests::test_projection_index(
        &fixture.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        attachment(&fixture.plate, &index),
        Err(PlateRefusal::EdgeName)
    );
    // The source has an unnamed entry.
    index.edge_names.insert(("fixture".to_owned(), 61), None);
    assert_eq!(
        attachment(&fixture.plate, &index),
        Err(PlateRefusal::EdgeName)
    );
    index
        .edge_names
        .insert(("fixture".to_owned(), 61), Some(named_edge()));
    let EdgeSelection::Generated { edges, .. } =
        attachment(&fixture.plate, &index).expect("generated")
    else {
        panic!("generated selection");
    };
    assert_eq!(edges.as_slice(), [named_edge()]);
}

#[test]
fn the_attachment_requires_wrappers_over_edge_collections() {
    use crate::feature::{PmDcFeaturePropertyKind as Kind, PmDcFeatureReferenceFamily as Family};
    use crate::test_support::sheet_metal::{list, property};
    for (slot, replacement) in [
        // The wrapper is not an edge selection.
        (
            51,
            Kind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        // The wrapper names a face collection.
        (
            52,
            Kind::References {
                family: Family::FaceCollection,
                items: list(&[53]),
            },
        ),
        // The list is another reference family.
        (
            11,
            Kind::References {
                family: Family::EdgeCollection,
                items: list(&[51]),
            },
        ),
    ] {
        let mut fixture = selecting_fixture();
        fixture
            .properties
            .retain(|p| p.identity.record_ordinal + 1 != slot);
        fixture.properties.push(property(slot, &replacement));
        let mut index = crate::feature::tests::test_projection_index(
            &fixture.properties,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        index
            .edge_names
            .insert(("fixture".to_owned(), 61), Some(named_edge()));
        assert_eq!(
            attachment(&fixture.plate, &index),
            Err(PlateRefusal::SelectionList),
            "property {slot}"
        );
    }
}

#[test]
fn a_repeated_generated_edge_is_refused_as_an_edge_name() {
    use crate::feature::{PmDcFeaturePropertyKind as Kind, PmDcFeatureReferenceFamily as Family};
    use crate::test_support::sheet_metal::{list, property};
    let mut fixture = selecting_fixture();
    fixture
        .properties
        .retain(|p| p.identity.record_ordinal + 1 != 52);
    fixture.properties.push(property(
        52,
        &Kind::References {
            family: Family::EdgeCollection,
            items: list(&[53, 53]),
        },
    ));
    let mut index = crate::feature::tests::test_projection_index(
        &fixture.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    index
        .edge_names
        .insert(("fixture".to_owned(), 61), Some(named_edge()));
    assert_eq!(
        attachment(&fixture.plate, &index),
        Err(PlateRefusal::EdgeName)
    );
}
