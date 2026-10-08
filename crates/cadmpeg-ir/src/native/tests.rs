// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use serde::Serialize;

use crate::diff;
use crate::examples::unit_cube;
use crate::native::NativeRecord;
use crate::validate::validate_neutral;

#[test]
fn native_conversion_resource_accessor_borrows_the_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::native::NativeConvertError::Arena {
        arena: "records".into(),
        source: Box::new(crate::native::NativeConvertError::WriteRecord {
            ordinal: 3,
            source: Box::new(crate::native::NativeConvertError::Resource(
                ctx.charge_work(1, "original borrowed refusal").unwrap_err(),
            )),
        }),
    };
    let crate::native::NativeConvertError::Arena { source, .. } = &error else {
        panic!("the fixture has an arena wrapper");
    };
    let crate::native::NativeConvertError::WriteRecord { source, .. } = source.as_ref() else {
        panic!("the fixture has a record wrapper");
    };
    let crate::native::NativeConvertError::Resource(CodecError::ResourceLimit(stored)) =
        source.as_ref()
    else {
        panic!("the zero-work policy must refuse");
    };
    assert!(std::ptr::eq(error.resource_limit().unwrap(), stored));
    assert_eq!(stored.operation, "original borrowed refusal");
    assert_eq!(ctx.resource_refusal().as_ref(), Some(stored));
}

#[test]
fn native_conversion_error_admission_preserves_refusals_and_formats_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();

    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (fused, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = fused.charge_work(1, "original work").unwrap_err()
    else {
        panic!("the zero-work policy must refuse");
    };
    let converted = crate::native::NativeConvertError::InvalidCollection("later detail".into())
        .into_codec_error_for_decode(&fused, "format native conversion error");
    let CodecError::ResourceLimit(converted) = converted else {
        panic!("the original fused refusal must remain a resource limit");
    };
    assert_eq!(converted, original);

    let mut source_policy = DecodePolicy::service();
    source_policy.limits.max_work_units = 0;
    let (source_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &source_policy).unwrap();
    let CodecError::ResourceLimit(nested) = source_ctx
        .charge_work(1, "nested conversion work")
        .unwrap_err()
    else {
        panic!("the nested source refusal must be a resource limit");
    };
    let (target, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let converted = crate::native::NativeConvertError::Arena {
        arena: "records".into(),
        source: Box::new(crate::native::NativeConvertError::Resource(
            CodecError::ResourceLimit(nested),
        )),
    }
    .into_codec_error_for_decode(&target, "format native conversion error");
    let CodecError::ResourceLimit(converted) = converted else {
        panic!("the nested refusal must remain a resource limit");
    };
    assert_eq!(converted, nested);
    assert_eq!(target.resource_refusal(), None);

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let converted = crate::native::NativeConvertError::InvalidCollection("detail".into())
        .into_codec_error_for_decode(&limited, "format native conversion error");
    let CodecError::ResourceLimit(formatted) = converted else {
        panic!("the diagnostic must respect the retained-byte budget");
    };
    assert_eq!(formatted.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(formatted.operation, "format native conversion error");
    assert_eq!(
        formatted.additional,
        u64::try_from("native collection is invalid: detail".len()).unwrap()
    );
    assert_eq!(limited.resource_refusal(), Some(formatted));

    let diagnostic = "native collection is invalid: detail";
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(diagnostic.len()).unwrap();
    let (exact, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let converted = crate::native::NativeConvertError::InvalidCollection("detail".into())
        .into_codec_error_for_decode(&exact, "format native conversion error");
    assert!(matches!(converted, CodecError::Malformed(message) if message == diagnostic));
}

#[test]
fn native_arena_lookup_admits_matches_and_misses_before_typed_reads() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let fixture = super::test_ctx();
    let mut namespace = super::NativeNamespace::default();
    namespace
        .set_arena(
            &fixture,
            "records",
            &[serde_json::json!({"id": "test:native:record#first"})],
        )
        .unwrap();

    for name in ["records", "missing"] {
        for lazy in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::TYPED_RECORD_READ_COUNT.with(|count| count.set(0));
            let error = if lazy {
                let mut records = namespace.arena_iter_as_for_decode::<serde_json::Value>(&ctx, name);
                let error = records.next().unwrap().unwrap_err();
                assert!(records.next().is_none());
                error
            } else {
                namespace.arena_as_for_decode::<serde_json::Value>(&ctx, name).unwrap_err()
            };
            super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 0));
            let CodecError::ResourceLimit(limit) = CodecError::from(error) else {
                panic!("arena lookup must preserve its resource refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "select typed native arena");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }
    let ctx = super::test_ctx();
    assert!(namespace.arena_as_for_decode::<serde_json::Value>(&ctx, "missing").unwrap().is_empty());
    assert!(namespace.arena_iter_as_for_decode::<serde_json::Value>(&ctx, "missing").next().is_none());
}

#[test]
fn native_arena_iterator_borrows_lookup_key_and_reads_only_requested_records() {
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Record {
        id: String,
    }

    let ctx = super::test_ctx();
    let mut namespace = super::NativeNamespace::default();
    namespace
        .set_arena(
            &ctx,
            "records",
            &[
                serde_json::json!({"id": "test:native:record#first"}),
                serde_json::json!({"id": "test:native:record#second", "unexpected": 7}),
            ],
        )
        .unwrap();
    super::TYPED_RECORD_READ_COUNT.with(|count| count.set(0));
    let mut records = namespace.arena_iter_as_for_decode::<Record>(&ctx, &String::from("records"));
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert_eq!(records.next().unwrap().unwrap().id, "test:native:record#first");
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 1));
    drop(records);
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 1));
    ctx.finish_session().unwrap();
}

