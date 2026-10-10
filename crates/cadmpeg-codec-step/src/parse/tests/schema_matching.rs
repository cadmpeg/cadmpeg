// SPDX-License-Identifier: Apache-2.0
//! Schema references use normalized full identifiers and bare names.

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;

use crate::parse::{schema_identifier_matches, schema_names_for_matching, AdmittedSchemaIdentifier};
use crate::test_support::{with_policy_context, with_service_context};

#[test]
fn schema_index_preserves_full_identifier_and_bare_name_matching() {
    with_service_context(&[], |_, ctx| {
        let declarations = [" ap242 { iso 39 } ", "CUSTOM_SCHEMA", "AP242 { 1 2 }"]
            .into_iter().map(|text| AdmittedSchemaIdentifier::admit(ctx, text.into())
                .expect("admission").expect("valid schema")).collect::<Vec<_>>();
        let index = schema_names_for_matching(&declarations, ctx).expect("index");
        for (query, expected) in [
            ("ap242", true), (" AP242 { iso 39 } ", true),
            ("AP242 { 1 2 }", true), ("custom_schema", true),
            ("AP242 { 1 3 }", false), ("AP242{ iso 39 }", false),
            ("MISSING", false),
        ] {
            assert_eq!(schema_identifier_matches(&index, query, ctx).expect("lookup"), expected, "{query}");
        }
    });
}

#[test]
fn schema_matching_work_scales_with_indexed_queries() {
    let work = |count| {
        with_service_context(&[], |_, ctx| {
            let declarations = (0..count).map(|i|
                AdmittedSchemaIdentifier::admit(ctx, format!("SCHEMA_{i:04}"))
                    .expect("admission").expect("valid schema")).collect::<Vec<_>>();
            let index = schema_names_for_matching(&declarations, ctx).expect("index");
            for _ in 0..count {
                assert!(schema_identifier_matches(&index, &format!("SCHEMA_{:04}", count - 1), ctx).expect("lookup"));
            }
            let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "test schema work").expect_err("work probe") else {
                panic!("work refusal");
            };
            limit.used
        })
    };
    // Doubling declarations and queries permits linear visits and logarithmic lookup work.
    assert!(work(128) < 3 * work(64));
}

#[test]
fn unnamed_data_counts_declarations_instead_of_schema_index_keys() {
    for (schemas, expected) in [("'AP242 { 1 2 }'", true), ("'AP242 { 1 2 }','AP242 { 1 3 }'", false)] {
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({schemas}));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;");
        let result = with_service_context(source.as_bytes(), crate::parse::parse_inner);
        assert_eq!(result.is_ok(), expected, "{result:?}");
        if !expected {
            assert!(matches!(result, Err(crate::parse::ParseError::Syntax { message, .. })
                if message == "an unnamed DATA section requires one FILE_SCHEMA identifier"));
        }
    }
}

#[test]
fn schema_index_storage_is_scoped_until_its_last_lookup() {
    let setup = cadmpeg_test_support::service_decode_context();
    let declarations = [AdmittedSchemaIdentifier::admit(&setup, "AP242 { 1 2 }".into())
        .expect("admission").expect("schema")];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65_536;
    with_policy_context(&[], &policy, |_, ctx| {
        let (index, storage) = ctx.with_scoped_storage("test schema index", ||
            schema_names_for_matching(&declarations, ctx)).expect("temporary index");
        assert!(schema_identifier_matches(&index, "AP242", ctx).expect("lookup"));
        drop(index);
        drop(storage);
        ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released index")
            .expect("index and query storage released");
    });
}
