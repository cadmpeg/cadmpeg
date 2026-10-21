// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

mod dependencies;
pub(super) mod reduction;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::native::sldprt_native;
use crate::test_support::native::update_sldprt_native;
use crate::test_support::parasolid::entity51;
use crate::test_support::parasolid::owned_triangle;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn semantic_writer_resolves_and_rewrites_owner_qualified_parameters() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="D1">2mm</Dimension></Feature><Feature Name="Sketch2" Type="Sketch" id="11"><Dimension Name="D1">3mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="12"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let sketch1 = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap();
    let sketch1_parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&sketch1.id) && parameter.name == "D1")
        .unwrap()
        .id
        .clone();
    {
        let mut ir_edit = decoded.ir_mut();
        let result = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Result")
            .unwrap();
        assert_eq!(
            result
                .expression
                .dependencies()
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
            vec![sketch1_parameter.clone()]
        );
    }

    decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == sketch1_parameter)
        .unwrap()
        .name = "Width".into();
    let mut renamed = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut renamed,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(
        crate::test_support::history::rendered_parameter(regenerated.ir(), result),
        "Width@Sketch1 * 2"
    );
    assert_eq!(
        result
            .expression
            .dependencies()
            .cloned()
            .collect::<Vec<_>>()
            .len(),
        1
    );
}

#[test]
fn semantic_writer_renders_source_equation_aliases_from_current_parameter_names() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="Width" EquationId="D1">2mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#));
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap(),
    );
    for name in ["D2", "D3"] {
        let id = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name != "Result")
            .unwrap()
            .id
            .clone();
        decoded
            .ir_mut()
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.id == id)
            .unwrap()
            .name = name.into();
        let mut encoded = Vec::new();
        crate::test_support::serialize_edited_history(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut encoded,
        )
        .unwrap();
        decoded = cadmpeg_test_support::EditableDecodeResult::from(
            SldprtCodec
                .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
                .unwrap(),
        );
        let result = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Result")
            .unwrap();
        let target = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        assert_eq!(
            crate::test_support::history::rendered_parameter(decoded.ir(), result),
            format!("{name}@Sketch1 * 2")
        );
        assert_eq!(
            result.expression.dependencies().collect::<Vec<_>>(),
            vec![&target.id]
        );
        assert!(target.properties.get("EquationId").is_none());
        assert!(sldprt_native(decoded.ir())
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .flat_map(|feature| feature.dimension_properties.values())
            .all(|properties| !properties.contains_key("EquationId")));
    }
}

#[test]
fn semantic_writer_rewrites_parameter_owners_when_features_are_renamed() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="Width" EquationId="D1@Sketch1">2mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let sketch = ir_edit
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Sketch1"))
            .unwrap();
        sketch.name = Some("Profile".into());
        ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Width")
            .unwrap()
            .name = "Gauge".into();
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(regenerated
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some("Profile")));
    let gauge = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Gauge")
        .unwrap();
    assert!(!gauge.properties.contains_key("EquationId"));
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(
        crate::test_support::history::rendered_parameter(regenerated.ir(), result),
        "Gauge@Profile * 2"
    );
    assert_eq!(
        result
            .expression
            .dependencies()
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
        vec![gauge.id.clone()]
    );
}

#[test]
fn semantic_writer_preserves_empty_dimensions() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12mm</Dimension><Dimension Name="External" Driven="true"/></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let empty = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "External")
        .unwrap();
    assert_eq!(
        crate::test_support::history::rendered_parameter(decoded.ir(), empty),
        ""
    );
    assert_eq!(empty.value, None);
    {
        let mut ir_edit = decoded.ir_mut();
        let depth = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .unwrap();
        depth.expression = "20mm".into();
        depth.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
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
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["External"], "");
    assert_eq!(feature.dimension_properties["External"]["Driven"], "true");
}

#[test]
fn semantic_writer_preserves_keywords_attributes() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket" Schema="34000" Revision="12"><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
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
    let history = &sldprt_native(regenerated.ir()).feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.properties["Schema"], "34000");
    assert_eq!(history.properties["Revision"], "12");
}

