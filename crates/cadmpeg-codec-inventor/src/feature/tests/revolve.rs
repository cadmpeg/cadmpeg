// SPDX-License-Identifier: Apache-2.0
//! Revolves and the work axes they turn about.

use super::*;
use crate::feature::{
    parse_work_axis, project_revolve, FeatureFamily, REVOLVE_CLASS_ID, REVOLVE_EXTENT_FULL,
};
use cadmpeg_ir::features::PlanarProfileRef;

/// A revolve of one profile selection about a work axis through
/// (0.5, 0, 0) cm along the sketch v axis, with extent value `extent`.
fn generated_revolve(extent: u16) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    generated_revolve_with(extent, |_, _, _, _| {})
}

fn generated_revolve_with(
    extent: u16,
    configure: impl FnOnce(
        &mut PmDcFeature,
        &mut Vec<PmDcFeatureProperty>,
        &mut Vec<DesignParameter>,
        &mut Vec<crate::feature::revolve::Dimension>,
    ),
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    generated_revolve_with_losses(extent, configure, &mut Vec::new())
}

fn generated_revolve_with_losses(
    extent: u16,
    configure: impl FnOnce(
        &mut PmDcFeature,
        &mut Vec<PmDcFeatureProperty>,
        &mut Vec<DesignParameter>,
        &mut Vec<crate::feature::revolve::Dimension>,
    ),
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let raw_sketch = Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 16,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(8, None, Vec::new()).expect("empty entity list"),
            transform: reference(0),
            direction: reference(0),
            values: [0; 2],
            auxiliary: None,
        },
        crate::record_identity::RecordTypeId::try_from(
            "114d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment().clone(),
        50,
    );
    let neutral_sketch = Sketch {
        id: SketchId::mint(format!("inventor:design:sketch#{}-50", segment()))
            .expect("valid test fixture"),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid test fixture"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(raw_sketch.id()),
    };
    let entity_link = Located::new(
        PmDcEntityStyleLinkPayload {
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                owner: reference(0),
                parent: reference(0),
                next: reference(0),
                flags_extension: None,
            },
            value: 0,
            associative_id: 1,
            entity_type: 1,
        },
        crate::record_identity::RecordTypeId::from_bytes(ENTITY_STYLE_LINK_TYPE),
        segment().clone(),
        51,
    );
    let real = |value: f64| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite");
    let mut properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 2,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(&[4]),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: reference(52),
                value: 0,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::WorkAxis {
                entity_flags: 0,
                values: vec![0, 0],
                references: reference_list(&[]),
                marker: false,
                origin: [real(0.5), real(0.0), real(0.0)],
                span: [real(0.0), real(2.0), real(0.0)],
                state: 1,
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: extent,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[9]),
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    properties.extend([
        test_property(
            9,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 3,
                value: 0,
            },
        ),
        test_property(
            10,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                items: reference_list(&[81]),
            },
        ),
        test_property(
            11,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 4,
                value: 0,
            },
        ),
        test_property(
            12,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        test_property(
            13,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
    ]);
    let raw_parameters = [raw_parameter(70)];
    let mut parameters = vec![neutral_parameter(
        &raw_parameters[0],
        ParameterValue::Angle(Angle::new(1.25).expect("angle")),
    )];
    let mut dimensions = vec![Located::new(
        crate::feature::revolve::DimensionPayload {
            parameter: reference(71),
            ..Default::default()
        },
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::revolve::DIMENSION_TYPE),
        segment().clone(),
        80,
    )];
    let mut feature = test_feature(
        101,
        20,
        &[
            (0, 1),
            (1, 2),
            (2, 4),
            (3, 5),
            (4, 70),
            (5, 9),
            (7, 10),
            (9, 11),
            (10, 12),
            (11, 2),
            (12, 13),
            (16, 13),
            (17, 7),
        ],
    );
    configure(
        &mut feature,
        &mut properties,
        &mut parameters,
        &mut dimensions,
    );
    properties = properties
        .into_iter()
        .map(|record| {
            use crate::feature::{
                AUXILIARY_ENUM_TYPE, BOOLEAN_TYPE, BOUNDARY_PATCH_TYPE, EXTENT_TYPE,
                FEATURE_DIMENSIONS_TYPE, OBJECT_COLLECTION_TYPE, PART_OPERATION_TYPE,
                PROFILE_SELECTION_TYPE, SURFACE_BODY_TYPE, WORK_AXIS_TYPE,
            };
            let type_id = match &record.kind {
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::PartOperation,
                    ..
                } => PART_OPERATION_TYPE,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Extent,
                    ..
                } => EXTENT_TYPE,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Auxiliary,
                    type_value: 3,
                    ..
                } => crate::feature::revolve::CONTROL_TYPE,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Auxiliary,
                    ..
                } => AUXILIARY_ENUM_TYPE,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    ..
                } => BOUNDARY_PATCH_TYPE,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FeatureDimensions,
                    ..
                } => FEATURE_DIMENSIONS_TYPE,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    ..
                } => OBJECT_COLLECTION_TYPE,
                PmDcFeaturePropertyKind::Boolean { .. } => BOOLEAN_TYPE,
                PmDcFeaturePropertyKind::WorkAxis { .. } => WORK_AXIS_TYPE,
                PmDcFeaturePropertyKind::SurfaceBody { .. } => SURFACE_BODY_TYPE,
                PmDcFeaturePropertyKind::ProfileSelection { .. } => PROFILE_SELECTION_TYPE,
                _ => panic!("unexpected revolve fixture property"),
            };
            Located::new(
                (*record).clone(),
                crate::record_identity::RecordTypeId::from_bytes(type_id),
                record.identity.segment_token.clone(),
                record.identity.record_ordinal,
            )
        })
        .collect();
    let label = test_label(101, 6, REVOLVE_CLASS_ID, &[51]);
    let mut index = test_projection_index(
        &properties,
        &raw_parameters,
        &parameters,
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        &[],
        &[],
        std::slice::from_ref(&entity_link),
    );
    index.dimensions = cadmpeg_core::decode::unique_index(dimensions.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project_revolve(&ctx, &feature, &label, &index, losses)
}

