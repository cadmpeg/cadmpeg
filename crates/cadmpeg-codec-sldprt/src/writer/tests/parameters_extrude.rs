// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_projects_and_validates_parameter_dependencies() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Base" EquationId="D1@Equations">2mm</Dimension><Dimension Name="Wall Thickness">4mm</Dimension><Dimension Name="Datum &quot;A&quot;">1mm</Dimension><Dimension Name="Driven" EquationId="D2@Equations">&quot;Wall Thickness&quot; + &quot;Datum &quot;&quot;A&quot;&quot;&quot; + D1@Equations + &quot;Wall Thickness&quot;</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(decoded.ir().model.parameters.len(), 4);
    assert_eq!(
        decoded.ir().model.parameters[3].dependencies,
        vec![
            decoded.ir().model.parameters[1].id.clone(),
            decoded.ir().model.parameters[2].id.clone(),
            decoded.ir().model.parameters[0].id.clone(),
        ]
    );

    decoded.ir_mut().model.parameters[0]
        .properties
        .insert("EquationId".into(), "D1@Renamed".into());
    decoded.ir_mut().model.parameters[1].name = "Wall Gauge".into();
    let mut renamed = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut renamed,
    )
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(
        decoded.ir().model.parameters[3].expression,
        "\"Wall Gauge\" + \"Datum \"\"A\"\"\" + D1@Renamed + \"Wall Gauge\""
    );
    assert_eq!(
        decoded.ir().model.parameters[3].dependencies,
        vec![
            decoded.ir().model.parameters[1].id.clone(),
            decoded.ir().model.parameters[2].id.clone(),
            decoded.ir().model.parameters[0].id.clone(),
        ]
    );

    decoded.ir_mut().model.parameters[3].expression = "6mm".into();
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("dependencies are inconsistent with their expressions"));
}

#[test]
fn semantic_writer_orders_forward_parameter_dependencies_before_consumers() {
    use crate::records::FeatureContent;
    use cadmpeg_ir::features::ParameterValue;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Result">Input + 1</Dimension><Dimension Name="Input">2</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let mut parameter_order = decoded
        .ir()
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
        .collect::<Vec<_>>();
    parameter_order.sort_unstable();
    assert_eq!(parameter_order, vec![(0, "Input"), (1, "Result")]);

    {
        let mut ir_edit = decoded.ir_mut();
        let result = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Result")
            .unwrap();
        result.expression = "Input + 2".into();
        result.value = Some(ParameterValue::Integer(4));
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
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(
        feature
            .content
            .iter()
            .filter_map(|content| match content {
                FeatureContent::Dimension(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["Input", "Result"]
    );
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "Input + 2");
    assert_eq!(result.value, Some(ParameterValue::Integer(4)));
    assert_eq!(result.dependencies.len(), 1);
}

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
        assert_eq!(result.dependencies, vec![sketch1_parameter.clone()]);
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
    assert_eq!(result.expression, "Width@Sketch1 * 2");
    assert_eq!(result.dependencies.len(), 1);
}

#[test]
fn semantic_writer_rewrites_qualified_bare_equation_ids() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="Width" EquationId="D1">2mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let width = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Width")
            .unwrap();
        width.properties.insert("EquationId".into(), "D2".into());
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
    let mut regenerated = cadmpeg_test_support::EditableDecodeResult::from(regenerated);
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D2@Sketch1 * 2");
    assert_eq!(result.dependencies.len(), 1);
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Width")
            .unwrap()
            .properties["EquationId"],
        "D2"
    );

    regenerated
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Width")
        .unwrap()
        .properties
        .insert("EquationId".into(), "D3@Sketch1".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        regenerated.ir(),
        regenerated.source_fidelity(),
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
            .find(|parameter| parameter.name == "Result")
            .unwrap()
            .expression,
        "D3@Sketch1 * 2"
    );
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
    assert_eq!(gauge.properties["EquationId"], "D1@Profile");
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D1@Profile * 2");
    assert_eq!(result.dependencies, vec![gauge.id.clone()]);
}

#[test]
fn semantic_writer_preserves_empty_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

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
    assert_eq!(empty.expression, "");
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
        depth.value = Some(ParameterValue::Length(Length(20.0)));
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
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["External"], "");
    assert_eq!(feature.dimension_properties["External"]["Driven"], "true");
}

