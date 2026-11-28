// SPDX-License-Identifier: Apache-2.0
use super::decode;
use crate::test_support::body_reference_tail_fixture::{archive, frame, metadata};
use crate::test_support::native_fixture::{f3d_native, update_f3d_native};

#[test]
fn body_reference_tail_replays_exactly_and_unsupported_edits_regenerate() {
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
    crate::test_support::assert_patch_declined(
        &edited,
        Some(decoded.source_fidelity()),
        "modified F3D IR contains edits beyond supported",
    );
}
