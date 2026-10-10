// SPDX-License-Identifier: Apache-2.0

fn transfer(held: bool) {
    crate::test_support::with_entry_context(|ctx, original| {
        let Some(first) = original else { return; };
        let mut ir = cadmpeg_ir::CadIr::empty();
        if held {
            let record = serde_json::from_value(serde_json::json!({"id": "sat:test:held#0"}))
                .expect("context-free native input fixture");
            ir.native.namespace_mut("test").arenas_mut().insert("body_native_keys".into(), vec![record]);
        }
        let before = serde_json::to_value(&ir).expect("context-free destination snapshot");
        let result = super::super::transfer_into_ir(ctx, &mut ir, "test", crate::brep::AsmBrep::default());
        assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first));
        assert_eq!(serde_json::to_value(&ir).expect("context-free destination snapshot"), before);
    });
}

#[test]
fn asm_empty_transfer_preserves_original_refusal() { transfer(false); }

#[test]
fn asm_held_native_transfer_preserves_original_refusal() { transfer(true); }