#[test]
fn semantic_writer_preserves_keywords_child_order() {
    use crate::records::HistoryContent;
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="First" Type="Custom" id="1"/>between<Configuration Name="Default" SourceIndex="0"/><Extrusion Name="Boss" Type="BossExtrude" id="2"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let depth = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .unwrap();
        depth.expression = "20mm".into();
        depth.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
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
    let history = &sldprt_native(regenerated.ir()).feature_histories[0];
    assert!(matches!(
        history.content.as_slice(),
        [
            HistoryContent::Feature(_),
            HistoryContent::Text(text),
            HistoryContent::Configuration(_),
            HistoryContent::Feature(_),
        ] if text == "between"
    ));
}

#[test]
fn semantic_writer_applies_history_root_ordinals() {
    use crate::records::HistoryContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="First" Type="Custom" id="1"/><Configuration Name="A" SourceIndex="0"/><Feature Name="Second" Type="Custom" id="2"/><Configuration Name="B" SourceIndex="1"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    for feature in &mut decoded.ir_mut().model.features {
        feature.ordinal = u64::from(feature.name.as_deref() == Some("First"));
    }
    for configuration in &mut decoded.ir_mut().model.configurations {
        configuration.ordinal = u32::from(configuration.name.as_deref() == Some("A"));
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
    let history = &sldprt_native(regenerated.ir()).feature_histories[0];
    let names = history
        .content
        .iter()
        .filter_map(|item| match item {
            HistoryContent::Feature(id) => history
                .features
                .iter()
                .find(|feature| feature.id == *id)
                .map(|feature| feature.name.as_str()),
            HistoryContent::Configuration(id) => history
                .configurations
                .iter()
                .find(|configuration| configuration.id == *id)
                .map(|configuration| configuration.name.as_str()),
            HistoryContent::Text(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Second", "B", "First", "A"]);
}

#[test]
fn semantic_writer_applies_neutral_parameter_order() {
    use crate::records::FeatureContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Ordered" Type="EquationDriven" id="41"><Dimension Name="First">1</Dimension><Child Name="Nested" Type="Folder" id="42"/><Dimension Name="Second">2</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    for parameter in &mut decoded.ir_mut().model.parameters {
        parameter.ordinal = match parameter.name.as_str() {
            "First" => 1,
            "Second" => 0,
            name => panic!("unexpected parameter {name}"),
        };
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "Second"), (1, "First")]
    );
    let content = &sldprt_native(regenerated.ir()).feature_histories[0].features[0].content;
    assert_eq!(
        content
            .iter()
            .filter_map(|item| match item {
                FeatureContent::Dimension(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["Second", "First"]
    );
    assert!(content
        .iter()
        .any(|item| matches!(item, FeatureContent::Feature(_))));
}

#[test]
fn semantic_writer_uses_neutral_parameters_over_native_edits() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equation" Type="EquationDriven" id="41"><Dimension Name="Depth">12mm</Dimension></Feature></Keywords>"#,
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
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .unwrap();
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
        update_sldprt_native(&mut ir_edit, |native| {
            native.feature_histories[0].features[0]
                .parameters
                .insert(cadmpeg_core::nonblank_literal!("Depth"), "30mm".into());
        });
    }

    let mut bytes = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut bytes)
        .expect("neutral parameters override conflicting native fields");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written parameters decode");
    let parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("neutral depth is written");
    assert_eq!(
        crate::test_support::history::rendered_parameter(regenerated.ir(), parameter),
        "20mm"
    );
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(20.0).unwrap()))
    );
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "20mm"
    );
}

#[test]
fn semantic_writer_uses_neutral_dimension_properties_over_native_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equation" Type="EquationDriven" id="41"><Dimension Name="Depth" Driven="false">12mm</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.parameters[0].driving = false;
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .dimension_properties
            .get_mut("Depth")
            .unwrap()
            .insert(cadmpeg_core::nonblank_literal!("Driven"), "native".into());
    });

    let mut bytes = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut bytes)
        .expect("neutral dimension properties override the native projection");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written dimension properties decode");
    assert_eq!(regenerated.ir().model.parameters[0].driving, false);
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].dimension_properties
            ["Depth"]["Driven"],
        "true"
    );
}

