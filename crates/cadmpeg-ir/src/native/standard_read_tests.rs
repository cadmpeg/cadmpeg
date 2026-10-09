// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{NativeConvertError, NativeNamespace, NativeRecord, MAX_NATIVE_NESTING_DEPTH};

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct NestedRecord {
    id: String,
    nested: Value,
}

fn namespace(fields: Map<String, Value>) -> Result<NativeNamespace, NativeConvertError> {
    let record = NativeRecord::new(crate::ids::Identity::new("test:native:record#standard")?, fields)?;
    let mut namespace = NativeNamespace::default();
    namespace.arenas_mut().insert("records".into(), vec![record]);
    Ok(namespace)
}

#[test]
fn standard_native_reads_cover_the_full_stored_nesting_bound() -> Result<(), Box<dyn Error>> {
    let mut nested = Value::Bool(true);
    for _ in 0..MAX_NATIVE_NESTING_DEPTH {
        nested = Value::Array(vec![nested]);
    }
    let namespace = namespace(Map::from_iter([("nested".into(), nested.clone())]))?;
    let expected = NestedRecord {
        id: "test:native:record#standard".into(),
        nested,
    };
    assert_eq!(namespace.arena_as::<NestedRecord>("records")?, vec![expected]);
    let mut records = namespace.arena_iter_as::<NestedRecord>("records");
    let read = records.next().ok_or("the stored record must be selected")??;
    let mut value = &read.nested;
    for _ in 0..MAX_NATIVE_NESTING_DEPTH {
        let Value::Array(items) = value else {
            panic!("every stored container must survive the read");
        };
        assert_eq!(items.len(), 1);
        value = &items[0];
    }
    assert_eq!(value, &Value::Bool(true));
    assert!(records.next().is_none());

    // The root record is a further container. Decode still applies the
    // caller's limit instead of inheriting the context-free stored bound.
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let Err(error) = namespace.arena_as_for_decode::<NestedRecord>(&ctx, "records") else {
        panic!("the caller's nesting limit must remain enforced");
    };
    let limit = error.resource_limit().ok_or("decode must report a resource refusal")?;
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, super::read::TYPED_READ);
    assert_eq!(limit.limit, policy.limits.max_recursion_depth);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
    Ok(())
}

#[test]
fn standard_native_reads_do_not_share_a_fused_decode_session() -> Result<(), Box<dyn Error>> {
    let namespace = namespace(Map::from_iter([("nested".into(), Value::Bool(true))]))?;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let Err(error) = namespace.arena_as_for_decode::<NestedRecord>(&ctx, "records") else {
        panic!("the zero-work caller must refuse arena selection");
    };
    let original = *error.resource_limit().ok_or("decode must report a resource refusal")?;
    assert_eq!(original.operation, "select typed native arena");
    assert_eq!(namespace.arena_as::<NestedRecord>("records")?.len(), 1);
    assert_eq!(ctx.resource_refusal(), Some(original));
    Ok(())
}

#[test]
fn standard_native_iterator_is_lazy_and_borrows_the_stored_arena_name() -> Result<(), Box<dyn Error>> {
    let mut namespace = namespace(Map::from_iter([("nested".into(), Value::Bool(true))]))?;
    namespace.arenas_mut().get_mut("records").ok_or("missing arena")?.push(NativeRecord::new(
        crate::ids::Identity::new("test:native:record#invalid")?,
        Map::from_iter([("unexpected".into(), Value::Null)]),
    )?);
    super::TYPED_RECORD_READ_COUNT.with(|count| count.set(0));
    let mut records = namespace.arena_iter_as::<NestedRecord>(&String::from("records"));
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert_eq!(records.next().ok_or("missing first record")??.nested, Value::Bool(true));
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 1));
    drop(records);
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 1));
    assert!(namespace.arena_iter_as::<NestedRecord>("absent").next().is_none());
    assert!(namespace.arena_as::<NestedRecord>("absent")?.is_empty());
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 1));
    Ok(())
}

#[test]
fn standard_native_collection_keeps_constructor_errors() -> Result<(), Box<dyn Error>> {
    #[derive(Debug)]
    struct Collection;
    impl TryFrom<Vec<NestedRecord>> for Collection {
        type Error = NativeConvertError;

        fn try_from(records: Vec<NestedRecord>) -> Result<Self, Self::Error> {
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].nested, Value::Bool(true));
            Err(NativeConvertError::InvalidCollection("constructor refusal".into()))
        }
    }

    let namespace = namespace(Map::from_iter([("nested".into(), Value::Bool(true))]))?;
    let Err(error) = namespace.arena_as_collection::<NestedRecord, Collection>("records") else {
        panic!("the collection constructor must be called");
    };
    assert!(matches!(error, NativeConvertError::InvalidCollection(message) if message == "constructor refusal"));
    Ok(())
}

#[test]
fn standard_native_read_errors_keep_decode_grammar_and_context() -> Result<(), Box<dyn Error>> {
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Record {
        id: String,
        count: u64,
    }
    // Both a type mismatch and an unknown field must keep the stored id and
    // arena. No diagnostic may copy the mismatched source text.
    for fields in [
        Map::from_iter([("count".into(), Value::String("private payload".into()))]),
        Map::from_iter([("count".into(), Value::from(7)), ("unexpected".into(), Value::Null)]),
    ] {
        let namespace = namespace(fields)?;
        let Err(standard) = namespace.arena_as::<Record>("records") else {
            panic!("the fixture must fail its typed grammar");
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
        let Err(decode) = namespace.arena_as_for_decode::<Record>(&ctx, "records") else {
            panic!("decode must enforce the same typed grammar");
        };
        assert_eq!(standard.to_string(), decode.to_string());
        assert!(!standard.to_string().contains("private payload"));
        let NativeConvertError::Arena { arena, source } = standard else {
            panic!("the standard error must identify its arena");
        };
        assert_eq!(arena, "records");
        let NativeConvertError::ReadRecord { id, source } = *source else {
            panic!("the standard error must identify its stored record");
        };
        assert_eq!(id.as_str(), "test:native:record#standard");
        assert!(source.is_data());
        assert_eq!(ctx.resource_refusal(), None);
    }
    // Read the fields on a valid value as well, so this type has no dead
    // production-shaped fields hidden behind failure-only fixtures.
    let valid = namespace(Map::from_iter([("count".into(), Value::from(7))]))?;
    let read = valid.arena_as::<Record>("records")?;
    assert_eq!(read[0].id, "test:native:record#standard");
    assert_eq!(read[0].count, 7);
    Ok(())
}

#[test]
fn standard_native_raw_value_keeps_the_stored_compact_json() -> Result<(), Box<dyn Error>> {
    #[derive(Debug, Deserialize)]
    struct Record {
        id: String,
        raw: Box<serde_json::value::RawValue>,
    }
    let namespace = namespace(Map::from_iter([(
        "raw".into(),
        serde_json::json!({"text": "a\nb", "values": [true, null, 7]}),
    )]))?;
    let read = namespace.arena_as::<Record>("records")?;
    assert_eq!(read[0].id, "test:native:record#standard");
    assert_eq!(read[0].raw.get(), r#"{"text":"a\nb","values":[true,null,7]}"#);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    let decoded = namespace.arena_as_for_decode::<Record>(&ctx, "records")?;
    assert_eq!(decoded[0].raw.get(), read[0].raw.get());
    Ok(())
}
