// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::Vector3;

use super::{
    base_geometry_line_font_valid, base_geometry_use_flag_valid, declared_affine_progression,
    enforce_transform_depth, is_finite_nonzero_vector, validate_declared_transform_frame,
    DeclaredInterval, DeclaredTransformFrameError,
};
use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn point_display_symbol_targets_follow_the_declared_dialect() {
    assert!(super::point_display_symbol_type_allowed(
        408,
        GlobalTable::V4_0
    ));
    assert!(!super::point_display_symbol_type_allowed(
        308,
        GlobalTable::V4_0
    ));
    assert!(super::point_display_symbol_type_allowed(
        308,
        GlobalTable::V5_0
    ));
    assert!(!super::point_display_symbol_type_allowed(
        408,
        GlobalTable::V5_0
    ));
    assert!(super::point_display_symbol_type_allowed(
        308,
        GlobalTable::Legacy
    ));
    assert!(super::point_display_symbol_type_allowed(
        408,
        GlobalTable::Legacy
    ));
}

#[test]
fn entity_use_flag_six_is_admitted_only_by_the_later_profile() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let file = |global: &[u8]| {
        owned_test_file_with_global(
            &[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "CONSTR".into(),
                status: "00000600",
                parameters: "116,1,2,3,0;".into(),
            }],
            global,
        )
    };

    let v4 = IgesCodec
        .decode(&mut Cursor::new(file(global_v4)), &DecodeOptions::default())
        .unwrap();
    assert!(v4.ir().model.points.is_empty());
    assert!(v4.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("Entity Use Flag 06 is outside")
    }));

    let v5 = IgesCodec
        .decode(&mut Cursor::new(file(global_v5)), &DecodeOptions::default())
        .unwrap();
    assert_eq!(v5.ir().model.points.len(), 1);
    assert!(!v5.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("Entity Use Flag 06 is outside")
    }));
}

#[test]
fn base_geometry_line_font_follows_the_declared_dialect() {
    for entity_type in [
        100, 104, 108, 110, 112, 114, 118, 120, 122, 126, 128, 130, 140, 142, 144,
    ] {
        assert!(!base_geometry_line_font_valid(
            entity_type,
            0,
            0,
            GlobalTable::V4_0
        ));
        assert!(base_geometry_line_font_valid(
            entity_type,
            0,
            1,
            GlobalTable::V4_0
        ));
    }
    assert!(base_geometry_line_font_valid(106, 1, 0, GlobalTable::V4_0));
    assert!(base_geometry_line_font_valid(106, 3, 0, GlobalTable::V4_0));
    for form in [11, 12, 13, 63] {
        assert!(!base_geometry_line_font_valid(
            106,
            form,
            0,
            GlobalTable::V4_0
        ));
    }
    assert!(base_geometry_line_font_valid(116, 0, 0, GlobalTable::V4_0));
    assert!(base_geometry_line_font_valid(110, 0, 0, GlobalTable::V5_0));
}

#[test]
fn base_geometry_use_flag_follows_the_declared_dialect() {
    for use_flag in [0, 1, 2, 5] {
        assert!(base_geometry_use_flag_valid(
            110,
            0,
            use_flag,
            GlobalTable::V4_0
        ));
    }
    for use_flag in [3, 4] {
        assert!(!base_geometry_use_flag_valid(
            110,
            0,
            use_flag,
            GlobalTable::V4_0
        ));
    }
    assert!(base_geometry_use_flag_valid(110, 0, 3, GlobalTable::V5_0));
    assert!(!base_geometry_use_flag_valid(116, 0, 3, GlobalTable::V4_0));
    assert!(base_geometry_use_flag_valid(125, 0, 3, GlobalTable::V4_0));
}

#[test]
fn decode_rejects_a_zero_v4_base_geometry_line_font() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00000000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                }],
                GLOBAL_V4,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss
                .message
                .contains("Line Font must be nonzero for this IGES 4.0 geometry entity")
    }));
}

