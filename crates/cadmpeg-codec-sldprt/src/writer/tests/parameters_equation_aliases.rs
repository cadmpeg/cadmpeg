// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn moved_parameter_keeps_its_equation_id() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="Width" EquationId="D1@Sketch1">2mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#));
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap(),
    );
    let owner = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Equations"))
        .unwrap()
        .id
        .clone();
    let mut model = decoded.ir_mut();
    let parameter = model
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Width")
        .unwrap();
    let parameter_id = parameter.id.clone();
    parameter.owner = Some(owner);
    parameter.ordinal = 1;
    let original = model
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap();
    original.source_content = original.source_content.iter().filter(|item| {
        !matches!(item, cadmpeg_ir::features::FeatureSourceContent::Parameter(id) if id == &parameter_id)
    }).cloned().collect::<Vec<_>>().try_into().unwrap();
    drop(model);
    let mut bytes = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut bytes,
    )
    .unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(result.ir());
    let equations = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.name == "Equations")
        .unwrap();
    assert_eq!(
        equations
            .dimension_properties
            .get("Width")
            .and_then(|p| p.get("EquationId"))
            .map(String::as_str),
        Some("D1@Equations"),
        "{equations:#?}"
    );
    assert_eq!(equations.parameters["Result"], "D1@Equations * 2");
}

#[test]
fn swapping_parameter_names_keeps_each_equation_id() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="A" EquationId="D1">2mm</Dimension><Dimension Name="B" EquationId="D2">3mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 + D2@Sketch1</Dimension></Feature></Keywords>"#));
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap(),
    );
    for parameter in &mut decoded.ir_mut().model.parameters {
        match parameter.name.as_str() {
            "A" => parameter.name = "B".into(),
            "B" => parameter.name = "A".into(),
            _ => {}
        }
    }
    let mut bytes = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut bytes,
    )
    .unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(result.ir());
    let sketch = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.name == "Sketch1")
        .unwrap();
    assert_eq!(sketch.dimension_properties["B"]["EquationId"], "D1");
    assert_eq!(sketch.dimension_properties["A"]["EquationId"], "D2");
    let equations = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.name == "Equations")
        .unwrap();
    assert_eq!(equations.parameters["Result"], "D1@Sketch1 + D2@Sketch1");
}