#[test]
fn native_record_conversion_refuses_retained_limit_before_record_growth() {
    use std::cell::Cell;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde::ser::SerializeMap;

    struct CountedRecord<'a>(&'a Cell<usize>);

    impl Serialize for CountedRecord<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.set(self.0.get() + 1);
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("id", "test:native:record#first")?;
            map.serialize_entry("payload", "a retained string")?;
            map.end()
        }
    }

    let json = br#"{"id":"test:native:record#first","payload":"a retained string"}"#;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(json.len()).unwrap() - 1;
    policy.limits.max_retained_bytes +=
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<NativeRecord>());

    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let calls = Cell::new(0);
    let error = crate::native::arena_from(
        &limited,
        [Ok::<_, crate::native::NativeConvertError>(CountedRecord(
            &calls,
        ))],
    )
    .unwrap_err();
    assert_eq!(calls.get(), 1);
    let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("writer refusal must remain a resource limit")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "serialize native record");

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let records = crate::native::arena_from(
        &service,
        [Ok::<_, crate::native::NativeConvertError>(CountedRecord(
            &calls,
        ))],
    )
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(serde_json::to_vec(&records[0]).unwrap(), json);
}

#[test]
fn native_writer_refuses_before_serializing_later_field() {
    use std::cell::Cell;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde::ser::SerializeMap;

    struct LaterField<'a>(&'a Cell<usize>);

    impl Serialize for LaterField<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.set(self.0.get() + 1);
            serializer.serialize_str("later")
        }
    }

    struct Record<'a> {
        later: LaterField<'a>,
    }

    impl Serialize for Record<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("id", "test:native:record#first")?;
            map.serialize_entry("payload", &self.later)?;
            map.end()
        }
    }

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<NativeRecord>()) + 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let visits = Cell::new(0);
    let record = Record {
        later: LaterField(&visits),
    };
    let error = crate::native::arena_from(
        &limited,
        [Ok::<_, crate::native::NativeConvertError>(&record)],
    )
    .unwrap_err();
    assert_eq!(visits.get(), 0);
    assert!(matches!(cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let stored = crate::native::arena_from(
        &service,
        [Ok::<_, crate::native::NativeConvertError>(&record)],
    )
    .unwrap();
    assert_eq!(visits.get(), 1);
    assert_eq!(stored[0].field("payload"), Some(serde_json::json!("later")));
}

#[test]
fn native_arena_stable_sort_keeps_identity_ties_past_insertion_sort() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    // More records than the in-place insertion sort handles, out of order and
    // in tied pairs, so the stable sort orders them through its index scratch.
    let records = (0..24_usize)
        .map(|ordinal| {
            serde_json::json!({
                "id": format!("test:native:record#{:02}", 23 - ordinal / 2),
                "ordinal": ordinal,
            })
        })
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let sorted = crate::native::arena_from(
        &service,
        records
            .iter()
            .map(Ok::<_, crate::native::NativeConvertError>),
    )
    .unwrap();
    for (position, record) in sorted.iter().enumerate() {
        let ordinal = 22 - 2 * (position / 2) + position % 2;
        assert_eq!(record.field("ordinal"), Some(serde_json::json!(ordinal)));
    }
}

#[test]
fn native_arena_json_copy_refuses_retained_limit_before_materialization() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = serde_json::json!({
        "id": "test:native:record#first",
        "payload": "a retained string"
    });
    let needed = cadmpeg_core::decode::u64_from_index(serde_json::to_vec(&record).unwrap().len());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = needed - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = crate::native::NativeNamespace::default();
    let error = namespace
        .set_arena(&limited, "records", std::slice::from_ref(&record))
        .unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("native storage must preserve the resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "store native record");
    assert!(namespace.arenas().is_empty());

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, "records", std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(
        namespace.arena_as::<serde_json::Value>("records").unwrap(),
        vec![record]
    );
}

#[test]
fn native_arena_typed_load_refuses_retained_limit_before_reading() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = serde_json::json!({
        "id": "test:native:record#first",
        "payload": {"labels": ["one", "two"]}
    });
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut namespace = crate::native::NativeNamespace::default();
    namespace
        .set_arena(&service, "records", std::slice::from_ref(&record))
        .unwrap();

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<serde_json::Value>()) - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::TYPED_RECORD_READ_COUNT.with(|count| count.set(0));
    let error = namespace
        .arena_as_for_decode::<serde_json::Value>(&limited, "records")
        .unwrap_err();
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "load typed native record"
    ));
    assert_eq!(
        namespace
            .arena_as_for_decode::<serde_json::Value>(&service, "records")
            .unwrap(),
        vec![record]
    );
}

#[test]
fn native_arena_typed_load_refuses_collection_limit_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = serde_json::json!({"id": "test:native:record#first", "payload": "one"});
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut namespace = crate::native::NativeNamespace::default();
    namespace
        .set_arena(&service, "records", std::slice::from_ref(&record))
        .unwrap();

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::TYPED_RECORD_READ_COUNT.with(|count| count.set(0));
    let error = namespace
        .arena_as_for_decode::<serde_json::Value>(&limited, "records")
        .unwrap_err();
    super::TYPED_RECORD_READ_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "load typed native record"
    ));
}

