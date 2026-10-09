use super::super::{parse_relation_signature, parse_schema_program, CATALOG_OPEN};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[test]
fn missing_schema_prefix_rejects_before_suffix_scan_and_copy() {
    let mut bytes = CATALOG_OPEN.to_vec();
    bytes.extend([0xfe, 0xfe, 0xfd]);
    bytes.extend([0x81; 4096]);
    let run = |ctx: &DecodeContext<'_>| parse_schema_program(ctx, &bytes, 0, None);
    assert!(crate::test_support::with_work_limit(0, run)
        .expect("fixed prefix rejection visits no suffix")
        .is_none());
    assert!(crate::test_support::with_collection_limit(0, run)
        .expect("fixed prefix rejection copies no suffix")
        .is_none());
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "before invalid schema prefix")
            .expect_err("zero work fuse")
        else {
            panic!("resource refusal");
        };
        let CodecError::ResourceLimit(actual) = run(ctx).expect_err("sticky schema refusal")
        else {
            panic!("resource refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    bytes[CATALOG_OPEN.len() + 2] = 0xfe;
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(limit) = parse_schema_program(ctx, &bytes, 0, None)
            .expect_err("present prefix requires the actual suffix scan")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.operation, "catia_legacy_schema_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn missing_relation_prefix_rejects_before_reverse_result_search() {
    let source = format!("{}) : Real\n", "x".repeat(4096));
    let run = |ctx: &DecodeContext<'_>| parse_relation_signature(ctx, &source);
    assert!(crate::test_support::with_work_limit(0, run)
        .expect("fixed opening parenthesis rejection visits no tail")
        .is_none());
    assert!(crate::test_support::with_collection_limit(0, run)
        .expect("fixed opening parenthesis rejection allocates no candidate")
        .is_none());
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "before invalid relation prefix")
            .expect_err("zero work fuse")
        else {
            panic!("resource refusal");
        };
        let CodecError::ResourceLimit(actual) = run(ctx).expect_err("sticky relation refusal")
        else {
            panic!("resource refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(limit) = parse_relation_signature(ctx, "() : Real\n")
            .expect_err("present prefix requires the actual result search")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.operation, "catia_legacy_relation_result_split");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}
