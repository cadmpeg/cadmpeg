// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn copious_tuple_ceiling_preserves_the_original_caller_session_refusal() {
    let mut entry = crate::test_support::directory_target(1, 106);
    entry.form = 12;
    let directory = [entry];
    let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    for count in [1_000_001, i64::MAX] {
        let values = [106, 2, count];
        let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
            values.into_iter().map(|value| Token {
                value: TokenValue::Integer(value), span: 0..0,
            }).collect(), Vec::new());
        let records = BTreeMap::from([(1, &record)]);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(&mut ir, &directory, &entries, &records, &global, &ctx, &mut sequences);
        let first = match result {
            Err(CodecError::ResourceLimit(first)) => first,
            _ => panic!("expected the declared tuple ceiling before allocating positions"),
        };
        assert_eq!(first.dimension, ResourceDimension::Codec("iges_copious_tuples"));
        assert_eq!(first.operation, "iges_copious_tuples");
        assert_eq!((first.limit, first.used, first.additional), (1_000_000, 1_000_000, u64::try_from(count).unwrap() - 1_000_000));
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert!(ir.model.curves.is_empty());
        assert!(ir.model.points.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}
