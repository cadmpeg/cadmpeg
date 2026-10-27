// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::PmDcFeaturePropertyKind as Kind;
use crate::test_support::sheet_metal::{feature, list, property, reference};

fn fixture() -> (PmDcFeature, PmDcFeature, Vec<PmDcFeatureProperty>) {
    let mut plate_slots = [0; 17];
    for (slot, value) in [(0, 4), (6, 9), (9, 1), (12, 3), (15, 5)] {
        plate_slots[slot] = value;
    }
    let sheet = |definition| Kind::SheetMetal { definition };
    let refs = |family, values: &[u32]| Kind::References {
        family,
        items: list(values),
    };
    let properties = vec![
        property(
            1,
            &sheet(SheetMetalProperty::Corner {
                control: reference(10),
                gap: reference(20),
                size: reference(21),
                relief_form: reference(12),
                corner_form: reference(13),
                secondary_form: reference(14),
                jacobi_radius: reference(22),
                value: 321,
                option: reference(11),
            }),
        ),
        property(2, &refs(PmDcFeatureReferenceFamily::FeatureDimensions, &[])),
        property(
            3,
            &refs(
                PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
                &[6],
            ),
        ),
        property(
            4,
            &Kind::ProfileSelection {
                entity_link: reference(0),
                value: 0,
            },
        ),
        property(5, &refs(PmDcFeatureReferenceFamily::ObjectCollection, &[])),
        property(
            6,
            &sheet(SheetMetalProperty::EdgeSelection {
                edges: reference(7),
            }),
        ),
        property(7, &refs(PmDcFeatureReferenceFamily::EdgeCollection, &[8])),
        property(
            8,
            &Kind::EdgeItem {
                index_references: crate::pmdc::PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([3, 0])),
                    vec![31, 32],
                )
                .expect("synthetic corner operands"),
                index_reference_value: -1,
                value: 0,
            },
        ),
        property(
            10,
            &Kind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        property(9, &Kind::SurfaceBody { body: reference(0) }),
        property(
            11,
            &Kind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        property(
            12,
            &sheet(SheetMetalProperty::Enumeration {
                type_value: 501,
                value: 7,
            }),
        ),
        property(
            13,
            &sheet(SheetMetalProperty::Enumeration {
                type_value: 8,
                value: 9,
            }),
        ),
        property(
            14,
            &sheet(SheetMetalProperty::Enumeration {
                type_value: 304,
                value: 11,
            }),
        ),
    ];
    (
        feature(80, &[1, 0, 2, 3, 4, 0, 5]),
        feature(70, &plate_slots),
        properties,
    )
}

fn decode(
    source: &PmDcFeature,
    plate: &PmDcFeature,
    properties: &[PmDcFeatureProperty],
) -> Option<Operands> {
    read(
        source,
        plate,
        |r| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == r.index())
        },
        |r| match r.index() {
            20 => Length::new(2.5),
            21 => Length::new(4.5),
            22 => Length::new(6.5),
            _ => None,
        },
    )
}

#[test]
fn corner_retains_exact_lengths_control_domains_and_selector_order() {
    let (source, plate, properties) = fixture();
    let values = decode(&source, &plate, &properties).expect("synthetic corner operands");
    assert_eq!(values.gap.get(), 2.5);
    assert_eq!(values.size.get(), 4.5);
    assert_eq!(values.jacobi_radius.get(), 6.5);
    assert_eq!(values.relief_form, (501, 7));
    assert_eq!(values.secondary_form, (304, 11));
    assert_eq!(values.corner_form, (8, 9));
    assert!(!values.control);
    assert!(values.option);
    assert_eq!(values.raw_value, 321);
    assert_eq!(values.selection_sources, [31, 32]);
    let parameters = values.parameters().expect("synthetic corner operands");
    assert_eq!(parameters.get("gap_mm").map(String::as_str), Some("2.5"));
    assert_eq!(
        parameters.get("secondary_form").map(String::as_str),
        Some("304:11")
    );
}