#[test]
fn projects_a_full_revolve_about_its_work_axis() {
    assert!(matches!(
        FeatureFamily::from_class_id(REVOLVE_CLASS_ID),
        Some(FeatureFamily::Revolve)
    ));
    let (projected, _) = generated_revolve(REVOLVE_EXTENT_FULL)
        .expect("revolve candidate")
        .expect("revolve projection");
    let FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction:
            cadmpeg_ir::features::RevolveConstruction::Resolved {
                axis,
                extent:
                    cadmpeg_ir::features::RevolveExtent::OneSided {
                        termination: cadmpeg_ir::features::AngularTermination::Angle { angle },
                    },
                ..
            },
        op: BooleanOp::Cut,
    }) = projected.evaluation.definition()
    else {
        panic!("full revolve");
    };
    assert_eq!(axis.origin.get(), Point3::new(5.0, 0.0, 0.0));
    assert_eq!(axis.direction.get(), Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(angle.get(), std::f64::consts::TAU);
}

#[test]
fn parses_generated_work_axis_records() {
    let axis = |marker: bool| {
        let mut bytes = content(5);
        bytes.extend_from_slice(&0x0042_4000u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        if marker {
            bytes.extend_from_slice(&0x203u32.to_le_bytes());
        }
        for value in [-1.0f64, 0.0, 0.0, 2.0, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0);
        bytes
    };
    for marker in [false, true] {
        let bytes = axis(marker);
        assert_eq!(bytes.len(), if marker { 95 } else { 91 });
        let parsed = parse(&bytes, |ctx, source| {
            parse_work_axis(ctx, source, 19).expect("work axis")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::WorkAxis { entity_flags: 0x0042_4000, marker: parsed_marker, origin, span, state: 0, .. }
                if parsed_marker == marker
                    && origin[0].get() == -1.0
                    && span[0].get() == 2.0
        ));
    }
}

#[test]
fn parses_a_version_25_work_axis_after_its_stamps_and_third_value() {
    let mut bytes = content_at(5, 25, &[-1, 2512]);
    bytes.extend_from_slice(&0x0042_4000u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for value in [0.0f64, 0.0, -1.0, 0.0, 0.0, 2.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0);
    let parsed = parse(&bytes, |ctx, source| {
        parse_work_axis(ctx, source, 25).expect("work axis")
    });
    assert_eq!(parsed.header.stamps, [-1, 2512]);
    let PmDcFeaturePropertyKind::WorkAxis {
        values,
        marker,
        origin,
        span,
        ..
    } = parsed.kind
    else {
        panic!("work axis");
    };
    assert_eq!(values.len(), 3);
    assert!(!marker);
    assert_eq!(origin[2].get(), -1.0);
    assert_eq!(span[2].get(), 2.0);
}

fn replace_slot(source: &mut PmDcFeature, slot: usize, value: u32) {
    let mut json = serde_json::to_value(&source).expect("feature JSON");
    json["properties"]["references"][slot] =
        serde_json::to_value(reference(value)).expect("reference JSON");
    *source = serde_json::from_value(json).expect("feature frame");
}

#[test]
fn nondefault_revolve_controls_stay_native() {
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, _| {
            replace_slot(source, 11, 99);
        })
        .is_none()
    );
    for slot in [6, 8, 13, 14, 15, 18, 19] {
        assert!(
            generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, _| replace_slot(
                source, slot, 99
            ))
            .is_none(),
            "slot {slot}"
        );
    }
    for slot in [1, 5, 7, 9, 10, 12, 16] {
        assert!(
            generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, _| replace_slot(
                source, slot, 0
            ))
            .is_none(),
            "missing slot {slot}"
        );
    }
    for (slot, value) in [(5, 2), (9, 1)] {
        assert!(
            generated_revolve_with(REVOLVE_EXTENT_FULL, |source, properties, _, _| {
                let reference = source.properties.references()[slot].index();
                let position = properties
                    .iter()
                    .position(|p| p.identity.record_ordinal + 1 == reference)
                    .expect("property");
                properties[position] = test_property(
                    reference - 1,
                    PmDcFeaturePropertyKind::Enumeration {
                        family: PmDcFeatureEnumFamily::Auxiliary,
                        type_value: if slot == 5 { 3 } else { 4 },
                        value,
                    },
                );
            })
            .is_none()
        );
    }
    for (slot, value) in [(10, false), (12, true)] {
        assert!(
            generated_revolve_with(REVOLVE_EXTENT_FULL, |source, properties, _, _| {
                properties.push(test_property(
                    90,
                    PmDcFeaturePropertyKind::Boolean {
                        name: String::new(),
                        name_value: 0,
                        value,
                    },
                ));
                replace_slot(source, slot, 91);
            })
            .is_none()
        );
    }
}