#[test]
fn semantic_writer_round_trips_sparse_positional_extrusions() {
    use cadmpeg_ir::{
        features::{
            BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation,
            LinearTermination, ParameterValue,
        },
        scalar::Length,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Boss-Extrude7" id="9"><Dimension Name="D1">200</Dimension></Extrusion>
            <Extrusion Name="Cortar-Extruir2" id="10"><Dimension Name="D1">3</Dimension></Extrusion>
            <Extrusion Name="Custom operation" id="11"><Dimension Name="D1">4</Dimension></Extrusion>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let first_definition = decoded.ir().model.features[0].evaluation.definition();
    assert!(
        matches!(
            first_definition,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    }
                },
                op: BooleanOp::Unresolved,
                ..
            }) if decoded.ir().dimension_value(&actual_length).unwrap().get() == 200.0
        ),
        "{first_definition:?}"
    );
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }) if decoded.ir().dimension_value(&actual_length).unwrap().get() == 3.0
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }) if decoded.ir().dimension_value(&actual_length).unwrap().get() == 4.0
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length::new(200.0).unwrap()))
    );
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Length(Length::new(3.0).unwrap()))
    );
    assert_eq!(
        decoded.ir().model.parameters[2].value,
        Some(ParameterValue::Length(Length::new(4.0).unwrap()))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        for (index, value) in [(0, 250.0), (1, 4.5)] {
            let id = ir_edit.model.features[index].id.clone();
            ir_edit
                .set_dimension(
                    &id,
                    "/extent/side/termination/length",
                    cadmpeg_ir::scalar::NonZeroLength::new(value).unwrap(),
                )
                .unwrap();
        }
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
    let mut regenerated = cadmpeg_test_support::EditableDecodeResult::from(regenerated);
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].parameters["D1"], "250");
    assert_eq!(native[1].parameters["D1"], "4.5");
    for feature in &native[..2] {
        assert!(!feature.parameters.contains_key("Depth"));
        assert!(!feature.properties.contains_key("EndCondition"));
        assert!(!feature.properties.contains_key("Operation"));
        assert!(!feature.properties.contains_key("Profile"));
    }
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }) if regenerated.ir().dimension_value(&actual_length).unwrap().get() == 250.0
    ));
    assert!(matches!(
        regenerated.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }) if regenerated.ir().dimension_value(&actual_length).unwrap().get() == 4.5
    ));
    assert!(matches!(
        regenerated.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }) if regenerated.ir().dimension_value(&actual_length).unwrap().get() == 4.0
    ));

    regenerated.ir_mut().model.parameters[0].expression = "225".into();
    regenerated.ir_mut().model.parameters[0].value =
        Some(ParameterValue::Length(Length::new(225.0).unwrap()));
    let mut parameter_encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        regenerated.ir(),
        regenerated.source_fidelity(),
        &mut parameter_encoded,
    )
    .unwrap();
    let parameter_regenerated = SldprtCodec
        .decode(
            &mut Cursor::new(parameter_encoded),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        sldprt_native(parameter_regenerated.ir()).feature_histories[0].features[0].parameters["D1"],
        "225"
    );
    assert_eq!(
        parameter_regenerated.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length::new(225.0).unwrap()))
    );
}

#[test]
fn semantic_writer_round_trips_feature_output_scope() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(base.ir().model.bodies.len(), 2);
    let scope = base.ir().model.bodies[0].id.as_str().to_owned();
    let mut source = sldprt_with_body(&body);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(
            r#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="{scope}"/></Keywords>"#
        )
        .as_bytes(),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(
        *decoded.ir().model.features[0].evaluation.outputs(),
        vec![decoded.ir().model.bodies[0].id.clone()]
    );
    let output = decoded.ir().model.bodies[1].id.clone();
    decoded.ir_mut().model.features[0]
        .evaluation
        .set_outputs(cadmpeg_ir::features::DistinctMembers::try_from(vec![output]).unwrap());

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
        regenerated.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    assert_eq!(
        regenerated.ir().model.faces.len(),
        decoded.ir().model.faces.len()
    );
    assert_eq!(
        regenerated.ir().model.edges.len(),
        decoded.ir().model.edges.len()
    );
    let points = |ir: &cadmpeg_ir::CadIr| {
        let mut points = ir
            .model
            .points
            .iter()
            .map(|point| {
                let point = point.position().get();
                [point.x, point.y, point.z]
            })
            .collect::<Vec<_>>();
        points.sort_by(|left, right| {
            left[0]
                .total_cmp(&right[0])
                .then(left[1].total_cmp(&right[1]))
                .then(left[2].total_cmp(&right[2]))
        });
        points
    };
    assert_eq!(points(regenerated.ir()), points(decoded.ir()));
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].properties["Scope"],
        regenerated.ir().model.bodies[1].id.as_str()
    );
    assert_eq!(
        *regenerated.ir().model.features[0].evaluation.outputs(),
        vec![regenerated.ir().model.bodies[1].id.clone()]
    );
}

