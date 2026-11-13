// SPDX-License-Identifier: Apache-2.0
//! NIST geometric-tolerance expectations from the CTC definitions workbook.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::pmi::{
    DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiDefinition, PmiQuantity,
    PmiTarget,
};

use crate::CreoCodec;

const MAGNITUDE_TOLERANCE: f64 = 1.0e-9;
const POSITION_NOMINAL_TOLERANCE: f64 = 0.01;
const PLANE_OFFSET_TOLERANCE: f64 = 1.0e-6;

#[test]
fn nist_ctc_01_transfers_geometric_tolerance_types_and_magnitudes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/creo/nist_ctc_01_asme1_rd_cr10030.prt.5");
    let bytes = std::fs::read(path).expect("read Creo specimen");
    let result = CreoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode Creo specimen");
    let tolerances: Vec<_> = result
        .ir()
        .model
        .pmi
        .iter()
        .filter_map(|pmi| {
            if let PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                ..
            } = &pmi.definition
            {
                Some((tolerance, magnitude.get()))
            } else {
                None
            }
        })
        .collect();
    let expected = [
        (GeometricToleranceKind::Position, 0.75, 2),
        (GeometricToleranceKind::Flatness, 0.2, 1),
        (GeometricToleranceKind::SurfaceProfile, 0.5, 1),
        (GeometricToleranceKind::SurfaceProfile, 1.25, 1),
        (GeometricToleranceKind::Perpendicularity, 1.5, 1),
    ];
    assert_eq!(tolerances.len(), 6);
    for (kind, magnitude, count) in expected {
        assert_eq!(
            tolerances
                .iter()
                .filter(|(actual_kind, actual)| {
                    **actual_kind == kind
                        && actual.quantity == PmiQuantity::Length
                        && (actual.value.get() - magnitude).abs() < MAGNITUDE_TOLERANCE
                })
                .count(),
            count,
            "{kind:?}, {magnitude}"
        );
    }
    for pmi in &result.ir().model.pmi {
        let PmiDefinition::GeometricTolerance {
            tolerance,
            magnitude,
            datum_system,
            ..
        } = &pmi.definition
        else {
            continue;
        };
        let expected: &[&str] = match tolerance {
            GeometricToleranceKind::Position => &["A", "B", "C"],
            GeometricToleranceKind::Flatness => &[],
            GeometricToleranceKind::SurfaceProfile
                if (magnitude.get().value.get() - 1.25).abs() < MAGNITUDE_TOLERANCE =>
            {
                &["A", "B", "C"]
            }
            GeometricToleranceKind::SurfaceProfile | GeometricToleranceKind::Perpendicularity => {
                &["A"]
            }
            _ => panic!("unexpected tolerance"),
        };
        let magnitude_mm = magnitude.get().value.get();
        if let PmiDefinition::GeometricTolerance { modifiers, .. } = &pmi.definition {
            assert_eq!(
                modifiers.iter().any(|modifier| modifier == "all_around"),
                *tolerance == GeometricToleranceKind::SurfaceProfile
                    && (magnitude_mm - 0.5).abs() < MAGNITUDE_TOLERANCE,
            );
        }
        if *tolerance == GeometricToleranceKind::SurfaceProfile
            && (magnitude_mm - 1.25).abs() < MAGNITUDE_TOLERANCE
        {
            assert!(result.ir().model.pmi.iter().any(|presentation| {
                matches!(&presentation.definition,
                    PmiDefinition::Presentation { text: Some(text), semantics, .. }
                    if text.contains("A↔B") && semantics.as_slice() == [pmi.id.clone()])
            }));
        }
        match (tolerance, pmi.targets.as_slice()) {
            (GeometricToleranceKind::Flatness, [face]) => {
                let (axis, offset) = target_plane(&result, face);
                assert_plane(axis, offset, [0.0, 0.0, 1.0], 0.0);
            }
            (GeometricToleranceKind::Perpendicularity, [face]) => {
                let (axis, offset) = target_plane(&result, face);
                assert_plane(axis, offset, [1.0, 0.0, 0.0], 400.0);
            }
            (GeometricToleranceKind::SurfaceProfile, [face])
                if (magnitude_mm - 1.25).abs() < MAGNITUDE_TOLERANCE =>
            {
                let (axis, offset) = target_plane(&result, face);
                assert_plane(axis, offset, [0.0, 1.0, 0.0], -225.0);
            }
            (GeometricToleranceKind::SurfaceProfile, [face]) => {
                let (axis, offset) = target_plane(&result, face);
                assert!(
                    axis[2].abs() < MAGNITUDE_TOLERANCE,
                    "hex side axis {axis:?}"
                );
                assert!((offset.abs() - 50.0).abs() < PLANE_OFFSET_TOLERANCE);
            }
            (GeometricToleranceKind::Position, targets) => {
                assert_slot_walls(&result, targets);
            }
            (kind, targets) => panic!("unexpected targets for {kind:?}: {targets:?}"),
        }
        let Some(system_id) = datum_system else {
            assert!(expected.is_empty());
            continue;
        };
        let system = result
            .ir()
            .model
            .pmi
            .iter()
            .find(|item| &item.id == system_id)
            .expect("datum system");
        let PmiDefinition::DatumSystem { references } = &system.definition else {
            panic!("datum system definition")
        };
        let actual: Vec<_> = references
            .as_slice()
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                assert_eq!(
                    usize::try_from(reference.precedence.get()).expect("precedence"),
                    index + 1
                );
                let datum = result
                    .ir()
                    .model
                    .pmi
                    .iter()
                    .find(|item| item.id == reference.datum)
                    .expect("referenced datum");
                let PmiDefinition::Datum { identification } = &datum.definition else {
                    panic!("datum definition")
                };
                identification.as_str()
            })
            .collect();
        assert_eq!(actual, expected);
    }
    let datum = |label: &str| {
        result
            .ir()
            .model
            .pmi
            .iter()
            .find(|item| {
                matches!(&item.definition, PmiDefinition::Datum { identification } if identification == label)
            })
            .expect("datum")
    };
    assert!(
        matches!(datum("A").targets.as_slice(), [PmiTarget::Face { face }] if face.as_str() == "creo:visibgeom:face#106")
    );
    for (label, center_x) in [("B", -325.0), ("C", 325.0)] {
        let [target] = datum(label).targets.as_slice() else {
            panic!("one datum {label} target")
        };
        let (origin, axis, radius) = target_cylinder(&result, target);
        assert!((axis[2].abs() - 1.0).abs() < MAGNITUDE_TOLERANCE);
        assert!((origin[0] - center_x).abs() < PLANE_OFFSET_TOLERANCE);
        assert!((origin[1] + 175.0).abs() < PLANE_OFFSET_TOLERANCE);
        assert!((radius - 17.5).abs() < PLANE_OFFSET_TOLERANCE);
    }
    let dimensions: Vec<_> = result
        .ir()
        .model
        .pmi
        .iter()
        .filter_map(|item| {
            if let PmiDefinition::Dimension(dimension) = &item.definition {
                Some(dimension)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(dimensions.len(), 10);
    let expected_dimensions = [
        (DimensionKind::Diameter, 35.0, 34.8, 35.2, true, 2),
        (DimensionKind::Diameter, 25.0, -0.15, 0.15, false, 1),
        (DimensionKind::Diameter, 35.0, -0.2, 0.0, false, 1),
        (DimensionKind::Diameter, 35.0, 0.0, 0.2, false, 1),
        (DimensionKind::Diameter, 20.0, -0.1, 0.05, false, 1),
        (DimensionKind::Diameter, 20.0, -0.05, 0.1, false, 1),
        (
            DimensionKind::Angular,
            std::f64::consts::PI / 3.0,
            -std::f64::consts::PI / 360.0,
            std::f64::consts::PI / 360.0,
            false,
            1,
        ),
    ];
    for (kind, nominal, lower, upper, limits, count) in expected_dimensions {
        let matches = dimensions
            .iter()
            .filter(|dimension| {
                if dimension.kind() != &kind
                    || !dimension.nominal().is_some_and(|value| {
                        (value.value.get() - nominal).abs() < MAGNITUDE_TOLERANCE
                    })
                {
                    return false;
                }
                match (limits, dimension.tolerance()) {
                    (
                        true,
                        Some(DimensionTolerance::Limits {
                            lower: actual_lower,
                            upper: actual_upper,
                        }),
                    )
                    | (
                        false,
                        Some(DimensionTolerance::PlusMinus {
                            lower: actual_lower,
                            upper: actual_upper,
                        }),
                    ) => {
                        (actual_lower.value.get() - lower).abs() < MAGNITUDE_TOLERANCE
                            && (actual_upper.value.get() - upper).abs() < MAGNITUDE_TOLERANCE
                    }
                    _ => false,
                }
            })
            .count();
        assert_eq!(matches, count, "{kind:?}, {nominal}, {lower}, {upper}");
    }
    for nominal in [85.0, 50.0] {
        assert!(dimensions.iter().any(|dimension| {
            dimension.kind() == &DimensionKind::Location
                && dimension.nominal().is_some_and(|value| {
                    (value.value.get() - nominal).abs() < POSITION_NOMINAL_TOLERANCE
                })
        }));
    }
    let mut targeted = 0;
    for item in &result.ir().model.pmi {
        let PmiDefinition::Dimension(dimension) = &item.definition else {
            continue;
        };
        if item.targets.is_empty() {
            continue;
        }
        targeted += 1;
        let nominal = dimension.nominal().expect("targeted nominal").value.get();
        match dimension.kind() {
            DimensionKind::Location => assert_slot_walls(&result, &item.targets),
            DimensionKind::Diameter => {
                for target in &item.targets {
                    let radius = target_cylinder_radius(&result, target);
                    assert!((2.0 * radius - nominal).abs() < PLANE_OFFSET_TOLERANCE);
                }
            }
            DimensionKind::Angular => {
                let [first, second] = item.targets.as_slice() else {
                    panic!("two angular targets")
                };
                let (first, _) = target_plane(&result, first);
                let (second, _) = target_plane(&result, second);
                let cosine: f64 = first.iter().zip(second).map(|(a, b)| a * b).sum();
                assert!((cosine.abs() - nominal.cos()).abs() < MAGNITUDE_TOLERANCE);
            }
            kind => panic!("unexpected targeted dimension {kind:?}"),
        }
    }
    assert_eq!(targeted, 10);
}

/// Assert the two opposed slot walls of the STEP twin's position targets:
/// faces 805 and 801 at x = -192.5 and -107.5, or faces 852 and 848 at
/// y = 25 and -25.
fn assert_slot_walls(result: &cadmpeg_ir::codec::DecodeResult, targets: &[PmiTarget]) {
    let [first, second] = targets else {
        panic!("two slot walls: {targets:?}")
    };
    let mut walls =
        [target_plane(result, first), target_plane(result, second)].map(|(axis, offset)| {
            let dominant = axis
                .iter()
                .position(|component| (component.abs() - 1.0).abs() < MAGNITUDE_TOLERANCE)
                .expect("axis-aligned wall");
            (dominant, offset * axis[dominant].signum())
        });
    walls.sort_by(|a, b| a.1.total_cmp(&b.1));
    let expected = match walls[0].0 {
        0 => [(0, -192.5), (0, -107.5)],
        1 => [(1, -25.0), (1, 25.0)],
        axis => panic!("unexpected wall axis {axis}"),
    };
    for ((axis, offset), (expected_axis, expected_offset)) in walls.into_iter().zip(expected) {
        assert_eq!(axis, expected_axis);
        assert!((offset - expected_offset).abs() < PLANE_OFFSET_TOLERANCE);
    }
}

fn target_plane(result: &cadmpeg_ir::codec::DecodeResult, target: &PmiTarget) -> ([f64; 3], f64) {
    let PmiTarget::Face { face } = target else {
        panic!("face target")
    };
    face_plane(result, face.as_str())
}

fn target_cylinder_radius(result: &cadmpeg_ir::codec::DecodeResult, target: &PmiTarget) -> f64 {
    target_cylinder(result, target).2
}

/// The origin, unit axis, and radius of a face target's cylindrical carrier.
fn target_cylinder(
    result: &cadmpeg_ir::codec::DecodeResult,
    target: &PmiTarget,
) -> ([f64; 3], [f64; 3], f64) {
    let PmiTarget::Face { face } = target else {
        panic!("face target")
    };
    let Some(cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cylinder(cylinder),
    )) = face_surface(result, face.as_str())
    else {
        panic!("cylindrical target carrier")
    };
    (
        <[f64; 3]>::from(cylinder.origin().get()),
        <[f64; 3]>::from(*cylinder.frame().axis().as_raw()),
        cylinder.radius().get(),
    )
}

fn face_surface<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    face: &str,
) -> Option<&'a cadmpeg_ir::geometry::SurfaceGeometry> {
    let model = &result.ir().model;
    let surface = &model
        .faces
        .iter()
        .find(|item| item.id.as_str() == face)?
        .surface;
    model
        .surfaces
        .iter()
        .find(|item| &item.id == surface)
        .map(|item| &item.geometry)
}

