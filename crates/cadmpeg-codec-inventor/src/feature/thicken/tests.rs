// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::test_fixtures::{content_at, parse};

#[test]
fn selection_properties_end_after_the_two_versioned_references() {
    for (version, stamps) in [
        (16, &[][..]),
        (19, &[][..]),
        (23, &[-1][..]),
        (25, &[-1, 1][..]),
        (29, &[-1, 1][..]),
    ] {
        let mut bytes = content_at(3, version, stamps);
        bytes.extend_from_slice(&0x8000_0005u32.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        parse(&bytes, |ctx, source| {
            let parser = property_parser(SELECTION_TYPE).expect("selection parser");
            let parsed = parser(ctx, source, version).expect("selection property");
            assert!(property_kind_matches_class(
                &crate::pmdc::type_id_string(SELECTION_TYPE),
                &parsed.kind
            ));
            let PmDcFeaturePropertyKind::ThickenSelection { faces, parameter } = parsed.kind else {
                panic!("selection property")
            };
            assert_eq!(faces.index(), 5);
            assert!(faces.qualified());
            assert_eq!(parameter.index(), 7);
            assert!(!parameter.qualified());
        });
        bytes.push(0);
        parse(&bytes, |ctx, source| {
            assert!(parse_selection(ctx, source, version).is_err());
        });
        bytes.truncate(bytes.len() - 2);
        parse(&bytes, |ctx, source| {
            assert!(parse_selection(ctx, source, version).is_err());
        });
    }
}

#[test]
fn control_properties_keep_the_type_and_value_words() {
    for (type_id, family, type_value, value) in [
        (
            CONTROL_13_TYPE,
            PmDcFeatureEnumFamily::ThickenControl13,
            3i16,
            3u16,
        ),
        (
            CONTROL_14_TYPE,
            PmDcFeatureEnumFamily::ThickenControl14,
            1,
            0,
        ),
    ] {
        for (version, stamps) in [
            (16, &[][..]),
            (19, &[][..]),
            (23, &[-1][..]),
            (25, &[-1, 1][..]),
            (29, &[-1, 1][..]),
        ] {
            let mut bytes = content_at(3, version, stamps);
            bytes.extend_from_slice(&type_value.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
            let parser = property_parser(type_id).expect("control parser");
            parse(&bytes, |ctx, source| {
                let parsed = parser(ctx, source, version).expect("control property");
                assert!(property_kind_matches_class(
                    &crate::pmdc::type_id_string(type_id),
                    &parsed.kind
                ));
                assert_eq!(
                    parsed.kind,
                    PmDcFeaturePropertyKind::Enumeration {
                        family,
                        type_value,
                        value
                    }
                );
            });
            bytes.push(0);
            parse(&bytes, |ctx, source| {
                assert!(parser(ctx, source, version).is_err());
            });
            bytes.truncate(bytes.len() - 2);
            parse(&bytes, |ctx, source| {
                assert!(parser(ctx, source, version).is_err());
            });
        }
    }
}

#[test]
fn every_thicken_property_kind_requires_its_record_class() {
    let kinds = [
        (
            SELECTION_TYPE,
            PmDcFeaturePropertyKind::ThickenSelection {
                faces: crate::pmdc::PmDcReference::from_packed(3),
                parameter: crate::pmdc::PmDcReference::from_packed(5),
            },
        ),
        (
            CONTROL_13_TYPE,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::ThickenControl13,
                type_value: -3,
                value: u16::MAX,
            },
        ),
        (
            CONTROL_14_TYPE,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::ThickenControl14,
                type_value: -5,
                value: u16::MAX,
            },
        ),
    ];
    for (required, kind) in kinds {
        for candidate in [SELECTION_TYPE, CONTROL_13_TYPE, CONTROL_14_TYPE, [0; 16]] {
            assert_eq!(
                property_kind_matches_class(&crate::pmdc::type_id_string(candidate), &kind),
                candidate == required
            );
        }
    }
    let boolean = PmDcFeaturePropertyKind::Boolean {
        name: String::new(),
        name_value: 0,
        value: true,
    };
    for candidate in [SELECTION_TYPE, CONTROL_13_TYPE, CONTROL_14_TYPE] {
        assert!(!property_kind_matches_class(
            &crate::pmdc::type_id_string(candidate),
            &boolean
        ));
    }
    assert!(property_kind_matches_class(
        &crate::pmdc::type_id_string([0; 16]),
        &boolean
    ));
}

struct Operands {
    source: PmDcFeature,
    properties: Vec<PmDcFeatureProperty>,
    parameters: Vec<crate::design::PmDcParameter>,
    values: Vec<cadmpeg_ir::features::DesignParameter>,
    dimensions: Vec<super::super::revolve::Dimension>,
}