#[test]
fn semantic_writer_round_trips_all_extrusion_forms() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation,
        LinearTermination, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="30"/><Extrusion Name="Blind" Type="BossExtrude" id="31" Profile="30" EndCondition="Blind" Operation="Join"><Dimension Name="Depth">2mm</Dimension></Extrusion><Extrusion Name="Symmetric" Type="BossExtrude" id="32" Profile="30" EndCondition="Symmetric" Direction="0,0,1" Operation="NewBody"><Dimension Name="Depth">4mm</Dimension><Dimension Name="Draft">5deg</Dimension></Extrusion><Extrusion Name="Two" Type="CutExtrude" id="33" Profile="30" EndCondition="TwoSided" Operation="Cut"><Dimension Name="Depth">3mm</Dimension><Dimension Name="Depth2">7mm</Dimension></Extrusion><Extrusion Name="Through" Type="CutExtrude" id="34" Profile="30" EndCondition="ThroughAll" Direction="0,1,0" Operation="Cut"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let profile_feature = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Feature(profile)),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: actual_length },
                    draft: None,
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }) if (profile == &profile_feature) && decoded.ir().dimension_value(&actual_length).unwrap().get() == 2.0
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: geometry_1,
                source: None,
            },
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: actual_length },
                    draft: Some(value),
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        }) if ( ((decoded.ir().dimension_value(value).unwrap().get() - 5f64.to_radians()).abs() < 1.0e-12) && decoded.ir().dimension_value(&actual_length).unwrap().get() == 4.0)
        && matches!(geometry_1.get(), Vector3 { x: 0.0, y: 0.0, z: 1.0 })
    ));
    assert!(matches!(
        decoded.ir().model.features[3].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length_2
                    },
                    ..
                },
            },
            op: BooleanOp::Cut,
            ..
        }) if decoded.ir().dimension_value(&actual_length).unwrap().get() == 3.0 && decoded.ir().dimension_value(&actual_length_2).unwrap().get() == 7.0
    ));
    assert!(matches!(
       decoded.ir().model.features[4].evaluation.definition(),
       FeatureDefinition::Operation(FeatureOperation::Extrude {
           direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
               vector: geometry_1,
               source: None,
           },
           extent: ExtrudeExtent::OneSided {
               side: ExtrudeSide {
                   termination: LinearTermination::ThroughAll {},
                   ..
               }
           },
           ..
       })
    if matches!(geometry_1.get(), Vector3 {
                   x: 0.0,
                   y: 1.0,
                   z: 0.0,
               })));

    let removed_use_parameters = decoded
        .ir()
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&decoded.ir().model.features[3].id))
        .map(|parameter| {
            (
                parameter.id.clone(),
                parameter.name.clone(),
                parameter.value.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(removed_use_parameters.len(), 2);
    {
        let mut ir_edit = decoded.ir_mut();
        let blind = ir_edit.model.features[1].id.clone();
        ir_edit
            .set_dimension(
                &blind,
                "/extent/side/termination/length",
                cadmpeg_ir::scalar::NonZeroLength::new(8.0).unwrap(),
            )
            .unwrap();
        ir_edit.model.features[1].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction,
                extent,
                op,
                ..
            }) = definition
            else {
                panic!("typed extrusion");
            };
            *direction = cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0))
                    .unwrap(),
                source: None,
            };
            let ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length },
                        ..
                    },
            } = extent
            else {
                panic!("original blind extrusion");
            };
            let length = length.clone();
            *extent = ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind { length },
                    draft: Some(cadmpeg_ir::Dimension::Literal(
                        cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap(),
                    )),
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::Dimension::Literal(
                            cadmpeg_ir::scalar::NonZeroLength::new(9.0).unwrap(),
                        ),
                    },
                    draft: None,
                },
            };
            *op = BooleanOp::Intersect;
        });
        ir_edit.model.features[3].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction, extent, ..
            }) = definition
            else {
                panic!("typed extrusion");
            };
            *direction = cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {};
            *extent = ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll {},
                    draft: None,
                },
            };
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[1].properties["EndCondition"], "TwoSided");
    assert_eq!(native[1].properties["Direction"], "1,0,0");
    assert_eq!(native[1].properties["Operation"], "Intersect");
    assert_eq!(native[1].parameters["Depth"], "8mm");
    assert_eq!(native[1].parameters["Depth2"], "9mm");
    assert_eq!(native[1].parameters["Draft"], "0.1rad");
    assert_eq!(native[3].properties["EndCondition"], "ThroughAll");
    // D1: ThroughAll removes both operation uses; independent parameters remain.
    for (id, name, value) in removed_use_parameters {
        assert!(native[3].parameters.contains_key(name.as_str()));
        let parameter = regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("same independent extrusion parameter");
        assert_eq!(parameter.name, name);
        assert_eq!(parameter.value, value);
    }
    assert!(matches!(
        regenerated.ir().model.features[3].definition().operation(),
        FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll {},
                    ..
                }
            },
            ..
        }
    ));
    assert!(!native[3].properties.contains_key("Direction"));
}

