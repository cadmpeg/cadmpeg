// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    point_file_with_field, point_file_with_version_flag, report_code_count, resolve_global_fields,
    strict_options, valid_global_fields,
};
use crate::loss::IgesLossCode;
use crate::test_support::{point_file, point_file_with_global};
use crate::IgesCodec;

fn dialect_losses(report: &cadmpeg_ir::report::DecodeReport) -> usize {
    report_code_count(report, IgesLossCode::SourceDialectUnverified)
}

#[test]
fn version_flags_clamp_unrecognized_values() {
    for (value, declared, expected) in [
        ("-1", -1, 3),
        ("0", 0, 3),
        ("12", 12, 11),
        ("99", 99, 11),
        ("6", 6, 6),
    ] {
        let mut fields = valid_global_fields();
        fields[22] = value.into();
        let (parsed, _) = resolve_global_fields(&fields);
        assert_eq!(parsed.declared_version_flag(), declared);
        assert_eq!(parsed.effective_version_flag(), expected);
    }
}

#[test]
fn fixed_ascii_5_1_and_5_2_decode_under_the_supported_profile() {
    for (encoded_version, version_name) in [(b"09", "5.1"), (b"10", "5.2")] {
        let mut bytes = point_file();
        let version = bytes
            .windows(b",11,0,".len())
            .position(|window| window == b",11,0,")
            .unwrap();
        bytes[version + 1..version + 3].copy_from_slice(encoded_version);

        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(bytes.clone()),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(summary
            .notes
            .contains(&format!("iges_version={version_name}")));
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["iges_version"],
            version_name
        );
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(
            result.report().losses.is_empty(),
            "{version_name}: {:#?}",
            result.report().losses
        );
        assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn declared_versions_outside_the_verified_set_decode_with_a_dialect_loss() {
    for (flag, version_name) in [("6", "4.0"), ("3", "2.0")] {
        let bytes = point_file_with_version_flag(flag);

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap();
        let source = result.ir().source.as_ref().unwrap();
        assert_eq!(source.attributes["iges_version"], version_name);
        assert_eq!(source.attributes["iges_version_flag"], flag);
        assert_eq!(result.ir().model.points.len(), 1);
        assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::SourceDialectUnverified.kind()
        );

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &strict_options(false))
            .unwrap_err();
        match error {
            CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
                loss_code,
                IgesLossCode::SourceDialectUnverified.kind().as_str()
            ),
            other => panic!("expected a shared-gate strict refusal, got {other:?}"),
        }
    }
}

#[test]
fn a_clamped_version_flag_is_recorded_verbatim_and_charges_the_dialect_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_version_flag("99")),
            &DecodeOptions::default(),
        )
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "5.3");
    assert_eq!(source.attributes["iges_version_flag"], "99");
    assert_eq!(dialect_losses(result.report()), 1);
}

#[test]
fn a_verified_declared_version_charges_no_dialect_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_version_flag("11")),
            &DecodeOptions::default(),
        )
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "5.3");
    assert_eq!(source.attributes["iges_version_flag"], "11");
    assert_eq!(dialect_losses(result.report()), 0);
}

#[test]
fn a_container_only_decode_reports_the_dialect_loss_and_strict_admits_it() {
    let bytes = point_file_with_version_flag("6");
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &options)
        .unwrap();
    assert_eq!(dialect_losses(result.report()), 1);

    let strict = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(true))
        .unwrap();
    assert_eq!(dialect_losses(strict.report()), 1);
}

#[test]
fn a_legacy_version_with_no_maximum_line_width_decodes_and_strict_names_the_dialect() {
    let mut fields = valid_global_fields();
    fields[16] = String::new();
    fields[22] = "6".into();
    let mut global = fields.join(",");
    global.push(';');
    let bytes = point_file_with_global(global.as_bytes());

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 2, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 1);
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::LineWeightScaleUnavailable),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::SourceDialectUnverified.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn a_malformed_version_flag_clamps_to_the_default_and_charges_the_dialect_loss() {
    let bytes = point_file_with_field(22, "1Hx");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "2.0");
    assert_eq!(source.attributes["iges_version_flag"], "3");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 1);
    assert!(result.report().losses[0].message.contains("field 23"));

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::SourceDialectUnverified.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn an_absent_version_flag_reports_the_default_dialect() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_field(22, "")),
            &DecodeOptions::default(),
        )
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "2.0");
    assert_eq!(source.attributes["iges_version_flag"], "3");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 1);
}