#[test]
fn native_arena_name_refuses_retained_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index("records".len()) - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = crate::native::NativeNamespace::default();
    let error = namespace
        .set_arena(&limited, "records", &[serde_json::Value::Null; 0])
        .unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("native arena name must preserve the resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "retain native arena name");
    assert!(namespace.arenas().is_empty());

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, "records", &[serde_json::Value::Null; 0])
        .unwrap();
    assert_eq!(namespace.arenas()["records"].len(), 0);
}

#[test]
fn native_arena_installation_admits_the_actual_tree_insertion_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The real insert into this 100-entry map fits within this work limit.
    // The old (name length + 1) * (map length + 1) * 2 estimate refused
    // before the B-tree insertion had a chance to admit its own work.
    policy.limits.max_work_units = 5_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = crate::native::NativeNamespace::default();
    for index in 0..100 {
        namespace
            .arenas_mut()
            .insert(format!("existing-{index:03}"), Vec::new());
    }

    namespace
        .set_arena(&ctx, "n".repeat(30), &[] as &[serde_json::Value])
        .expect("the admitted B-tree insertion fits its work budget");
    assert_eq!(namespace.arenas().len(), 101);
    assert!(namespace.arenas().contains_key(&"n".repeat(30)));
    ctx.finish_session()
        .expect("arena installation did not exceed the caller's work budget");
}

#[test]
fn native_record_slot_refuses_collection_limit_before_json_materialization() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = serde_json::json!({"id": "test:native:record#first", "payload": "value"});
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let mut namespace = crate::native::NativeNamespace::default();
    for collection_limit in [1, 0] {
        policy.limits.max_collection_items = collection_limit;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        namespace.arenas_mut().clear();
        let error = namespace
            .set_arena(&limited, "records", std::slice::from_ref(&record))
            .unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
        else {
            panic!("native record storage must preserve the resource refusal")
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(
            limit.operation,
            if collection_limit == 0 {
                "store native record"
            } else {
                "serialize native record"
            }
        );
        assert_eq!(limited.resource_refusal(), Some(limit));
        assert!(namespace.arenas().is_empty());
    }

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, "records", std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(namespace.arenas()["records"].len(), 1);
}

#[test]
fn second_native_record_refuses_before_output_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let records = [
        serde_json::json!({"id": "test:native:record#second", "payload": 2}),
        serde_json::json!({"id": "test:native:record#first", "payload": 1}),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The first record charges its slot and one item for each of its two
    // canonical object members.
    policy.limits.max_collection_items = 3;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::native::arena_from(
        &limited,
        records
            .iter()
            .map(Ok::<_, crate::native::NativeConvertError>),
    )
    .unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("second native row must preserve its resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "store native record");

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let stored = crate::native::arena_from(
        &service,
        records
            .iter()
            .map(Ok::<_, crate::native::NativeConvertError>),
    )
    .unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].id(), "test:native:record#first");
    assert_eq!(stored[1].id(), "test:native:record#second");
}

#[test]
fn native_arena_slot_refuses_collection_limit_before_insert() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = crate::native::NativeNamespace::default();
    let error = namespace
        .set_arena(&limited, "records", &[serde_json::Value::Null; 0])
        .unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("native arena storage must preserve the resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "store native arena");
    assert!(namespace.arenas().is_empty());
}

#[test]
fn typed_native_read_errors_identify_the_arena_and_stored_record() {
    use crate::native::NativeConvertError;
    use std::error::Error;

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Record {
        id: crate::ids::Identity,
        value: u32,
    }

    let first = "test:native:record#first";
    let refused = "test:native:record#refused";
    let mut document = crate::CadIr::empty();
    document
        .native
        .namespace_mut("future")
        .set_arena(
            &crate::native::test_ctx(),
            "records",
            &[
                serde_json::json!({"id": first, "value": 7}),
                serde_json::json!({"id": refused, "value": "not an integer"}),
            ],
        )
        .unwrap();
    let document: crate::CadIr =
        serde_json::from_value(serde_json::to_value(document).unwrap()).unwrap();
    let namespace = document.native.namespace("future").unwrap();
    // The iterator borrows the stored arena key, so a temporary lookup name
    // does not have to survive the iterator.
    let mut records = namespace.arena_iter_as::<Record>(&String::from("records"));
    let record = records.next().unwrap().unwrap();
    assert_eq!(record.id.as_str(), first);
    assert_eq!(record.value, 7);
    let error = records.next().unwrap().unwrap_err();
    assert!(error.to_string().contains("records"));
    assert!(error.to_string().contains(refused));
    assert!(error.source().unwrap().source().is_some());
    let NativeConvertError::Arena { arena, source } = error else {
        panic!("arena context")
    };
    assert_eq!(arena, "records");
    let NativeConvertError::ReadRecord { id, source } = *source else {
        panic!("record context")
    };
    assert_eq!(id.as_str(), refused);
    assert!(source.is_data());
    assert!(records.next().is_none());
    assert!(namespace.arena_iter_as::<Record>("absent").next().is_none());
}