#[test]
fn an_absent_profile_repeat_keeps_the_selected_profile_and_result() {
    let (expected, expected_result) = generated_revolve(REVOLVE_EXTENT_FULL)
        .expect("candidate")
        .expect("revolve");
    let (actual, actual_result) = generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, _| {
        replace_slot(source, 11, 0);
    })
    .expect("candidate")
    .expect("revolve");
    assert_eq!(
        actual.evaluation.definition(),
        expected.evaluation.definition()
    );
    assert_eq!(actual_result, expected_result);
}

#[test]
fn a_multi_member_profile_requires_the_same_repeated_patch() {
    let configure =
        |source: &mut PmDcFeature, properties: &mut Vec<PmDcFeatureProperty>, different: bool| {
            properties[1] = test_property(
                2,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    items: reference_list(&[4, 92]),
                },
            );
            properties.push(test_property(
                91,
                PmDcFeaturePropertyKind::ProfileSelection {
                    entity_link: reference(52),
                    value: 0,
                },
            ));
            if different {
                properties.push(test_property(
                    92,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::BoundaryPatch,
                        items: reference_list(&[92, 4]),
                    },
                ));
                replace_slot(source, 11, 93);
            }
        };
    let (feature, _) = generated_revolve_with(REVOLVE_EXTENT_FULL, |source, properties, _, _| {
        configure(source, properties, false);
    })
    .expect("candidate")
    .expect("revolve");
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction: cadmpeg_ir::features::RevolveConstruction::Resolved {
                profile: PlanarProfileRef::SketchSelection { selections, .. }, ..
            }, ..
        }) if selections.len() == 2
    ));
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |source, properties, _, _| {
            configure(source, properties, true);
        })
        .is_none()
    );
}

