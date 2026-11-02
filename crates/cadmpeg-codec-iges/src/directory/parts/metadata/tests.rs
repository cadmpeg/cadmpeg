// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use super::{admit_field, FieldValue};
use crate::global::GlobalTable;

#[test]
fn optional_directory_scalar_uses_actual_nonzero_field_and_refuses_foreign_source() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"pre       7post", &arena, &DecodePolicy::service())
            .expect("source session");
    let source = root.child(3, 11).expect("exact nonzero field");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    assert_eq!(
        admit_field(&ctx, source, 12, GlobalTable::Legacy, &mut output).expect("actual scalar"),
        Some(FieldValue::Integer(7))
    );
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(native
        .namespace("iges")
        .is_none_or(|namespace| namespace.arenas().is_empty()));
    assert!(losses.is_empty());
    assert!(fidelity.retained_records().is_empty());
}
