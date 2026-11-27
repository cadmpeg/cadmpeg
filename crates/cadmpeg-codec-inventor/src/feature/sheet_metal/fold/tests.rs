// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::PmDcFeaturePropertyKind as Kind;
use crate::record_identity::RecordTypeId;
use crate::test_support::sheet_metal::{feature, list, property};
use cadmpeg_core::serde_erase::json;

/// An admitted fold record and its properties, with invented indexes.
struct FoldFixture {
    record: PmDcFeature,
    properties: Vec<PmDcFeatureProperty>,
}

const SLOTS: [u32; 8] = [41, 42, 43, 44, 45, 0, 0, 48];

fn face_item(values: &[u32]) -> Kind {
    json::from_value(cadmpeg_core::json!({
        "form": "face_item",
        "index_references": {
            "marker": 2,
            "metadata": if values.is_empty() {
                serde_json::Value::Null
            } else {
                cadmpeg_core::json!({"values": [values.len(), 268_435_456], "width": "u32"})
            },
            "values": values,
        }
    }))
    .expect("face item")
}

fn typed(index: u32, kind: &Kind, type_id: [u8; 16]) -> PmDcFeatureProperty {
    let mut record = property(index, kind);
    record.identity.type_id = RecordTypeId::try_from(type_id_string(type_id)).expect("type");
    record
}

impl FoldFixture {
    fn new(kind: FoldKind) -> Self {
        let references = |family, items: &[u32]| Kind::References {
            family,
            items: list(items),
        };
        let properties = vec![
            property(
                41,
                &references(PmDcFeatureReferenceFamily::BodySelection, &[7]),
            ),
            property(42, &face_item(&[310])),
            property(
                43,
                &references(PmDcFeatureReferenceFamily::FaceCollection, &[51, 52]),
            ),
            property(
                44,
                &Kind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: true,
                },
            ),
            property(
                45,
                &references(PmDcFeatureReferenceFamily::SheetMetalFoldList, &[]),
            ),
            typed(
                48,
                &Kind::SheetMetal {
                    definition: super::super::SheetMetalProperty::Enumeration {
                        type_value: 5,
                        value: 1,
                    },
                },
                kind.mode_type(),
            ),
            property(51, &face_item(&[320])),
            property(52, &face_item(&[330, 331])),
        ];
        Self {
            record: feature(30, &SLOTS),
            properties,
        }
    }

    fn property(&self, reference: PmDcReference) -> Option<&PmDcFeatureProperty> {
        self.properties
            .iter()
            .find(|p| Some(p.identity.record_ordinal) == reference.record_ordinal())
    }

    fn operands(&self, kind: FoldKind) -> Result<FoldOperands, FoldRefusal> {
        operands(&self.record, kind, |r| self.property(r))
    }

    fn replace(&mut self, index: u32, kind: &Kind) {
        let type_id = self
            .properties
            .iter()
            .find(|p| p.identity.record_ordinal == index - 1)
            .map(|p| p.identity.type_id.clone());
        self.properties
            .retain(|p| p.identity.record_ordinal != index - 1);
        let mut record = property(index, kind);
        if let Some(type_id) = type_id {
            record.identity.type_id = type_id;
        }
        self.properties.push(record);
    }
}

#[test]
fn fold_operands_name_the_body_the_stationary_item_and_the_bend_collection() {
    for kind in [FoldKind::Unfold, FoldKind::Refold] {
        let fixture = FoldFixture::new(kind);
        let values = fixture.operands(kind).expect("admitted fold");
        assert_eq!(values.body.index(), 7);
        assert_eq!(
            values.stationary,
            fixture
                .property(PmDcReference::from_packed(42))
                .expect("item")
                .id()
        );
        assert_eq!(
            values.bends,
            fixture
                .property(PmDcReference::from_packed(43))
                .expect("collection")
                .id()
        );
    }
}

#[test]
fn unfold_states_native_face_operands_and_leaves_the_rest_unresolved() {
    let fixture = FoldFixture::new(FoldKind::Unfold);
    let values = fixture.operands(FoldKind::Unfold).expect("admitted unfold");
    let FeatureOperation::SheetMetalUnfold { construction } = operation(FoldKind::Unfold, &values)
    else {
        panic!("unfold operation");
    };
    assert_eq!(
        construction.stationary,
        Some(FaceSelection::Native(values.stationary.clone()))
    );
    assert_eq!(
        construction.bends,
        Some(FaceSelection::Native(values.bends.clone()))
    );
    assert_eq!(construction.all_bends, None);
    assert_eq!(construction.neutral_factor, None);
    assert_eq!(
        operation(FoldKind::Refold, &values),
        FeatureOperation::SheetMetalRefold {
            construction: SheetMetalRefoldConstruction::default()
        }
    );
}

