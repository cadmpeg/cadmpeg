// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Curve, CurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;

use crate::parameter::{Token, TokenValue};
use crate::test_support::*;
use crate::IgesCodec;
use crate::{directory::DirectoryEntry, directory::Status, parameter::ParameterRecord};

const EPS_OFFSET_ENDPOINT_MATCH: f64 = 1.0e-9;
const EPS_SOURCE_PARAMETER_DOMAIN: f64 = 1.0e-12;
const EPS_PLACED_OFFSET: f64 = 1.0e-12;

fn vector_distance(left: Vector3, right: Vector3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

fn source_entry(entity_type: i64, form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type,
        parameter_start: 1,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 1,
            use_flag: 0,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 1,
        form,
        reserved: [[b' '; 8]; 2],
        label: *b"SOURCE  ",
        subscript: 0,
    }
}

fn numeric_record(values: &[(usize, f64)]) -> ParameterRecord {
    let length = values.iter().map(|(index, _)| index + 1).max().unwrap_or(0);
    let mut tokens = (0..length)
        .map(|_| Token {
            value: TokenValue::Real(0.0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    for (index, value) in values {
        tokens[*index].value = TokenValue::Real(*value);
    }
    let parameter_end = tokens.len();
    ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens,
        parameter_end,
        comment: Vec::new(),
    }
}

#[test]
fn source_parameter_map_uses_the_iges_domain_for_each_bounded_curve_form() {
    let neutral = [2.0, 5.0];
    for (entity_type, form, native) in [
        (102, 0, [2.0, 5.0]),
        (110, 0, [0.0, 1.0]),
        (106, 11, [2.0, 5.0]),
        (106, 12, [2.0, 5.0]),
        (106, 13, [2.0, 5.0]),
        (106, 63, [2.0, 5.0]),
        (112, 0, [2.0, 5.0]),
        (126, 0, [2.0, 5.0]),
        (126, 5, [2.0, 5.0]),
    ] {
        let map = super::source_parameter_map(
            &source_entry(entity_type, form),
            &numeric_record(&[]),
            neutral,
        )
        .expect("bounded source domain");
        assert_eq!(map.native, native, "Type {entity_type} Form {form}");
        assert!((map.to_neutral(native[0]) - neutral[0]).abs() < EPS_SOURCE_PARAMETER_DOMAIN);
        assert!((map.to_neutral(native[1]) - neutral[1]).abs() < EPS_SOURCE_PARAMETER_DOMAIN);
    }
}

#[test]
fn source_parameter_map_preserves_absolute_and_explicit_native_domains() {
    let circle = super::source_parameter_map(
        &source_entry(100, 0),
        &numeric_record(&[(4, 0.0), (5, -1.0), (6, 1.0), (7, 0.0)]),
        [10.0, 20.0],
    )
    .expect("circular-arc domain");
    assert!(
        (circle.native[0] - 3.0 * std::f64::consts::FRAC_PI_2).abs() < EPS_SOURCE_PARAMETER_DOMAIN
    );
    assert!((circle.native[1] - 2.0 * std::f64::consts::PI).abs() < EPS_SOURCE_PARAMETER_DOMAIN);
    assert!((circle.to_neutral(circle.native[0]) - 10.0).abs() < EPS_SOURCE_PARAMETER_DOMAIN);
    assert!((circle.to_neutral(circle.native[1]) - 20.0).abs() < EPS_SOURCE_PARAMETER_DOMAIN);

    let explicit = super::source_parameter_map(
        &source_entry(130, 0),
        &numeric_record(&[(13, -4.0), (14, 6.0)]),
        [10.0, 20.0],
    )
    .expect("offset-curve domain");
    assert_eq!(explicit.native, [-4.0, 6.0]);
    assert!((explicit.to_neutral(-4.0) - 10.0).abs() < EPS_SOURCE_PARAMETER_DOMAIN);
    assert!((explicit.to_neutral(6.0) - 20.0).abs() < EPS_SOURCE_PARAMETER_DOMAIN);
}

#[test]
fn source_parameter_map_rejects_unbounded_line_forms() {
    for form in [1, 2] {
        assert!(super::source_parameter_map(
            &source_entry(110, form),
            &numeric_record(&[]),
            [0.0, 1.0],
        )
        .is_none());
    }
}

#[test]
fn source_parameter_map_rejects_non_affine_curve_and_non_curve_domains() {
    for (entity_type, form) in [
        (104, 0),
        (104, 1),
        (104, 2),
        (104, 3),
        (106, 1),
        (106, 2),
        (106, 3),
        (106, 20),
        (106, 40),
    ] {
        assert!(super::source_parameter_map(
            &source_entry(entity_type, form),
            &numeric_record(&[]),
            [0.0, 1.0],
        )
        .is_none());
    }
}

#[test]
fn offset_source_range_uses_the_unique_curve_endpoint_match() {
    let source_id = CurveId("source".into());
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: source_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: PointId("wrong-start-point".into()),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("wrong-end-point".into()),
            position: Point3::new(11.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("matching-start-point".into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("matching-end-point".into()),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("wrong-start".into()),
            point: PointId("wrong-start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("wrong-end".into()),
            point: PointId("wrong-end-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("matching-start".into()),
            point: PointId("matching-start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("matching-end".into()),
            point: PointId("matching-end-point".into()),
            tolerance: None,
        },
    ]);
    ir.model.edges.extend([
        Edge {
            id: EdgeId("wrong-occurrence".into()),
            curve: Some(source_id.clone()),
            start: VertexId("wrong-start".into()),
            end: VertexId("wrong-end".into()),
            param_range: Some([5.0, 6.0]),
            tolerance: None,
        },
        Edge {
            id: EdgeId("matching-occurrence".into()),
            curve: Some(source_id.clone()),
            start: VertexId("matching-start".into()),
            end: VertexId("matching-end".into()),
            param_range: Some([0.0, 2.0]),
            tolerance: None,
        },
    ]);

    let source = &ir.model.curves[0];
    assert_eq!(
        super::source_parameter_range(&ir, &source_id, &source.geometry, EPS_OFFSET_ENDPOINT_MATCH,),
        Some([0.0, 2.0])
    );
}

#[test]
fn decode_defaults_unused_uniform_offset_scalars_to_zero() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(uniform_offset_circle_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Circle { radius, .. } = offset.geometry else {
        panic!("expected an exact circular offset carrier");
    };
    assert_eq!(radius, 1.5);
    let edge = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D3")
        .unwrap();
    assert_eq!(edge.param_range, Some([0.0, std::f64::consts::FRAC_PI_2]));
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_places_uniform_offset_circle_with_a_proper_transform() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(placed_uniform_offset_circle_file(
                0,
                b"124,0,-1,0,5,1,0,0,0,0,0,1,0;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .expect("placed offset carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = offset.geometry
    else {
        panic!("expected an exact placed circular offset carrier");
    };
    assert!(center.distance(Point3::new(5.0, 0.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!(vector_distance(axis, Vector3::new(0.0, 0.0, 1.0)) < EPS_PLACED_OFFSET);
    assert!(vector_distance(ref_direction, Vector3::new(0.0, 1.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!((radius - 1.5).abs() < EPS_PLACED_OFFSET);
    let procedural = &result.ir().model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
        source,
        side: cadmpeg_ir::geometry::OffsetSide::PlaneNormal(normal),
        ..
    } = procedural.definition()
    else {
        panic!("expected an offset construction");
    };
    assert_eq!(source.0, "iges:model:curve#D3-placed-source");
    assert!(vector_distance(*normal, Vector3::new(0.0, 0.0, 1.0)) < EPS_PLACED_OFFSET);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_places_uniform_offset_line_with_a_proper_transform() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(placed_uniform_offset_line_file(
                0,
                b"124,0,-1,0,5,1,0,0,0,0,0,1,0;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .expect("placed line offset carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } = offset.geometry else {
        panic!("expected an exact placed line offset carrier");
    };
    assert!(origin.distance(Point3::new(4.5, 0.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!(vector_distance(direction, Vector3::new(0.0, 1.0, 0.0)) < EPS_PLACED_OFFSET);
    let end = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.0 == "iges:model:point#D3:end")
        .expect("placed line offset end point");
    assert!(end.position.distance(Point3::new(4.5, 2.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_corrects_offset_normal_handedness_for_a_reflection() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(placed_uniform_offset_circle_file(
                1,
                b"124,-1,0,0,5,0,1,0,0,0,0,1,0;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .expect("reflected offset carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = offset.geometry
    else {
        panic!("expected an exact reflected circular offset carrier");
    };
    assert!(center.distance(Point3::new(5.0, 0.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!(vector_distance(axis, Vector3::new(0.0, 0.0, -1.0)) < EPS_PLACED_OFFSET);
    assert!(vector_distance(ref_direction, Vector3::new(-1.0, 0.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!((radius - 1.5).abs() < EPS_PLACED_OFFSET);
    let start = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.0 == "iges:model:point#D3:start")
        .expect("reflected offset start point");
    assert!(start.position.distance(Point3::new(3.5, 0.0, 0.0)) < EPS_PLACED_OFFSET);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_maps_absolute_arc_parameters_to_the_neutral_domain() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(offset_quarter_circle_with_absolute_native_parameters()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let edge = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D3")
        .expect("offset arc");
    assert_eq!(edge.param_range, Some([0.0, std::f64::consts::FRAC_PI_2]));
    let start = result
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .and_then(|vertex| {
            result
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
        })
        .expect("offset start point");
    assert_eq!(start.position, cadmpeg_ir::math::Point3::new(0.0, 1.5, 0.0));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_does_not_default_an_unused_offset_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(uniform_offset_circle_file_with_parameters(
                b"130,1,1,,,,0.5,,,,0,0,1,0,1.5707963267948966;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.0 != "iges:model:curve#D3"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("DE2 is not explicit integer zero") }));
}

#[test]
fn decode_applies_declared_real_significance_to_curve_offset_normals() {
    for (normal_z, decoded) in [
        (".9999995", true),
        (".99999949", false),
        (".9999999D0", false),
    ] {
        let parameters = format!("130,1,1,0,,,0.5,,,,0,0,{normal_z},0,1.5707963267948966;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(uniform_offset_circle_file_with_parameters(
                    parameters.as_bytes(),
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id.0 == "iges:model:curve#D3");
        assert_eq!(offset, decoded, "{normal_z}");
        if !decoded {
            assert!(result.report().losses.iter().any(|loss| loss
                .message
                .contains("offset plane normal is not a unit vector")));
        }
    }
}

#[test]
fn decode_solves_a_parameter_linear_line_offset() {
    for (basis_code, expected_basis) in [
        (1, cadmpeg_ir::geometry::CurveOffsetLawBasis::ArcLength),
        (2, cadmpeg_ir::geometry::CurveOffsetLawBasis::Parameter),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(linear_offset_line_file(basis_code)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.0 == "iges:model:curve#D3")
            .unwrap();
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &offset.geometry else {
            panic!("expected an exact degree-one offset carrier");
        };
        assert_eq!(nurbs.knots, vec![0.0, 0.0, 10.0, 10.0]);
        assert_eq!(
            nurbs.control_points,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 3.0, 0.0),
            ]
        );
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
            range:
                Some(cadmpeg_ir::geometry::CurveOffsetRange::Variable {
                    distance_law:
                        cadmpeg_ir::geometry::CurveOffsetDistanceLaw::Linear {
                            basis,
                            distances,
                            control_range,
                        },
                    ..
                }),
            ..
        } = &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected a retained linear offset law");
        };
        assert_eq!(*basis, expected_basis);
        assert_eq!(*distances, [1.0, 3.0]);
        assert_eq!(*control_range, [0.0, 10.0]);
        assert!(result.report().losses.is_empty());
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_solves_a_polynomial_coordinate_function_offset() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(function_offset_line_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &offset.geometry else {
        panic!("expected an exact function-offset carrier");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 10.0, 10.0]);
    assert_eq!(
        nurbs.control_points,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 3.0, 0.0),
        ]
    );
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
        range:
            Some(cadmpeg_ir::geometry::CurveOffsetRange::Variable {
                distance_law:
                    cadmpeg_ir::geometry::CurveOffsetDistanceLaw::Coordinate {
                        function,
                        coordinate,
                        basis,
                        function_parameter_offset,
                        function_parameter_scale,
                    },
                ..
            }),
        ..
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected a retained coordinate-function offset law");
    };
    assert_eq!(function.0, "iges:model:curve#D3");
    assert_eq!(*coordinate, 2);
    assert_eq!(*basis, cadmpeg_ir::geometry::CurveOffsetLawBasis::Parameter);
    assert_eq!(*function_parameter_offset, 0.0);
    assert_eq!(*function_parameter_scale, 0.1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
