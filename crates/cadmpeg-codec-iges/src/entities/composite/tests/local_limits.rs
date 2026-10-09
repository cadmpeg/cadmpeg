// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn composite_declared_child_population_fuses_the_original_caller_session() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    for count in [100_001, i64::MAX] {
        let entity_type = 102;
        let operation = "iges_composite_children";
        let limit = 100_000;
        let requested = u64::try_from(count).unwrap();
        let values = [102, count];
        let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
            values.into_iter().map(|value| Token {
                value: TokenValue::Integer(value), span: 0..0,
            }).collect(), Vec::new());
        let directory = [crate::test_support::directory_target(1, entity_type)];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
        let records = BTreeMap::from([(1, &record)]);
        let result = super::super::project(&mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences);
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected the declared population ceiling before lane construction"),
        };
        assert_eq!(first.dimension, ResourceDimension::Codec(operation));
        assert_eq!(first.operation, operation);
        assert_eq!((first.limit, first.used, first.additional), (limit, limit, requested - limit));
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert!(ir.model.curves.is_empty());
        assert!(ir.model.surfaces.is_empty());
        drop(result);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}
