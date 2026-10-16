// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::geometry::SolvedCurveGeometry;

use std::io::Cursor;

use cadmpeg_core::decode::DecodeArena;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;

use crate::test_support::test_owned::owned_test_file;
use crate::test_support::test_owned::owned_test_file_with_global_and_line_fonts;
use crate::test_support::test_owned::OwnedTestEntity;
use crate::test_support::test_procedural_surfaces::interval_certified_linear_bezier_ruled_surface_file;
use crate::test_support::test_surface_fixtures::circular_ruled_surface_file;
use crate::test_support::test_surface_fixtures::composite_ruled_surface_file;
use crate::test_support::test_surface_fixtures::composite_tabulated_cylinder_file;
use crate::test_support::test_surface_fixtures::ellipse_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::hyperbola_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::hyperbola_surface_of_revolution_file_with_global;
use crate::test_support::test_surface_fixtures::line_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::line_surface_of_revolution_file_with_global;
use crate::test_support::test_surface_fixtures::offset_nurbs_surface_file;
use crate::test_support::test_surface_fixtures::placed_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::rational_ruled_surface_file;
use crate::test_support::test_surface_fixtures::ruled_surface_file;
use crate::test_support::test_surface_fixtures::ruled_surface_file_with_developable_flag;
use crate::test_support::test_surface_fixtures::surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::trimmed_surface_of_revolution_file;

use crate::IgesCodec;

use crate::global::GlobalTable;

const EPS_RATIONAL_RULED: f64 = 1.0e-10;
const EPS_LINEAR_BEZIER_RULED: f64 = 1.0e-5;

use super::{angular_basis, offset_indicator_parameters, tabulated_directrix_type_allowed};

fn type128_surface_with_closure(
    global: &[u8],
    closed_u: i64,
    closed_v: i64,
    poles: &str,
) -> Vec<u8> {
    let parameters =
        format!("128,1,1,1,1,{closed_u},{closed_v},1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,{poles},0,1,0,1;");
    owned_test_file_with_global_and_line_fonts(
        &[OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SURFACE".into(),
            status: "00000000",
            parameters,
        }],
        global,
        &[(1, 1)],
    )
}

#[test]
fn tabulated_directrix_types_follow_the_declared_dialect() {
    assert!(tabulated_directrix_type_allowed(102, 0, GlobalTable::V4_0));
    assert!(tabulated_directrix_type_allowed(112, 0, GlobalTable::V4_0));
    assert!(!tabulated_directrix_type_allowed(112, 1, GlobalTable::V4_0));
    assert!(!tabulated_directrix_type_allowed(112, 3, GlobalTable::V5_0));
    assert!(!tabulated_directrix_type_allowed(130, 0, GlobalTable::V4_0));
    assert!(tabulated_directrix_type_allowed(130, 0, GlobalTable::V5_0));
    assert!(tabulated_directrix_type_allowed(
        142,
        0,
        GlobalTable::V5Later
    ));
}

#[test]
fn type_140_indicator_parameters_use_bounded_midpoint_or_unbounded_origin() {
    fn bounds(raw: [Option<f64>; 4]) -> Option<cadmpeg_ir::geometry::RecordBounds> {
        cadmpeg_ir::geometry::RecordBounds::try_new(raw).ok()
    }
    assert_eq!(
        offset_indicator_parameters(bounds([Some(-2.0), Some(6.0), Some(4.0), Some(8.0)])),
        [2.0, 6.0]
    );
    assert_eq!(
        offset_indicator_parameters(bounds([Some(-2.0), Some(6.0), None, Some(8.0)])),
        [0.0, 0.0]
    );
    assert_eq!(offset_indicator_parameters(None), [0.0, 0.0]);
}

#[test]
fn decode_type_140_uses_the_bounded_support_midpoint_normal() {
    for indicator in [
        "-0.4082482904638631D0,-0.4082482904638631D0,0.8164965809277261D0",
        "0,0,1",
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_nurbs_surface_file(indicator)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id.as_str() == "iges:model:surface#D1"));
        assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
        assert_eq!(result.report().losses.len(), 1);
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::GeometryNotProjected.kind()
        );
    }
}

#[test]
fn decode_skips_a_nurbs_surface_over_its_pole_limit() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 128,
                form: 0,
                label: "SURFACE".into(),
                status: "00000000",
                parameters: "128,1000,1000,1,1,0,0,0,0,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::GeometryNotProjected.kind()
            && loss.message.contains("surface pole count exceeds 1000000")
    }));
}