#[test]
fn fold_classes_select_the_fold_kind() {
    assert_eq!(FoldKind::of(UNFOLD_CLASS), Some(FoldKind::Unfold));
    assert_eq!(FoldKind::of(REFOLD_CLASS), Some(FoldKind::Refold));
    assert_eq!(FoldKind::of(super::super::BEND_CLASS), None);
}

fn refusal(edit: impl FnOnce(&mut FoldFixture)) -> FoldRefusal {
    let mut fixture = FoldFixture::new(FoldKind::Unfold);
    edit(&mut fixture);
    fixture
        .operands(FoldKind::Unfold)
        .expect_err("refused fold")
}

#[test]
fn each_failed_fold_check_names_its_refusal() {
    let references = |family, items: &[u32]| Kind::References {
        family,
        items: list(items),
    };
    assert_eq!(
        refusal(|f| f.record = feature(30, &SLOTS[..7])),
        FoldRefusal::PropertyCount
    );
    assert_eq!(
        refusal(|f| f.replace(
            41,
            &references(PmDcFeatureReferenceFamily::BodySelection, &[7, 8])
        )),
        FoldRefusal::Body
    );
    assert_eq!(
        refusal(|f| f.replace(42, &face_item(&[310, 311]))),
        FoldRefusal::Stationary
    );
    assert_eq!(
        refusal(|f| f.replace(
            43,
            &references(PmDcFeatureReferenceFamily::FaceCollection, &[])
        )),
        FoldRefusal::Bends
    );
    assert_eq!(
        refusal(|f| f.replace(52, &face_item(&[]))),
        FoldRefusal::Bends
    );
    assert_eq!(
        refusal(|f| f.replace(
            44,
            &Kind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            }
        )),
        FoldRefusal::Control
    );
    assert_eq!(
        refusal(|f| f.replace(
            45,
            &references(PmDcFeatureReferenceFamily::SheetMetalFoldList, &[51])
        )),
        FoldRefusal::List
    );
    assert_eq!(
        refusal(|f| {
            let mut slots = SLOTS;
            slots[6] = 49;
            f.record = feature(30, &slots);
        }),
        FoldRefusal::ReservedSlot
    );
    assert_eq!(
        refusal(|f| f.replace(
            48,
            &Kind::SheetMetal {
                definition: super::super::SheetMetalProperty::Enumeration {
                    type_value: 5,
                    value: 2,
                },
            }
        )),
        FoldRefusal::Mode
    );
    let unfold = FoldFixture::new(FoldKind::Unfold);
    assert_eq!(unfold.operands(FoldKind::Refold), Err(FoldRefusal::Mode));
}

#[test]
fn fold_refusal_details_are_pinned() {
    let details = [
        FoldRefusal::PropertyCount,
        FoldRefusal::Body,
        FoldRefusal::Stationary,
        FoldRefusal::Bends,
        FoldRefusal::Control,
        FoldRefusal::List,
        FoldRefusal::ReservedSlot,
        FoldRefusal::Mode,
    ]
    .map(FoldRefusal::detail);
    assert_eq!(
        details,
        [
            "property-count",
            "body",
            "stationary",
            "bends",
            "control",
            "list",
            "reserved-slot",
            "mode"
        ]
    );
}

fn fold_feature(kind: FoldKind, name: &str, ordinal: u64) -> Feature {
    let fixture = FoldFixture::new(kind);
    let values = fixture.operands(kind).expect("admitted fold");
    Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint(format!("test:design:feature#{name}"))
            .expect("id"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(kind.source_tag().into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation(
            kind, &values,
        ))),
        native_ref: Some(fixture.record.id()),
    }
}

#[test]
fn fold_losses_name_retained_controls_the_unresolved_source_and_refusals() {
    let unfold = fold_feature(FoldKind::Unfold, "unfold", 2);
    let refold = fold_feature(FoldKind::Refold, "refold", 5);
    let codes = |feature: &Feature| {
        super::super::retained_control_losses([feature])
            .into_iter()
            .map(|note| json::to_string(&note).expect("note"))
            .collect::<Vec<_>>()
    };
    let unfold_notes = codes(&unfold);
    assert_eq!(unfold_notes.len(), 1);
    assert!(unfold_notes[0].contains("feature.sheet-metal-fold-controls-retained"));
    let refold_notes = codes(&refold);
    assert_eq!(refold_notes.len(), 2);
    assert!(refold_notes[0].contains("feature.sheet-metal-fold-controls-retained"));
    assert!(refold_notes[1].contains("feature.sheet-metal-refold-source-unresolved"));

    let mut native = unfold.clone();
    native.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("InventorFeature".into()),
            parameters: BTreeMap::new(),
        },
    ));
    let mut refusals = Refusals::default();
    refusals.insert(
        native.native_ref.clone().expect("source"),
        FoldRefusal::Bends,
    );
    let notes = refusals.losses(&[native, unfold]);
    assert_eq!(notes.len(), 1);
    let text = json::to_string(&notes[0]).expect("note");
    assert!(text.contains("feature.sheet-metal-fold-refused"));
    assert!(text.contains("bends"));
}