#[test]
fn typed_native_write_errors_identify_input_ordinal_without_replacing_the_arena() {
    use crate::native::{NativeConvertError, NativeNamespace};
    let good = serde_json::json!({"id": "test:native:record#first", "value": 7});
    let mut namespace = NativeNamespace::default();
    namespace
        .set_arena(
            &crate::native::test_ctx(),
            "records",
            std::slice::from_ref(&good),
        )
        .unwrap();
    let before = namespace.clone();
    let error = namespace
        .set_arena(
            &crate::native::test_ctx(),
            "records",
            &[good, serde_json::json!({"value": 8})],
        )
        .unwrap_err();
    assert!(error.to_string().contains("records"));
    assert!(error.to_string().contains("ordinal 1"));
    let NativeConvertError::Arena { arena, source } = error else {
        panic!("arena context")
    };
    assert_eq!(arena, "records");
    let NativeConvertError::WriteRecord { ordinal, source } = *source else {
        panic!("input ordinal")
    };
    assert_eq!(ordinal, 1);
    assert!(matches!(*source, NativeConvertError::MissingId));
    assert_eq!(namespace, before);

    let source_error = NativeConvertError::InvalidOwner("producer record at offset 42".into());
    let result = crate::native::arena_from(
        &crate::native::test_ctx(),
        [Err::<serde_json::Value, _>(source_error)],
    );
    assert!(
        matches!(result, Err(NativeConvertError::InvalidOwner(message)) if message == "producer record at offset 42")
    );
}

#[test]
fn native_loss_counts_carry_a_nonempty_arena_population() {
    let mut native = crate::native::Native::default();
    native
        .namespace_mut("future")
        .arenas_mut()
        .insert("empty".into(), Vec::new());
    native.namespace_mut("future").arenas_mut().insert(
        "records".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new("test:native:record#counted").expect("valid identity"),
            serde_json::Map::new(),
        )
        .unwrap()],
    );
    let counts = native.loss_counts();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].format, "future");
    assert_eq!(counts[0].kind, "records");
    assert_eq!(counts[0].count.get(), 1);
    let wire = serde_json::to_value(&counts[0]).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::native::LossCount>(wire.clone()).unwrap(),
        counts[0]
    );
    let mut empty = wire;
    empty["count"] = serde_json::json!(0);
    assert!(serde_json::from_value::<crate::native::LossCount>(empty).is_err());
}

#[test]
fn complete_document_refuses_duplicate_native_keys_at_every_depth() {
    let empty = serde_json::to_string(&crate::CadIr::empty()).unwrap();
    assert_eq!(empty.matches("\"native\":{}").count(), 1);
    let document = |native: &str| empty.replace("\"native\":{}", &format!("\"native\":{native}"));
    let control = r#"{"future":{"records":[{"id":"test:native:record#first","fields":{"name":"value"}}]},"another_future":{"different_records":[]}}"#;
    let wire = document(control);
    let admitted = crate::CadIr::from_json(&wire).unwrap();
    assert_eq!(
        serde_json::to_value(&admitted).unwrap(),
        serde_json::from_str::<serde_json::Value>(&wire).unwrap(),
    );
    for (native, duplicate) in [
        (
            r#"{"future":{"records":[]},"future":{"other":[]}}"#,
            "future",
        ),
        (r#"{"future":{"records":[],"records":[]}}"#, "records"),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","id":"test:native:record#second"}]}}"#,
            "id",
        ),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","name":1,"name":2}]}}"#,
            "name",
        ),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","fields":{"name":1,"name":2}}]}}"#,
            "name",
        ),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","fields":[{"name":1,"name":2}]}]}}"#,
            "name",
        ),
    ] {
        let error = crate::CadIr::from_json(&document(native)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("duplicate key {duplicate}")),
            "{error}"
        );
    }
}

#[test]
fn deeply_nested_native_values_survive_every_stored_record_reader() {
    #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Record {
        id: String,
        nested: serde_json::Value,
    }

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth =
        cadmpeg_core::decode::u64_from_index(super::MAX_NATIVE_NESTING_DEPTH + 2);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let mut nested = serde_json::json!({"value": "retained", "absent": null});
    for _ in 0..140 {
        nested = serde_json::Value::Array(vec![nested]);
    }
    let id = "test:native:record#deep";
    let fields = serde_json::Map::from_iter([("nested".to_owned(), nested.clone())]);
    let constructed = NativeRecord::new(
        crate::ids::Identity::new(id).expect("valid identity"),
        fields.clone(),
    )
    .unwrap();
    let typed = Record {
        id: id.into(),
        nested: nested.clone(),
    };
    assert_eq!(NativeRecord::from_typed(&typed).unwrap(), constructed);
    assert_eq!(constructed.to_typed::<Record>(&ctx).unwrap(), typed);

    let mut document = crate::CadIr::empty();
    document
        .native
        .namespace_mut("future")
        .arenas_mut()
        .insert("records".into(), vec![constructed]);
    let wire = serde_json::to_value(&document).unwrap();
    let admitted = serde_json::from_value::<crate::CadIr>(wire).unwrap();
    let record = &admitted.native.namespace("future").unwrap().arenas()["records"][0];
    assert_eq!(record.fields(), &fields);
    assert_eq!(record.field("nested"), Some(nested));
    assert_eq!(record.field("missing"), None);
    assert_eq!(record.field("id"), None);
    assert_eq!(record.to_typed::<Record>(&ctx).unwrap(), typed);
    assert_eq!(
        serde_json::to_string(&admitted).unwrap(),
        serde_json::to_string(&document).unwrap()
    );
}