impl Operands {
    fn new() -> Self {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        let bytes = crate::test_support::thicken::offset_fixture();
        let ir = crate::InventorCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthetic operands")
            .into_parts()
            .0;
        let native = ir.native.namespace("inventor").expect("Inventor namespace");
        let mut sources: Vec<PmDcFeature> = native.arena_as("pm_dc_features").expect("sources");
        Self {
            source: sources.pop().expect("one source"),
            properties: native
                .arena_as("pm_dc_feature_properties")
                .expect("properties"),
            parameters: native.arena_as("pm_dc_parameters").expect("parameters"),
            dimensions: native
                .arena_as("pm_dc_feature_dimensions")
                .expect("dimensions"),
            values: ir.model.parameters,
        }
    }

    fn operation(&self) -> Result<FeatureOperation, NativeReason> {
        operation(
            &self.source,
            |reference| {
                self.properties
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == reference.record_ordinal())
            },
            |reference| {
                let raw = self
                    .parameters
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == reference.record_ordinal())?;
                let value = self
                    .values
                    .iter()
                    .find(|p| p.native_ref.as_deref() == Some(raw.id().as_str()))?
                    .value
                    .as_ref()?;
                Some((raw, value))
            },
            |reference| {
                self.dimensions
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == reference.record_ordinal())
            },
            |ordinal| {
                self.properties
                    .iter()
                    .find(|p| p.identity.record_ordinal == ordinal)
                    .map(|p| p.identity.type_id.as_str())
            },
        )
    }

    fn change_slot(&mut self, slot: usize, change: impl FnOnce(&mut PmDcFeaturePropertyKind)) {
        let ordinal = self.source.properties.references()[slot]
            .record_ordinal()
            .expect("slot ordinal");
        let mut property = self
            .properties
            .iter()
            .find(|p| p.identity.record_ordinal == ordinal)
            .expect("slot property")
            .clone();
        let ordinal = self
            .properties
            .iter()
            .map(|p| p.identity.record_ordinal)
            .max()
            .expect("properties")
            + 1;
        property.identity.record_ordinal = ordinal;
        property.edit(|p| change(&mut p.kind));
        self.properties.push(property);
        self.set_slot(slot, PmDcReference::from_packed(ordinal + 1));
    }

    fn set_slot(&mut self, slot: usize, reference: PmDcReference) {
        let mut refs = self.source.properties.references().to_vec();
        refs[slot] = reference;
        self.source.edit(|p| {
            p.properties = crate::pmdc::PmDcReferenceList::new(
                2,
                Some(crate::pmdc::PmDcListMetadata::U32([18, 0])),
                refs,
            )
            .expect("property vector");
        });
    }
}

#[test]
fn offset_requires_each_nonnull_operand_and_each_reserved_null() {
    for slot in 0..18 {
        let mut fixture = Operands::new();
        let mut refs = fixture.source.properties.references().to_vec();
        refs[slot] = PmDcReference::from_packed(u32::from([5, 8, 11].contains(&slot)));
        fixture.source.edit(|p| {
            p.properties = crate::pmdc::PmDcReferenceList::new(
                2,
                Some(crate::pmdc::PmDcListMetadata::U32([18, 0])),
                refs,
            )
            .expect("synthetic property vector");
        });
        assert!(fixture.operation().is_err(), "slot {slot}");
    }
}

#[test]
fn offset_checks_control_words_and_every_boolean_polarity() {
    for slot in [1, 13, 14] {
        for word in [0, 1] {
            let mut fixture = Operands::new();
            fixture.change_slot(slot, |kind| {
                let PmDcFeaturePropertyKind::Enumeration {
                    type_value, value, ..
                } = kind
                else {
                    panic!("enum")
                };
                if word == 0 {
                    *type_value += 1;
                } else {
                    *value += 1;
                }
            });
            assert!(fixture.operation().is_err(), "slot {slot}, word {word}");
        }
    }
    for slot in [3, 4, 7, 10, 16, 17] {
        let mut fixture = Operands::new();
        fixture.change_slot(slot, |kind| {
            let PmDcFeaturePropertyKind::Boolean { value, .. } = kind else {
                panic!("Boolean")
            };
            *value = !*value;
        });
        assert!(fixture.operation().is_err(), "slot {slot}");
    }
    let mut fixture = Operands::new();
    fixture.change_slot(2, |kind| {
        *kind = PmDcFeaturePropertyKind::TerminationFlag { value: 0 }
    });
    assert_eq!(fixture.operation(), Err(NativeReason::Slot(2)));
}

