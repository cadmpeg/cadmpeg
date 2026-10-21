// SPDX-License-Identifier: Apache-2.0
//! Identity-token parameter dependency writer tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
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
        decoded.ir().model.parameters[3]
            .expression
            .dependencies()
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
        vec![
            decoded.ir().model.parameters[1].id.clone(),
            decoded.ir().model.parameters[2].id.clone(),
            decoded.ir().model.parameters[0].id.clone(),
            decoded.ir().model.parameters[1].id.clone(),
        ]
    );

    decoded.ir_mut().model.parameters[0].name = "Renamed Base".into();
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
        crate::test_support::history::rendered_parameter(
            decoded.ir(),
            &decoded.ir().model.parameters[3]
        ),
        "\"Wall Gauge\" + \"Datum \"\"A\"\"\" + \"Renamed Base\" + \"Wall Gauge\""
    );
    assert_eq!(
        decoded.ir().model.parameters[3]
            .expression
            .dependencies()
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
        vec![
            decoded.ir().model.parameters[1].id.clone(),
            decoded.ir().model.parameters[2].id.clone(),
            decoded.ir().model.parameters[0].id.clone(),
            decoded.ir().model.parameters[1].id.clone(),
        ]
    );

    let id = decoded.ir().model.parameters[3].id.clone();
    decoded
        .ir_mut()
        .set_parameter_expression(
            &id,
            "6mm".into(),
            Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(6.0).unwrap(),
            )),
        )
        .unwrap();
    assert!(decoded.ir().model.parameters[3]
        .expression
        .dependencies()
        .next()
        .is_none());
    assert!(decoded.ir().model.features[0].needs_recompute);
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap();
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
    assert_eq!(parameter_order, vec![(0, "Result"), (1, "Input")]);

    {
        let input = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Input")
            .unwrap()
            .id
            .clone();
        let result = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Result")
            .unwrap()
            .id
            .clone();
        decoded
            .ir_mut()
            .set_parameter_expression(
                &result,
                cadmpeg_ir::features::Expression {
                    tokens: vec![
                        cadmpeg_ir::features::ExpressionToken::Parameter(input),
                        cadmpeg_ir::features::ExpressionToken::Text(" + 2".into()),
                    ],
                },
                Some(ParameterValue::Integer(4)),
            )
            .unwrap();
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
    assert_eq!(
        crate::test_support::history::rendered_parameter(regenerated.ir(), result),
        "Input + 2"
    );
    assert_eq!(result.value, Some(ParameterValue::Integer(4)));
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
