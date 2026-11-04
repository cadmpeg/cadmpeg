// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn semantic_writer_derives_resolved_feature_section_names() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.source_fidelity_mut().annotations = cadmpeg_ir::Annotations::default();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities[0]
            .reclassify(crate::records::SketchInputKind::from_native_code(9));
    });

    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-0-ResolvedFeatures") }));

    let (mut unscoped, _, fidelity) = decoded.into_parts();
    update_sldprt_native(&mut unscoped, |native| {
        native.feature_input_lanes[0].configuration = None;
    });
    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        &unscoped,
        &fidelity,
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.name() == Some("Contents/ResolvedFeatures")));
}