#[test]
fn offset_uses_the_matching_positive_dimension_parameter() {
    let fixture = Operands::new();
    assert!(
        matches!(fixture.operation(), Ok(FeatureOperation::MoveFace {
        motion: FaceMotion::Offset { distance }, ..
    }) if distance.get() == 4.0)
    );
    for value in [-0.4, 0.0] {
        let mut fixture = Operands::new();
        let parameter = fixture.dimensions[0].parameter;
        let raw = fixture
            .parameters
            .iter_mut()
            .find(|p| Some(p.identity.record_ordinal) == parameter.record_ordinal())
            .expect("dimension parameter");
        raw.edit(|p| p.model_value = cadmpeg_ir::scalar::FiniteReal::new(value).expect("scalar"));
        let id = raw.id();
        fixture
            .values
            .iter_mut()
            .find(|p| p.native_ref.as_deref() == Some(id.as_str()))
            .expect("neutral dimension")
            .value = Some(ParameterValue::Length(
            Length::new(value * 10.0).expect("length"),
        ));
        assert_eq!(fixture.operation(), Err(NativeReason::Slot(9)));
    }
    let mut fixture = Operands::new();
    fixture.dimensions[0].edit(|d| d.parameter = fixture.source.properties.references()[6]);
    assert_eq!(fixture.operation(), Err(NativeReason::Slot(9)));
}

#[test]
fn offset_requires_single_selection_dimension_and_result_members() {
    for slot in [0, 9, 15] {
        for count in [0, 2] {
            let mut fixture = Operands::new();
            fixture.change_slot(slot, |kind| {
                let PmDcFeaturePropertyKind::References { items, .. } = kind else {
                    panic!("references")
                };
                let member = items.references()[0];
                let refs = if count == 0 {
                    Vec::new()
                } else {
                    vec![member, member]
                };
                *items = crate::pmdc::PmDcReferenceList::new(
                    2,
                    if count == 0 {
                        None
                    } else {
                        Some(crate::pmdc::PmDcListMetadata::U32([2, 0]))
                    },
                    refs,
                )
                .expect("list");
            });
            assert!(fixture.operation().is_err(), "slot {slot}, count {count}");
        }
    }
}

#[test]
fn offset_requires_nonempty_distinct_faces_and_single_nonzero_indices() {
    for mode in 0..7 {
        let mut fixture = Operands::new();
        let faces = fixture
            .properties
            .iter_mut()
            .find(|p| {
                matches!(
                    p.kind,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::FaceCollection,
                        ..
                    }
                )
            })
            .expect("face collection");
        if mode < 2 {
            faces.edit(|p| {
                let PmDcFeaturePropertyKind::References { items, .. } = &mut p.kind else {
                    panic!("faces")
                };
                let member = items.references()[0];
                *items = if mode == 0 {
                    crate::pmdc::PmDcReferenceList::new(2, None, Vec::new())
                } else {
                    crate::pmdc::PmDcReferenceList::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([2, 0])),
                        vec![member, member],
                    )
                }
                .expect("face list");
            });
        } else {
            let face = fixture
                .properties
                .iter_mut()
                .find(|p| matches!(p.kind, PmDcFeaturePropertyKind::FaceItem { .. }))
                .expect("face item");
            face.edit(|p| {
                if mode == 6 {
                    p.kind = PmDcFeaturePropertyKind::TerminationFlag { value: 0 };
                    return;
                }
                let PmDcFeaturePropertyKind::FaceItem { index_references } = &mut p.kind else {
                    panic!("indices")
                };
                let values = match mode {
                    2 => vec![],
                    3 => vec![0],
                    4 => vec![1, 2],
                    _ => vec![1],
                };
                let metadata = if values.is_empty() {
                    None
                } else {
                    Some(crate::pmdc::PmDcListMetadata::U32([
                        1,
                        if mode == 5 { 0 } else { 0x1000_0000 },
                    ]))
                };
                *index_references =
                    crate::pmdc::PmDcU32List::new(2, metadata, values).expect("index list");
            });
        }
        assert_eq!(
            fixture.operation(),
            Err(NativeReason::Slot(0)),
            "mode {mode}"
        );
    }
}

#[test]
fn offset_requires_a_resolved_segment_version() {
    let mut fixture = Operands::new();
    for version in 0..=u8::MAX {
        fixture.source.edit(|p| p.save_version_major = version);
        if matches!(version, 19 | 25) {
            assert!(fixture.operation().is_ok());
        } else {
            assert_eq!(fixture.operation(), Err(NativeReason::Version));
        }
    }
    assert_eq!(
        NativeReason::Version.note().code,
        crate::loss::InventorLossCode::FeatureThickenConstructionNative.kind()
    );
}
