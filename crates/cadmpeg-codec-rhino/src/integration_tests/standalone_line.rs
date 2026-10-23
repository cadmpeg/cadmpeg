// SPDX-License-Identifier: Apache-2.0
//! Finite standalone lines retain their locus and parameter domain without source data.

use crate::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::LineCurve;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;
use std::io::Cursor;

fn native_line_geometry(bytes: &[u8]) -> CurveGeometry {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("written archive source");
    let scan = crate::container::scan(&ctx, bytes).expect("written native line framing");
    assert_eq!(scan.objects.len(), 1);
    let object = scan.objects[0].framed().expect("framed native line object");
    assert_eq!(
        object.class_uuid.to_wire(),
        crate::test_support::test_archive::LINE_CLASS
    );
    let parsed = crate::curves::decode(
        &ctx,
        bytes,
        object.class_uuid,
        object.class_data_range(),
        crate::settings::MillimeterScale::IDENTITY,
        scan.archive,
    )
    .expect("native line fields");
    let crate::curves::DecodedGeometry::Curve {
        curve: crate::curves::DecodedCurve::Leaf { geometry, .. },
    } = parsed
    else {
        panic!("native line leaf")
    };
    geometry
}

fn round_trip(origin: Point3, direction: UnitVector3, domain: [f64; 2], endpoints: [Point3; 2]) {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        native_ref: None,
        id: "test:geometry:curve#0"
            .try_into()
            .expect("standalone curve identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::new(origin).expect("finite line origin"),
            direction,
        ))),
        parameter_range: Some(IncreasingParameterInterval::new(domain).expect("finite line range")),
        source_object: None,
    });
    assert!(ir.model.bodies.is_empty());
    assert!(ir.native.namespace("rhino").is_none());
    let before = serde_json::to_value(&ir).expect("authored line snapshot");
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let plan = RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .expect("finite standalone line can be authored");
        assert!(!plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::RhinoLossCode::WriterEntityOmitted.kind()));
        let mut output = Vec::new();
        plan.write_to(&mut output)
            .expect("source-less line archive writes");
        let native_geometry = native_line_geometry(&output);
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("native bounded line archive reads");
        assert!(
            decoded.report().losses.is_empty(),
            "{:?}",
            decoded.report().losses
        );
        assert!(decoded.ir().model.bodies.is_empty());
        assert_eq!(decoded.ir().model.curves.len(), 1);
        let curve = &decoded.ir().model.curves[0];
        assert_eq!(curve.geometry, ir.model.curves[0].geometry);
        assert_eq!(curve.parameter_range, ir.model.curves[0].parameter_range);
        let Some(SolvedCurveGeometry::Nurbs(nurbs)) = native_geometry.solved() else {
            panic!("native line transfers to the specified degree-one NURBS carrier");
        };
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(
            nurbs.knots().as_slice(),
            [domain[0], domain[0], domain[1], domain[1]]
        );
        assert_eq!(
            nurbs
                .control_points()
                .into_iter()
                .map(FinitePoint3::get)
                .collect::<Vec<_>>(),
            endpoints
        );
        for parameter in [domain[0], (domain[0] + domain[1]) / 2.0, domain[1]] {
            let expected = cadmpeg_ir::eval::decode::curve_point(
                EvaluationAdmission::Standard,
                &ir.model.curves[0].geometry,
                parameter,
            )
            .expect("original finite line value");
            let actual = cadmpeg_ir::eval::decode::curve_point(
                EvaluationAdmission::Standard,
                &curve.geometry,
                parameter,
            )
            .expect("written finite line value");
            assert_eq!(actual, expected);
        }
    }
    assert_eq!(
        serde_json::to_value(&ir).expect("unchanged line input"),
        before
    );
}

#[test]
fn source_less_unit_line_retains_the_cli_control_geometry_and_range() {
    round_trip(
        Point3::new(0.0, 0.0, 0.0),
        UnitVector3::X_AXIS,
        [0.0, 1.0],
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
    );
}

#[test]
fn source_less_translated_line_retains_a_nonunit_parameter_domain() {
    round_trip(
        Point3::new(1.0, -2.0, 3.0),
        UnitVector3::Y_AXIS,
        [-2.0, 6.0],
        [Point3::new(1.0, -4.0, 3.0), Point3::new(1.0, 4.0, 3.0)],
    );
}