#[test]
fn project_skips_a_revolution_surface_over_its_pole_limit() {
    // A full-circle angular basis has 9 controls; 111_112 generatrix poles
    // yield 1_000_008 surface poles, one above MAX_SURFACE_POLES. The poles
    // live on an in-memory NURBS so the skip does not parse a megabyte of
    // Type 126 Parameter Data.
    let pole_count = 111_112_usize;
    let last = u32::try_from(pole_count - 1).unwrap();
    assert!(super::revolution_pole_count_exceeds_limit(pole_count, 9));
    assert!(!super::revolution_pole_count_exceeds_limit(2, 9));

    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "110,0,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "PROFILE".into(),
            status: "00010000",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 120,
            form: 0,
            label: "REVOLVE".into(),
            status: "00000000",
            parameters: "120,1,3,0,6.283185307179586;".into(),
        },
    ]);
    let scan = crate::card::scan(&bytes).unwrap();
    let (global, _) = crate::test_support::parse_global(&scan).unwrap();
    let (directory, parameters) = crate::test_support::with_service_context(&bytes, |ctx| {
        let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), ctx)?;
        let parameters = crate::parameter::assemble(&scan, &directory, &quarantined, &global)?;
        Ok::<_, CodecError>((directory, parameters))
    })
    .unwrap();

    let mut knots = vec![0.0, 0.0];
    for index in 1..last {
        knots.push(f64::from(index));
    }
    knots.push(f64::from(last));
    knots.push(f64::from(last));
    let mut points = Vec::new();
    for index in 0..pole_count {
        let x = f64::from(u32::try_from(index).unwrap());
        points.push(Point3::new(x, 0.0, 0.0));
    }
    let generatrix = NurbsCurve::from_lanes(1, knots, points, None, false).unwrap();
    let interval = [0.0, f64::from(last)];

    let axis_id = crate::ids::curve(&crate::ids::Stem::directory(1_u32));
    let generatrix_id = crate::ids::curve(&crate::ids::Stem::directory(3_u32));
    let generatrix_stem = crate::ids::Stem::directory(3_u32);
    let start_point = crate::ids::point(&generatrix_stem.tail(crate::ids::Word::Start));
    let end_point = crate::ids::point(&generatrix_stem.tail(crate::ids::Word::End));
    let start_vertex = crate::ids::vertex(&generatrix_stem.tail(crate::ids::Word::Start));
    let end_vertex = crate::ids::vertex(&generatrix_stem.tail(crate::ids::Word::End));
    let edge_id = crate::ids::edge(&generatrix_stem);

    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: axis_id,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: generatrix_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(generatrix)),
        source_object: None,
    });
    ir.model.points.extend([
        Point::new(
            start_point.clone(),
            FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            None,
        ),
        Point::new(
            end_point.clone(),
            FinitePoint3::new(Point3::new(f64::from(last), 0.0, 0.0)).unwrap(),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex.clone(),
            point: start_point,
            tolerance: None,
        },
        Vertex {
            id: end_vertex.clone(),
            point: end_point,
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: edge_id,
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(generatrix_id), Some(interval))
            .unwrap(),
        start: start_vertex,
        end: end_vertex,
        tolerance: None,
    });

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let outcome = super::project(
        &mut ir,
        &directory,
        &parameters.records,
        &global.length_context().unwrap(),
        &ctx,
        &mut super::super::geometry::SourceSequences::default(),
    )
    .unwrap();

    assert!(ir.model.surfaces.is_empty());
    assert!(!outcome.decoded.contains(&5_u32));
    assert!(outcome.losses.iter().any(|loss| {
        loss.code == IgesLossCode::GeometryNotProjected.kind()
            && loss
                .message
                .contains("revolution pole count exceeds 1000000")
    }));
}

#[test]
fn angular_basis_canonicalizes_a_full_sweep_with_decimal_roundoff() {
    let basis = angular_basis(0.0, std::f64::consts::TAU + std::f64::consts::TAU * 5.0e-13)
        .expect("a near-full finite sweep has an exact rational basis");

    assert_eq!(basis.controls.len(), 9);
    assert_eq!(basis.knots.last(), Some(&std::f64::consts::TAU));
}

