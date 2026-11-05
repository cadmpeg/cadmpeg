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

#[test]
fn source_less_extrusion_keeps_its_unresolved_operation() {
    let mut ir = source_less_extrusion();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrusion");
        };
        *op = BooleanOp::Unresolved;
    });
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
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Unresolved,
            ..
        })
    )));
}

#[test]
fn source_less_extrusion_keeps_its_unresolved_extent() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
    let mut ir = source_less_extrusion();
    ir.model.parameters.clear();
    ir.model.features[0].source_content = Default::default();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition
        else {
            panic!("extrusion");
        };
        *extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Unresolved {},
                draft: None,
            },
        };
    });
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
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Unresolved {},
                    ..
                }
            },
            ..
        })
    )));
}

#[test]
fn edited_extrusion_can_clear_its_operation() {
    for original in [BooleanOp::Join, BooleanOp::Cut] {
        let mut source = source_less_extrusion();
        source.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
            else {
                panic!("extrusion")
            };
            *op = original;
        });
        let plan = SldprtCodec
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        ir.model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Boss"))
            .unwrap()
            .evaluation
            .edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
                else {
                    panic!("extrusion")
                };
                *op = BooleanOp::Unresolved;
            });
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let plan = SldprtCodec
                .plan(EncodeInput::new(&ir, fidelity), TargetRequest::Inherit)
                .unwrap();
            assert!(!plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("retains its solved result")));
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes).unwrap();
            let output = SldprtCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(
                output.ir().model.features.iter().any(|feature| matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Extrude {
                        op: BooleanOp::Unresolved,
                        ..
                    })
                )),
                "initial {original:?}, fidelity {}, features {:?}",
                fidelity.is_some(),
                output.ir().model.features
            );
        }
    }
}

#[test]
fn edited_extrusion_clears_operation_in_its_native_element_tag() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><BossExtrude Name="Boss" id="9"><Dimension Name="D1">20</Dimension></BossExtrude></Keywords>"#));
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
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrusion")
        };
        *op = BooleanOp::Unresolved;
    });
    for fidelity in [None, Some(decoded.source_fidelity())] {
        let plan = SldprtCodec
            .plan(EncodeInput::new(&ir, fidelity), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(
            output.ir().model.features.iter().any(|feature| matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    op: BooleanOp::Unresolved,
                    ..
                })
            )),
            "fidelity {}, features {:?}",
            fidelity.is_some(),
            output.ir().model.features
        );
    }
}

#[test]
fn source_less_surface_extrusion_is_authored() {
    let mut ir = source_less_extrusion();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) = definition
        else {
            panic!("extrusion")
        };
        *solid = Some(false);
    });
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
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            solid: Some(false),
            op: BooleanOp::Join,
            ..
        })
    )));
}

#[test]
fn edited_extrusion_changes_between_solid_and_surface() {
    for original in [true, false] {
        let mut source = source_less_extrusion();
        source.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) = definition
            else {
                panic!("extrusion")
            };
            *solid = Some(original);
        });
        let plan = SldprtCodec
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) = definition
            else {
                panic!("extrusion")
            };
            *solid = Some(!original);
        });
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let plan = SldprtCodec
                .plan(EncodeInput::new(&ir, fidelity), TargetRequest::Inherit)
                .unwrap();
            assert!(!plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("retains its solved result")));
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes).unwrap();
            let output = SldprtCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(output.ir().model.features.iter().any(|feature| matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude { solid: Some(value), op: BooleanOp::Join, .. }) if *value == !original
            )), "original {original}, fidelity {}", fidelity.is_some());
        }
    }
}