#[test]
fn semantic_writer_round_trips_extrusion_to_face() {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, FeatureOperation,
        LinearTermination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="30"/><Extrusion Name="UpTo" Type="BossExtrude" id="31" Profile="30" EndCondition="ToFace" Face="face:12" Operation="Join"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[1].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) =
            &mut updated_ir_edit_definition
        else {
            panic!("typed extrusion");
        };
        assert_eq!(
            extent,
            &ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native("face:12".into()),
                        offset: None,
                    },
                    draft: None,
                }
            }
        );
        *extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToFace {
                    face: FaceSelection::Native("face:13".into()),
                    offset: None,
                },
                draft: None,
            },
        };
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[1];
    assert_eq!(native.properties["EndCondition"], "ToFace");
    assert_eq!(native.properties["Face"], "face:13");
    assert!(matches!(
        regenerated.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native(face),
                        ..
                    },
                    ..
                }
            },
            ..
        }) if face == "face:13"
    ));
}

#[test]
fn semantic_writer_retains_unresolved_native_edge_treatments() {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        FeatureDefinition, FeatureOperation,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Round" Type="Custom" id="10" Edges="edge:1"><Dimension Name="Radius">NaNmm</Dimension></Feature><Feature Name="Bevel" Type="Custom" id="11" Edges="edge:2"><Dimension Name="Distance">NaNmm</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "Round", 10),
            ("Chamfer_c", "Bevel", 11),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::FilletGroup {
            radius: RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant) },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            ..
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::ChamferGroup {
            spec: ChamferSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::ChamferForm::Distance) },
            ..
        }])
    ));

    for (index, reason) in [
        (0, "unresolved fillet radius law"),
        (1, "unresolved chamfer dimensions"),
    ] {
        let mut detached = decoded.ir().clone();
        detached.model.features[index].native_ref = None;
        assert_solved_feature_reduction(&detached, decoded.source_fidelity(), index, reason);
    }

    decoded.ir_mut().model.features[0].name = Some("Renamed round".into());
    decoded.ir_mut().model.features[1].name = Some("Renamed bevel".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].parameters["Radius"], "NaNmm");
    assert_eq!(native[1].parameters["Distance"], "NaNmm");
    assert_eq!(native[0].properties["Edges"], "edge:1");
    assert_eq!(native[1].properties["Edges"], "edge:2");
}

#[test]
fn semantic_writer_round_trips_typed_fillet_radius() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round" Type="Fillet" id="10" Edges="edge:1,edge:2"><Dimension Name="Radius">2mm</Dimension></Fillet></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                radius: actual_radius,
            },
            ..
        }] if (selection == "edge:1,edge:2") && decoded.ir().dimension_value(&actual_radius).unwrap().get() == 2.0)
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let id = ir_edit.model.features[0].id.clone();
        ir_edit
            .set_dimension(
                &id,
                "/groups/0/radius/radius",
                cadmpeg_ir::scalar::PositiveLength::new(3.5).unwrap(),
            )
            .unwrap();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Fillet { groups },
            ) = definition
            else {
                panic!("typed fillet feature");
            };
            groups[0].edges = cadmpeg_ir::features::EdgeSelection::Native("edge:3".into());
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
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Radius"],
        "3.5mm"
    );
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].properties["Edges"],
        "edge:3"
    );
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::FilletGroup {
            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                radius: actual_radius,
            },
            ..
        }] if regenerated.ir().dimension_value(&actual_radius).unwrap().get() == 3.5)
    ));
}