/// The bound belongs to the value, so it is stated where a caller-owned map
/// enters the record. Every reader of a constructed record — `Serialize`,
/// `fields`, `field`, `to_typed` and `Drop` — then descends a bounded value.
#[test]
fn a_field_nested_past_the_native_bound_never_enters_a_record() {
    use crate::native::{NativeConvertError, MAX_NATIVE_NESTING_DEPTH};

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Record {
        id: String,
        nested: serde_json::Value,
    }

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth =
        cadmpeg_core::decode::u64_from_index(super::MAX_NATIVE_NESTING_DEPTH + 2);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let id = "test:native:record#bound";
    let chain = |containers: usize| {
        let mut nested = serde_json::json!(7);
        for _ in 0..containers {
            nested = serde_json::Value::Array(vec![nested]);
        }
        nested
    };
    let record = |nested: serde_json::Value| {
        NativeRecord::new(
            crate::ids::Identity::new(id).expect("valid identity"),
            serde_json::Map::from_iter([("nested".to_owned(), nested)]),
        )
    };

    let admitted = chain(MAX_NATIVE_NESTING_DEPTH);
    assert_eq!(
        record(admitted.clone())
            .unwrap()
            .to_typed::<Record>(&ctx)
            .unwrap(),
        Record {
            id: id.to_owned(),
            nested: admitted,
        }
    );

    let refused = chain(MAX_NATIVE_NESTING_DEPTH + 1);
    let error = record(refused.clone()).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "native record {id}: field nested nests deeper than \
             {MAX_NATIVE_NESTING_DEPTH} containers"
        )
    );
    let NativeConvertError::FieldNestsTooDeep { id: offered, field } = error else {
        panic!("field context")
    };
    assert_eq!(offered.as_str(), id);
    assert_eq!(field, "nested");

    // The wire reader builds through the same constructor, so a document
    // handed in as a value tree cannot state a deeper field either.
    let wire = serde_json::json!({"id": id, "nested": refused});
    let error = serde_json::from_value::<NativeRecord>(wire).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("nests deeper than {MAX_NATIVE_NESTING_DEPTH}")),
        "{error}"
    );
}

#[test]
fn native_identity_admission_is_shared_by_all_construction_paths() {
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'a str,
    }

    for id in [
        "",
        "f3d:record#1",
        "f3d:test:record#",
        "f3d:test:record#1 space",
        "f3d:test:record#1#2",
    ] {
        assert!(matches!(
            crate::ids::Identity::new(id),
            Err(crate::ids::IdentityError::InvalidId { .. })
        ));
        assert!(matches!(
            NativeRecord::from_typed(&Record { id }),
            Err(crate::native::NativeConvertError::InvalidIdentity(_))
        ));
        assert!(serde_json::from_value::<NativeRecord>(serde_json::json!({ "id": id })).is_err());
    }

    let id = "f3d:test:record#1";
    let record = NativeRecord::new(
        crate::ids::Identity::new(id).expect("valid identity"),
        serde_json::Map::from_iter([(
            "id".to_owned(),
            serde_json::Value::String("ignored".into()),
        )]),
    )
    .unwrap();
    let typed = NativeRecord::from_typed(&Record { id }).unwrap();
    let decoded =
        serde_json::from_value::<NativeRecord>(serde_json::to_value(&record).unwrap()).unwrap();
    let admitted = NativeRecord::from_identity(crate::ids::UnknownId::mint(id).unwrap(), []);
    assert_eq!(record, admitted);
    assert_eq!(record, typed);
    assert_eq!(record, decoded);
    assert_eq!(record.id(), id);
}

/// The infallible constructor takes fields that state their own shape, so its
/// records read back through the fallible one without measurement.
#[test]
fn flat_fields_build_a_record_no_reader_has_to_measure() {
    use crate::native::NativeField;

    let id = "step:test:drawing-target#1";
    let record = NativeRecord::from_identity(
        crate::ids::Identity::new(id).unwrap(),
        [
            ("source_id".to_owned(), NativeField::Text("#42".to_owned())),
            (
                "links".to_owned(),
                NativeField::TextList(vec!["step:test:target#0".to_owned()]),
            ),
            ("id".to_owned(), NativeField::Text("ignored".to_owned())),
        ],
    );
    assert_eq!(record.id(), id);
    assert_eq!(record.field("id"), None);
    assert_eq!(record.field("source_id"), Some(serde_json::json!("#42")));
    assert_eq!(
        record.field("links"),
        Some(serde_json::json!(["step:test:target#0"]))
    );
    assert_eq!(
        NativeRecord::new(
            crate::ids::Identity::new(id).expect("valid identity"),
            record.fields().clone()
        )
        .unwrap(),
        record,
        "the measured constructor admits what the flat one builds"
    );
}

#[test]
fn rejected_typed_identity_does_not_replace_an_existing_arena() {
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'a str,
    }

    let mut namespace = crate::native::NativeNamespace::default();
    namespace
        .set_arena(
            &crate::native::test_ctx(),
            "records",
            &[Record {
                id: "f3d:test:record#1",
            }],
        )
        .unwrap();
    let before = namespace.clone();
    assert!(namespace
        .set_arena(
            &crate::native::test_ctx(),
            "records",
            &[Record { id: "invalid" }]
        )
        .is_err());
    assert_eq!(namespace, before);
}

