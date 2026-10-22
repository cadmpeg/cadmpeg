// SPDX-License-Identifier: Apache-2.0
//! Entity limits apply at the actual Rhino object and table emission boundaries.

use super::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::{ids::Identity, CadIr, NativeRecord};

fn points() -> CadIr {
    let mut ir = CadIr::empty();
    for (id, x) in [("test:model:point#one", 1.0), ("test:model:point#two", 2.0)] {
        ir.model.points.push(cadmpeg_ir::topology::Point::new(
            id.try_into().expect("point identity"),
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(x, 0.0, 0.0))
                .expect("finite point"),
            None,
        ));
    }
    ir
}

fn emit(ir: &CadIr, limit: u64, already_used: u64) -> Result<Vec<u8>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = limit;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    ctx.charge_entities(already_used, "earlier writer work")?;
    let mut bytes = Vec::new();
    super::super::write(&ctx, ir, RhinoArchiveVersion::V8, &mut bytes)?;
    ctx.finish_session()?;
    Ok(bytes)
}

#[test]
fn rhino_exact_table_and_object_budget_does_not_charge_omitted_native_records() {
    let mut ir = points();
    // Version property, units setting, default layer, and two point objects.
    let expected = emit(&ir, 5, 0).expect("five actual serialized records fit");
    ir.native.namespace_mut("foreign").arenas_mut().insert(
        "objects".into(),
        vec![NativeRecord::new(
            Identity::new("foreign:native:object#omitted").expect("identity"),
            serde_json::Map::new(),
        )
        .expect("record")],
    );
    assert_eq!(
        emit(&ir, 5, 0).expect("foreign native content is not emitted"),
        expected
    );
    let error = emit(&ir, 4, 0).expect_err("second point exceeds four records");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("original entity refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!((limit.limit, limit.used, limit.additional), (4, 4, 1));
    assert_eq!(limit.operation, "write Rhino object record");
    let written = RhinoCodec
        .decode(
            &mut std::io::Cursor::new(expected),
            &DecodeOptions::default(),
        )
        .expect("read bounded writer bytes");
    assert_eq!(written.ir().model.points.len(), 2);
}

#[test]
fn rhino_writer_retains_the_earlier_session_charges() {
    let error =
        emit(&points(), 5, 1).expect_err("five new records cannot reset one earlier charge");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("cumulative entity refusal");
    };
    assert_eq!((limit.limit, limit.used, limit.additional), (5, 5, 1));
    assert_eq!(
        emit(&points(), 6, 1).expect("one earlier plus five new records"),
        emit(&points(), 5, 0).expect("same output bytes")
    );
}

#[test]
fn public_rhino_plan_obeys_a_caller_limit_below_a_golden_record_count() {
    let source = RhinoCodec
        .decode(
            &mut std::io::Cursor::new(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/golden/fixtures/arc.3dm"
            ))),
            &DecodeOptions::default(),
        )
        .expect("public arc golden source");
    assert!(!source.ir().model.curves.is_empty());
    let mut input = EncodeInput::new(source.ir(), None);
    input.limits.max_entities = 0;
    let error = RhinoCodec
        .plan(input, TargetRequest::Explicit("rhino:archive-80"))
        .expect_err("no entity budget cannot emit the golden's records");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("real caller entity refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
}

#[test]
fn rhino_staged_write_preserves_the_actual_destination_io_error() {
    use std::fs::File;
    use std::io::Write;
    let mut file =
        File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).expect("read-only manifest");
    let expected = file.write_all(b"x").expect_err("read-only destination");
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let error = super::super::write(&ctx, &points(), RhinoArchiveVersion::V8, &mut file)
        .err()
        .expect("copying staged bytes to a read-only file fails");
    let CodecError::Io(actual) = error else {
        panic!("actual destination IO evidence");
    };
    assert_eq!(actual.kind(), expected.kind());
    assert_eq!(actual.raw_os_error(), expected.raw_os_error());
    assert_eq!(actual.to_string(), expected.to_string());
}

#[test]
fn rhino_coverage_read_uses_the_same_caller_entity_budget() {
    let ir = points();
    let mut input = EncodeInput::new(&ir, None);
    input.limits.max_entities = 5;
    let error = RhinoCodec
        .plan(input, TargetRequest::Explicit("rhino:archive-80"))
        .expect_err("five emitted records leave no budget for verification reads");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("coverage cannot reset or swallow the session budget");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!(limit.limit, 5);
    assert_eq!(limit.used, 5);
    assert!(limit.additional > 0);
    assert!(!limit.operation.starts_with("write Rhino"));
}