#[test]
fn semantic_writer_round_trips_positional_fillet_and_localized_chamfer_dimensions() {
    use cadmpeg_ir::{
        features::{
            edge_treatments::{ChamferSpec, RadiusSpec},
            EdgeSelection, FeatureDefinition, FeatureOperation, ParameterValue,
        },
        scalar::Length,
    };

    let keywords = format!(
        r#"<Keywords>
            <Feature Name="Round" Type="Fillet" id="10"><Dimension Name="D1">R1</Dimension></Feature>
            <Feature Name="Bevel" Type="Chafl{acute}n" id="11"><Dimension Name="D1">0.3</Dimension><Dimension Name="D2">45.00{degree}</Dimension></Feature>
        </Keywords>"#,
        acute = '\u{00e1}',
        degree = '\u{00b0}',
    );
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "Round", 10),
            ("Chamfer_c", "Bevel", 11),
        ]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Constant {
                radius: actual_radius
            }, ..
        }] if decoded.ir().dimension_value(&actual_radius).unwrap().get() == 1.0)
    ));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            ..
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::ChamferGroup {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::DistanceAngle {
                distance: actual_distance,
                angle,
            },
        }] if ((decoded.ir().dimension_value(&angle).unwrap().get() - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12) && decoded.ir().dimension_value(&actual_distance).unwrap().get() == 0.3)
    ));
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Length(Length::new(0.3).unwrap()))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let fillet = ir_edit.model.features[0].id.clone();
        let chamfer = ir_edit.model.features[1].id.clone();
        ir_edit
            .set_dimension(
                &fillet,
                "/groups/0/radius/radius",
                cadmpeg_ir::scalar::PositiveLength::new(2.5).unwrap(),
            )
            .unwrap();
        ir_edit
            .set_dimension(
                &chamfer,
                "/groups/0/spec/distance",
                cadmpeg_ir::scalar::PositiveLength::new(0.6).unwrap(),
            )
            .unwrap();
        ir_edit
            .set_dimension(
                &chamfer,
                "/groups/0/spec/angle",
                cadmpeg_ir::scalar::InteriorAngle::new(30.0_f64.to_radians()).unwrap(),
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].parameters["D1"], "R2.5");
    assert!(!native[0].parameters.contains_key("Radius"));
    assert_eq!(native[1].kind, format!("Chafl{}n", '\u{00e1}'));
    assert_eq!(native[1].parameters["D1"], "0.6");
    assert_eq!(native[1].parameters["D2"], format!("30{}", '\u{00b0}'));
    assert!(!native[1].parameters.contains_key("Distance"));
    assert!(!native[1].parameters.contains_key("Angle"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: actual_radius
            },
            ..
        }] if regenerated.ir().dimension_value(&actual_radius).unwrap().get() == 2.5)
    ));
    assert!(matches!(
        regenerated.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            ..
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::ChamferGroup {
            spec: ChamferSpec::DistanceAngle {
                distance: actual_distance,
                angle,
            },
            ..
        }] if ((regenerated.ir().dimension_value(&angle).unwrap().get() - 30.0_f64.to_radians()).abs() < 1.0e-12) && regenerated.ir().dimension_value(&actual_distance).unwrap().get() == 0.6)
    ));
}