#[test]
fn decode_applies_v4_base_geometry_use_flag_03_by_dialect() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                &[OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00000300",
                    parameters: "110,0,0,0,1,0,0;".into(),
                }],
                global_v4,
                &[(1, 1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains(
                "Entity Use Flag 03 is outside the IGES 4.0 base geometry values 00, 01, 02, and 05",
            )
    }));

    let later = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                &[OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00000300",
                    parameters: "110,0,0,0,1,0,0;".into(),
                }],
                global_v5,
                &[(1, 1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(later.ir().model.curves.len(), 1);
    assert!(!later.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("base geometry values")
    }));
}

#[test]
fn point_display_symbol_pointer_targets_follow_the_declared_dialect() {
    fn file(global: &[u8], pointer: u32) -> Vec<u8> {
        owned_test_file_with_global(
            &[
                OwnedTestEntity {
                    entity_type: 308,
                    form: 0,
                    label: "DEF".into(),
                    status: "00000200",
                    parameters: "308,0,3HDEF,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 408,
                    form: 0,
                    label: "INST".into(),
                    status: "00000000",
                    parameters: "408,1,0,0,0,1;".into(),
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "POINT".into(),
                    status: "00000000",
                    parameters: format!("116,1,2,3,{pointer};"),
                },
            ],
            global,
        )
    }

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    let v4 = IgesCodec
        .decode(
            &mut Cursor::new(file(global_v4, 3)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v4
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.0 == "iges:model:point#D5"));
    assert!(!v4.report().losses.iter().any(|loss| {
        loss.message
            .contains("Type 116 display symbol pointer is invalid")
    }));

    let v5 = IgesCodec
        .decode(
            &mut Cursor::new(file(global_v5, 1)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v5
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.0 == "iges:model:point#D5"));
    assert!(!v5.report().losses.iter().any(|loss| {
        loss.message
            .contains("Type 116 display symbol pointer is invalid")
    }));

    for (global, pointer) in [(&global_v4[..], 1), (&global_v5[..], 3)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(file(global, pointer)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(result
            .ir()
            .model
            .points
            .iter()
            .any(|point| point.id.0 == "iges:model:point#D5"));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::DisplayDataNotProjected.kind()
                && loss
                    .message
                    .contains("Type 116 display symbol pointer is invalid")
        }));
    }
}

#[test]
fn type125_flash_forms_project_reference_points_and_retain_shape_parameters() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 125,
                    form: 0,
                    label: "FLASH0".into(),
                    status: "00000000",
                    parameters: "125,1,2,0,0,0,11;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 1,
                    label: "FLASH1".into(),
                    status: "00000000",
                    parameters: "125,3,4,10,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 2,
                    label: "FLASH2".into(),
                    status: "00000000",
                    parameters: "125,5,6,10,20,0.5,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 3,
                    label: "FLASH3".into(),
                    status: "00000000",
                    parameters: "125,7,8,30,10,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 125,
                    form: 4,
                    label: "FLASH4".into(),
                    status: "00000000",
                    parameters: "125,9,10,40,20,0.75,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 100,
                    form: 0,
                    label: "DEFINER".into(),
                    status: "00000000",
                    parameters: "100,0,0,0,1,0,0,1;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result
            .ir()
            .model
            .points
            .iter()
            .filter(|point| {
                matches!(
                    point.id.0.as_str(),
                    "iges:model:point#D1"
                        | "iges:model:point#D3"
                        | "iges:model:point#D5"
                        | "iges:model:point#D7"
                        | "iges:model:point#D9"
                )
            })
            .count(),
        5
    );
    for (sequence, x, y) in [
        (1, 1.0, 2.0),
        (3, 3.0, 4.0),
        (5, 5.0, 6.0),
        (7, 7.0, 8.0),
        (9, 9.0, 10.0),
    ] {
        let point = result
            .ir()
            .model
            .points
            .iter()
            .find(|point| point.id.0 == format!("iges:model:point#D{sequence}"))
            .unwrap();
        assert_eq!(point.position, cadmpeg_ir::math::Point3::new(x, y, 0.0));
    }
    let flashes = &result.ir().native.namespace("iges").unwrap().arenas["flashes"];
    assert_eq!(flashes.len(), 5);
    assert_eq!(flashes[0].fields()["form"], 0);
    assert_eq!(
        flashes[0].fields()["reference_entity"],
        "iges:entity:directory#11"
    );
    assert_eq!(flashes[2].fields()["dimension_1"], 10.0);
    assert_eq!(flashes[2].fields()["dimension_2"], 20.0);
    assert_eq!(flashes[2].fields()["rotation"], 0.5);
    assert!(
        result.report().losses.is_empty(),
        "{:?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn type125_flash_is_admitted_in_v4_and_v5() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for global in [global_v4.as_slice(), global_v5.as_slice()] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[OwnedTestEntity {
                        entity_type: 125,
                        form: 2,
                        label: "FLASH".into(),
                        status: "00000000",
                        parameters: "125,3,4,10,20,0.5,0;".into(),
                    }],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().model.points.len(), 1);
        assert_eq!(
            result.ir().native.namespace("iges").unwrap().arenas["flashes"].len(),
            1
        );
        assert!(!result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::EntityOutsideEnvelope.kind()
                || loss.code == IgesLossCode::EntityNotProjected.kind()
        }));
    }
}

