// SPDX-License-Identifier: Apache-2.0
//! Native attachment failure keeps decoded geometry and no attachment record.

use std::io::Cursor;

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::unknown::part::record_unreadable_kind;
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
    pinned: bool,
) -> Result<cadmpeg_ir::codec::DecodeResult, DecodeFailure> {
    let _injected = inject(point, error);
    let mut options = DecodeOptions::default();
    options.strictness.pinned = pinned;
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

fn charges_the_object_model(loss: &LossNote) -> bool {
    loss.code == record_unreadable_kind()
        && loss.severity == Severity::Error
        && loss
            .message
            .starts_with("part object-model stream=\"nx:container\" offset=0 length=")
}

/// The stub id of the part that a charge gives up.
fn charged_record(loss: &LossNote) -> &str {
    loss.message
        .split_once(" record=")
        .and_then(|(_, rest)| rest.split_once(": "))
        .map(|(id, _)| id)
        .expect("the charge names its record")
}

fn assert_attachment_given_up(result: &EditableDecodeResult) {
    let errors = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.severity == Severity::Error)
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 2, "{errors:#?}");
    assert_eq!(attachment_failures(result).len(), 1);
    assert_eq!(errors[0].code.namespace(), "nx");
    assert_eq!(errors[0].code.local_code(), "native.attachment-failed");
    assert!(
        errors[0].message.contains(INJECTED_MESSAGE),
        "{}",
        errors[0].message
    );
    assert!(charges_the_object_model(errors[1]), "{:#?}", errors[1]);
}

#[test]
fn attachment_failure_keeps_geometry_and_gives_up_the_attachment() {
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
    assert_attachment_given_up(&contained);
    assert_valid(&contained);
}

/// The ids that the charge on the container names as dropped with it.
fn dropped_with_the_container(loss: &LossNote) -> Vec<&str> {
    loss.message
        .split_once("dropped=[")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(ids, _)| ids.split(", ").filter(|id| !id.is_empty()).collect())
        .unwrap_or_default()
}

/// A failure after every write keeps the document of a failure before any
/// write. The charge on the container names each record the attachment wrote
/// before it failed, and only those.
#[test]
fn attachment_failure_after_every_write_equals_failure_before_any_write() {
    for fixture in [prt_with_arrangements, composed_feature_history_prt] {
        let before = decode_failing_at(fixture(), Point::Entry);
        let after = decode_failing_at(fixture(), Point::Exit);
        assert_eq!(after.ir(), before.ir());
        assert_eq!(after.source_fidelity(), before.source_fidelity());
        let (mut after_report, mut before_report) =
            (after.report().clone(), before.report().clone());
        let take_charge = |report: &mut cadmpeg_ir::report::decode::DecodeReport| {
            let index = report
                .losses
                .iter()
                .position(charges_the_object_model)
                .expect("the container is charged");
            report.losses.remove(index)
        };
        let (after_charge, before_charge) = (
            take_charge(&mut after_report),
            take_charge(&mut before_report),
        );
        assert_eq!(after_report, before_report);
        assert!(dropped_with_the_container(&before_charge).is_empty());
        let dropped = dropped_with_the_container(&after_charge);
        assert!(!dropped.is_empty(), "{after_charge:#?}");
        let unknowns = after.ir().native_unknowns("nx").expect("unknowns read");
        for id in dropped {
            assert!(
                unknowns.iter().all(|unknown| unknown.id.as_str() != id),
                "{id} is held"
            );
        }
    }
}

#[test]
fn the_attachment_charge_gives_up_the_container_bytes_as_a_part() {
    let bytes = prt_with_arrangements();
    let length = bytes.len();
    let contained = decode_failing_at(bytes, Point::Exit);
    let losses = &contained.report().losses;
    let charge = losses
        .iter()
        .find(|loss| charges_the_object_model(loss))
        .unwrap_or_else(|| panic!("the container is given up as a part: {losses:#?}"));
    assert!(
        charge.message.starts_with(&format!(
            "part object-model stream=\"nx:container\" offset=0 length={length} record="
        )),
        "{charge:#?}"
    );
    let stub = charged_record(charge);
    let unknowns = contained.ir().native_unknowns("nx").expect("unknowns read");
    assert!(unknowns.iter().any(|unknown| unknown.id.as_str() == stub));
    assert!(contained.source_fidelity().retained_record(stub).is_some());
    assert!(losses
        .iter()
        .all(|loss| !loss.message.contains("nx:container:object-model#0")));
}

#[test]
fn attachment_failure_without_geometry_keeps_the_container_decode() {
    let complete = decode(composed_feature_history_prt());
    assert!(!complete.ir().model.features.is_empty());

    let contained = decode_failing_at(composed_feature_history_prt(), Point::Exit);
    assert!(contained.ir().model.features.is_empty());
    assert!(contained.ir().geometry.faces.is_empty());
    assert_attachment_given_up(&contained);
}

/// A topology part whose Parasolid prologue states no schema token, so no
/// unverified kernel dialect refuses it under strict mode.
fn schemaless_topology_prt() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let schema = stream
        .windows(4)
        .position(|window| window == b"SCH_")
        .expect("fixture prologue states a schema token");
    stream[schema] = b'X';
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", zlib_compress(&stream))])
}

#[test]
fn strict_decode_refuses_a_contained_attachment_failure() {
    let mut strict = DecodeOptions::default();
    strict.strictness.pinned = true;
    NxCodec
        .decode(&mut Cursor::new(schemaless_topology_prt()), &strict)
        .expect("strict decode admits the fixture without an attachment failure");
    let salvaged = try_decode_failing_at(schemaless_topology_prt(), Point::Entry, malformed, false)
        .expect("salvage decode keeps the geometry");
    let salvaged = &salvaged.report().losses;
    assert!(salvaged
        .iter()
        .any(|loss| loss.code.local_code() == "native.attachment-failed"));
    assert!(salvaged.iter().any(charges_the_object_model));
    let error = try_decode_failing_at(schemaless_topology_prt(), Point::Entry, malformed, true)
        .expect_err("strict decode refuses the attachment failure");
    match error {
        DecodeFailure::StrictRejected { rejection } => {
            assert!(
                charges_the_object_model(rejection.loss()),
                "{:#?}",
                rejection.loss()
            );
            assert!(rejection
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "native.attachment-failed"
                    && loss.code.strict_floor().is_none()));
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn attachment_resource_refusal_still_refuses_the_decode() {
    let error = try_decode_failing_at(prt_with_arrangements(), Point::Exit, resource_limit, false)
        .expect_err("a resource refusal is not contained");
    assert!(
        matches!(error, DecodeFailure::Codec(CodecError::ResourceLimit(_))),
        "{error:?}"
    );
}
