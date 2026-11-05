// SPDX-License-Identifier: Apache-2.0

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn source_less_move_body_keeps_unresolved_selection() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><MoveBody Name="Partial" Type="MoveBody" id="32" Translation="1mm,2mm,3mm" Copies="0"/></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Unresolved,
            ..
        })
    ));
    let mut ir = decoded.ir().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    for feature in &mut ir.model.features {
        feature.native_ref = None;
    }
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let definition = output
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Partial"))
        .unwrap()
        .evaluation
        .definition();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies: BodySelection::Unresolved, translation, rotation: None, copies }) if translation.iter().map(|value| output.ir().dimension_value(value).unwrap().get()).collect::<Vec<_>>() == [1.0, 2.0, 3.0] && output.ir().dimension_value(copies) == Some(0))
    );
}