#[test]
fn type125_form0_without_defining_entity_reports_display_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 125,
                form: 0,
                label: "FLASH".into(),
                status: "00000000",
                parameters: "125,1,2,0,0,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::DisplayDataNotProjected.kind()
            && loss
                .message
                .contains("Type 125 Form 0 has no defining entity pointer")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn transform_depth_overflow_is_a_structured_resource_refusal() {
    fn transform_entry(sequence: u32, transform: i64) -> crate::directory::DirectoryEntry {
        crate::directory::DirectoryEntry {
            source_offset: 0,
            sequence,
            entity_type: 124,
            parameter_start: 0,
            structure: 0,
            line_font: 0,
            level: 0,
            view: 0,
            transform,
            label_display: 0,
            status: crate::directory::Status {
                blank: 0,
                subordinate: 0,
                use_flag: 0,
                hierarchy: 0,
            },
            line_weight: 0,
            color: 0,
            parameter_line_count: 0,
            form: 0,
            reserved: [[b' '; 8]; 2],
            label: [b' '; 8],
            subscript: 0,
        }
    }

    let transform_count = 65_u32;
    let mut directory = (0..transform_count)
        .map(|index| {
            let sequence = 1 + index * 2;
            let transform = if index + 1 < transform_count {
                sequence + 2
            } else {
                0
            };
            transform_entry(sequence, i64::from(transform))
        })
        .collect::<Vec<_>>();
    directory.push(transform_entry(1 + transform_count * 2, 1));

    let error = enforce_transform_depth(&directory, None).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_transform_depth")
                && limit.limit == 64
                && limit.used == 64
                && limit.additional == 1
    ));
}

#[test]
fn decode_preserves_rational_bspline_weights_and_multiplicities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.weights, Some(vec![1.0, 0.5, 1.0]));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0 / 3.0, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_a_rational_declaration_with_equal_weights() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(equal_weight_rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityNotProjected.kind()
    );
}

#[test]
fn decode_rejects_inconsistent_type_126_planar_and_closed_flags() {
    for (flags, normal) in [
        ([1, 0, 0, 0], [1, 0, 0]),
        ([1, 1, 0, 0], [0, 0, 1]),
        ([0, 0, 0, 0], [0, 0, 0]),
    ] {
        let parameters = format!(
            "126,2,2,{},{},{},{},0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,{},{},{};",
            flags[0], flags[1], flags[2], flags[3], normal[0], normal[1], normal[2]
        );
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            result.ir().model.curves.is_empty(),
            "flags={flags:?}: losses={:?}",
            result.report().losses
        );
        assert_eq!(result.report().losses.len(), 1, "flags={flags:?}");
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::EntityNotProjected.kind(),
            "flags={flags:?}"
        );
    }
}

#[test]
fn decode_uses_strict_global_resolution_for_type_126_closed_flag() {
    for (endpoint, prop2, decoded) in [
        ("0.000999", 1, true),
        ("0.001", 0, true),
        ("0.001", 1, false),
        ("0.001001", 0, true),
    ] {
        let parameters =
            format!("126,1,1,1,{prop2},1,0,0,0,1,1,1,1,0,0,0,{endpoint},0,0,0,1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "endpoint={endpoint}, PROP2={prop2}"
        );
        assert_eq!(
            result.report().losses.is_empty(),
            decoded,
            "endpoint={endpoint}, PROP2={prop2}: {:?}",
            result.report().losses
        );
        if !decoded {
            assert_eq!(
                result.report().losses[0].code,
                IgesLossCode::EntityNotProjected.kind()
            );
        }
    }
}

