// SPDX-License-Identifier: Apache-2.0
//! Required geometry fields, optional claims and source point dependency.
#![allow(clippy::unwrap_used)]
use crate::test_support::test_curves_and_surfaces::polynomial_nurbs_curve_file;
use crate::test_support::test_owned::{owned_test_file_with_global, OwnedTestEntity};
use crate::{loss::IgesLossCode, IgesCodec};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use std::io::Cursor;

#[test]
fn physically_dependent_point_retains_source_without_becoming_a_free_vertex() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[
        crate::test_support::test_owned::OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SUPPORT".into(),
            status: "00010000",
            parameters: "116,2,3,4,0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert!(result.ir().model.vertices.is_empty());
    let source = result.ir().model.points[0].source_object.as_ref().unwrap();
    assert_eq!(source.object_id.as_str(), "D1");
    assert_eq!(
        source.geometry_role,
        Some(cadmpeg_ir::SourceGeometryRole::Support)
    );
}

#[test]
fn unreadable_required_nurbs_pole_has_a_geometry_loss_and_refuses_strict_mode() {
    for literal in ["3Hbad", "", "1E999", "bad"] {
        let parameters = format!("126,1,1,0,0,1,0,0,0,1,1,1,1,{literal},0,0,2,0,0,0,1,0,0,1;");
        let bytes = polynomial_nurbs_curve_file(parameters.as_bytes());
        let recovered = IgesCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        assert!(recovered.ir().model.curves.is_empty(), "{literal}");
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::GeometryNotProjected.kind()));
        let mut strict = DecodeOptions::default();
        strict.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
        assert!(
            matches!(
                IgesCodec.decode(&mut Cursor::new(bytes), &strict),
                Err(DecodeFailure::StrictRejected { .. })
            ),
            "{literal}"
        );
    }
}

#[test]
fn optional_nurbs_literals_do_not_hide_readable_poles() {
    for (claim, normal) in [("1E999", "0,0,1"), ("bad", "0,0,1"), ("0", "bad,1E999,")] {
        let parameters = format!("126,1,1,{claim},0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,{normal};");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().model.curves.len(), 1, "{claim}, {normal}");
        assert!(result.ir().native.namespace("iges").unwrap().arenas()
            ["quarantined_parameter_records"]
            .is_empty());
    }
}

#[test]
fn spline_claim_recovery_is_rejected_by_strict_mode() {
    let parameters = b"126,1,1,2,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,0,0,1;";
    let input = polynomial_nurbs_curve_file(parameters);
    let recovered = IgesCodec
        .decode(&mut Cursor::new(&input), &DecodeOptions::default())
        .unwrap();
    assert_eq!(recovered.ir().model.curves.len(), 1);
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::SplineClaimRecovered.kind()));
    let mut strict = DecodeOptions::default();
    strict.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
    assert!(matches!(
        IgesCodec.decode(&mut Cursor::new(input), &strict),
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. })
    ));
}

#[test]
fn unusable_metadata_use_flag_does_not_report_a_geometry_drop() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let bytes = owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "POINT".into(),
                status: "00000000",
                parameters: "116,1,2,3,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 314,
                form: 0,
                label: "COLOR".into(),
                status: "00000600",
                parameters: "314,100,0,0;".into(),
            },
        ],
        global,
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::GeometryNotProjected.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("Entity Use Flag 06 is outside")));
}
