// SPDX-License-Identifier: Apache-2.0
//! Native attachment failure keeps decoded geometry and no attachment record.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_test_support::EditableDecodeResult;

use super::{assert_valid, decode};
use crate::native::attach_failure::{inject, Point};
use crate::test_support::test_bytes::zlib_compress;
use crate::test_support::test_prt::{
    composed_feature_history_prt, prt_with_arrangements, prt_with_named_payloads,
};
use crate::test_support::test_streams::topology_partition_stream;
use crate::NxCodec;

const INJECTED_MESSAGE: &str = "injected attachment failure";

fn malformed() -> CodecError {
    CodecError::malformed(INJECTED_MESSAGE)
}

fn resource_limit() -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        scope: cadmpeg_core::decode::LimitScope::for_dimension(ResourceDimension::Entities),
        dimension: ResourceDimension::Entities,
        reason: ResourceFailure::BudgetExceeded,
        limit: 1,
        used: 1,
        additional: 1,
        operation: "injected attachment refusal",
    })
}

fn decode_failing_at(bytes: Vec<u8>, point: Point) -> EditableDecodeResult {
    let _injected = inject(point, malformed);
    decode(bytes)
}

fn try_decode_failing_at(
    bytes: Vec<u8>,
    point: Point,
    error: fn() -> CodecError,
    mode: DecodeMode,
) -> Result<cadmpeg_ir::codec::DecodeResult, DecodeFailure> {
    let _injected = inject(point, error);
    let mut options = DecodeOptions::default();
    options.policy.mode = mode;
    NxCodec.decode(&mut Cursor::new(bytes), &options)
}

fn attachment_failures(result: &EditableDecodeResult) -> Vec<&LossNote> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "native.attachment-failed")
        .collect()
}

fn assert_one_error_loss(result: &EditableDecodeResult) {
    let errors = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.severity == Severity::Error)
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(attachment_failures(result).len(), 1);
    assert_eq!(errors[0].code.namespace(), "nx");
    assert_eq!(errors[0].code.local_code(), "native.attachment-failed");
    assert!(
        errors[0].message.contains(INJECTED_MESSAGE),
        "{}",
        errors[0].message
    );
}

#[test]
fn attachment_failure_keeps_geometry_and_reports_one_error_loss() {
    let complete = decode(prt_with_arrangements());
    assert!(attachment_failures(&complete).is_empty());
    assert!(!complete.ir().model.configurations.is_empty());
    assert!(!complete.ir().model.attributes.is_empty());
    assert!(!complete.ir().geometry.faces.is_empty());

    let contained = decode_failing_at(prt_with_arrangements(), Point::Exit);
    let (model, geometry, complete_geometry) = (
        &contained.ir().model,
        &contained.ir().geometry,
        &complete.ir().geometry,
    );
    assert_eq!(geometry.bodies, complete_geometry.bodies);
    assert_eq!(geometry.shells, complete_geometry.shells);
    assert_eq!(geometry.faces, complete_geometry.faces);
    assert_eq!(geometry.loops, complete_geometry.loops);
    assert_eq!(geometry.coedges, complete_geometry.coedges);
    assert_eq!(geometry.edges, complete_geometry.edges);
    assert_eq!(geometry.vertices, complete_geometry.vertices);
    assert_eq!(geometry.surfaces, complete_geometry.surfaces);
    assert_eq!(geometry.curves, complete_geometry.curves);
    assert!(model.configurations.is_empty());
    assert!(model.attributes.is_empty());
    assert!(model.features.is_empty());
    assert_one_error_loss(&contained);
    assert_valid(&contained);
}

#[test]
fn attachment_failure_after_every_write_equals_failure_before_any_write() {
    for fixture in [prt_with_arrangements, composed_feature_history_prt] {
        let before = decode_failing_at(fixture(), Point::Entry);
        let after = decode_failing_at(fixture(), Point::Exit);
        assert_eq!(after.ir(), before.ir());
        assert_eq!(after.report(), before.report());
        assert_eq!(after.source_fidelity(), before.source_fidelity());
    }
}

#[test]
fn attachment_failure_without_geometry_keeps_the_container_decode() {
    let complete = decode(composed_feature_history_prt());
    assert!(!complete.ir().model.features.is_empty());

    let contained = decode_failing_at(composed_feature_history_prt(), Point::Exit);
    assert!(contained.ir().model.features.is_empty());
    assert!(contained.ir().geometry.faces.is_empty());
    assert_one_error_loss(&contained);
}

fn topology_prt() -> Vec<u8> {
    prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        zlib_compress(&topology_partition_stream()),
    )])
}

#[test]
fn strict_decode_reads_and_reports_a_contained_attachment_failure() {
    let decoded =
        try_decode_failing_at(topology_prt(), Point::Entry, malformed, DecodeMode::Strict)
            .expect("the attachment-failure code pins no strict floor");
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.attachment-failed"));
}

#[test]
fn attachment_resource_refusal_still_refuses_the_decode() {
    let error = try_decode_failing_at(
        prt_with_arrangements(),
        Point::Exit,
        resource_limit,
        DecodeMode::Salvage,
    )
    .expect_err("a resource refusal is not contained");
    assert!(
        matches!(error, DecodeFailure::Codec(CodecError::ResourceLimit(_))),
        "{error:?}"
    );
}
