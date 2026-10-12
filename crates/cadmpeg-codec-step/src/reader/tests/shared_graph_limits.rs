// SPDX-License-Identifier: Apache-2.0
//! Visited closure admission on a diamond chain.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn record_closure_diamond_keeps_visited_work_and_original_refusal() {
    let mut records = String::from("#1=ITEM((#10,#11));#2=ITEM();");
    for layer in 0..24 {
        let left = 10 + 2 * layer;
        let children = if layer == 23 {
            String::from("#2")
        } else {
            format!("#{},#{}", left + 2, left + 3)
        };
        for id in [left, left + 1] {
            writeln!(records, "#{id}=ITEM(({children}));").unwrap();
        }
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap();
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::record_closure(&BTreeSet::from([1]), &exchange, &ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == *limit)
            );
        } else {
            ctx.finish_session().unwrap();
        }
        result
    };
    // The path count is 2^24; one visit per source node fits this allowance.
    let closure = run(500_000).expect("visited closure fits");
    let expected = [1, 2].into_iter().chain(10..58).collect::<BTreeSet<_>>();
    assert_eq!(closure, expected);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP mod worklist step",
        run,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "STEP mod worklist step" && limit.limit < 500_000)
    );
}