#[test]
fn decode_solves_a_parameter_matched_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact NURBS ruled cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            surface,
            0.25,
            0.75
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_an_interval_certified_linear_bezier_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(interval_certified_linear_bezier_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::GeometryNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .and_then(|surface| match surface.geometry.solved() {
            Some(SolvedSurfaceGeometry::Nurbs(surface)) => Some(surface),
            _ => None,
        })
        .expect("linear Bezier ruled surface");
    let midpoint = cadmpeg_ir::eval::decode::nurbs_surface_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        surface,
        0.5,
        0.5,
    )
    .expect("linear Bezier ruled midpoint");
    assert!(
        midpoint.distance(Point3::new(1.5, 0.5, 0.0)) <= EPS_LINEAR_BEZIER_RULED,
        "{midpoint:?}"
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
}

#[test]
fn decode_reconciles_rational_ruled_rail_denominators_exactly() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational ruled cache");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (4, 1));
    assert!(surface.weights().is_some());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::GeometryNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    let curve_point = |sequence: u32, parameter: f64| {
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == format!("iges:model:curve#D{sequence}"))
            .expect("rail curve");
        cadmpeg_ir::eval::decode::curve_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve.geometry,
            parameter,
        )
        .expect("rail point")
    };
    for (u, v) in [(0.2, 0.25), (0.5, 0.5), (0.8, 0.75)] {
        let first = curve_point(1, u);
        let second = curve_point(3, u);
        let expected = Point3::new(
            (1.0 - v) * first.x + v * second.x,
            (1.0 - v) * first.y + v * second.y,
            (1.0 - v) * first.z + v * second.z,
        );
        let actual = cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            surface,
            u,
            v,
        )
        .expect("surface point");
        assert!(
            actual.distance(expected) <= EPS_RATIONAL_RULED,
            "{actual:?} vs {expected:?}"
        );
    }
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
}

#[test]
fn homogeneous_ruled_carrier_splits_mismatched_knot_partitions() {
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("valid first rail");
    let second = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        Some(vec![1.0, 0.5]),
        false,
    )
    .expect("valid second rail");
    let surface = super::ruled_surface_carrier(&first, &second)
        .expect("ruled lanes pair")
        .expect("partition-aligned rational ruled carrier");
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (5, 2));
    assert_eq!(
        surface.u_knots().as_slice(),
        [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0]
    );
    for (u, v) in [(0.25, 0.4), (0.75, 0.6)] {
        let first_point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &first,
            u,
        )
        .expect("first rail point");
        let second_point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &second,
            u,
        )
        .expect("second rail point");
        let expected = Point3::new(
            (1.0 - v) * first_point.x + v * second_point.x,
            (1.0 - v) * first_point.y + v * second_point.y,
            (1.0 - v) * first_point.z + v * second_point.z,
        );
        let actual = cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            u,
            v,
        )
        .expect("ruled surface point");
        assert!(actual.distance(expected) <= EPS_RATIONAL_RULED);
    }
}

#[test]
fn decode_projects_rational_circular_arc_length_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(circular_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact circular ruled cache");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert!(surface.weights().is_some());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::GeometryNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
}

