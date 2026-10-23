// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode contracts over synthetic packages.

use std::io::Cursor;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::ContainerSummary;

use crate::loss::PvzLossCode;
use crate::test_support::{package, OL_08, OL_09, PVS_0301, PVS_0304};
use crate::PvzCodec;

fn inspect(bytes: &[u8]) -> ContainerSummary {
    PvzCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap()
}

fn decode_at(bytes: &[u8], depth: DecodeDepth) -> Result<DecodeResult, CodecError> {
    let options = DecodeOptions {
        depth,
        ..DecodeOptions::default()
    };
    PvzCodec
        .decode(&mut Cursor::new(bytes), &options)
        .map_err(|failure| match failure {
            cadmpeg_ir::DecodeFailure::Codec(error) => error,
            other => panic!("unexpected decode failure: {other}"),
        })
}

fn decode(bytes: &[u8]) -> DecodeResult {
    decode_at(bytes, DecodeDepth::default()).unwrap()
}

fn primary_dialect(result: &DecodeResult) -> &str {
    result
        .ir()
        .source
        .as_ref()
        .and_then(|source| source.dialect())
        .map(|matched| matched.dialect().as_str())
        .unwrap()
}

fn loss_codes(result: &DecodeResult) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect()
}

#[test]
fn inspect_lists_every_member_with_its_role_and_signature() {
    let bytes = package(&[
        ("model.pvs", PVS_0304),
        ("part_a.ol", OL_08),
        ("part_b.ol", OL_09),
        ("part_c.ol", b"no signature"),
        ("thumbnail.png", b"\x89PNG"),
    ]);
    let summary = inspect(&bytes);

    assert_eq!(summary.format(), "pvz");
    assert_eq!(summary.container_kind.as_str(), "zip");
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "pvz:pvs-0304");
    assert_eq!(primary.admission(), &Admission::Admitted);
    assert!(summary.losses.is_empty(), "{:?}", summary.losses);

    let rows = summary
        .entries
        .iter()
        .map(|entry| {
            (
                entry.name.as_str(),
                entry.role,
                entry.attributes.get("signature").map(String::as_str),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("model.pvs", ContainerRole::Document, Some("PV-PVS0304")),
            ("part_a.ol", ContainerRole::ModelData, Some("DIV-OL08BL")),
            ("part_b.ol", ContainerRole::ModelData, Some("DIV-OL09BL")),
            ("part_c.ol", ContainerRole::ModelData, Some("unrecognized")),
            ("thumbnail.png", ContainerRole::Other, None),
        ]
    );
}

#[test]
fn decode_classifies_the_package_and_reports_undecoded_content() {
    let bytes = package(&[("model.pvs", PVS_0301), ("model.ol", OL_09)]);
    let result = decode(&bytes);

    let source = result.ir().source.as_ref().unwrap();
    let primary = source.dialect().unwrap();
    assert_eq!(primary.dialect().as_str(), "pvz:pvs-0301");
    assert_eq!(primary.admission(), &Admission::Admitted);
    assert_eq!(
        source
            .attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("geometry_member_count", "1"),
            ("structure_member", "model.pvs")
        ]
    );

    assert!(!result.report().geometry_transferred());
    assert_eq!(
        loss_codes(&result),
        [PvzLossCode::PackageContentUndecoded.kind()]
    );
    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.faces.is_empty());

    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn a_container_decode_reports_no_content_loss() {
    let bytes = package(&[("model.pvs", PVS_0304), ("model.ol", OL_09)]);
    let result = decode_at(&bytes, DecodeDepth::Container).unwrap();

    assert!(result.report().container_only());
    assert!(loss_codes(&result).is_empty());
}

#[test]
fn an_unknown_structure_signature_is_residual_and_charged() {
    let bytes = package(&[("model.pvs", b"PV-PVS9999 body"), ("model.ol", OL_09)]);

    let summary = inspect(&bytes);
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "pvz:unknown");
    assert_eq!(primary.admission(), &Admission::Residual);
    assert!(summary
        .losses
        .iter()
        .any(|loss| loss.code == PvzLossCode::SourceDialectUnverified.kind()));

    let result = decode(&bytes);
    assert_eq!(primary_dialect(&result), "pvz:unknown");
    assert_eq!(
        loss_codes(&result),
        [
            PvzLossCode::SourceDialectUnverified.kind(),
            PvzLossCode::PackageContentUndecoded.kind(),
        ]
    );
}

#[test]
fn an_unreadable_central_directory_is_recovered_and_charged() {
    let bytes = package(&[("model.pvs", PVS_0304), ("model.ol", OL_09)]);
    let central = bytes
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .unwrap();
    let local_only = &bytes[..central];

    let result = decode(local_only);
    assert_eq!(primary_dialect(&result), "pvz:pvs-0304");
    assert_eq!(
        loss_codes(&result),
        [
            PvzLossCode::ContainerDirectoryRecovered.kind(),
            PvzLossCode::PackageContentUndecoded.kind(),
        ]
    );
}

#[test]
fn a_package_without_a_structure_member_is_the_wrong_format() {
    let bytes = package(&[("model.ol", OL_09)]);
    assert!(matches!(
        decode_at(&bytes, DecodeDepth::default()),
        Err(CodecError::WrongFormat(_))
    ));
}

#[test]
fn a_package_with_two_structure_members_is_malformed() {
    let bytes = package(&[
        ("a.pvs", PVS_0304),
        ("b.pvs", PVS_0304),
        ("model.ol", OL_09),
    ]);
    assert!(matches!(
        decode_at(&bytes, DecodeDepth::default()),
        Err(CodecError::Malformed(_))
    ));
}
