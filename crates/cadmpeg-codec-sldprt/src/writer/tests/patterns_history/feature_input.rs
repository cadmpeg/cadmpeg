// SPDX-License-Identifier: Apache-2.0
//! Feature history edits and feature-input lane output.

use super::*;

#[test]
fn semantic_writer_preserves_parametric_history() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "15mm".into());
    });

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    let native = sldprt_native(regenerated.ir());
    let history = &native.feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.configurations[0].name, "Default");
    assert_eq!(history.configurations[0].material.as_deref(), Some("Steel"));
    assert_eq!(history.features.len(), 2);
    assert_eq!(history.features[0].kind, "BossExtrude");
    assert_eq!(history.features[0].parameters["Depth"], "12.5mm");
    assert_eq!(
        history.features[1].parent_source_id(),
        crate::records::FeatureSource::from_value(7)
    );
}

#[test]
fn semantic_writer_applies_neutral_feature_edits() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let moved = ir_edit.model.points[0].position().get();
        ir_edit.model.points[0].set_position(
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                moved.x,
                moved.y,
                moved.z + 1.0,
            ))
            .expect("a finite position is a point"),
        );
        let owner = ir_edit.model.features[0].id.clone();
        ir_edit
            .set_dimension(
                &owner,
                "/extent/side/termination/length",
                cadmpeg_ir::scalar::NonZeroLength::new(18.0).unwrap(),
            )
            .unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "18mm"
    );
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: actual_length,
                    },
                    ..
                }
            },
            ..
        }) if regenerated.ir().dimension_value(actual_length).unwrap().get() == 18.0
    ));
}

#[test]
fn semantic_writer_uses_consistent_neutral_features_over_native_edits() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let depth = ir_edit
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Depth")
            .expect("owned depth parameter")
            .id
            .clone();
        ir_edit
            .set_parameter_expression(
                &depth,
                "18mm".into(),
                Some(cadmpeg_ir::features::ParameterValue::Length(
                    cadmpeg_ir::scalar::Length::new(18.0).unwrap(),
                )),
            )
            .unwrap();
        update_sldprt_native(&mut ir_edit, |native| {
            native.feature_histories[0].features[0]
                .parameters
                .insert(cadmpeg_core::nonblank_literal!("Depth"), "20mm".into());
        });
    }

    let mut bytes = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut bytes)
        .expect("consistent neutral feature fields override native edits");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the neutral feature decodes");
    let depth = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth is written");
    assert_eq!(depth.expression, "18mm".into());
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind { length }, ..
                }
            }, ..
        }) if regenerated.ir().dimension_value(length).unwrap().get() == 18.0
    ));
}

#[test]
fn semantic_writer_accepts_matching_resolved_feature_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Extrude { extent, .. },
            ) = definition
            else {
                panic!("typed extrusion feature");
            };
            *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::Dimension::Literal(
                            cadmpeg_ir::scalar::NonZeroLength::new(50.0).unwrap(),
                        ),
                    },
                    draft: None,
                },
            };
        });
        update_sldprt_native(&mut ir_edit, |native| {
            native.feature_histories[0].part_name = Some("Edited".into());
            let scalar = &mut native.feature_input_lanes[0].scalars[0];
            scalar.value = cadmpeg_ir::scalar::FiniteReal::new(0.05).expect("finite test scalar");
            let offset = usize::try_from(scalar.offset.expect("source scalar offset")).unwrap();
            native.feature_input_lanes[0].native_payload[offset..offset + 8]
                .copy_from_slice(&0.05f64.to_le_bytes());
        });
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: actual_length,
                    },
                    ..
                }
            },
            ..
        }) if regenerated.ir().dimension_value(actual_length).unwrap().get() == 50.0
    ));
}