#[test]
fn semantic_writer_round_trips_variable_radius_fillet() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::{
        features::{
            edge_treatments::{RadiusSpec, VariableRadius},
            EdgeSelection, FeatureDefinition, FeatureOperation,
        },
        scalar::Length,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Blend" Type="Fillet" id="61"><Dimension Name="Position0">0</Dimension><Dimension Name="Radius0">2mm</Dimension><Dimension Name="Position1">0.5</Dimension><Dimension Name="Radius1">4mm</Dimension><Dimension Name="Position2">1</Dimension><Dimension Name="Radius2">3mm</Dimension></Fillet></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Variable { points }, ..
        }] if points.as_slice().iter().map(|point| {
            let point = point.to_raw();
            (point.parameter, decoded.ir().dimension_value(&point.radius).unwrap().get())
        }).collect::<Vec<_>>() == [(0.0, 2.0), (0.5, 4.0), (1.0, 3.0)])
    ));
    {
        let mut ir_edit = decoded.ir_mut();
        let id = ir_edit.model.features[0].id.clone();
        ir_edit
            .set_dimension(
                &id,
                "/groups/0/radius/points/1/radius",
                Length::new(5.0).unwrap(),
            )
            .unwrap();
        let position = ir_edit
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Position1")
            .unwrap()
            .id
            .clone();
        let value = cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(0.4).unwrap(),
        );
        ir_edit
            .set_parameter_expression(&position, value.literal_expression(), Some(value))
            .unwrap();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[0].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
            &mut updated_ir_edit_definition
        else {
            panic!("variable fillet");
        };
        let RadiusSpec::Variable { points } = &mut groups[0].radius else {
            panic!("variable fillet radius")
        };
        let mut samples = points
            .as_slice()
            .iter()
            .map(VariableRadius::to_raw)
            .collect::<Vec<_>>();
        samples[1].parameter = 0.4;
        *points = cadmpeg_ir::features::edge_treatments::VariableRadii::new(samples).unwrap();
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
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
    let mut regenerated = cadmpeg_test_support::EditableDecodeResult::from(regenerated);
    let native = sldprt_native(regenerated.ir());
    assert_eq!(
        native.feature_histories[0].features[0].parameters["Position1"],
        "0.4"
    );
    assert_eq!(
        native.feature_histories[0].features[0].parameters["Radius1"],
        "5mm"
    );

    {
        let mut ir_edit = regenerated.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Fillet { groups, .. }) = definition
            else {
                panic!("variable fillet after regeneration");
            };
            groups[0].radius = RadiusSpec::Constant {
                radius: cadmpeg_ir::Dimension::Literal(
                    cadmpeg_ir::scalar::PositiveLength::new(6.0).unwrap(),
                ),
            };
        });
    }
    // D1/S-entity: the original indexed radius controls remain independent.
    // Their native names would reactivate a variable law in this operation.
    reduction::assert_independent_parameter_reduction(
        regenerated.ir(),
        regenerated.source_fidelity(),
        0,
        "variable fillet radius law",
    );
    let mut positive = reduction::source_less_operation(
        regenerated.ir().model.features[0].definition().clone(),
        "Constant radius",
    );
    // A fresh supported fillet needs an actual held edge. The source-derived
    // control above retains its original unresolved selection unchanged.
    let edge = positive.model.edges[0].id.clone();
    positive.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
            panic!("fresh constant fillet");
        };
        groups[0].edges = EdgeSelection::Edges(vec![edge.clone()]);
    });
    let mut encoded = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&positive, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut encoded)
        .unwrap();
    let final_ir = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameters = &sldprt_native(final_ir.ir()).feature_histories[0].features[0].parameters;
    assert_eq!(parameters["Radius"], "6mm");
    let written_edge = &final_ir.ir().model.edges[0].id;
    assert_eq!(
        sldprt_native(final_ir.ir()).feature_histories[0].features[0].properties["Edges"],
        written_edge.as_str()
    );
    assert!(matches!(
        final_ir.ir().model.features[0].definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(&groups[0].radius, RadiusSpec::Constant { radius }
                if final_ir.ir().dimension_value(radius).unwrap().get() == 6.0)
    ));
    assert!(!parameters
        .keys()
        .any(|name| name.as_str().starts_with("Position")));
    assert!(!parameters.keys().any(|name| name == "Radius0"));
}