#[test]
fn native_records_use_own_ids_for_counts_diff_and_validation() {
    let left = unit_cube().expect("valid unit cube fixture");
    let mut right = left.clone();
    right.native.namespace_mut("f3d").arenas_mut().insert(
        "act_guids".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new("f3d:test:act-guid#0").expect("valid identity"),
            serde_json::Map::new(),
        )
        .expect("valid native identity")],
    );
    right.native.namespace_mut("sldprt").arenas_mut().insert(
        "configurations".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new("sldprt:test:configuration#0").expect("valid identity"),
            serde_json::Map::new(),
        )
        .expect("valid native identity")],
    );
    right
        .native
        .finalize_with(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");

    let result = diff(&left, &right);
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.f3d.act_guids")
            .unwrap()
            .added,
        ["f3d:test:act-guid#0"]
    );
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.sldprt.configurations")
            .unwrap()
            .added,
        ["sldprt:test:configuration#0"]
    );
    let report = validate_neutral(&right, Vec::new()).expect("resource allocation did not fail");
    assert_eq!(report.entity_counts["native.f3d.act_guids"], 1);
    assert_eq!(report.entity_counts["native.sldprt.configurations"], 1);
    assert!(report.is_ok(), "{:?}", report.findings);

    right
        .native
        .namespace_mut("sldprt")
        .arenas_mut()
        .get_mut("configurations")
        .unwrap()[0] = NativeRecord::new(
        crate::ids::Identity::new("f3d:test:act-guid#0").expect("valid identity"),
        serde_json::Map::new(),
    )
    .expect("valid native identity");
    right
        .native
        .finalize_with(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");
    assert!(validate_neutral(&right, Vec::new())
        .expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.message == "entity id is not globally unique"));
}

/// The streaming canonical serializer must render the byte-exact text of the
/// `serde_json::to_value` route it replaced for a record of finite numbers:
/// recursively sorted object keys, `f32` widened to `f64`, and externally
/// tagged enum forms. A non-finite number, which that route wrote as `null`,
/// is refused by its path.
#[test]
fn from_typed_matches_value_tree_canonical_text() {
    #[derive(Serialize)]
    enum CanonShape {
        Unit,
        Newtype(u32),
        Tuple(i8, bool),
        Struct { zulu: f64, alpha: Option<String> },
    }

    #[derive(Serialize)]
    struct Shape<'a> {
        id: &'a str,
        #[serde(flatten)]
        fields: &'a serde_json::Map<String, serde_json::Value>,
    }

    #[derive(Serialize)]
    struct CanonRecord {
        id: String,
        zulu: f64,
        alpha: Vec<f64>,
        nested: BTreeMap<String, Vec<CanonShape>>,
        keyed: std::collections::HashMap<u32, char>,
        wide: f32,
        text: String,
        gone: Option<u8>,
        none_at_all: Option<u8>,
    }

    let record = CanonRecord {
        id: "f3d:test:canon#0-\"quotes\"-\u{1F980}".into(),
        zulu: -0.0,
        alpha: vec![f64::MIN_POSITIVE, f64::MAX, 0.1, -1.5e300, 3.0],
        nested: BTreeMap::from([(
            "b\nkey".to_owned(),
            vec![
                CanonShape::Unit,
                CanonShape::Newtype(7),
                CanonShape::Tuple(-3, true),
                CanonShape::Struct {
                    zulu: f64::MIN,
                    alpha: Some("s".into()),
                },
            ],
        )]),
        keyed: std::collections::HashMap::from([(12, 'x')]),
        wide: 0.1_f32,
        text: "line\u{0}break\ttab".into(),
        gone: Some(9),
        none_at_all: None,
    };

    // The oracle is the replaced route itself: a `Value` tree flattened
    // behind a leading string `id`.
    let serde_json::Value::Object(mut fields) = serde_json::to_value(&record).unwrap() else {
        panic!("record serializes as an object");
    };
    let serde_json::Value::String(id) = fields.remove("id").unwrap() else {
        panic!("id serializes as a string");
    };
    let expected = serde_json::to_string(&Shape {
        id: &id,
        fields: &fields,
    })
    .unwrap();

    let native = NativeRecord::from_typed(&record).unwrap();
    assert_eq!(native.id(), id);
    assert_eq!(serde_json::to_string(&native).unwrap(), expected);
}

/// The refusal message a typed record holding a non-finite number converts to.
fn non_finite_refusal<T: Serialize>(record: &T) -> String {
    let error = crate::native::NativeNamespace::default()
        .set_arena(
            &crate::native::test_ctx(),
            "records",
            std::slice::from_ref(record),
        )
        .expect_err("a non-finite number has no native value");
    match cadmpeg_core::CodecError::from(error) {
        cadmpeg_core::CodecError::Malformed(message) => message,
        other => panic!("expected a malformed-record refusal, got {other}"),
    }
}

#[test]
fn a_nan_record_field_is_refused_by_its_path() {
    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        bounds: [f64; 3],
    }

    let message = non_finite_refusal(&Record {
        id: "test:native:face#0",
        bounds: [1.0, f64::NAN, 3.0],
    });
    assert!(
        message.contains("field bounds[1] holds a non-finite number"),
        "{message}"
    );
}