#[test]
fn declared_transform_validation_separates_frame_and_handedness_invariants() {
    let intervals = |rows: [[f64; 3]; 3]| {
        std::array::from_fn::<_, 9, _>(|index| {
            DeclaredInterval::around(rows[index / 3][index % 3], 0.0)
        })
    };

    assert_eq!(
        validate_declared_transform_frame(
            intervals([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Ok(())
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            -1.0,
        ),
        Ok(())
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Err(DeclaredTransformFrameError::WrongDeterminant)
    );
    assert_eq!(
        validate_declared_transform_frame(
            intervals([[1.1, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            1.0,
        ),
        Err(DeclaredTransformFrameError::NotOrthonormal)
    );
}

#[test]
fn declared_intervals_prove_or_reject_an_affine_control_polygon() {
    assert!(declared_affine_progression(
        &[0.0, 1.0, 2.0, 3.0],
        &[0.0; 4]
    ));
    assert!(declared_affine_progression(
        &[0.0, 1.000_002, 2.000_004, 3.0],
        &[0.0, 5.0e-6, 5.0e-6, 0.0]
    ));
    assert!(!declared_affine_progression(
        &[0.0, 1.0, 2.2, 3.0],
        &[0.0; 4]
    ));
}

#[test]
fn type_123_accepts_a_finite_non_unit_direction() {
    assert!(is_finite_nonzero_vector(Vector3::new(2.0, -3.0, 4.0)));
    assert!(!is_finite_nonzero_vector(Vector3::new(0.0, 0.0, 0.0)));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let direction = &result.ir().native.namespace("iges").unwrap().arenas["directions"][0];
    assert_eq!(
        direction.fields()["components"],
        serde_json::json!([2.0, -3.0, 4.0])
    );
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityRetainedUnprojected.kind()
    );
}

#[test]
fn decode_treats_type_126_periodic_flag_as_evaluation_metadata() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(polynomial_nurbs_curve_file(
                b"126,2,2,1,0,1,1,0,0,0,1,1,1,1,1,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.is_empty());
    let CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry else {
        panic!("expected a NURBS carrier");
    };
    assert!(!nurbs.periodic);
}

#[test]
fn decode_rejects_type_126_without_required_normal_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(polynomial_nurbs_curve_file(
                b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityNotProjected.kind()
    );
}

#[test]
fn decode_accepts_omitted_type_126_normal_for_nonplanar_v4_and_v5() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let parameters = "126,3,1,0,0,1,0,0,0,1,2,3,3,1,1,1,1,0,0,0,1,0,0,1,1,0,0,1,1,0,3,,,;";

    for global in [&global_v4[..], &global_v5[..]] {
        let file = owned_test_file_with_global_and_line_fonts(
            &[OwnedTestEntity {
                entity_type: 126,
                form: 0,
                label: "NURBS".into(),
                status: "00000000",
                parameters: parameters.into(),
            }],
            global,
            &[(1, 1)],
        );
        let result = IgesCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), 1);
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    }
}

#[test]
fn decode_projects_a_bounded_polynomial_bspline_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.control_points.len(), 2);
    assert_eq!(nurbs.weights, None);
    assert!(!nurbs.periodic);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_degree_zero_polynomial_bspline_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(polynomial_nurbs_curve_file(
                b"126,0,0,0,1,1,0,0,1,1,1,2,3,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 0);
    assert_eq!(nurbs.knots, vec![0.0, 1.0]);
    assert_eq!(nurbs.control_points.len(), 1);
    assert_eq!(nurbs.weights, None);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_applies_declared_real_significance_to_polynomial_weights() {
    for (weights, decoded) in [
        ("1.,0.9999999", true),
        ("1.,0.99", false),
        ("1.D0,0.9999999D0", false),
    ] {
        let parameters = format!("126,1,1,1,0,1,0,0,0,1,1,{weights},0,0,0,2,0,0,0,1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "{weights}"
        );
        assert_eq!(result.report().losses.is_empty(), decoded, "{weights}");
        if decoded {
            let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) =
                &result.ir().model.curves[0].geometry
            else {
                panic!("expected a NURBS carrier");
            };
            assert_eq!(nurbs.weights, None);
        } else {
            assert!(result.report().losses[0]
                .message
                .contains("polynomial spline has unequal weights"));
        }
    }
}

#[test]
fn decode_clamps_bspline_parameter_range_within_declared_real_significance() {
    for (range_start, decoded) in [("0.12345695", true), ("0.12", false)] {
        let parameters =
            format!("126,1,1,1,0,1,0,0.123457,0.123457,1,1,1,1,0,0,0,2,0,0,{range_start},1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(polynomial_nurbs_curve_file(parameters.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.edges.len(),
            usize::from(decoded),
            "{range_start}"
        );
        if decoded {
            assert_eq!(
                result.ir().model.edges[0].param_range,
                Some([0.123_457, 1.0])
            );
            assert!(result.report().losses.is_empty());
        } else {
            assert!(result.report().losses[0]
                .message
                .contains("parameter range lies outside the spline knot domain"));
        }
    }
}

#[test]
fn decode_projects_a_counterclockwise_circular_arc() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(circular_arc_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    let cadmpeg_ir::geometry::CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a circle carrier");
    };
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        *ref_direction,
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(*radius, 1.0);
    assert_eq!(
        result.ir().model.edges[0].param_range,
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position == cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_rounded_transformed_circular_arc_frame() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1.0000049,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Circle { radius, .. } =
        &result.ir().model.curves[0].geometry
    else {
        panic!("expected a circle carrier");
    };
    assert!((*radius - 1.0).abs() < 1.0e-12);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_transform_roundoff_beyond_its_declared_precision() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1.0000051,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("not orthonormal within its declared numeric precision")
    }));
}