#[test]
fn semantic_writer_omits_marker_framing_without_typed_operands() {
    use crate::records::{FeatureInputClassRole, SketchInputKind};

    assert_eq!(
        serde_json::from_str::<SketchInputKind>(r#""curve""#).unwrap(),
        SketchInputKind::LineOrCircle
    );
    assert_eq!(
        serde_json::to_string(&SketchInputKind::LineOrCircle).unwrap(),
        r#""line_or_circle""#
    );

    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1, 2, 3, 9]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_input_lanes.len(), 1);
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.configuration.as_deref(), Some("0"));
    assert_eq!(
        lane.classes
            .iter()
            .map(|class| class.name.as_str())
            .collect::<Vec<_>>(),
        [
            "sgPointHandle",
            "sgLineHandle",
            "sgArcHandle",
            "sgPntPntDist"
        ]
    );
    assert!(lane.classes[..3]
        .iter()
        .all(|class| class.role() == FeatureInputClassRole::SketchEntity));
    assert_eq!(
        lane.classes[3].role(),
        FeatureInputClassRole::SketchConstraint
    );
    assert_eq!(
        lane.names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Sketch1", "Boss-Extrude1", "D1"]
    );
    assert_eq!(lane.scalars.len(), 1);
    assert_eq!(lane.scalars[0].name, lane.names[2].id);
    assert_eq!(lane.scalars[0].value.get(), 0.025);
    assert_eq!(lane.scalars[0].object_id, 1);
    assert_eq!(lane.scalars[0].entity_indices(), [0, 2]);
    assert_eq!(lane.references.len(), 2);
    assert_eq!(lane.references[0].object_index, 0);
    assert_eq!(lane.references[1].object_index, 2);
    assert!(lane
        .references
        .iter()
        .all(|reference| reference.kind == crate::records::FeatureInputOperandKind::D6));
    assert_eq!(lane.scalars[0].operands.len(), 2);
    assert_eq!(lane.scalars[0].operands[0].entity_index, 0);
    assert_eq!(lane.scalars[0].operands[1].entity_index, 2);
    assert_eq!(
        lane.scalars[0].operands[0].reference_ref,
        lane.references[0].id
    );
    assert_eq!(
        lane.scalars[0].operands[1].reference_ref,
        lane.references[1].id
    );
    assert!(lane.scalars[0]
        .operands
        .iter()
        .all(|operand| operand.kind == crate::records::FeatureInputOperandKind::D6));
    assert_eq!(lane.relation_bindings.len(), 1);
    assert_eq!(
        lane.relation_bindings[0].family,
        crate::records::FeatureInputRelationFamily::PointPointDistance
    );
    assert_eq!(lane.relation_bindings[0].class_ref, lane.classes[3].id);
    assert_eq!(lane.relation_bindings[0].scalar_ref, lane.scalars[0].id);
    assert_eq!(lane.relation_bindings[0].feature_ref, None);
    assert_eq!(
        lane.scalars[0].role,
        crate::records::FeatureInputScalarRole::Driving
    );
    assert!(
        lane.classes
            .iter()
            .enumerate()
            .all(|(ordinal, class)| class.ordinal
                == u32::try_from(ordinal).expect("ordinal fits u32"))
    );
    assert!(lane
        .sketch_entities
        .windows(2)
        .all(|entities| entities[0].offset() < entities[1].offset()));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.ordinal()
            == u32::try_from(ordinal).expect("ordinal fits u32")));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.local_id()
            == Some(u32::try_from(ordinal).expect("ordinal fits u32") + 1)));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(
            |(ordinal, entity)| entity.state_value.map(cadmpeg_ir::scalar::FiniteReal::get)
                == Some(
                    cadmpeg_core::convert::f64_from_index(ordinal).expect("ordinal is exact") + 1.0
                )
        ));
    let by_ordinal = |ordinal| {
        lane.sketch_entities
            .iter()
            .find(|entity| entity.ordinal() == ordinal)
            .unwrap()
    };
    assert_eq!(by_ordinal(0).kind(), SketchInputKind::Point);
    assert_eq!(
        by_ordinal(1).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
    );
    assert_eq!(
        by_ordinal(2).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Angle)
    );
    assert_eq!(
        by_ordinal(3).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Radius)
    );
    assert_eq!(
        by_ordinal(4).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Coincident)
    );
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let entity = native.feature_input_lanes[0]
            .sketch_entities
            .iter_mut()
            .find(|entity| entity.ordinal() == 1)
            .unwrap();
        entity.reclassify(SketchInputKind::from_native_code(5));
        entity.state_value = cadmpeg_ir::scalar::FiniteReal::new(12.5);
    });

    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
    assert!(matches!(plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason })
            if reason.contains("reconstructible typed payload")));
    let mut encoded = Vec::new();
    let report = plan.write_to(&mut encoded).unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    crate::test_support::native::assert_marker_framing_omission(
        &report,
        &sldprt_native(decoded.ir()),
        &sldprt_native(regenerated.ir()),
    );
    assert_eq!(
        regenerated.ir().model.features.len(),
        decoded.ir().model.features.len()
    );
    let synthesized =
        crate::test_support::assert_source_free_synthesis_agrees(decoded.ir(), regenerated.ir());
    crate::test_support::native::assert_marker_framing_omission(
        &report,
        &sldprt_native(decoded.ir()),
        &sldprt_native(synthesized.ir()),
    );
}

