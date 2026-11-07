// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn colour_in_an_unused_style_is_unbound_and_written_as_rgb() {
    let result = decode_inline("#1=COLOUR_RGB('unused',0.125,0.25,0.5);\n#2=FILL_AREA_STYLE_COLOUR('',#1);\n#3=FILL_AREA_STYLE('',(#2));");
    assert_eq!(result.ir().model.appearances.len(), 1);
    let appearance = &result.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("unused"));
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
