// SPDX-License-Identifier: Apache-2.0

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{
    BooleanOp, FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use std::io::Cursor;

pub(super) fn source_less_extrusion() -> cadmpeg_ir::CadIr {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Extrusion Name="Boss" id="9"><Dimension Name="D1">20</Dimension></Extrusion></Keywords>"#));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &crate::test_support::history::resolved_feature_classes_with_ids(&[(
            "moExtrusion_c",
            "Boss",
            9,
        )]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    let feature = &mut ir.model.features[0];
    feature.native_ref = None;
    feature.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrusion");
        };
        *op = BooleanOp::Join;
    });
    ir
}

#[test]
fn source_less_extrusion_keeps_its_unresolved_profile() {
    let mut ir = source_less_extrusion();
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("retains its solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Join,
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            ..
        })
    )));
}
