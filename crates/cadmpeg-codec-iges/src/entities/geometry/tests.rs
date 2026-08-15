// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use super::enforce_transform_depth;
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

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
fn decode_preserves_a_rational_declaration_with_equal_weights() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(equal_weight_rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.weights, Some(vec![1.0, 1.0, 1.0]));
    assert!(result.report().losses.is_empty());
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