#[test]
fn revolve_requires_an_angle_parameter_and_matching_feature_dimension() {
    for value in [
        None,
        Some(ParameterValue::Length(Length::new(1.0).expect("length"))),
    ] {
        assert!(
            generated_revolve_with(REVOLVE_EXTENT_FULL, |_, _, parameters, _| parameters[0]
                .value =
                value)
            .is_none()
        );
    }
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |_, _, _, dimensions| dimensions
            .clear())
        .is_none()
    );
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |_, _, _, dimensions| {
            let mut json = serde_json::to_value(&dimensions[0]).expect("dimension JSON");
            json["parameter"] = serde_json::to_value(reference(72)).expect("reference JSON");
            dimensions[0] = serde_json::from_value(json).expect("dimension");
        })
        .is_none()
    );
}

#[test]
fn native_dimension_annotation_data_does_not_change_the_angular_travel() {
    let (feature, _) = generated_revolve_with(1, |_, _, _, dimensions| {
        let mut json = serde_json::to_value(&dimensions[0]).expect("dimension JSON");
        json["item_count"] = serde_json::json!(1);
        json["metadata"] = serde_json::to_value(crate::pmdc::PmDcListMetadata::U32([1, 0]))
            .expect("metadata JSON");
        json["items"] =
            serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::new([0x5au8; 24]))
                .expect("data JSON");
        dimensions[0] = serde_json::from_value(json).expect("dimension");
    })
    .expect("candidate")
    .expect("revolve");
    assert!(
        matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: cadmpeg_ir::features::RevolveConstruction::Resolved {
            extent: cadmpeg_ir::features::RevolveExtent::OneSided {
                termination: cadmpeg_ir::features::AngularTermination::Angle { angle },
            }, ..
        }, ..
    }) if angle.get() == 1.25)
    );
}

#[test]
fn revolve_rejects_an_unknown_operation_a_non_axis_and_a_zero_span() {
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |_, properties, _, _| {
            properties[0] = test_property(
                1,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::PartOperation,
                    type_value: 5,
                    value: 99,
                },
            );
        })
        .is_none()
    );
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, _| replace_slot(
            source, 2, 6
        ))
        .is_none()
    );
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |_, properties, _, _| {
            let mut json = serde_json::to_value(&properties[3]).expect("axis JSON");
            json["kind"]["span"] = serde_json::json!([0.0, 0.0, 0.0]);
            properties[3] = serde_json::from_value(json).expect("axis");
        })
        .is_none()
    );
}

#[test]
fn revolve_extents_without_closed_operands_stay_native() {
    for extent in [0, 2, 4, 5, 6, 7, 8, u16::MAX] {
        assert!(generated_revolve(extent).is_none(), "extent {extent}");
    }
}

#[test]
fn projects_the_positive_one_sided_angular_travel() {
    for value in [1.25, std::f64::consts::TAU + 1.0] {
        let (feature, _) = generated_revolve_with(1, |_, _, parameters, _| {
            parameters[0].value = Some(ParameterValue::Angle(Angle::new(value).expect("angle")));
        })
        .expect("candidate")
        .expect("partial revolve");
        let FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction:
                cadmpeg_ir::features::RevolveConstruction::Resolved {
                    axis,
                    extent:
                        cadmpeg_ir::features::RevolveExtent::OneSided {
                            termination: cadmpeg_ir::features::AngularTermination::Angle { angle },
                        },
                    solid: Some(true),
                    ..
                },
            ..
        }) = feature.evaluation.definition()
        else {
            panic!("partial revolve");
        };
        assert_eq!(axis.direction.get(), Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(angle.get(), value);
    }
}

