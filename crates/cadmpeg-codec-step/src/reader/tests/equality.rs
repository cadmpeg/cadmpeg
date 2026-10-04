// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::reader::RecordExt;

#[test]
fn partial_record_first_match_preserves_equality_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(ALPHA() BETA());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    let record = exchange.records().get(&1).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Both partial visits fit; the first name comparison refuses before a later match.
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
    let error = record.partial(&ctx, "BETA").unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("partial comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP partial equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    let service = cadmpeg_test_support::service_decode_context();
    assert_eq!(record.partial(&service, "BETA").unwrap().unwrap().name, "BETA");
    assert!(record.partial(&service, "GAMMA").unwrap().is_none());
}
