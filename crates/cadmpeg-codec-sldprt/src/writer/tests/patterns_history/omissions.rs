// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn semantic_writer_reports_omitted_linear_pattern_instances() {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords>
        <Feature Name="Seed" Type="NativeSeed" id="7"/>
        <Pattern Name="Rows" Type="LinearPattern" id="18" Seeds="7" Direction="1,0,0"><Dimension Name="Count">3</Dimension><Dimension Name="Spacing">10mm</Dimension></Pattern>
    </Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.features[1]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                definition
            else {
                panic!("pattern");
            };
            let mut transform = pattern.definition().clone();
            let PatternTransform::Linear {
                omitted_instances, ..
            } = &mut transform
            else {
                panic!("linear pattern");
            };
            omitted_instances.push([1, 0]);
            *pattern = PatternKind::new(transform).unwrap();
        });
    let feature = decoded.ir().model.features[1].id.clone();
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Inherit,
    )
    .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(feature.as_str()) && loss.message.contains("[1, 0]")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. })
        if matches!(pattern.definition(), PatternTransform::Linear { omitted_instances, count, distance, .. }
            if omitted_instances.is_empty() && output.ir().dimension_value(count) == Some(3)
                && output.ir().dimension_value(distance.dimension()).is_some_and(|value| value.get() == 10.0))
    )));
}