#[test]
fn semantic_writer_rejects_edited_feature_input_class_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].classes[0].name = "sgOtherHandle".into();
    });
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .unwrap()
    .iter()
    .any(|finding| finding.message.contains("class index does not match")));

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("class index does not match"));
}

#[test]
fn semantic_writer_rewrites_feature_input_name_values() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let scalars = lane.scalars.len();
    let rename = crate::history::write::features::FeatureInputRename {
        lane: lane.id.clone(),
        name_index: 1,
        value: cadmpeg_core::nonblank_literal!("Depth"),
    };

    let payload = crate::writer::resolved_feature_payload(
        lane,
        &native.feature_histories,
        std::slice::from_ref(&rename),
    )
    .unwrap();

    let written = decoded_object_names(&payload, &lane.id);
    assert_eq!(written[1].value, "Depth");
    assert_eq!(written[0].value, lane.names[0].value);
    assert_eq!(written[2].value, lane.names[2].value);
    assert_eq!(
        crate::resolved_features::scalars::named_scalars(&payload, &lane.id, &written)
            .unwrap()
            .len(),
        scalars
    );
}

#[test]
fn semantic_writer_refuses_a_feature_input_name_value_that_disagrees_with_its_payload() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let mut lane = native.feature_input_lanes[0].clone();
    lane.names[1].value = "Depth".into();

    let error =
        crate::writer::resolved_feature_payload(&lane, &native.feature_histories, &[]).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("name value does not match its native payload"),
        "{error}"
    );
}

#[test]
fn semantic_writer_rejects_edited_feature_input_scalar_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].scalars[0].value =
            cadmpeg_ir::scalar::FiniteReal::new(0.050).expect("finite test scalar");
    });
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .unwrap()
    .iter()
    .any(|finding| finding.message.contains("scalar index does not match")));

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("scalar index does not match"));
}

#[test]
fn semantic_writer_updates_linked_resolved_feature_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D1")
            .expect("projected D1 parameter")
            .id
            .clone();
        ir_edit
            .set_parameter_expression(
                &parameter,
                "50mm".into(),
                Some(cadmpeg_ir::features::ParameterValue::Length(
                    cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
                )),
            )
            .unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("regenerated D1 parameter");
    // D5 preserves the original authored token, distinct from the API's
    // canonical literal spelling. Both denote the exact same typed cache.
    assert_eq!(parameter.expression, "50mm".into());
    let expected_value = cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
    );
    assert_eq!(parameter.value, Some(expected_value.clone()));
    assert_eq!(expected_value.literal_expression(), "50 mm".into());
    assert_eq!(
        parameter.expression.literal_value_as(&expected_value),
        Some(expected_value)
    );
    // W2: retain the typed native copy, regenerated from the canonical 50 mm
    // parameter. Its semantic scalar link identifies that copy, not authority.
    let scalar_id = parameter
        .native_ref
        .as_deref()
        .expect("native scalar counterpart");
    let native = sldprt_native(regenerated.ir());
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| lane.scalars.iter().any(|scalar| scalar.id == scalar_id))
        .expect("linked native scalar lane");
    let scalar = lane
        .scalars
        .iter()
        .find(|scalar| scalar.id == scalar_id)
        .unwrap();
    assert_eq!(scalar.value.get(), 0.05);
    let name = lane
        .names
        .iter()
        .find(|name| name.id == scalar.name)
        .unwrap();
    assert_eq!(name.value, "D1");
    assert_eq!(
        scalar.feature_ref.as_deref(),
        regenerated.ir().model.features[0].native_ref.as_deref()
    );
}