#[test]
fn a_fixed_angle_requires_positive_travel_and_a_proven_direction() {
    for value in [0.0, -1.0] {
        assert!(generated_revolve_with(1, |_, _, parameters, _| {
            parameters[0].value = Some(ParameterValue::Angle(Angle::new(value).expect("angle")));
        })
        .is_none());
    }
    assert!(generated_revolve_with(1, |_, properties, _, _| {
        let control = properties
            .iter_mut()
            .find(|p| p.identity.record_ordinal == 9)
            .expect("control");
        *control = test_property(
            9,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 3,
                value: 1,
            },
        );
    })
    .is_none());
}

#[test]
fn a_full_turn_does_not_consume_the_stored_angle_value() {
    for value in [0.0, -1.0, 1.25, std::f64::consts::TAU + 1.0] {
        let (feature, _) = generated_revolve_with(REVOLVE_EXTENT_FULL, |_, _, parameters, _| {
            parameters[0].value = Some(ParameterValue::Angle(Angle::new(value).expect("angle")));
        })
        .expect("candidate")
        .expect("full turn");
        assert!(
            matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction: cadmpeg_ir::features::RevolveConstruction::Resolved {
                extent: cadmpeg_ir::features::RevolveExtent::OneSided {
                    termination: cadmpeg_ir::features::AngularTermination::Angle { angle },
                }, ..
            }, ..
        }) if angle.get() == std::f64::consts::TAU)
        );
    }
}

#[test]
fn revolve_axis_controls_require_the_complete_default_pattern() {
    for field in ["entity_flags", "values", "references", "marker", "state"] {
        assert!(
            generated_revolve_with(REVOLVE_EXTENT_FULL, |_, properties, _, _| {
                let mut json = serde_json::to_value(&properties[3]).expect("axis JSON");
                json["kind"][field] = match field {
                    "entity_flags" => serde_json::json!(1),
                    "values" => serde_json::json!([0, 1]),
                    "references" => serde_json::to_value(reference_list(&[4])).expect("list JSON"),
                    "marker" => serde_json::json!(true),
                    "state" => serde_json::json!(2),
                    _ => unreachable!(),
                };
                properties[3] = serde_json::from_value(json).expect("axis");
            })
            .is_none(),
            "axis {field}"
        );
    }
}

#[test]
fn a_full_turn_accepts_both_stored_control_values() {
    let (feature, _) = generated_revolve_with(REVOLVE_EXTENT_FULL, |_, properties, _, _| {
        let position = properties
            .iter()
            .position(|p| p.identity.record_ordinal == 9)
            .expect("control");
        properties[position] = test_property(
            9,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: 3,
                value: 1,
            },
        );
    })
    .expect("candidate")
    .expect("full turn");
    let FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: cadmpeg_ir::features::RevolveConstruction::Resolved { axis, .. },
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("revolve");
    };
    assert_eq!(axis.direction.get(), Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn control_enumerations_keep_all_values_and_require_exact_versioned_frames() {
    for (version, stamps) in [(16, Vec::new()), (23, vec![-1]), (29, vec![-1, 2901])] {
        for value in [0u16, 1, 2, 3, u16::MAX] {
            let mut bytes = content_at(9, version, &stamps);
            bytes.extend_from_slice(&3i16.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
            let parser =
                crate::feature::feature_property_parser(crate::feature::revolve::CONTROL_TYPE)
                    .expect("control parser");
            let property = parse(&bytes, |ctx, view| {
                parser(ctx, view, version).expect("control frame")
            });
            assert_eq!(property.header.stamps, stamps);
            assert!(
                matches!(property.kind, PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary, type_value: 3, value: actual,
            } if actual == value)
            );
            parse(&bytes[..bytes.len() - 1], |ctx, view| {
                assert!(parser(ctx, view, version).is_err());
            });
            bytes.push(0);
            parse(&bytes, |ctx, view| {
                assert!(parser(ctx, view, version).is_err());
            });
        }
    }
}

#[test]
fn reference_qualifiers_do_not_change_dimension_or_profile_joins() {
    let projected = generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, dimensions| {
        let mut json = serde_json::to_value(&source).expect("feature JSON");
        json["properties"]["references"][11]["qualified"] = serde_json::json!(false);
        *source = serde_json::from_value(json).expect("feature frame");
        let mut json = serde_json::to_value(&dimensions[0]).expect("dimension JSON");
        json["parameter"]["qualified"] = serde_json::json!(false);
        dimensions[0] = serde_json::from_value(json).expect("dimension frame");
    });
    assert!(projected.expect("same selected records").is_ok());
}

