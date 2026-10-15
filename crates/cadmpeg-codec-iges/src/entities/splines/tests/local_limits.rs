// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn spline_declared_populations_fuse_the_original_caller_session() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    // Type114 has (3*u+1)*(3*v+1) poles before any breakpoint lane.
    for (entity_type, values, operation, limit, requested) in [
        (112, [112, 3, 1, 3, 100_001], "iges_spline_segments", 100_000, 100_001),
        (112, [112, 3, 1, 3, i64::MAX], "iges_spline_segments", 100_000, u64::try_from(i64::MAX).unwrap()),
        (114, [114, 3, 1, 1_000, 1_000], "iges_spline_surface_poles", 1_000_000, 3_001 * 3_001),
        (114, [114, 3, 1, i64::MAX, i64::MAX], "iges_spline_surface_poles", 1_000_000, u64::MAX),
    ] {
        let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
            values.into_iter().map(|value| Token {
                value: TokenValue::Integer(value), span: 0..0,
            }).collect(), Vec::new());
        let directory = [crate::test_support::directory_target(1, entity_type)];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(&mut ir, &directory, &[record], &global, &ctx, &mut sequences);
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected the declared population ceiling before lane construction"),
        };
        drop(result);
        assert_eq!(first.dimension, ResourceDimension::Codec(operation));
        assert_eq!(first.operation, operation);
        assert_eq!((first.limit, first.used, first.additional), (limit, limit, requested - limit));
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert!(ir.model.curves.is_empty());
        assert!(ir.model.surfaces.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}