#[test]
fn decode_retains_both_ruled_surface_developability_values_in_native_parameters() {
    for developable_flag in [0, 1] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(ruled_surface_file_with_developable_flag(developable_flag)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let entity = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
            .iter()
            .find(|entity| entity.id() == "iges:entity:directory#5")
            .unwrap();
        assert_eq!(
            entity.fields()["parameters"][4]["value"]["value"],
            developable_flag
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    }
}

#[test]
fn decode_projects_composite_ruled_and_tabulated_carriers() {
    let ruled = IgesCodec
        .decode(
            &mut Cursor::new(composite_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(ruled.ir().model.procedural_surfaces.len(), 1);
    assert!(ruled
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    assert!(cadmpeg_ir::validate_neutral(ruled.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());

    let tabulated = IgesCodec
        .decode(
            &mut Cursor::new(composite_tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(tabulated.ir().model.procedural_surfaces.len(), 1);
    assert!(
        tabulated.report().losses.is_empty(),
        "{:#?}",
        tabulated.report().losses
    );
    assert!(cadmpeg_ir::validate_neutral(tabulated.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
}

#[test]
fn decode_solves_a_surface_of_revolution_as_rational_quadratic_spans() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.v_degree(), 2);
    assert_eq!(surface.pole_weights().unwrap().len(), 6);
    let point = cadmpeg_ir::eval::decode::nurbs_surface_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        surface,
        0.5,
        std::f64::consts::FRAC_PI_4,
    )
    .unwrap();
    let expected = 0.5_f64.sqrt();
    assert!((point.x - expected).abs() < 1.0e-12);
    assert!((point.y - expected).abs() < 1.0e-12);
    assert!((point.z - 1.0).abs() < 1.0e-12);
    assert!(result.report().losses.is_empty());
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_surface_of_revolution_from_an_ellipse_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ellipse_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational ellipse revolution cache");
    };
    let point = cadmpeg_ir::eval::decode::nurbs_surface_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        surface,
        std::f64::consts::FRAC_PI_4,
        std::f64::consts::FRAC_PI_4,
    )
    .expect("ellipse revolution evaluates");
    assert!((point.x - 0.5).abs() < 1.0e-12);
    assert!((point.y - 1.5).abs() < 1.0e-12);
    assert!(point.z.abs() < 1.0e-12);
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_surface_of_revolution_from_a_line_with_roundoff_endpoints() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .expect("line revolution fixture decodes");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .expect("line revolution surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } = &surface.geometry
    else {
        panic!("expected an exact construction-backed revolution");
    };
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)
        .expect("line revolution construction");
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload_0) =
        procedural.definition()
    else {
        panic!("expected an exact revolution definition");
    };
    let directrix = definition_payload_0.directrix();
    let Some(parameter_interval) = &definition_payload_0
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        panic!("expected an exact revolution definition");
    };
    assert_eq!(directrix.as_str(), "iges:model:curve#D3");
    assert!(matches!(
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("line generatrix")
            .geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ));
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    assert_eq!(
        procedural
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        Some([Some(0.0), Some(6.606_051_667_958_6), None, None])
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::GeometryNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_uses_recovered_global_resolution_for_line_revolution_admission() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,64,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,2e-06.,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_surface_of_revolution_file_with_global(global)),
            &DecodeOptions::default(),
        )
        .expect("line revolution with recoverable Global syntax decodes");

    assert_eq!(result.ir().tolerances.linear.get(), 2e-6);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::GlobalNumericSyntaxRecovered.kind())
            .count(),
        1
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::GeometryNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
}

#[test]
fn decode_solves_a_surface_of_revolution_from_an_exact_hyperbola_carrier() {
    const EPS_REVOLUTION_POINT: f64 = 1.0e-12;

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", hyperbola_surface_of_revolution_file()),
        (
            "4.0",
            hyperbola_surface_of_revolution_file_with_global(global_v4),
        ),
        (
            "5.0",
            hyperbola_surface_of_revolution_file_with_global(global_v5),
        ),
    ];
    for (version, bytes) in fixtures {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            version
        );

        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
            .expect("hyperbola revolution surface");
        let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } =
            &surface.geometry
        else {
            panic!("expected a construction-backed revolution surface");
        };
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .expect("hyperbola revolution construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact revolution definition");
        };
        let directrix = definition_payload_0.directrix();
        let Some(parameter_interval) = &definition_payload_0
            .parameter_interval()
            .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        else {
            panic!("expected an exact revolution definition");
        };
        let angular_interval = definition_payload_0.angular_interval().endpoints();
        assert_eq!(directrix.as_str(), "iges:model:curve#D3");
        assert_eq!(angular_interval, [0.0, std::f64::consts::FRAC_PI_2]);
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("hyperbola directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_))
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let source_point = cadmpeg_ir::eval::decode::curve_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            directrix_geometry,
            parameter,
        )
        .expect("hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::build(result.ir());
        let quarter_turn = cadmpeg_ir::eval::model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface.id,
            parameter,
            std::f64::consts::FRAC_PI_2,
        )
        .expect("hyperbola revolution evaluates");
        let expected = Point3::new(-source_point.y, source_point.x, source_point.z);
        assert!(quarter_turn.distance(expected) < EPS_REVOLUTION_POINT);
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::GeometryNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("validate the neutral model");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_projects_a_trimmed_revolution_at_an_intermediate_native_angle() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(
        result
            .ir()
            .model
            .faces
            .iter()
            .any(|face| face.id.as_str() == "iges:model:face#D13"),
        "losses={:#?}",
        result.report().losses
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .expect("trimmed revolution support");
    assert!(matches!(
        surface.geometry.solved_cache(),
        Some(SolvedSurfaceGeometry::Nurbs(_))
    ));
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| {
            result.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
        })
        .expect("trimmed revolution construction");
    assert_eq!(
        procedural
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        Some([Some(0.0), Some(2.0), None, None])
    );
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload_0) =
        procedural.definition()
    else {
        panic!("expected bounded trimmed revolution");
    };
    let Some(parameter_interval) = &definition_payload_0
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        panic!("expected bounded trimmed revolution");
    };
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_places_a_surface_of_revolution_and_its_procedural_carriers_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(placed_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.poles().first().copied().unwrap().x, 11.0);
    let procedural = &result.ir().model.procedural_surfaces[0];
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload) =
        procedural.definition()
    else {
        panic!("expected a revolution definition");
    };
    let directrix = definition_payload.directrix();
    let axis_origin = definition_payload.axis_origin();
    assert_eq!(axis_origin.x, 10.0);
    assert_eq!(directrix.as_str(), "iges:model:curve#D7-placed-generatrix");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