#[test]
fn slot16_projects_both_values_and_reports_only_the_true_control() {
    let (expected, expected_result) = generated_revolve(REVOLVE_EXTENT_FULL)
        .expect("candidate")
        .expect("revolve");
    for extent in [1, REVOLVE_EXTENT_FULL] {
        for value in [false, true] {
            let mut losses = Vec::new();
            let (projected, result) = generated_revolve_with_losses(
                extent,
                |source, properties, _, _| {
                    properties.push(test_property(
                        90,
                        PmDcFeaturePropertyKind::Boolean {
                            name: String::new(),
                            name_value: 0,
                            value,
                        },
                    ));
                    replace_slot(source, 16, 91);
                    let arena = DecodeArena::new();
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                            .expect("native arena context");
                    let mut ir = cadmpeg_ir::CadIr::empty();
                    let namespace = ir.native.namespace_mut("inventor");
                    namespace
                        .set_arena(&ctx, "pm_dc_feature_properties", properties)
                        .expect("native properties");
                    let retained: Vec<PmDcFeatureProperty> = namespace
                        .arena_as_for_decode(&ctx, "pm_dc_feature_properties")
                        .expect("retained native properties");
                    assert_eq!(retained.len(), properties.len());
                    for property in properties.iter() {
                        assert!(retained.contains(property));
                    }
                    let control = retained
                        .iter()
                        .find(|property| property.identity.record_ordinal == 90)
                        .expect("slot-16 property");
                    assert!(matches!(control.kind,
                        PmDcFeaturePropertyKind::Boolean { value: actual, .. } if actual == value));
                },
                &mut losses,
            )
            .expect("candidate")
            .expect("revolve");
            if extent == REVOLVE_EXTENT_FULL {
                assert_eq!(
                    projected.evaluation.definition(),
                    expected.evaluation.definition()
                );
                assert_eq!(result, expected_result);
            }
            assert_eq!(losses.len(), usize::from(value));
            if value {
                let loss = &losses[0];
                assert_eq!(loss.code.namespace(), "inventor");
                assert_eq!(loss.code.local_code(), "feature.revolve-control-unresolved");
                assert!(loss.message.contains("slot-16"));
                assert!(loss
                    .message
                    .contains("retained natively with unresolved meaning"));
            }
        }
    }
}

#[test]
fn an_unprojected_true_slot16_does_not_report_a_projected_control_loss() {
    let mut losses = Vec::new();
    assert!(generated_revolve_with_losses(
        REVOLVE_EXTENT_FULL,
        |source, _, _, _| {
            replace_slot(source, 16, 13);
            replace_slot(source, 14, 99);
        },
        &mut losses,
    )
    .is_none());
    assert!(losses.is_empty());
}

#[test]
fn slot16_requires_a_boolean_record() {
    assert!(
        generated_revolve_with(REVOLVE_EXTENT_FULL, |source, _, _, _| {
            replace_slot(source, 16, 11);
        })
        .is_none()
    );
}
