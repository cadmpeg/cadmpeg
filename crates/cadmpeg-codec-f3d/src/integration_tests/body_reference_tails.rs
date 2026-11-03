// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::WritePath;

use super::decode;
use crate::test_support::body_reference_tail_test::{archive, frame, metadata};
use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::F3dCodec;

#[test]
fn body_reference_tail_replays_exactly_and_unsupported_edits_are_refused() {
    let (bulk, frame) = frame(true, 9, 0);
    let source = archive(&bulk, &metadata(frame, 1));
    let decoded = decode(source.clone());
    let tails = f3d_native(decoded.ir()).design_body_reference_tails;
    assert_eq!(tails.len(), 1);
    assert_eq!(tails[0].serialization_form.value, 0);
    assert!(tails[0].first_references.is_empty());
    assert_eq!(tails[0].second_references[0].target(), Some(u64::MAX));
    assert_eq!(tails[0].trailing_form.value, 9);
    let mut output = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .expect("unchanged reference tail replays");
    assert_eq!(output, source);
    let mut edited = decoded.ir().clone();
    update_f3d_native(&mut edited, |native| {
        native.design_body_reference_tails[0].trailing_form.value = 8;
    });
    // §1.4: an edit the patch path cannot express falls back to synthesis
    // with an explicit cause instead of refusing.
    let plan = F3dCodec
        .plan(
            EncodeInput::new(&edited, Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("unsupported native tail edit falls back to synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { .. })
    ));
    let mut fallback_output = Vec::new();
    plan.write_to(&mut fallback_output)
        .expect("fallback synthesis writes");
    F3dCodec
        .decode(
            &mut Cursor::new(&fallback_output),
            &DecodeOptions::default(),
        )
        .expect("fallback synthesis decodes");
}