#[test]
fn corner_accepts_the_six_slot_form_with_its_shared_result_body() {
    let (_, plate, mut properties) = fixture();
    let source = feature(80, &[1, 9, 2, 3, 4, 0]);
    assert!(decode(&source, &plate, &properties).is_some());
    assert!(result_shared(&source, &plate));
    for body in [0, 4, 5] {
        let source = feature(80, &[1, body, 2, 3, 4, 0]);
        assert!(decode(&source, &plate, &properties).is_none());
        assert!(!result_shared(&source, &plate));
    }
    *properties
        .iter_mut()
        .find(|p| p.identity.record_ordinal == 8)
        .expect("synthetic body property") = property(
        9,
        &Kind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            items: list(&[]),
        },
    );
    assert!(decode(&source, &plate, &properties).is_none());
}

#[test]
fn corner_requires_the_exact_shared_operands_and_null_slots() {
    let (_, plate, properties) = fixture();
    for (slot, replacement) in [(0, 4), (1, 1), (2, 5), (3, 5), (4, 5), (5, 1), (6, 4)] {
        let mut slots = [1, 0, 2, 3, 4, 0, 5];
        slots[slot] = replacement;
        assert!(decode(&feature(80, &slots), &plate, &properties).is_none());
    }
}

#[test]
fn missing_dimensions_and_ambiguous_edge_selectors_stay_native() {
    let (source, plate, mut properties) = fixture();
    assert!(read(
        &source,
        &plate,
        |r| properties
            .iter()
            .find(|p| p.identity.record_ordinal + 1 == r.index()),
        |_| None
    )
    .is_none());
    properties.retain(|p| p.identity.record_ordinal != 7);
    assert!(decode(&source, &plate, &properties).is_none());
}

#[test]
fn unchanged_corner_requires_one_edge_complete_names_and_the_exact_control_branch() {
    use crate::feature::edge_selection::{EdgeInventory, SourceNames};
    let (source, plate, properties) = fixture();
    let mut operands = decode(&source, &plate, &properties).expect("corner graph");
    operands.selection_sources.truncate(1);
    operands.corner_form = (8, 3);
    operands.relief_form = (501, 100);
    operands.secondary_form = (501, 303);
    operands.raw_value = 0;
    let mut names = EdgeInventory {
        generated_sources: Some(std::collections::HashMap::default()),
        generated_sources_complete: true,
        ..Default::default()
    };
    assert!(unchanged(&operands, 81, &names));
    names.generated_sources_complete = false;
    assert!(!unchanged(&operands, 81, &names));
    names.generated_sources_complete = true;
    names
        .generated_sources
        .as_mut()
        .expect("source inventory")
        .insert(81, SourceNames::default());
    assert!(!unchanged(&operands, 81, &names));
    names
        .generated_sources
        .as_mut()
        .expect("source inventory")
        .clear();
    operands.selection_sources.push(32);
    assert!(!unchanged(&operands, 81, &names));
    operands.selection_sources.truncate(1);
    operands.corner_form = (8, 5);
    assert!(!unchanged(&operands, 81, &names));
    operands.corner_form = (8, 3);
    operands.option = false;
    assert!(!unchanged(&operands, 81, &names));
    operands.option = true;
    operands.raw_value = 1;
    assert!(!unchanged(&operands, 81, &names));
}

#[test]
fn unplaced_suppressed_and_flat_pattern_features_cannot_supply_an_unchanged_proof() {
    let mut feature: Feature = serde_json::from_value(serde_json::json!({
        "id":"test:model:feature#corner", "ordinal":3,
        "suppressed":false, "definition":{"definition":"native","kind":"SheetMetalCorner"}
    }))
    .expect("synthetic history feature");
    assert!(active_main(&feature));
    feature.suppressed = None;
    assert!(!active_main(&feature));
    feature.suppressed = Some(true);
    assert!(!active_main(&feature));
    feature.suppressed = Some(false);
    insert(
        &mut feature.source_properties,
        "history",
        "flat_pattern".into(),
    )
    .expect("history key");
    assert!(!active_main(&feature));
}

#[test]
fn independent_result_collections_retain_operands_but_do_not_supply_the_shared_result_proof() {
    let (source, plate, mut properties) = fixture();
    assert!(result_shared(&source, &plate));
    properties.push(property(
        15,
        &Kind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            items: list(&[]),
        },
    ));
    let source = feature(80, &[1, 0, 2, 3, 4, 0, 15]);
    assert!(decode(&source, &plate, &properties).is_some());
    assert!(!result_shared(&source, &plate));
}