#[test]
fn an_infinite_nested_record_field_is_refused_by_its_path() {
    #[derive(Serialize)]
    enum Carrier {
        Bounds { corners: Vec<[f64; 2]> },
    }

    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        carrier: Carrier,
    }

    let message = non_finite_refusal(&Record {
        id: "test:native:face#0",
        carrier: Carrier::Bounds {
            corners: vec![[0.0, 0.0], [f64::INFINITY, 1.0]],
        },
    });
    assert!(
        message.contains("field carrier.Bounds.corners[1][0] holds a non-finite number"),
        "{message}"
    );
}

#[test]
fn a_non_finite_f32_record_field_is_refused_by_its_path() {
    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        wide: f32,
    }

    let message = non_finite_refusal(&Record {
        id: "test:native:face#0",
        wide: f32::NEG_INFINITY,
    });
    assert!(
        message.contains("field wide holds a non-finite number"),
        "{message}"
    );
}

#[test]
fn native_arena_charged_load_preserves_values_and_error_context() {
    #[derive(Debug, serde::Deserialize)]
    struct Record {
        id: crate::ids::Identity,
        value: u32,
    }

    use crate::native::{NativeConvertError, NativeNamespace};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let records = [
        serde_json::json!({"id": "test:native:record#first", "value": 7,
            "payload": [null, true, false, 1.25, "line\nquoted\"", {"nested": [1, 2]}]}),
        serde_json::json!({"id": "test:native:record#refused", "value": "invalid"}),
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut namespace = NativeNamespace::default();
    namespace.set_arena(&ctx, "records", &records).unwrap();
    assert_eq!(
        namespace
            .arena_as_for_decode::<serde_json::Value>(&ctx, "records")
            .unwrap(),
        records
    );

    let first = namespace.arenas()["records"][0]
        .to_typed::<Record>(&ctx)
        .unwrap();
    assert_eq!(first.id.as_str(), "test:native:record#first");
    assert_eq!(first.value, 7);
    let NativeConvertError::Arena { arena, source } = namespace
        .arena_as_for_decode::<Record>(&ctx, "records")
        .unwrap_err()
    else {
        panic!("arena error context");
    };
    assert_eq!(arena, "records");
    let NativeConvertError::ReadRecord { id, source } = *source else {
        panic!("record error context");
    };
    assert_eq!(id.as_str(), "test:native:record#refused");
    assert!(source.is_data());
}

#[test]
fn native_arena_index_sort_preserves_cycles_and_identity_ties() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    for suffixes in [
        ["e", "d", "c", "b", "a"],
        ["c", "a", "e", "b", "d"],
        ["c", "a", "c", "b", "a"],
        ["a", "a", "a", "a", "a"],
    ] {
        let records = suffixes.into_iter().enumerate().map(|(ordinal, suffix)| {
            Ok::<_, crate::native::NativeConvertError>(serde_json::json!({
                "id": format!("test:native:record#{suffix}"), "ordinal": ordinal,
            }))
        });
        let sorted = crate::native::arena_from(&ctx, records).unwrap();
        let mut expected = suffixes.into_iter().enumerate().collect::<Vec<_>>();
        expected.sort_by_key(|(_, suffix)| *suffix);
        for (record, (ordinal, suffix)) in sorted.iter().zip(expected) {
            assert_eq!(record.id(), format!("test:native:record#{suffix}"));
            assert_eq!(record.field("ordinal"), Some(serde_json::json!(ordinal)));
        }
    }
}