#[test]
fn decode_applies_declared_double_precision_to_transform_coefficients() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,.8D0,-.6000001D0,0,0,.6D0,.8D0,0,0,0,0,1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("not orthonormal within its declared numeric precision")
    }));
}

#[test]
fn decode_canonicalizes_a_rounded_left_handed_transform() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file_with_form(
                1,
                b"124,.7071068,-.7071068,0,0,.7071068,.7071068,0,0,0,0,-1,0;",
                b"100,0,0,0,1,0,0,1;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Circle { axis, radius, .. } =
        &result.ir().model.curves[0].geometry
    else {
        panic!("expected a circle carrier");
    };
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, -0.0, 1.0));
    assert_eq!(*radius, 1.0);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_arc_endpoints_within_model_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,16,0,0,16.000999;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Circle { radius, .. } =
        &result.ir().model.curves[0].geometry
    else {
        panic!("expected a circle carrier");
    };
    assert!((*radius - 16.0).abs() < 1.0e-12);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_arc_endpoints_beyond_model_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(transformed_circular_arc_file(
                b"124,1,0,0,0,0,1,0,0,0,0,1,0;",
                b"100,0,0,0,16,0,0,16.001001;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("arc start and terminate points have different radii")
    }));
}

#[test]
fn decode_projects_a_line_as_a_normalized_bounded_wire_edge() {
    let result = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.points.len(), 2);
    let cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } =
        &result.ir().model.curves[0].geometry
    else {
        panic!("expected a line carrier");
    };
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.6, 0.8, 0.0));
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 5.0]));
    assert_eq!(result.ir().model.shells[0].wire_edges.len(), 1);
    assert!(result.ir().model.shells[0].free_vertices.is_empty());
    assert_eq!(
        result.ir().model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .object_id,
        "D1"
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_semi_bounded_and_unbounded_line_domains_natively() {
    for form in [1, 2] {
        let result = IgesCodec
            .decode(&mut Cursor::new(line_file(form)), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), 1);
        assert!(result.ir().model.edges.is_empty());
        assert!(result.ir().model.bodies.is_empty());
        assert_eq!(
            result.ir().model.curves[0]
                .source_object
                .as_ref()
                .unwrap()
                .object_id,
            "D1"
        );
        assert!(result.report().losses.is_empty());
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(native.arenas["entities"][0].fields()["form"], form);
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_applies_nested_transforms_reflection_units_and_model_scale_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_transformed_point_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 0.0);
    assert_eq!(result.ir().model.points[0].position.y, 80.0);
    assert_eq!(result.ir().model.points[0].position.z, 60.0);
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["transformations"].len(),
        2
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