mod projection;

mod implicit_planes;

mod tabulated;

#[test]
fn ruled_developability_claim_does_not_hide_readable_rails() {
    for claim in ["2", "", "3Hbad", "1E999", "bad"] {
        let bytes = owned_test_file(&[
            OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "FIRST".into(),
                status: "00000000",
                parameters: "110,0,0,0,1,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "SECOND".into(),
                status: "00000000",
                parameters: "110,0,1,0,1,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 118,
                form: 0,
                label: "RULED".into(),
                status: "00000000",
                parameters: format!("118,1,3,0,{claim};"),
            },
        ]);
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.surfaces.len(), 1, "claim {claim}");
        let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
            result.ir().model.surfaces[0].geometry.solved()
        else {
            panic!("exact ruled cache")
        };
        assert_eq!(
            cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                surface,
                0.25,
                0.75
            )
            .map(FinitePoint3::get),
            Ok(Point3::new(0.25, 0.75, 0.0))
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityRecovered.kind()));
    }
}

#[test]
fn type122_projection_keeps_rational_directrix_weights() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "RATIONAL".into(),
            status: "00000000",
            parameters: "126,2,2,1,0,0,0,0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 122,
            form: 0,
            label: "TABULATE".into(),
            status: "00000000",
            parameters: "122,1,0,0,2;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
            cache: Some(SolvedSurfaceGeometry::Nurbs(nurbs)),
            ..
        } if nurbs.weights().is_some()
    )));
}

#[test]
fn decode_projects_an_exact_placed_hyperbola_revolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_surface_fixtures::placed_hyperbola_surface_of_revolution_file(),
            ),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!result.ir().model.surfaces.is_empty());
}

#[test]
fn nonfinite_placed_carriers_report_a_transform_loss() {
    for bytes in [
        crate::test_support::test_tabulated_surfaces::placed_tabulated_nurbs_overflow_file(),
        crate::test_support::test_surface_fixtures::placed_overflow_surface_of_revolution_file(),
    ] {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| { loss.code == IgesLossCode::NurbsTransformNonFinite.kind() }),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn reversed_ruled_rail_with_nonfinite_knots_reports_a_transform_loss() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "RAIL1".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "RAIL2".into(),
            status: "00010000",
            parameters:
                "126,1,1,1,0,1,0,8D307,8D307,1D308,1D308,1,1,0,1,0,1,1,0,8D307,1D308,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 118,
            form: 1,
            label: "RULED".into(),
            status: "00000000",
            parameters: "118,1,3,1,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(
        result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::NurbsTransformNonFinite.kind()
                && loss.message == "IGES reversed second rail knots are non-finite"
        }),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn homogeneous_ruled_carrier_aligns_relative_parameter_partitions() {
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid first rail");
    let second = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .expect("valid second rail");
    let surface = super::ruled_surface_carrier(&first, &second)
        .expect("ruled lanes pair")
        .expect("relative-parameter rational ruled carrier");
    assert_eq!((surface.u_degree(), surface.v_degree()), (3, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (4, 2));
    for (u, v) in [(0.2, 0.25), (0.6, 0.75), (0.9, 0.5)] {
        let first_point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &first,
            u,
        )
        .expect("first rail point");
        let second_point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &second,
            2.0 * u,
        )
        .expect("second rail point");
        let expected = Point3::new(
            (1.0 - v) * first_point.x + v * second_point.x,
            (1.0 - v) * first_point.y + v * second_point.y,
            (1.0 - v) * first_point.z + v * second_point.z,
        );
        let actual = cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            u,
            v,
        )
        .expect("ruled surface point");
        assert!(actual.distance(expected) <= EPS_RATIONAL_RULED);
    }
}