#[test]
fn semantic_writer_preserves_keywords_attributes() {
    use cadmpeg_ir::features::{Length, ParameterValue};

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
        parameter.value = Some(ParameterValue::Length(Length(20.0)));
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
    let history = &sldprt_native(regenerated.ir()).feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.properties["Schema"], "34000");
    assert_eq!(history.properties["Revision"], "12");
}

#[test]
fn semantic_writer_preserves_keywords_child_order() {
    use crate::records::HistoryContent;
    use cadmpeg_ir::features::{Length, ParameterValue};

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
        depth.value = Some(ParameterValue::Length(Length(20.0)));
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
        configuration.ordinal = u32::from(configuration.name == "A");
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
fn semantic_writer_rejects_conflicting_parameter_edits() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
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
        parameter.value = Some(ParameterValue::Length(Length(20.0)));
        update_sldprt_native(&mut ir_edit, |native| {
            native.feature_histories[0].features[0]
                .parameters
                .insert("Depth".into(), "30mm".into());
        });
    }

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT parameter edits"));
}

#[test]
fn semantic_writer_rejects_conflicting_dimension_property_edits() {
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
    decoded.ir_mut().model.parameters[0]
        .properties
        .insert("Driven".into(), "neutral".into());
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .dimension_properties
            .get_mut("Depth")
            .unwrap()
            .insert("Driven".into(), "native".into());
    });

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT parameter edits"));
}

#[test]
fn semantic_writer_round_trips_sparse_positional_extrusions() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ParameterValue,
        Termination,
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
    let first_definition = &decoded.ir().model.features[0].definition;
    assert!(
        matches!(
            first_definition,
            FeatureDefinition::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Blind {
                            length: Length(200.0)
                        },
                        ..
                    }
                },
                op: BooleanOp::Unresolved,
                ..
            }
        ),
        "{first_definition:?}"
    );
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(3.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(4.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length(200.0)))
    );
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Length(Length(3.0)))
    );
    assert_eq!(
        decoded.ir().model.parameters[2].value,
        Some(ParameterValue::Length(Length(4.0)))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Extrude {
            extent:
                ExtrudeExtent::OneSided {
                    side:
                        ExtrudeSide {
                            termination: Termination::Blind { length },
                            ..
                        },
                },
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed positional boss extrusion");
        };
        *length = Length(250.0);
        let FeatureDefinition::Extrude {
            extent:
                ExtrudeExtent::OneSided {
                    side:
                        ExtrudeSide {
                            termination: Termination::Blind { length },
                            ..
                        },
                },
            ..
        } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed positional cut extrusion");
        };
        *length = Length(4.5);
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
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(250.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir().model.features[1].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(4.5)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir().model.features[2].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(4.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));

    regenerated.ir_mut().model.parameters[0].expression = "225".into();
    regenerated.ir_mut().model.parameters[0].value = Some(ParameterValue::Length(Length(225.0)));
    let mut parameter_encoded = Vec::new();
    crate::test_support::plan_inherited_write(
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
        Some(ParameterValue::Length(Length(225.0)))
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
    let scope = base.ir().model.bodies[0].id.0.clone();
    let mut source = sldprt_with_body(&body);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(
            r#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="{scope}"/></Keywords>"#
        )
        .as_bytes(),
    ));
    let source_partition = container::scan_bytes(&source)
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap()
        .payload
        .clone();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(
        decoded.ir().model.features[0].outputs,
        vec![decoded.ir().model.bodies[0].id.clone()]
    );
    decoded.ir_mut().model.features[0].outputs = vec![decoded.ir().model.bodies[1].id.clone()];

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let written_partition = container::scan_bytes(&encoded)
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap()
        .payload
        .clone();
    assert_eq!(written_partition, source_partition);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].properties["Scope"],
        regenerated.ir().model.bodies[1].id.0
    );
    assert_eq!(
        regenerated.ir().model.features[0].outputs,
        vec![regenerated.ir().model.bodies[1].id.clone()]
    );
}

