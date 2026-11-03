// SPDX-License-Identifier: Apache-2.0
//! Native registry conformance (#231 W3): the declared dispositions cover the
//! decoder's arena registry and state what the writer emits.

use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use crate::{IgesCodec, IgesVersion};
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, NativeDisposition};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn dispositions(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::report::export::ExportReport {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("tetrahedron synthesizes");
    let mut written = Vec::new();
    plan.write_to(&mut written)
        .expect("tetrahedron bytes write")
}

#[test]
fn native_registry_equals_dispositions() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("tetrahedron fixture decodes");
    let declared: std::collections::BTreeSet<_> = dispositions(decoded.ir())
        .native_dispositions
        .namespaces
        .get("iges")
        .expect("iges namespace is declared")
        .arenas
        .keys()
        .cloned()
        .collect();
    let registry: std::collections::BTreeSet<_> = crate::native::ARENAS
        .iter()
        .map(|arena| (*arena).to_owned())
        .collect();
    assert_eq!(declared, registry);
    let report = dispositions(decoded.ir());
    let disposition = |arena: &str| report.native_dispositions.disposition("iges", arena);
    assert_eq!(
        disposition("manifold_solids"),
        NativeDisposition::Regenerated
    );
    assert_eq!(disposition("entities"), NativeDisposition::Dropped);
    assert_eq!(disposition("directions"), NativeDisposition::Dropped);
}

#[test]
fn iges_declares_no_reduced_fields() {
    assert!(crate::writer::REDUCED_FIELDS.is_empty());
}