/// The unit axis and signed origin offset of a face's planar carrier.
fn face_plane(result: &cadmpeg_ir::codec::DecodeResult, face: &str) -> ([f64; 3], f64) {
    let Some(cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    )) = face_surface(result, face)
    else {
        panic!("planar target carrier")
    };
    let axis = <[f64; 3]>::from(*plane.frame().axis().as_raw());
    let origin = <[f64; 3]>::from(plane.origin().get());
    let offset = axis.iter().zip(origin).map(|(a, o)| a * o).sum();
    (axis, offset)
}

/// Compare a carrier with a STEP-twin plane up to orientation.
fn assert_plane(axis: [f64; 3], offset: f64, expected_axis: [f64; 3], expected_offset: f64) {
    let dot: f64 = axis.iter().zip(expected_axis).map(|(a, e)| a * e).sum();
    assert!((dot.abs() - 1.0).abs() < MAGNITUDE_TOLERANCE, "{axis:?}");
    assert!((offset * dot.signum() - expected_offset).abs() < PLANE_OFFSET_TOLERANCE);
}

#[test]
fn transfers_a_second_composite_segment_from_its_display_value() {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition};

    let mut neutral = b"\xe0\x00attributes\0\xf4\x05\xf8\x05\xf7\x1b\xfb\xe3\xe0\x0aname\0PART_NUMBER\0\xe0\x00value\0\xe2\xe0\x01type\0\x33\xe0\x0avalue(s_val)\0PART\0\xe0\x01ref_id\0\xf1\xf6\xe0\x01ref_type\0\x02\xe0\x0aref_name\0\xe1\xe0\x0aunit\0\xe1\xe0\x01designated\0\x01\xf1\xf7\x1b".to_vec();
    neutral.extend_from_slice(b"\xe3PTC_GTOL_TYPE\0\xe2\x33\xf7\x1fPOSITION\0\x2a\x03\xe1\xe1\x00");
    neutral.extend_from_slice(b"\xe3PTC_GTOL_PRIMARY_TOL\0\xe2\x32\xe0\x02value(d_val)\0\x2a\xf8\x00\xf1\xf7\x20\x2a\x03\xe1mm\0\x00");
    neutral.extend_from_slice(
        b"\xe3PTC_GTOL_2_TOL_STRING\0\xe2\x33\xe8\xe2\x8c\x800.3\0\x2a\x03\xe1\xe1\x00",
    );
    neutral.extend_from_slice(b"\xe3PTC_GTOL_1_DTM_PRIM_STRING\0\xe2\x33A-B\0\x2a\x03\xe1\xe1\x00");
    let elements = b"\xe0\x00annot_elems\0\xf2\xf8\x02\xf7\x05\xfb\xe3\xe0\x00AE_Header\0\xe2\xe0\x00AE_Item\0\xe2\xe0\x01AE_Id\0\x01\xe0\x01AE_Type\0\x02\xe0\x01AE_Attr\0\xf1\xf8\x02\x00\xe4\xe0\x00AE_Annot\0\xe2\xe0\x01Annot_Id\0\xf6\xe0\x01Annot_Type\0\xf6\xe0\x00AE_Appl\0\xe2\xe0\x01Appl_Id\0\xf6\xe0\x01Appl_Type\0\xf6\xe0\x00AE_Owner\0\xe2\xe0\x01Owner_Type\0\x03\xe0\x01Owner_Id\0\x01\xe0\x01Owner_Sub_Id\0\x01\xe0\x00AE_GenInfo\0\xe2\xe0\x00AE_trf\0\xe1\xf2\xf7\x05\xe3\xe2\xe2\x2a\x03\x00\xe4\xe2\x11\x20\xe2\xf6\xf6\xe2\x03\x01\x01\xe2\xe1";
    let source = crate::test_support::build_prt(
        "composite",
        &[("NeuPrtSld", neutral), ("BasFullData", elements.to_vec())],
    );
    let result = crate::CreoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthetic part");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::CreoLossCode::PmiAttributeUnitAssumed.kind()));
    let values: Vec<_> = result
        .ir()
        .model
        .pmi
        .iter()
        .filter_map(|pmi| {
            if let PmiDefinition::GeometricTolerance {
                tolerance: GeometricToleranceKind::Position,
                magnitude,
                ..
            } = &pmi.definition
            {
                Some(magnitude.get().value.get())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(values.len(), 2);
    for expected in [1.5, 0.3] {
        assert!(values
            .iter()
            .any(|value| (value - expected).abs() < MAGNITUDE_TOLERANCE));
    }
    let systems: Vec<_> = result
        .ir()
        .model
        .pmi
        .iter()
        .filter_map(|item| {
            if let PmiDefinition::DatumSystem { references } = &item.definition {
                Some(references)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(systems.len(), 1);
    let references = systems[0].as_slice();
    assert_eq!(references.len(), 2);
    assert!(references
        .iter()
        .all(|reference| reference.precedence.get() == 1 && reference.common_group == Some(1)));
    let labels: Vec<_> = references
        .iter()
        .map(|reference| {
            let datum = result
                .ir()
                .model
                .pmi
                .iter()
                .find(|item| item.id == reference.datum)
                .expect("datum");
            let PmiDefinition::Datum { identification } = &datum.definition else {
                panic!("datum definition")
            };
            identification.as_str()
        })
        .collect();
    assert_eq!(labels, ["A", "B"]);
}

#[test]
fn keeps_a_tag_separate_when_unparsed_rows_can_repeat_its_label() {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::pmi::{PmiDefinition, PmiTarget};

    let mut neutral = b"\xe0\x00attributes\0\xf4\x05\xf8\x04\xf7\x1b\xfb\xe3\xe0\x0aname\0PART_NUMBER\0\xe0\x00value\0\xe2\xe0\x01type\0\x33\xe0\x0avalue(s_val)\0PART\0\xe0\x01ref_id\0\xf1\xf6\xe0\x01ref_type\0\x02\xe0\x0aref_name\0\xe1\xe0\x0aunit\0\xe1\xe0\x01designated\0\x01\xf1\xf7\x1b".to_vec();
    neutral.extend_from_slice(b"\xe3PTC_GTOL_TYPE\0\xe2\x33\xf7\x1fPOSITION\0\x2a\x03\xe1\xe1\x00");
    neutral.extend_from_slice(b"\xe3PTC_GTOL_PRIMARY_TOL\0\xe2\x32\xe0\x02value(d_val)\0\x2a\xf8\x00\xf1\xf7\x20\x2a\x03\xe1mm\0\x00");
    neutral.extend_from_slice(b"\xe3PTC_GTOL_1_DTM_PRIM_STRING\0\xe2\x33A\0\x2a\x03\xe1\xe1\x00");
    let mut elements = b"\xe0\x00annot_elems\0\xf2\xf8\x02\xf7\x05\xfb\xe3\xe0\x00AE_Header\0\xe2\xe0\x00AE_Item\0\xe2\xe0\x01AE_Id\0\x01\xe0\x01AE_Type\0\x02\xe0\x01AE_Attr\0\xf1\xf8\x02\x00\xe4\xe0\x00AE_Annot\0\xe2\xe0\x01Annot_Id\0\xf6\xe0\x01Annot_Type\0\xf6\xe0\x00AE_Appl\0\xe2\xe0\x01Appl_Id\0\xf6\xe0\x01Appl_Type\0\xf6\xe0\x00AE_Owner\0\xe2\xe0\x01Owner_Type\0\x03\xe0\x01Owner_Id\0\x01\xe0\x01Owner_Sub_Id\0\x01\xe0\x00AE_GenInfo\0\xe2\xe0\x00AE_trf\0\xe1\xf2\xf7\x05\xe3\xe2\xe2\x2a\x03\x00\xe4\xe2\x11\x20\xe2\xf6\xf6\xe2\x03\x01\x01\xe2\xe1".to_vec();
    elements.extend_from_slice(&synthetic_datum_definition_array());
    elements.extend_from_slice(&synthetic_datum_tag_array_with_unread_duplicate());
    elements.extend_from_slice(b"\xe0\x00drf_data_xar\0\xe1");

    let source = crate::test_support::build_prt(
        "datum-tag-collision",
        &[("NeuPrtSld", neutral), ("BasFullData", elements)],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthetic datum-tag collision");
    let datum_a: Vec<_> = result
        .ir()
        .model
        .pmi
        .iter()
        .filter(|item| {
            matches!(&item.definition, PmiDefinition::Datum { identification } if identification == "A")
        })
        .collect();
    assert_eq!(datum_a.len(), 2);
    assert!(datum_a.iter().any(|item| {
        item.id.as_str() == "creo:annotation:pmi#datum:A" && item.targets.is_empty()
    }));
    assert!(datum_a.iter().any(|item| {
        item.id
            .as_str()
            .starts_with("creo:annotation:pmi#tag:BasFullData:")
            && item.targets.is_empty()
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::CreoLossCode::PmiDatumDefinitionSlotAssumed.kind()
    }));
    assert!(!datum_a.iter().any(|item| {
        item.targets
            .iter()
            .any(|target| matches!(target, PmiTarget::Face { .. }))
    }));
}

fn synthetic_datum_array(
    name: &[u8],
    count: u8,
    class_id: u8,
    nulls: usize,
    prototype: &[u8],
) -> Vec<u8> {
    let mut bytes = b"\xe0".to_vec();
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&[0, 0xf2, 0xf8, count, 0xf7, class_id, 0xfb]);
    bytes.extend(std::iter::repeat_n(0xe1, nulls));
    bytes.extend_from_slice(prototype);
    bytes
}

fn synthetic_datum_definition_array() -> Vec<u8> {
    let mut bytes = synthetic_datum_array(
        b"\x00gtol_datum_def_xar",
        16,
        10,
        13,
        b"\xe3\xe0\x01datum_type\0\x01\xe0\x01datum_id\0\xf6\xe0\x01feat_id\0\xf6\xe0\x01gtol_id\0\xf6\xe0\x00p_ldr\0\xe3\xf1\xf7\x0a\xe3",
    );
    bytes.extend_from_slice(b"\x01\xf6\xf6\xf6\xe3\x01\xf6\xf6\xf6\xe3");
    bytes
}

fn synthetic_datum_tag_array_with_unread_duplicate() -> Vec<u8> {
    let mut bytes = synthetic_datum_array(
        b"\x00set_datum_tag_xar",
        16,
        30,
        13,
        b"\xe3\xe0\x09attributes\0\x01\xe0\x01dtm_def_id\0\x0d\xe0\x00p_sld_info\0\xe3\xe0\x01ref_type\0\x01\xe0\x01ref_id\0\x01\xe0\x00p_cosm\0\xf1\xe3\xe0\x09attributes\0\x01\xe0\x00gen_info\0\xe2\xe0\x00p_text\0\xf1\xe3\xe0\x01db_type\0\x3e\xe0\x01text_type\0\x12\xe0\x0atext_value\0X\0\xe0\x00p_addl_text\0\xf1\xe3\xe0\x00p_attach\0\xf1\xe1\xe0\x0ap_datum_tag_name\0A\0\xf1\xf7\x1e\xe3",
    );
    bytes.extend_from_slice(b"\xa0\x01\x0e\xe3\x80\xdb\xe3\x3e\x12B\0");
    bytes.extend_from_slice(b"\xe3\xa0\x01\x0f\xe3\x80\xdb\xe3\x3e\x12A\0\xe3\x3e\x12\0");
    bytes
}