#[test]
fn semantic_writer_round_trips_all_extrusion_forms() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ProfileRef,
        Termination,
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
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind { length: Length(2.0) },
                    draft: None,
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        } if profile == &profile_feature
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: Termination::Blind { length: Length(4.0) },
                    draft: Some(Angle(value)),
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        } if (value - 5f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(matches!(
        decoded.ir().model.features[3].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(3.0)
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(7.0)
                    },
                    ..
                },
            },
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    ..
                }
            },
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Extrude {
            direction,
            extent,
            op,
            ..
        } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed extrusion");
        };
        *direction = cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(1.0, 0.0, 0.0));
        *extent = ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(8.0),
                },
                draft: Some(Angle(0.1)),
                offset: None,
            },
            second: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(9.0),
                },
                draft: None,
                offset: None,
            },
        };
        *op = BooleanOp::Intersect;
        let FeatureDefinition::Extrude {
            direction, extent, ..
        } = &mut ir_edit.model.features[3].definition
        else {
            panic!("typed extrusion");
        };
        *direction = cadmpeg_ir::features::ExtrudeDirection::ProfileNormal;
        *extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::ThroughAll,
                draft: None,
                offset: None,
            },
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[1].properties["EndCondition"], "TwoSided");
    assert_eq!(native[1].properties["Direction"], "1,0,0");
    assert_eq!(native[1].properties["Operation"], "Intersect");
    assert_eq!(native[1].parameters["Depth"], "8mm");
    assert_eq!(native[1].parameters["Depth2"], "9mm");
    assert_eq!(native[1].parameters["Draft"], "0.1rad");
    assert_eq!(native[3].properties["EndCondition"], "ThroughAll");
    assert!(!native[3].parameters.contains_key("Depth"));
    assert!(!native[3].parameters.contains_key("Depth2"));
    assert!(!native[3].properties.contains_key("Direction"));
}

#[test]
fn semantic_writer_round_trips_extrusion_to_face() {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, Termination,
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
        let FeatureDefinition::Extrude { extent, .. } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed extrusion");
        };
        assert_eq!(
            extent,
            &ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Native("face:12".into()),
                        offset: None,
                    },
                    draft: None,
                    offset: None,
                }
            }
        );
        *extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::ToFace {
                    face: FaceSelection::Native("face:13".into()),
                    offset: None,
                },
                draft: None,
                offset: None,
            },
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[1];
    assert_eq!(native.properties["EndCondition"], "ToFace");
    assert_eq!(native.properties["Face"], "face:13");
    assert!(matches!(
        &regenerated.ir().model.features[1].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Native(face),
                        ..
                    },
                    ..
                }
            },
            ..
        } if face == "face:13"
    ));
}

#[test]
fn semantic_writer_retains_unresolved_native_edge_treatments() {
    use cadmpeg_ir::features::{
        ChamferForm, ChamferSpec, FeatureDefinition, RadiusForm, RadiusSpec,
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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Unresolved {
                form: Some(RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::Unresolved {
                form: Some(ChamferForm::Distance),
            },
            ..
        }])
    ));

    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("unresolved fillet radius law"));
    detached.model.features[0] = decoded.ir().model.features[0].clone();
    detached.model.features[1].native_ref = None;
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("unresolved chamfer dimensions"));

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
        &decoded.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(2.0),
            },
            ..
        }] if selection == "edge:1,edge:2")
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed fillet feature");
        };
        groups[0].radius = cadmpeg_ir::features::RadiusSpec::Constant {
            radius: cadmpeg_ir::features::Length(3.5),
        };
        groups[0].edges = cadmpeg_ir::features::EdgeSelection::Native("edge:3".into());
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
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Radius"],
        "3.5mm"
    );
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].properties["Edges"],
        "edge:3"
    );
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(3.5),
            },
            ..
        }])
    ));
}

