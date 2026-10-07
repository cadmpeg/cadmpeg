// SPDX-License-Identifier: Apache-2.0

use crate::reader::RecordExt;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

/// A partial lookup charges one step per partial it visits, and one more for
/// the end probe when nothing matches; comparing a partial's name with the
/// schema literal is fixed work.
#[test]
fn partial_record_search_charges_each_visited_partial() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(ALPHA() BETA());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    let record = exchange.records().get(&1).unwrap();
    let lookup = |limit: u64, name: &'static str| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
        record
            .partial(&ctx, name)
            .map(|partial| partial.map(|partial| partial.name.clone()))
    };
    for (name, visits, found) in [("ALPHA", 1, true), ("BETA", 2, true), ("GAMMA", 3, false)] {
        let Err(CodecError::ResourceLimit(refusal)) = lookup(visits - 1, name) else {
            panic!("{name} needs {visits} visits");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP partial record search");
        assert_eq!(
            lookup(visits, name).unwrap(),
            found.then(|| name.to_owned())
        );
    }
}
