// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode contracts over synthetic packages.

use std::io::Cursor;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::ContainerSummary;

use crate::loss::EdrawingsLossCode;
use crate::test_support::{
    assembly_package, emodel, emodel_with, package_of, part_package, zip_of, zlib_stored,
};
use crate::EdrawingsCodec;

/// Signature of a ZIP central directory header.
const CENTRAL_SIGNATURE: &[u8] = b"PK";
/// Cuts at or below this length leave no complete local file header.
const MIN_HEADER_LEN: usize = 4;

fn inspect(bytes: &[u8]) -> Result<ContainerSummary, CodecError> {
    EdrawingsCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default())
}

fn decode_with(bytes: &[u8], options: &DecodeOptions) -> Result<DecodeResult, CodecError> {
    EdrawingsCodec
        .decode(&mut Cursor::new(bytes), options)
        .map_err(|failure| match failure {
            cadmpeg_ir::DecodeFailure::Codec(error) => error,
            other => panic!("unexpected decode failure: {other}"),
        })
}

fn decode(bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    decode_with(bytes, &DecodeOptions::default())
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
fn inspect_lists_every_member_with_its_role() {
    let summary = inspect(&assembly_package(b"body")).unwrap();

    assert_eq!(summary.format(), "edrawings");
    assert_eq!(summary.container_kind.as_str(), "zip");
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "edrawings:hsf-19-10");
    assert_eq!(primary.admission(), &Admission::Admitted);
    assert!(summary.losses.is_empty());

    let rows = summary
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.role))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("eModel", ContainerRole::ModelData),
            ("preview.jpg", ContainerRole::PreviewImage),
            ("OLE", ContainerRole::Opaque),
            ("scene.xml", ContainerRole::Auxiliary),
            ("materials.xml", ContainerRole::MaterialTexture),
            ("FileProperties.xml", ContainerRole::Properties),
        ]
    );
    let model = summary.entries.first().unwrap();
    assert_eq!(
        model.attributes.get("hsf_version").map(String::as_str),
        Some("19.10")
    );
}

#[test]
fn inspect_of_an_unknown_stream_version_is_residual_and_charged() {
    let member = emodel_with("20.00", &[], &zlib_stored(b"x"), &[]);
    let summary = inspect(&package_of(&member, &[])).unwrap();
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "edrawings:unknown");
    assert_eq!(primary.admission(), &Admission::Residual);
    assert_eq!(
        summary
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [EdrawingsLossCode::SourceDialectUnverified.kind()]
    );
}

#[test]
fn inspect_reports_a_model_member_whose_wrapper_does_not_read() {
    let summary = inspect(&package_of(b"not a stream", &[])).unwrap();
    let primary = summary.dialects().unwrap().primary();
    assert_eq!(primary.dialect().as_str(), "edrawings:unknown");
    let model = summary.entries.first().unwrap();
    assert!(model.attributes.contains_key("wrapper_error"));
}

