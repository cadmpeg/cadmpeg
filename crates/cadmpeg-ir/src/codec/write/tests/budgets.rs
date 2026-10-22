// SPDX-License-Identifier: Apache-2.0
//! Real CADIR serialization admits exactly the records present on its wire.

use super::{CadirEncoder, CodecError, EncodeInput, Encoder, TargetRequest};
use crate::{ids::Identity, NativeRecord};
use cadmpeg_core::decode::ResourceDimension;
use std::fs::File;
use std::io::Write;

#[test]
fn cadir_exact_record_budget_preserves_bytes_across_all_serialized_namespaces() {
    let mut ir = crate::examples::unit_cube().expect("canonical cube fixture");
    for (namespace, arena, id) in [
        ("own", "settings", "own:native:setting#one"),
        ("foreign", "objects", "foreign:native:object#two"),
        ("foreign", "unknowns", "foreign:native:unknown#three"),
    ] {
        ir.native.namespace_mut(namespace).arenas_mut().insert(
            arena.into(),
            vec![
                NativeRecord::new(Identity::new(id).expect("identity"), serde_json::Map::new())
                    .expect("record"),
            ],
        );
    }
    ir.native
        .namespace_mut("empty")
        .arenas_mut()
        .insert("objects".into(), Vec::new());
    let mut expected = ir.to_canonical_json().expect("existing serializer");
    expected.push('\n');
    let wire: serde_json::Value = serde_json::from_str(&expected).expect("existing wire shape");
    let neutral: usize = wire["model"]
        .as_object()
        .expect("model")
        .values()
        .map(|arena| arena.as_array().expect("arena").len())
        .sum();
    let native: usize = wire["native"]
        .as_object()
        .expect("native")
        .values()
        .flat_map(|namespace| namespace.as_object().expect("namespace").values())
        .map(|arena| arena.as_array().expect("arena").len())
        .sum();
    assert_eq!(
        native, 3,
        "opaque stubs are still serialized records, not omission losses"
    );
    let count = cadmpeg_core::decode::u64_from_index(neutral + native);
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = count;
    let plan = CadirEncoder
        .plan(input, TargetRequest::Inherit)
        .expect("exact emitted record count fits");
    let mut actual = Vec::new();
    plan.write_to(&mut actual).expect("write planned CADIR");
    assert_eq!(actual, expected.as_bytes());
    input.limits.max_entities = count - 1;
    let error = CadirEncoder
        .plan(input, TargetRequest::Inherit)
        .expect_err("one fewer record is refused at emission");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource evidence must remain typed");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!(
        (limit.limit, limit.used, limit.additional),
        (count - 1, count - 1, 1)
    );
    assert_eq!(limit.operation, "serialize CADIR record");
}

#[test]
fn empty_cadir_uses_no_entity_budget_and_destination_io_keeps_its_kind() {
    let ir = crate::CadIr::empty();
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = 0;
    let plan = CadirEncoder
        .plan(input, TargetRequest::Inherit)
        .expect("no serialized arena records");
    let mut file =
        File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).expect("read-only manifest");
    let expected = file
        .write_all(b"x")
        .expect_err("read-only file cannot be written");
    let error = plan
        .write_to(&mut file)
        .expect_err("actual destination write fails");
    let CodecError::Io(actual) = error else {
        panic!("actual I/O error must remain Io");
    };
    assert_eq!(actual.kind(), expected.kind());
    assert_eq!(actual.raw_os_error(), expected.raw_os_error());
    assert_eq!(actual.to_string(), expected.to_string());
}
