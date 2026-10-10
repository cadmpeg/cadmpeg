// SPDX-License-Identifier: Apache-2.0
//! Visits stop when validation rejects a value.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::implementation_level::ImplementationLevel;
use crate::parse::{HeaderDataReferences, HeaderRecord, ValidationError, Value};
use crate::test_support::{with_policy_context, with_service_context};

fn work_before_invalid<T>(
    expected: &'static str,
    validate: impl FnOnce(&DecodeContext<'_>) -> Result<T, ValidationError>,
) -> u64 {
    with_service_context(&[], |_, ctx| {
        assert!(
            matches!(validate(ctx), Err(ValidationError::Invalid(message)) if message == expected)
        );
        let CodecError::ResourceLimit(refusal) = ctx
            .charge_work(u64::MAX, "test validation work")
            .expect_err("work probe refuses")
        else {
            panic!("work refusal required");
        };
        refusal.used
    })
}

fn header() -> Vec<HeaderRecord> {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        with_service_context(SOURCE, crate::parse::parse_inner).expect("valid header");
    exchange.header().to_vec()
}

#[test]
fn schema_validation_does_not_admit_the_suffix_after_an_invalid_identifier() {
    let work = |count| {
        let mut header = header();
        header[2].parameters = vec![Value::List(vec![Value::Omitted; count])];
        work_before_invalid(
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
            |ctx| crate::parse::validate_header(&header, ctx),
        )
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn population_validation_does_not_admit_the_suffix_after_an_invalid_section() {
    let work = |count| {
        let parameters = [
            Value::String(b"AP242".to_vec()),
            Value::String(b"x".to_vec()),
            Value::List(vec![Value::Omitted; count]),
        ];
        work_before_invalid("FILE_POPULATION has invalid parameters", |ctx| {
            crate::parse::admit_file_population(
                &parameters,
                &std::collections::BTreeSet::from([String::from("AP242")]),
                ImplementationLevel::Edition3Class3,
                ctx,
            )
        })
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn optional_header_validation_does_not_admit_rows_after_an_invalid_record() {
    let work = |count| {
        let mut header = header();
        header.extend((0..count).map(|_| HeaderRecord {
            name: "BAD".into(),
            parameters: Vec::new(),
            offset: 0,
        }));
        work_before_invalid("HEADER contains an unsupported entity", |ctx| {
            crate::parse::validate_header_sections(
                ImplementationLevel::Edition3Class3,
                &header,
                &std::collections::BTreeSet::from([String::from("AP242")]),
                ctx,
            )
        })
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn header_data_validation_does_not_admit_rows_after_an_unknown_reference() {
    let work = |count| {
        let references = (0..count)
            .map(|_| HeaderDataReferences::Section("missing".into()))
            .collect::<Vec<_>>();
        work_before_invalid(
            "header section reference names an unknown DATA section",
            |ctx| crate::parse::validate_header_data_references(ctx, &references, &BTreeSet::new()),
        )
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn population_data_validation_does_not_admit_sections_after_an_unknown_name() {
    let work = |count| {
        let sections = (0..count)
            .map(|index| format!("s{index:04}"))
            .collect::<BTreeSet<_>>();
        let references = [HeaderDataReferences::FilePopulation(sections)];
        work_before_invalid("FILE_POPULATION names an unknown DATA section", |ctx| {
            crate::parse::validate_header_data_references(ctx, &references, &BTreeSet::new())
        })
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn typed_classifiers_admit_each_wrapper_without_recharging_list_children() {
    for resource in [false, true] {
        let mut value = if resource {
            Value::Resource("a".into())
        } else {
            Value::ExternalReference(1)
        };
        for _ in 0..3 {
            value = Value::Typed("T".into(), Box::new(value));
        }
        let classify = |ctx: &DecodeContext<'_>, value: &Value| {
            if resource {
                crate::parse::contains_resource_value(ctx, value)
            } else {
                crate::parse::contains_class3_occurrence(ctx, value)
            }
        };
        let operation = if resource {
            "STEP typed resource value traversal"
        } else {
            "STEP typed class-3 occurrence traversal"
        };
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                with_policy_context(&[], &policy, |_, ctx| classify(ctx, &value))
            },
        );
        let mut policy = DecodePolicy::service();
        // One list child visit and three typed child visits reach the matching leaf.
        policy.limits.max_work_units = 4;
        with_policy_context(&[], &policy, |_, ctx| {
            assert!(classify(ctx, &Value::List(vec![value])).expect("four actual visits fit"));
        });
    }
}

#[test]
fn anchor_list_resolution_does_not_admit_children_after_a_cycle() {
    let work = |count| {
        let bindings = [(String::from("cycle"), Value::Resource("cycle".into()))]
            .into_iter()
            .collect();
        let value = Value::List(vec![Value::Resource("cycle".into()); count]);
        with_service_context(&[], |_, ctx| {
            let mut resolver = crate::parse::AnchorResolver::new(&bindings, ctx).expect("resolver");
            assert!(
                matches!(resolver.resolve_root(&value), Err(crate::parse::ResolveError::Syntax(message))
                if message == "cyclic anchor binding <cycle>")
            );
            let CodecError::ResourceLimit(refusal) = ctx
                .charge_work(u64::MAX, "test resolver work")
                .expect_err("work probe refuses")
            else {
                panic!("work refusal required");
            };
            refusal.used
        })
    };
    assert_eq!(work(1), work(1024));
}

#[test]
fn reference_worklist_preserves_per_visit_refusal() {
    let value = Value::List(vec![Value::Reference(1), Value::ExternalReference(2)]);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP value worklist traversal",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            with_policy_context(&[], &policy, |_, ctx| {
                crate::parse::references(&value, &mut Vec::new(), &mut Vec::new(), ctx)
                    .or_else(|error| Err(error.into_codec_error(ctx)?))
            })
        },
    );
    with_service_context(&[], |_, ctx| {
        let mut entities = Vec::new();
        let mut values = Vec::new();
        crate::parse::references(&value, &mut entities, &mut values, ctx)
            .expect("worklist completes");
        assert_eq!(entities, [1]);
        assert_eq!(values, [2]);
    });
}

#[test]
fn reference_materialization_does_not_admit_suffix_after_a_depth_refusal() {
    for count in [1, 1024] {
        let mut values = vec![Value::Typed("T".into(), Box::new(Value::Typed("T".into(), Box::new(Value::Integer(1)))))];
        values.extend((1..count).map(|_| Value::Integer(1)));
        let value = Value::List(values);
        let mut policy = DecodePolicy::service();
        // The list root, first child visit and typed node use three work units.
        // The typed node's child reaches the next depth gate before its work.
        policy.limits.max_work_units = 3;
        policy.limits.max_recursion_depth = 2;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let anchors = std::collections::BTreeMap::new();
            let mut resolver = crate::parse::ReferenceResolver::new(&[], &anchors, &ctx).expect("empty resolver");
            let error = resolver.resolve_value(&value, 0).expect_err("first child exceeds depth");
            let crate::parse::ResolveError::Resource(CodecError::ResourceLimit(refusal)) = error else {
                panic!("original depth refusal required");
            };
            assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
            assert_eq!(refusal.operation, "step_reference_expansion");
            assert_eq!(ctx.resource_refusal(), Some(refusal));
            drop(resolver);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
    }
}

#[test]
fn resolver_children_have_one_collection_admission_per_output_slot() {
    for value in [
        Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        Value::List(vec![Value::Typed("T".into(), Box::new(Value::Integer(1)))]),
        Value::List(vec![Value::List(vec![Value::Integer(1)])]),
    ] {
        // Two list slots, one list slot and one box, or one slot in each list.
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        with_policy_context(&[], &policy, |_, ctx| {
            let anchors = std::collections::BTreeMap::new();
            let mut resolver = crate::parse::AnchorResolver::new(&anchors, ctx).expect("resolver");
            assert_eq!(resolver.resolve_root(&value).expect("two output slots"), value);
        });
        with_policy_context(&[], &policy, |_, ctx| {
            let anchors = std::collections::BTreeMap::new();
            let mut resolver = crate::parse::ReferenceResolver::new(&[], &anchors, ctx).expect("resolver");
            assert_eq!(resolver.resolve_value(&value, 0).expect("two output slots"), value);
        });
    }
}

#[test]
fn anchor_memo_copies_admit_only_their_allocated_child_slots() {
    let anchors = std::collections::BTreeMap::from([
        (String::from("a"), Value::List(vec![Value::Integer(1), Value::Integer(2)])),
    ]);
    let mut policy = DecodePolicy::service();
    // First expansion, memo population, and memo retrieval allocate two slots each.
    // The memo table and expansion stack admit one item each.
    // Root values occupy no collection slot.
    policy.limits.max_collection_items = 8;
    with_policy_context(&[], &policy, |_, ctx| {
        let mut resolver = crate::parse::AnchorResolver::new(&anchors, ctx).expect("resolver");
        for _ in 0..2 {
            assert_eq!(resolver.resolve_root(&Value::Resource("a".into())).expect("memo copies fit"),
                Value::List(vec![Value::Integer(1), Value::Integer(2)]));
        }
    });
}