#[test]
fn semantic_writer_updates_resolved_scalar_from_feature_edit() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let owner = ir_edit.model.features[0].id.clone();
        ir_edit
            .set_dimension(
                &owner,
                "/extent/side/termination/length",
                cadmpeg_ir::scalar::NonZeroLength::new(50.0).unwrap(),
            )
            .unwrap();
    }

    let mut encoded = Vec::new();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    let report = plan.report().clone();
    plan.write_to(&mut encoded).unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: actual_length,
                    },
                    ..
                }
            },
            ..
        }) if regenerated.ir().dimension_value(actual_length).unwrap().get() == 50.0
    ));
    let native = sldprt_native(regenerated.ir());
    // P rebuilds lanes by their current typed owners. The independent source
    // framing remains first; the operation's scalar belongs to its owner lane.
    let framing = &native.feature_input_lanes[0];
    assert_eq!(framing.configuration.as_deref(), Some("0"));
    assert_eq!(
        framing
            .classes
            .iter()
            .map(|class| class.name.as_str())
            .collect::<Vec<_>>(),
        ["sgPointHandle", "sgLineHandle", "sgArcHandle"]
    );
    assert!(framing
        .classes
        .iter()
        .all(|class| class.feature_ref.is_none()));
    assert_eq!(framing.names.len(), 1);
    assert_eq!(framing.names[0].value, "Sketch1");
    assert_eq!(framing.names[0].feature_ref, None);
    assert!(framing.scalars.is_empty());

    let feature = &regenerated.ir().model.features[0];
    let owner = feature
        .native_ref
        .as_deref()
        .expect("native extrusion owner");
    let matching = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            lane.scalars.iter().filter(move |scalar| {
                scalar.feature_ref.as_deref() == Some(owner)
                    && lane.names.iter().any(|name| {
                        name.id == scalar.name
                            && name.value == "D1"
                            && name.feature_ref.as_deref() == Some(owner)
                    })
            })
        })
        .collect::<Vec<_>>();
    let [scalar] = matching.as_slice() else {
        panic!("one generated scalar for the extrusion's D1; report={report:?}; native={native:?}");
    };
    assert_eq!(scalar.value.get(), 0.05);
    let parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("canonical extrusion parameter");
    assert_eq!(parameter.native_ref.as_deref(), Some(scalar.id.as_str()));
}

#[test]
fn semantic_writer_types_resolved_relation_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Sketch1", "D1"]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.sketch_entities.len(), 1);
    assert_eq!(lane.sketch_entities[0].ordinal(), 0);
    let relation = &lane.relation_instances[0];
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::PointPointDistance
    );
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_index)
            .collect::<Vec<_>>(),
        [0, 2]
    );
    // Operand 2 has no authored marker. This source relation does not create a
    // typed length use that could prohibit changing the independent parameter.
    // A Native constraint can retain a generic parameter ID without imposing Q.
    assert!(
        decoded
            .ir()
            .model
            .sketch_constraints
            .iter()
            .all(|constraint| {
                matches!(
                    constraint.definition.kind(),
                    cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { .. }
                )
            }),
        "constraints={:#?}",
        decoded.ir().model.sketch_constraints
    );
    let scalar_id = relation.parameter_scalar_ref().unwrap().to_owned();
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .expect("projected D1 parameter");
        parameter.expression = "0.5".into();
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
        ));
        let id = parameter.id.clone();
        let value = parameter.value.clone().unwrap();
        for configuration in &mut ir_edit.model.configurations {
            if let Some(current) = configuration.parameter_values.get_mut(&id) {
                *current = value.clone();
            }
        }
    }

    let mut encoded = Vec::new();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(
        plan.report().losses.iter().any(|loss| {
            loss.code == crate::loss::SldprtLossCode::WriterNativeRecordsOmitted.kind()
            && loss.message.contains(&scalar_id)
            && loss.message.contains(
                "canonical parameter quantity does not match the retained native scalar quantity",
            )
        }),
        "scalar={scalar_id}; report={:#?}; source lane={lane:#?}",
        plan.report()
    );
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "sldprt",
            "feature_input_scalars",
            &scalar_id,
        ),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    plan.write_to(&mut encoded).unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("regenerated D1 parameter");
    assert_eq!(parameter.expression, "0.5".into());
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap()
        ))
    );
    assert!(parameter.native_ref.is_none());
}

#[test]
fn complete_typed_marker_content_keeps_the_patch_payload_path() {
    let native = crate::test_support::native::complete_marker_source();
    let [lane] = native.feature_input_lanes.as_slice() else {
        panic!("one complete source lane");
    };
    crate::resolved_features::native_lane_write::require_reconstructible_content(
        lane,
        &native.feature_histories,
    )
    .expect("all actual marker bodies and scalar operands are representable");
    let payload = crate::writer::resolved_feature_payload(lane, &native.feature_histories, &[])
        .expect("the supported actual typed-marker patch path remains available");
    assert_eq!(payload, lane.native_payload.to_vec());
    assert_eq!(lane.scalars[0].value.get(), 0.025);
    assert_eq!(lane.scalars[0].entity_indices(), [0, 2]);
    assert_eq!(
        lane.scalars[0].operands[0].entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id())
    );
    assert_eq!(
        lane.scalars[0].operands[1].entity_ref.as_deref(),
        Some(lane.sketch_entities[2].id())
    );
}
