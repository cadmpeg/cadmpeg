// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn invalid_unbound_predefined_colour_name_is_reported_once() {
    let result = decode_inline(r"#1=DRAUGHTING_PRE_DEFINED_COLOUR('\X2\GGGG\X0\');");
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == crate::loss::StepLossCode::AttributeStringInvalid.kind())
            .count(),
        1
    );
    assert!(result.ir().model.appearances.is_empty());
}

#[test]
fn unbound_predefined_colours_report_the_construct_reduction() {
    let result = decode_inline(
        "#1=DRAUGHTING_PRE_DEFINED_COLOUR('red');\n#2=DRAUGHTING_PRE_DEFINED_COLOUR('custom');",
    );
    assert!(result.ir().model.appearance_bindings.is_empty());
    let red = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.name.as_deref() == Some("red"))
        .expect("red colour");
    let custom = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.name.as_deref() == Some("custom"))
        .expect("custom colour");
    assert_eq!(
        red.base_color,
        cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0)
    );
    assert!(custom.base_color.is_none());
    assert!(red.schema.is_none());
    assert!(custom.schema.is_none());
    let mut bytes = Vec::new();
    let report = write_step(
        result.ir(),
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write colours");
    for appearance in [red, custom] {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.message.contains(appearance.id.as_str())));
    }
    assert!(std::str::from_utf8(&bytes)
        .expect("STEP text")
        .contains("COLOUR_RGB('red',1.,0.,0.)"));
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written colour");
    assert!(decoded.ir().model.appearance_bindings.is_empty());
}

#[test]
fn colour_in_an_unused_style_is_unbound_and_written_as_rgb() {
    let result = decode_inline("#1=COLOUR_RGB('unused',0.125,0.25,0.5);\n#2=FILL_AREA_STYLE_COLOUR('',#1);\n#3=FILL_AREA_STYLE('',(#2));");
    assert_eq!(result.ir().model.appearances.len(), 1);
    let appearance = &result.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("unused"));
    assert!(appearance.schema.is_none());
    assert_eq!(
        appearance.base_color,
        cadmpeg_ir::topology::Color::new(0.125, 0.25, 0.5, 1.0)
    );
    assert!(result.ir().model.appearance_bindings.is_empty());
    let mut bytes = Vec::new();
    write_step(
        result.ir(),
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write unbound colour");
    assert!(std::str::from_utf8(&bytes)
        .expect("STEP text")
        .contains("COLOUR_RGB('unused',0.125,0.25,0.5)"));
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode unbound colour");
    assert_eq!(decoded.ir().model.appearances.len(), 1);
    assert_eq!(
        decoded.ir().model.appearances[0].base_color,
        appearance.base_color
    );
    assert!(decoded.ir().model.appearance_bindings.is_empty());
}
