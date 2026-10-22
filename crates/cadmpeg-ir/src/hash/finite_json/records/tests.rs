// SPDX-License-Identifier: Apache-2.0
//! Only arena elements that the serializer actually emits consume entity budget.

use super::super::{admission::EncodeAdmission, write_canonical_json, CanonicalJsonError};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct Document {
    model: Model,
    native: BTreeMap<&'static str, BTreeMap<&'static str, Vec<Record>>>,
}
#[derive(Serialize)]
struct Model {
    points: Vec<Record>,
    #[serde(skip_serializing)]
    omitted: Vec<Record>,
}
#[derive(Serialize)]
struct Record {
    id: &'static str,
    nested_values: Vec<u8>,
}

#[test]
fn absent_fields_empty_arenas_and_nested_arrays_do_not_charge_records() {
    let doc = Document {
        model: Model {
            points: vec![Record {
                id: "test:model:point#one",
                nested_values: vec![1, 2, 3],
            }],
            omitted: vec![Record {
                id: "test:model:omitted#one",
                nested_values: vec![4],
            }],
        },
        native: [
            ("own", [("empty", Vec::new())].into_iter().collect()),
            ("empty", BTreeMap::new()),
        ]
        .into_iter()
        .collect(),
    };
    assert_eq!(doc.model.omitted.len(), 1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut output = Vec::new();
    write_canonical_json(EncodeAdmission(&ctx), &mut output, &doc)
        .expect("one emitted record exactly fits");
    assert_eq!(
        output,
        serde_json::to_vec_pretty(&doc).expect("reference serializer")
    );
    ctx.finish_session().expect("no hidden entity charge");
}

#[test]
fn refusal_stops_before_serializing_the_next_record_and_preserves_resource_evidence() {
    let doc = Document {
        model: Model {
            points: vec![Record {
                id: "test:model:point#one",
                nested_values: vec![],
            }],
            omitted: vec![],
        },
        native: [(
            "foreign",
            [(
                "records",
                vec![Record {
                    id: "foreign:native:record#two",
                    nested_values: vec![],
                }],
            )]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut output = Vec::new();
    let error = write_canonical_json(EncodeAdmission(&ctx), &mut output, &doc)
        .expect_err("second emitted record exceeds one");
    let CanonicalJsonError::Resource(cadmpeg_core::CodecError::ResourceLimit(limit)) = error else {
        panic!("original resource refusal must survive Serde");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!((limit.limit, limit.used, limit.additional), (1, 1, 1));
    assert_eq!(limit.operation, "serialize CADIR record");
    let text = String::from_utf8(output).expect("partial JSON is text");
    assert!(text.contains("test:model:point#one"));
    assert!(!text.contains("foreign:native:record#two"));
}
