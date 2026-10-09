// SPDX-License-Identifier: Apache-2.0
use crate::dialect::dialect_loss;
use cadmpeg_core::CodecError;
use super::resolved_global;

#[test]
fn verified_iges_dialect_loss_preserves_original_refusal() {
    let globals = [resolved_global("6"), resolved_global("8"), resolved_global("11")];
    crate::test_support::with_entry_context(|ctx, original| {
        for global in &globals {
            let result = dialect_loss(global, ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}