#[test]
fn native_arena_sort_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let record = serde_json::json!({"id":"test:native:record#first"});
    let error =
        crate::native::arena_from(&ctx, [Ok::<_, crate::native::NativeConvertError>(record)])
            .unwrap_err();
    assert!(matches!(cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn small_native_arena_sort_needs_no_scratch() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let records = [
        serde_json::json!({"id":"test:native:record#same","ordinal":1}),
        serde_json::json!({"id":"test:native:record#same","ordinal":2}),
        serde_json::json!({"id":"test:native:record#first","ordinal":3}),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let sorted = crate::native::arena_from(
        &ctx,
        records
            .iter()
            .map(Ok::<_, crate::native::NativeConvertError>),
    )
    .unwrap();
    assert_eq!(sorted[0].id(), "test:native:record#first");
    assert_eq!(sorted[1].field("ordinal"), Some(serde_json::json!(1)));
    assert_eq!(sorted[2].field("ordinal"), Some(serde_json::json!(2)));
}

#[test]
fn arena_write_error_refuses_before_allocating_its_box() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let name = "invalid";
    let first_storage = name.len()
        + 4 * std::mem::size_of::<super::NativeRecord>()
        + 3
        + std::mem::size_of::<super::NativeConvertError>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(first_storage).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::NativeNamespace::default()
        .set_arena(&ctx, name, &["bad"])
        .unwrap_err();
    assert!(
        matches!(cadmpeg_core::CodecError::from(error), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain native arena error"
            && limit.additional == u64::try_from(std::mem::size_of::<super::NativeConvertError>()).unwrap())
    );
    let error = super::NativeNamespace::default()
        .set_arena(&super::test_ctx(), name, &["bad"])
        .unwrap_err();
    assert!(matches!(error, super::NativeConvertError::Arena { .. }));
}

#[test]
fn typed_native_field_rewrite_preserves_session_refusals() {
    use crate::ids::BodyId;
    use crate::topology::{Body, BodyKind};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let body = Body {
        id: BodyId::mint("test:model:body#one").unwrap(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: Some("test:model:body#one".into()),
        color: None,
        visible: None,
    };
    let record = NativeRecord::from_typed(&body).unwrap();
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RetainedBytes,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("test covers reconstruction dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = record
            .rewrite_fields::<Body, _>(&ctx, |source| {
                ctx.copy_retained_text(source, "test native target")
            })
            .unwrap_err();
        let CodecError::ResourceLimit(limit) = CodecError::from(error) else {
            panic!("typed native refusal must stay outside serde");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
    let ctx = super::test_ctx();
    let fields = record
        .rewrite_fields::<Body, _>(&ctx, |source| {
            ctx.format_retained(
                format_args!(
                    "test:occurrence:{}",
                    source.strip_prefix("test:model:").unwrap()
                ),
                "test native target",
            )
        })
        .unwrap();
    assert_eq!(fields["name"], serde_json::json!("test:model:body#one"));
    assert!(!fields.contains_key("id"));
}

#[test]
fn native_field_rewrite_does_not_require_serde_reconstruction() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    use crate::topology::Body;
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::CodecError;

    struct FieldOwner;
    impl RewriteIdentities for FieldOwner {
        fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
            ctx: &DecodeContext<'_>,
            value: &mut serde_json::Value,
            map: &mut IdentityMap<'_, F>,
        ) -> Result<(), CodecError> {
            Body::rewrite_native_value(ctx, value, map)
        }
        fn visit_identity_references(
            &self,
            ctx: &DecodeContext<'_>,
            _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            ctx.charge_work(1, "test field owner")
        }
        fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
            self,
            ctx: &DecodeContext<'_>,
            _map: &mut IdentityMap<'_, F>,
        ) -> Result<Self, CodecError> {
            ctx.charge_work(1, "test field owner")?;
            Ok(self)
        }
    }
    let record = NativeRecord::new(
        crate::ids::Identity::new("test:model:body#one").unwrap(),
        serde_json::from_value(serde_json::json!({
            "kind": "solid",
            "regions": ["test:model:region#one"],
            "name": "test:model:region#one"
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = super::test_ctx();
    let fields = record
        .rewrite_fields::<FieldOwner, _>(&ctx, |id| {
            ctx.format_retained(
                format_args!(
                    "test:occurrence:{}",
                    id.strip_prefix("test:model:").unwrap()
                ),
                "test native identity",
            )
        })
        .unwrap();
    assert_eq!(
        fields["regions"],
        serde_json::json!(["test:occurrence:region#one"])
    );
    assert_eq!(fields["name"], serde_json::json!("test:model:region#one"));
    assert!(!fields.contains_key("id"));
    ctx.finish_session().unwrap();
}

#[test]
fn native_attribute_target_rewrite_preserves_every_wire_kind() {
    use crate::attributes::AttributeTarget;
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let ctx = super::test_ctx();
    for kind in [
        "body", "face", "shell", "loop", "coedge", "edge", "vertex", "document",
    ] {
        let mut value = serde_json::json!({"kind": kind});
        if kind != "document" {
            value["id"] = serde_json::json!(format!("test:model:{kind}#one"));
        }
        let expected: AttributeTarget = serde_json::from_value(value.clone()).unwrap();
        let remap = |id: &str| {
            ctx.format_retained(
                format_args!(
                    "test:occurrence:{}",
                    id.strip_prefix("test:model:").unwrap()
                ),
                "test native identity",
            )
        };
        let mut map = IdentityMap::new(&ctx, "test native identity", remap).unwrap();
        AttributeTarget::rewrite_native_value(&ctx, &mut value, &mut map).unwrap();
        map.finish(&ctx).unwrap();
        let expected = crate::schema::rewrite::identities(
            &ctx,
            "test typed attribute target",
            expected,
            remap,
        )
        .unwrap();
        assert_eq!(value, serde_json::to_value(expected).unwrap());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn native_field_copy_preserves_bits_and_borrowed_storage() {
    let record = NativeRecord::new(
        crate::ids::Identity::new("test:native:record#fields").unwrap(),
        serde_json::from_value(serde_json::json!({
            "text": "retained text",
            "unsigned": u64::MAX,
            "negative_zero": -0.0,
            "nested": [{"text": "nested text"}]
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(std::ptr::eq(record.fields(), record.fields()));
    let ctx = super::test_ctx();
    let copied = record.copy_fields(&ctx).unwrap();
    assert_eq!(&copied, record.fields());
    assert_ne!(
        copied["text"].as_str().unwrap().as_ptr(),
        record.fields()["text"].as_str().unwrap().as_ptr()
    );
    assert_eq!(copied["unsigned"].as_u64(), Some(u64::MAX));
    assert_eq!(
        copied["negative_zero"].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    ctx.finish_session().unwrap();
}

#[test]
fn native_field_copy_preserves_caller_resource_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let record = NativeRecord::new(
        crate::ids::Identity::new("test:native:record#fields").unwrap(),
        serde_json::from_value(serde_json::json!({"nested": [{"text": "retained"}]})).unwrap(),
    )
    .unwrap();
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = if dimension == ResourceDimension::MaterializedBytes {
            ctx.with_scoped_storage("native field copy scope", || record.copy_fields(&ctx))
                .unwrap_err()
        } else {
            record.copy_fields(&ctx).unwrap_err()
        };
        let CodecError::ResourceLimit(limit) = CodecError::from(error) else {
            panic!("native field copy must retain its resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}
