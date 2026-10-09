// SPDX-License-Identifier: Apache-2.0
use crate::entities::copious::has_forbidden_form_63_duplicate;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

#[test]
fn copious_two_point_recovery_preserves_original_refusal() {
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    crate::test_support::with_entry_context(|ctx, original| {
        let result = has_forbidden_form_63_duplicate(&points, 0.0, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(true))),
        }
    });
}