#[test]
fn decode_classifies_the_package_and_reports_a_broken_stream_as_blocking() {
    let result = decode(&part_package(b"synthetic model stream")).unwrap();

    let source = result.ir().source.as_ref().unwrap();
    let primary = source.dialect().unwrap();
    assert_eq!(primary.dialect().as_str(), "edrawings:hsf-19-10");
    assert_eq!(primary.admission(), &Admission::Admitted);
    assert_eq!(
        source
            .attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("hsf_version", "19.10"),
            ("member_count", "4"),
            ("model_stream_bytes", "22"),
        ]
    );

    assert!(!result.report().geometry_transferred());
    assert_eq!(
        loss_codes(&result),
        [EdrawingsLossCode::ModelMalformed.kind()]
    );
    let note = result.report().losses.first().unwrap();
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Blocking);
    assert!(note.message.contains("at byte 5"), "{}", note.message);
    assert!(result.ir().model.bodies.is_empty());

    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn container_depth_decode_skips_the_model_stream() {
    let options = DecodeOptions {
        depth: DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    let result = decode_with(&part_package(b"body"), &options).unwrap();
    assert!(!result.report().geometry_transferred());
    assert!(loss_codes(&result).is_empty());
}

#[test]
fn an_unknown_stream_version_decodes_on_the_residual_path() {
    let member = emodel_with("20.00", &[], &zlib_stored(b"z"), &[0]);
    let result = decode(&package_of(&member, &[])).unwrap();
    let primary = result.ir().source.as_ref().unwrap().dialect().unwrap();
    assert_eq!(primary.dialect().as_str(), "edrawings:unknown");
    assert_eq!(
        loss_codes(&result),
        [
            EdrawingsLossCode::SourceDialectUnverified.kind(),
            EdrawingsLossCode::ModelNoGeometry.kind(),
        ]
    );
}

#[test]
fn bytes_after_the_zlib_stream_are_charged_unless_they_are_the_observed_tail() {
    let member = emodel_with("19.10", &[], &zlib_stored(b"z"), &[0, 1, 2]);
    let result = decode(&package_of(&member, &[])).unwrap();
    assert_eq!(
        loss_codes(&result),
        [
            EdrawingsLossCode::StreamTrailingBytes.kind(),
            EdrawingsLossCode::ModelNoGeometry.kind(),
        ]
    );
}

#[test]
fn a_truncated_zlib_stream_decodes_with_a_blocking_stream_loss() {
    let member = emodel(&[9_u8; 200]);
    let cut = member[..member.len() - 8].to_vec();
    let result = decode(&package_of(&cut, &[])).unwrap();
    assert_eq!(
        loss_codes(&result),
        [EdrawingsLossCode::StreamMalformed.kind()]
    );
    assert!(!result.report().geometry_transferred());
}

#[test]
fn an_expansion_past_the_session_cap_is_a_resource_refusal() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_decompressed_bytes_per_expand = 128;
    let error = decode_with(&part_package(&[3_u8; 4096]), &options).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(_)), "{error}");
}

#[test]
fn hostile_packages_are_errors_and_never_panics() {
    let not_edrawings = zip_of(&[("readme.txt", b"text")]);
    assert!(matches!(
        decode(&not_edrawings),
        Err(CodecError::WrongFormat(_))
    ));
    assert!(matches!(
        inspect(&not_edrawings),
        Err(CodecError::WrongFormat(_))
    ));

    let bad_wrapper = package_of(b"\xff\xff\xff\xff", &[]);
    assert!(decode(&bad_wrapper).is_err());
    let bad_tag = package_of(b";; HSF V19.10 \nQ", &[]);
    assert!(matches!(decode(&bad_tag), Err(CodecError::Malformed(_))));

    let package = part_package(b"body");
    for len in [0, 1, 4, 30, 40, 100, package.len() / 2, package.len() - 1] {
        let cut = package.get(..len).unwrap();
        let (decoded, inspected) = (decode(cut), inspect(cut));
        if len <= MIN_HEADER_LEN {
            assert!(decoded.is_err() && inspected.is_err(), "{len}");
        }
    }
    let empty = emodel_with("19.10", &[], &[], &[]);
    let result = decode(&package_of(&empty, &[])).unwrap();
    assert_eq!(
        loss_codes(&result),
        [EdrawingsLossCode::StreamMalformed.kind()]
    );
}

#[test]
fn members_recovered_from_local_headers_are_charged() {
    let package = part_package(b"z");
    let central = package
        .windows(CENTRAL_SIGNATURE.len())
        .position(|window| window == CENTRAL_SIGNATURE)
        .unwrap();
    let cut = package.get(..central).unwrap();
    let result = decode(cut).unwrap();
    assert_eq!(
        loss_codes(&result),
        [
            EdrawingsLossCode::ContainerDirectoryRecovered.kind(),
            EdrawingsLossCode::ModelNoGeometry.kind(),
        ]
    );
    let summary = inspect(cut).unwrap();
    assert_eq!(summary.losses.len(), 1);
}
