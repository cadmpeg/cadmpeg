// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn semantic_writer_refuses_omitted_linear_pattern_instances() {
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
    let error = crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("omitted pattern instances"),
        "{error}"
    );
}