#[test]
fn semantic_writer_round_trips_positional_fillet_and_localized_chamfer_dimensions() {
    use cadmpeg_ir::features::{
        Angle, ChamferSpec, EdgeSelection, FeatureDefinition, Length, ParameterValue, RadiusSpec,
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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Constant {
                radius: Length(1.0)
            }, ..
        }])
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::DistanceAngle {
                distance: Length(0.3),
                angle: Angle(angle),
            },
        }] if (angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12)
    ));
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Length(Length(0.3)))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Fillet { groups, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed positional fillet");
        };
        groups[0].radius = RadiusSpec::Constant {
            radius: Length(2.5),
        };
        let FeatureDefinition::Chamfer { groups, .. } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed positional chamfer");
        };
        groups[0].spec = ChamferSpec::DistanceAngle {
            distance: Length(0.6),
            angle: Angle(30.0_f64.to_radians()),
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].parameters["D1"], "R2.5");
    assert!(!native[0].parameters.contains_key("Radius"));
    assert_eq!(native[1].kind, format!("Chafl{}n", '\u{00e1}'));
    assert_eq!(native[1].parameters["D1"], "0.6");
    assert_eq!(native[1].parameters["D2"], format!("30{}", '\u{00b0}'));
    assert!(!native[1].parameters.contains_key("Distance"));
    assert!(!native[1].parameters.contains_key("Angle"));
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(2.5)
            },
            ..
        }])
    ));
    assert!(matches!(
        &regenerated.ir().model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::DistanceAngle {
                distance: Length(0.6),
                angle: Angle(angle),
            },
            ..
        }] if (angle - 30.0_f64.to_radians()).abs() < 1.0e-12)
    ));
}

#[test]
fn semantic_writer_round_trips_variable_radius_fillet() {
    use cadmpeg_ir::features::{
        EdgeSelection, FeatureDefinition, Length, RadiusSpec, VariableRadius,
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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Variable { points }, ..
        }] if points == &vec![
            VariableRadius { parameter: 0.0, radius: Length(2.0) },
            VariableRadius { parameter: 0.5, radius: Length(4.0) },
            VariableRadius { parameter: 1.0, radius: Length(3.0) },
        ])
    ));
    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Fillet { groups } = &mut ir_edit.model.features[0].definition else {
            panic!("variable fillet");
        };
        let RadiusSpec::Variable { points } = &mut groups[0].radius else {
            panic!("variable fillet radius")
        };
        points[1].parameter = 0.4;
        points[1].radius = Length(5.0);
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
        let FeatureDefinition::Fillet { groups, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("variable fillet after regeneration");
        };
        groups[0].radius = RadiusSpec::Constant {
            radius: Length(6.0),
        };
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        regenerated.ir(),
        regenerated.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let final_ir = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameters = &sldprt_native(final_ir.ir()).feature_histories[0].features[0].parameters;
    assert_eq!(parameters["Radius"], "6mm");
    assert!(!parameters.keys().any(|name| name.starts_with("Position")));
    assert!(!parameters.keys().any(|name| name == "Radius0"));
}

#[test]
fn semantic_writer_round_trips_all_typed_chamfer_forms() {
    use cadmpeg_ir::features::{ChamferSpec, EdgeSelection, FeatureDefinition, Length};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Native(edges),
            spec: ChamferSpec::Distance {
                distance: Length(2.0),
            },
            ..
        }] if edges == "edge:1")
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::TwoDistances {
                first: Length(3.0),
                second: Length(6.35),
            },
            ..
        }])
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::DistanceAngle {
                distance: Length(4.0),
                angle,
            },
            ..
        }] if (angle.0 - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12)
    ));

    let replacements = [
        ChamferSpec::Distance {
            distance: Length(2.5),
        },
        ChamferSpec::TwoDistances {
            first: Length(3.5),
            second: Length(7.0),
        },
        ChamferSpec::DistanceAngle {
            distance: Length(4.5),
            angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_6),
        },
    ];
    for (index, (feature, replacement)) in decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .zip(replacements)
        .enumerate()
    {
        let FeatureDefinition::Chamfer { groups, .. } = &mut feature.definition else {
            panic!("typed chamfer feature");
        };
        groups[0].spec = replacement;
        groups[0].edges = EdgeSelection::Native(format!("edge:{}", index + 4));
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
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Native(faces),
            thickness: None,
            outward: None,
            ..
        } if faces == "face:1"
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Native(faces),
            thickness: None,
            side: None,
        } if faces == "face:2"
    ));

    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("unresolved shell construction"));
    detached.model.features[0] = decoded.ir().model.features[0].clone();
    detached.model.features[1].native_ref = None;
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("unresolved thicken construction"));

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
