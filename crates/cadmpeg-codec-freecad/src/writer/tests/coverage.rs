// SPDX-License-Identifier: Apache-2.0
//! Native registry conformance (#231 W3): the declared dispositions cover the
//! decoder's arena registry and state what the writer emits.

use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn dispositions(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::report::export::ExportReport {
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .expect("seed synthesizes");
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("seed bytes write")
}

#[test]
fn native_registry_equals_dispositions() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("seed decodes");
    let declared: std::collections::BTreeSet<_> = dispositions(decoded.ir())
        .native_dispositions
        .namespaces
        .get("fcstd")
        .expect("fcstd namespace is declared")
        .arenas
        .keys()
        .cloned()
        .collect();
    let registry: std::collections::BTreeSet<_> = crate::native::ARENAS
        .iter()
        .map(|arena| (*arena).to_owned())
        .collect();
    assert_eq!(declared, registry);
}