#[test]
fn semantic_writer_round_trips_all_typed_chamfer_forms() {
    use cadmpeg_ir::features::{
        edge_treatments::ChamferSpec, EdgeSelection, FeatureDefinition, FeatureOperation,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Chamfer Name="Equal" Type="Chamfer" id="11" Edges="edge:1"><Dimension Name="Distance">2mm</Dimension></Chamfer>
            <Chamfer Name="Unequal" Type="Chamfer" id="12" Edges="edge:2"><Dimension Name="Distance1">3mm</Dimension><Dimension Name="Distance2">0.25in</Dimension></Chamfer>
            <Chamfer Name="Angled" Type="Chamfer" id="13" Edges="edge:3"><Dimension Name="Distance">4mm</Dimension><Dimension Name="Angle">45deg</Dimension></Chamfer>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            ..
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::ChamferGroup {
            edges: EdgeSelection::Native(edges),
            spec: ChamferSpec::Distance {
                distance: actual_distance,
            },
            ..
        }] if (edges == "edge:1") && decoded.ir().dimension_value(&actual_distance).unwrap().get() == 2.0)
    ));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            ..
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::ChamferGroup {
            spec: ChamferSpec::TwoDistances {
                first: actual_first,
                second: actual_second,
            },
            ..
        }] if decoded.ir().dimension_value(&actual_first).unwrap().get() == 3.0 && decoded.ir().dimension_value(&actual_second).unwrap().get() == 6.35)
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            ..
        }) if matches!(groups.as_slice(), [cadmpeg_ir::features::edge_treatments::ChamferGroup {
            spec: ChamferSpec::DistanceAngle {
                distance: actual_distance,
                angle,
            },
            ..
        }] if ((decoded.ir().dimension_value(&angle).unwrap().get() - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12) && decoded.ir().dimension_value(&actual_distance).unwrap().get() == 4.0)
    ));

    {
        let mut ir = decoded.ir_mut();
        for (index, path, value) in [
            (0, "/groups/0/spec/distance", 2.5),
            (1, "/groups/0/spec/first", 3.5),
            (1, "/groups/0/spec/second", 7.0),
            (2, "/groups/0/spec/distance", 4.5),
        ] {
            let id = ir.model.features[index].id.clone();
            ir.set_dimension(
                &id,
                path,
                cadmpeg_ir::scalar::PositiveLength::new(value).unwrap(),
            )
            .unwrap();
        }
        let id = ir.model.features[2].id.clone();
        ir.set_dimension(
            &id,
            "/groups/0/spec/angle",
            cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_6).unwrap(),
        )
        .unwrap();
        for (index, feature) in ir.model.features.iter_mut().enumerate() {
            feature.evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
                    definition
                else {
                    panic!("typed chamfer feature");
                };
                groups[0].edges = EdgeSelection::Native(format!("edge:{}", index + 4));
            });
        }
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
    let features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(features[0].parameters["Distance"], "2.5mm");
    assert_eq!(features[0].properties["Edges"], "edge:4");
    assert_eq!(features[1].properties["Edges"], "edge:5");
    assert_eq!(features[2].properties["Edges"], "edge:6");
    assert_eq!(features[1].parameters["Distance1"], "3.5mm");
    assert_eq!(features[1].parameters["Distance2"], "7mm");
    assert_eq!(
        features[2].parameters["Angle"],
        format!("{}rad", std::f64::consts::FRAC_PI_6)
    );
}

#[test]
fn semantic_writer_retains_partial_native_wall_operations() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Shell Name="Unknown shell" Type="Shell" id="14" RemovedFaces="face:1"><Dimension Name="Thickness">NaNmm</Dimension></Shell><Thicken Name="Unknown thicken" Type="Thicken" id="15" Faces="face:2" BothSides="invalid"><Dimension Name="Thickness">NaNmm</Dimension></Thicken></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Shell {
            removed_faces: FaceSelection::Native(faces),
            thickness: None,
            outward: None,
            ..
        }) if faces == "face:1"
    ));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: FaceSelection::Native(faces),
            thickness: None,
            side: None,
        }) if faces == "face:2"
    ));

    for (index, reason) in [
        (0, "unresolved shell construction"),
        (1, "unresolved thicken construction"),
    ] {
        let mut detached = decoded.ir().clone();
        detached.model.features[index].native_ref = None;
        assert_solved_feature_reduction(&detached, decoded.source_fidelity(), index, reason);
    }

    decoded.ir_mut().model.features[0].name = Some("Renamed shell".into());
    decoded.ir_mut().model.features[1].name = Some("Renamed thicken".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].parameters["Thickness"], "NaNmm");
    assert!(!native[0].properties.contains_key("Outward"));
    assert_eq!(native[1].parameters["Thickness"], "NaNmm");
    assert_eq!(native[1].properties["BothSides"], "invalid");
}

/// An unsupported operation retains its solved result and names the export loss.
pub(super) fn assert_solved_feature_reduction(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
    feature_index: usize,
    reason: &str,
) {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::report::{export::WritePath, Severity};

    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)
        .expect("an unsupported operation selects synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "writer.design-reduced"
            && loss.severity == Severity::Error
            && loss.message.contains(reason)
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the held solved carrier");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the degraded part decodes");
    assert_eq!(regenerated.ir().model.bodies.len(), ir.model.bodies.len());
    assert_eq!(
        regenerated.ir().model.features.len(),
        ir.model.features.len()
    );
    assert!(matches!(
        regenerated.ir().model.features[feature_index]
            .evaluation
            .definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    ));
}
