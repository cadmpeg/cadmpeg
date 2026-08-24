// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn decode_refuses_a_copious_tuple_count_over_its_projection_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(12, b"106,2,1000001;", "00000000")),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_copious_tuples")
                && limit.limit == 1_000_000
                && limit.used == 1_000_000
                && limit.additional == 1
    ));
}

#[test]
fn decode_projects_copious_linear_paths_with_segment_parameters() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,3,0,0,0,1,0,0,1,2,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.degree, 1);
    assert_eq!(path.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &path.knots, &path.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0))
    );
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn v4_one_tuple_linear_path_is_projected_as_its_authored_point() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                &[OwnedTestEntity {
                    entity_type: 106,
                    form: 11,
                    label: "V4PATH".into(),
                    status: "00000000",
                    parameters: "106,1,1,0,3,4;".into(),
                }],
                global_v4,
                &[(1, 1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(result.ir().model.curves.is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SourceDialectUnverified.kind()));
}

#[test]
fn v5_one_tuple_linear_path_keeps_the_later_minimum() {
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[OwnedTestEntity {
                    entity_type: 106,
                    form: 11,
                    label: "V5PATH".into(),
                    status: "00000000",
                    parameters: "106,1,1,0,3,4;".into(),
                }],
                global_v5,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.points.is_empty());
    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss
                .message
                .contains("linear paths require at least 2 tuple(s)")
    }));
}

#[test]
fn decode_preserves_coincident_segments_in_a_copious_linear_path() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,4,0,0,0,0,0,0,1,0,0,1,1,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.control_points[0], path.control_points[1]);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_crossing_segments_in_a_copious_linear_path() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,4,0,0,0,1,1,0,0,1,0,1,0,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.control_points.len(), 4);
    assert!(path.control_points[1].x > path.control_points[0].x);
    assert!(path.control_points[1].y > path.control_points[0].y);
    assert!(path.control_points[3].x > path.control_points[0].x);
    assert!(result.report().losses.is_empty());
}

#[test]
fn decode_closes_form_63_with_the_global_minimum_resolution() {
    for (gap, decoded) in [("0.000999", true), ("0.001", false), ("0.001001", false)] {
        let parameters = format!("106,1,4,0,0,0,1,0,0,1,{gap},0;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(copious_data_file(63, parameters.as_bytes(), "00000000")),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "{gap}"
        );
        if decoded {
            assert_eq!(result.ir().model.edges[0].tolerance, Some(0.001));
            assert_eq!(
                result.ir().model.edges[0].start,
                result.ir().model.edges[0].end
            );
            assert!(result.report().losses.is_empty());
            let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
            assert!(validation.is_ok(), "{:#?}", validation.findings);
        } else {
            assert!(result.report().losses.iter().any(|loss| loss
                .message
                .contains("endpoints disagree beyond the minimum resolution")));
        }
    }
}

#[test]
fn decode_rejects_a_form_63_non_endpoint_duplicate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                63,
                b"106,1,5,0,0,0,1,0,1,1,1,0,0,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("coincident non-endpoint points")));
}

#[test]
fn decode_rejects_form_63_self_intersections_without_duplicate_points() {
    for parameters in [
        b"106,1,5,0,0,0,1,1,0,1,1,0,0,0;".as_slice(),
        b"106,1,4,0,0,0,2,0,1,0,0,0;".as_slice(),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(copious_data_file(63, parameters, "00000000")),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(result.ir().model.curves.is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    }
}

#[test]
fn decode_rejects_a_copious_interpretation_that_disagrees_with_its_form() {
    let bytes = copious_data_file(11, b"106,2,2,0,0,0,1,0,0;", "00000000");
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.message
                .contains("interpretation flag disagrees with the entity form")
        })
        .expect("copious-data projection loss");
    let provenance = loss.provenance.as_ref().expect("Directory provenance");
    assert_eq!(provenance.format, "iges");
    assert_eq!(provenance.stream, "iges");
    assert_eq!(provenance.tag.as_deref(), Some("directory_entry:D1"));
    assert_eq!(bytes[provenance.offset as usize + 72], b'D');
    let transfer = &result.report().transfer_ledger.entries[0];
    assert_eq!(transfer.source, "D1");
    assert_eq!(
        transfer.note.as_deref(),
        Some("native record retained; semantic projection omitted with an attributed loss")
    );
}

#[test]
fn semantic_copious_projection_uses_entity_boundary_before_generic_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 106,
                    form: 11,
                    label: "COPIOUS".into(),
                    status: "00000000",
                    parameters: "106,1,2,0,0,0,1,9,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "P0".into(),
                    status: "00000000",
                    parameters: "116,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "P1".into(),
                    status: "00000000",
                    parameters: "116,1,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "P2".into(),
                    status: "00000000",
                    parameters: "116,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 1,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,1;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("tuple array is truncated")));

    let native = result.ir().native.namespace("iges").unwrap();
    let copious = &native.arenas["copious_data"][0];
    assert_eq!(copious.fields()["declared_tuple_count"], 2);
    assert_eq!(copious.fields()["tuples"].as_array().unwrap().len(), 2);
    let entity = native.arenas["entities"]
        .iter()
        .find(|record| record.fields()["directory_sequence"] == 1)
        .expect("copious entity");
    assert!(entity.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn strict_decode_reports_an_attributed_projection_loss_without_refusal() {
    let bytes = copious_data_file(11, b"106,2,2,0,0,0,1,0,0;", "00000000");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let result = IgesCodec.decode(&mut Cursor::new(bytes), &options).unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_separates_copious_points_vectors_and_presentation_forms() {
    let points = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                3,
                b"106,3,2,1,2,3,0,0,1,4,5,6,1,0,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(points.ir().model.points.len(), 2);
    assert_eq!(points.ir().model.vertices.len(), 2);
    let native = points.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["copious_data"].len(), 1);
    assert_eq!(
        native.arenas["copious_data"][0].fields()["tuples"][0][5],
        1.0
    );
    assert!(points.report().losses.is_empty());

    let witness = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(40, b"106,1,3,0,0,0,1,0,2,0;", "00000100")),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!witness.report().geometry_transferred);
    assert!(witness.ir().model.curves.is_empty());
    assert!(witness
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::DisplayDataNotProjected.kind()));
    assert_eq!(
        witness.report().transfer_ledger.entries[0].note.as_deref(),
        Some("native record retained; semantic projection omitted with an attributed loss")
    );
    let validation = cadmpeg_ir::validate_neutral(witness.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
