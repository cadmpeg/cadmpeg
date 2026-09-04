// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

use crate::global::GlobalTable;

const EPS_RATIONAL_RULED: f64 = 1.0e-10;
const EPS_LINEAR_BEZIER_RULED: f64 = 1.0e-5;

use super::{
    angular_basis, homogeneous_curve_boundary_matches, offset_indicator_parameters,
    tabulated_directrix_type_allowed,
};

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
    assert_eq!(
        offset_indicator_parameters(Some([Some(-2.0), Some(6.0), Some(4.0), Some(8.0),])),
        [2.0, 6.0]
    );
    assert_eq!(
        offset_indicator_parameters(Some([Some(-2.0), Some(6.0), None, Some(8.0)])),
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
            .any(|surface| surface.id.0 == "iges:model:surface#D1"));
        assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
        assert_eq!(result.report().losses.len(), 1);
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::EntityNotProjected.kind()
        );
    }
}

#[test]
fn decode_refuses_a_nurbs_surface_over_its_pole_limit() {
    let error = IgesCodec
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
        .unwrap_err();

    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("iges_surface_poles")
                && limit.limit == 1_000_000
                && limit.used == 1_000_000
                && limit.additional == 2_001
    ));
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
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact NURBS ruled cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "iges:model:surface#D5")
        .and_then(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("linear Bezier ruled surface");
    let midpoint = cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5)
        .expect("linear Bezier ruled midpoint");
    assert!(
        midpoint.distance(Point3::new(1.5, 0.5, 0.0)) <= EPS_LINEAR_BEZIER_RULED,
        "{midpoint:?}"
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_reconciles_rational_ruled_rail_denominators_exactly() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact rational ruled cache");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (4, 1));
    assert!(surface.weights().is_some());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    let curve_point = |sequence: u32, parameter: f64| {
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.0 == format!("iges:model:curve#D{sequence}"))
            .expect("rail curve");
        cadmpeg_ir::eval::curve_point(&curve.geometry, parameter).expect("rail point")
    };
    for (u, v) in [(0.2, 0.25), (0.5, 0.5), (0.8, 0.75)] {
        let first = curve_point(1, u);
        let second = curve_point(3, u);
        let expected = Point3::new(
            (1.0 - v) * first.x + v * second.x,
            (1.0 - v) * first.y + v * second.y,
            (1.0 - v) * first.z + v * second.z,
        );
        let actual = cadmpeg_ir::eval::nurbs_surface_point(surface, u, v).expect("surface point");
        assert!(
            actual.distance(expected) <= EPS_RATIONAL_RULED,
            "{actual:?} vs {expected:?}"
        );
    }
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn homogeneous_ruled_carrier_aligns_relative_parameter_partitions() {
    let first = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid first rail");
    let second = NurbsCurve::new(
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
    let surface = super::ruled_surface_carrier(&first, &second, None)
        .expect("relative-parameter rational ruled carrier");
    assert_eq!((surface.u_degree(), surface.v_degree()), (3, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (4, 2));
    for (u, v) in [(0.2, 0.25), (0.6, 0.75), (0.9, 0.5)] {
        let first_point = cadmpeg_ir::eval::nurbs_curve_point(
            first.degree(),
            first.knots(),
            first.control_points(),
            first.weights(),
            u,
        )
        .expect("first rail point");
        let second_point = cadmpeg_ir::eval::nurbs_curve_point(
            second.degree(),
            second.knots(),
            second.control_points(),
            second.weights(),
            2.0 * u,
        )
        .expect("second rail point");
        let expected = Point3::new(
            (1.0 - v) * first_point.x + v * second_point.x,
            (1.0 - v) * first_point.y + v * second_point.y,
            (1.0 - v) * first_point.z + v * second_point.z,
        );
        let actual =
            cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).expect("ruled surface point");
        assert!(actual.distance(expected) <= EPS_RATIONAL_RULED);
    }
}

#[test]
fn homogeneous_ruled_carrier_splits_mismatched_knot_partitions() {
    let first = NurbsCurve::new(
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
    let second = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        Some(vec![1.0, 0.5]),
        false,
    )
    .expect("valid second rail");
    let surface = super::ruled_surface_carrier(&first, &second, None)
        .expect("partition-aligned rational ruled carrier");
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (5, 2));
    assert_eq!(surface.u_knots(), [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0]);
    for (u, v) in [(0.25, 0.4), (0.75, 0.6)] {
        let first_point = cadmpeg_ir::eval::nurbs_curve_point(
            first.degree(),
            first.knots(),
            first.control_points(),
            first.weights(),
            u,
        )
        .expect("first rail point");
        let second_point = cadmpeg_ir::eval::nurbs_curve_point(
            second.degree(),
            second.knots(),
            second.control_points(),
            second.weights(),
            u,
        )
        .expect("second rail point");
        let expected = Point3::new(
            (1.0 - v) * first_point.x + v * second_point.x,
            (1.0 - v) * first_point.y + v * second_point.y,
            (1.0 - v) * first_point.z + v * second_point.z,
        );
        let actual =
            cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).expect("ruled surface point");
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
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact circular ruled cache");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert!(surface.weights().is_some());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
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
        let entity = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
    assert!(cadmpeg_ir::validate_neutral(ruled.ir(), Vec::new()).is_ok());

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
    assert!(cadmpeg_ir::validate_neutral(tabulated.ir(), Vec::new()).is_ok());
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
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.v_degree(), 2);
    assert_eq!(surface.weights().unwrap().len(), 6);
    let point =
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, std::f64::consts::FRAC_PI_4).unwrap();
    let expected = 0.5_f64.sqrt();
    assert!((point.x - expected).abs() < 1.0e-12);
    assert!((point.y - expected).abs() < 1.0e-12);
    assert!((point.z - 1.0).abs() < 1.0e-12);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact rational ellipse revolution cache");
    };
    let point = cadmpeg_ir::eval::nurbs_surface_point(
        surface,
        std::f64::consts::FRAC_PI_4,
        std::f64::consts::FRAC_PI_4,
    )
    .expect("ellipse revolution evaluates");
    assert!((point.x - 0.5).abs() < 1.0e-12);
    assert!((point.y - 1.5).abs() < 1.0e-12);
    assert!(point.z.abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
        .find(|surface| surface.id.0 == "iges:model:surface#D5")
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
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
        directrix,
        parameter_interval: Some(parameter_interval),
        ..
    } = procedural.definition()
    else {
        panic!("expected an exact revolution definition");
    };
    assert_eq!(directrix.0, "iges:model:curve#D3");
    assert!(matches!(
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("line generatrix")
            .geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { .. }
    ));
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    assert_eq!(
        procedural.record_bounds,
        Some([Some(0.0), Some(6.606_051_667_958_6), None, None])
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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

    assert_eq!(result.ir().tolerances.linear, 2e-6);
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
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
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
            .find(|surface| surface.id.0 == "iges:model:surface#D5")
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
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            directrix,
            parameter_interval: Some(parameter_interval),
            angular_interval,
            ..
        } = procedural.definition()
        else {
            panic!("expected an exact revolution definition");
        };
        assert_eq!(directrix.0, "iges:model:curve#D3");
        assert_eq!(*angular_interval, [0.0, std::f64::consts::FRAC_PI_2]);
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
            cadmpeg_ir::geometry::CurveGeometry::Hyperbola { .. }
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let source_point = cadmpeg_ir::eval::curve_point(directrix_geometry, parameter)
            .expect("hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
        let quarter_turn = cadmpeg_ir::eval::model_surface_point_by_id(
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
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
            .any(|face| face.id.0 == "iges:model:face#D13"),
        "losses={:#?}",
        result.report().losses
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "iges:model:surface#D5")
        .expect("trimmed revolution support");
    assert!(matches!(
        surface.geometry.solved_cache(),
        Some(SurfaceGeometry::Nurbs(_))
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
        procedural.record_bounds,
        Some([Some(0.0), Some(2.0), None, None])
    );
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
        parameter_interval: Some(parameter_interval),
        ..
    } = procedural.definition()
    else {
        panic!("expected bounded trimmed revolution");
    };
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.control_points()[0].x, 11.0);
    let procedural = &result.ir().model.procedural_surfaces[0];
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
        directrix,
        axis_origin,
        ..
    } = procedural.definition()
    else {
        panic!("expected a revolution definition");
    };
    assert_eq!(axis_origin.x, 10.0);
    assert_eq!(directrix.0, "iges:model:curve#D7-placed-generatrix");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_solves_a_tabulated_cylinder_as_an_exact_extrusion() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected an exact NURBS extrusion cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5),
        Some(cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_tabulated_surface_from_a_type_142_model_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "MODEL".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 108,
                    form: 0,
                    label: "PLANE".into(),
                    status: "00010000",
                    parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "PCURVE".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "CURVSRF".into(),
                    status: "00010000",
                    parameters: "142,0,3,5,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 122,
                    form: 0,
                    label: "TABULATE".into(),
                    status: "00000000",
                    parameters: "122,7,0,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            result
                .ir()
                .model
                .procedural_surface_owner(&surface.id)
                .map(SurfaceId::as_str)
                == Some("iges:model:surface#D9")
        })
        .expect("Type 122 neutral carrier");
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { directrix, .. } =
        procedural.definition()
    else {
        panic!("expected an extrusion definition");
    };
    assert_eq!(directrix.0, "iges:model:curve#D1");
    assert!(
        result.report().losses.is_empty(),
        "{:?}",
        result.report().losses
    );
}

#[test]
fn decode_solves_a_tabulated_surface_from_an_exact_hyperbola_directrix() {
    const EPS_TABULATED_POINT: f64 = 1.0e-12;

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", tabulated_hyperbola_file()),
        ("4.0", tabulated_hyperbola_file_with_global(global_v4)),
        ("5.0", tabulated_hyperbola_file_with_global(global_v5)),
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
            .find(|surface| surface.id.0 == "iges:model:surface#D3")
            .expect("hyperbola tabulated surface");
        let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } =
            &surface.geometry
        else {
            panic!("expected a construction-backed tabulated surface");
        };
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .expect("hyperbola tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval: Some(parameter_interval),
            direction,
            native_position: Some(native_position),
            ..
        } = procedural.definition()
        else {
            panic!("expected an exact extrusion definition");
        };
        assert_eq!(directrix.0, "iges:model:curve#D1");
        assert_eq!(
            *native_position,
            Point3::new(3.086_161_269_630_487, 3.525_603_580_931_404, 2.0)
        );
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
            cadmpeg_ir::geometry::CurveGeometry::Hyperbola { .. }
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let directrix_point = cadmpeg_ir::eval::curve_point(directrix_geometry, parameter)
            .expect("hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
        let surface_point =
            cadmpeg_ir::eval::model_surface_point_by_id(&index, &surface.id, parameter, 1.0)
                .expect("hyperbola tabulated surface evaluates");
        assert!(
            surface_point.distance(directrix_point.translated(*direction, 1.0))
                < EPS_TABULATED_POINT
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_places_a_tabulated_surface_and_its_exact_directrix() {
    const EPS_PLACED_TABULATED_POINT: f64 = 1.0e-12;

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", placed_tabulated_hyperbola_file()),
        (
            "4.0",
            placed_tabulated_hyperbola_file_with_global(global_v4),
        ),
        (
            "5.0",
            placed_tabulated_hyperbola_file_with_global(global_v5),
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
            .find(|surface| surface.id.0 == "iges:model:surface#D5")
            .expect("placed tabulated surface");
        let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } =
            &surface.geometry
        else {
            panic!("expected a construction-backed placed tabulated surface");
        };
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .expect("placed tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval: Some(parameter_interval),
            direction,
            native_position: Some(native_position),
            ..
        } = procedural.definition()
        else {
            panic!("expected an exact placed extrusion definition");
        };
        assert_eq!(directrix.0, "iges:model:curve#D5-placed-directrix");
        assert!(
            native_position.distance(Point3::new(
                13.086_161_269_630_487,
                23.525_603_580_931_404,
                32.0,
            )) < EPS_PLACED_TABULATED_POINT
        );
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("placed hyperbola directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Transformed { .. }
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let directrix_point = cadmpeg_ir::eval::curve_point(directrix_geometry, parameter)
            .expect("placed hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
        let surface_point =
            cadmpeg_ir::eval::model_surface_point_by_id(&index, &surface.id, parameter, 1.0)
                .expect("placed hyperbola tabulated surface evaluates");
        assert!(
            surface_point.distance(directrix_point.translated(*direction, 1.0))
                < EPS_PLACED_TABULATED_POINT
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_places_a_nurbs_tabulated_surface_and_its_exact_directrix() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", placed_tabulated_line_file()),
        ("4.0", placed_tabulated_line_file_with_global(global_v4)),
        ("5.0", placed_tabulated_line_file_with_global(global_v5)),
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
            .find(|surface| surface.id.0 == "iges:model:surface#D5")
            .expect("placed NURBS tabulated surface");
        assert!(matches!(
            surface.geometry.solved_cache(),
            Some(cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(_))
        ));
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| {
                result.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
            })
            .expect("placed NURBS tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            native_position: Some(native_position),
            ..
        } = procedural.definition()
        else {
            panic!("expected an exact placed NURBS extrusion definition");
        };
        assert_eq!(directrix.0, "iges:model:curve#D5-placed-directrix");
        assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0));
        assert_eq!(*native_position, Point3::new(10.0, 20.0, 32.0));
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("placed NURBS directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
        ));
        assert_eq!(
            cadmpeg_ir::eval::curve_point(directrix_geometry, 0.5),
            Some(Point3::new(10.5, 20.0, 30.0))
        );
        assert_eq!(
            cadmpeg_ir::eval::surface_point(&surface.geometry, 0.5, 0.5),
            Some(Point3::new(10.5, 20.0, 31.0))
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_projects_an_unbounded_plane_from_implicit_coefficients() {
    let result = IgesCodec
        .decode(&mut Cursor::new(plane_file()), &DecodeOptions::default())
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected a plane carrier");
    };
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0));
    assert_eq!(*normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(
        cadmpeg_ir::eval::surface_point(&result.ir().model.surfaces[0].geometry, 1.0, 3.0),
        Some(cadmpeg_ir::math::Point3::new(1.0, 3.0, 2.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_asymmetric_nurbs_surface_parameter_domains() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(asymmetric_parameter_domain_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_permuted_nurbs_surface_parameter_domains() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(alternate_asymmetric_parameter_domain_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("u parameter range")));
}

#[test]
fn decode_retains_nurbs_surface_parameter_subranges() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(subrange_nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            result
                .ir()
                .model
                .procedural_surface_owner(&surface.id)
                .map(SurfaceId::as_str)
                == Some("iges:model:surface#D1")
        })
        .expect("Type 128 parameter-domain record");
    assert_eq!(
        procedural.record_bounds,
        Some([Some(0.2), Some(0.8), Some(-1.0), Some(1.0)])
    );
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Exact {
        spline: cadmpeg_ir::geometry::ExactSpline::Legacy { ranges, .. },
    } = procedural.definition()
    else {
        panic!("expected exact Type 128 construction")
    };
    assert_eq!(*ranges, [[0.2, 0.8], [-1.0, 1.0]]);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_signed_analytic_offset_surfaces() {
    for (indicator_z, expected_z) in [(1.0, 2.0), (-1.0, -2.0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_plane_file(indicator_z, 2.0)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.0 == "iges:model:surface#D3")
            .unwrap();
        let cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. } = offset.geometry else {
            panic!("expected an exact plane offset carrier");
        };
        assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, expected_z));
        assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { distance, .. } =
            result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected an offset dependency");
        };
        assert_eq!(*distance, expected_z);
        assert!(result.report().losses.is_empty());
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_uses_the_cylinder_normal_at_the_designated_parameters() {
    for (indicator_x, expected_radius) in [(1.0, 12.0), (-1.0, 8.0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_cylinder_file(indicator_x)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.0 == "iges:model:surface#D7")
            .expect("offset cylinder");
        let cadmpeg_ir::geometry::SurfaceGeometry::Cylinder { radius, .. } = surface.geometry
        else {
            panic!("expected cylindrical offset carrier")
        };
        assert_eq!(radius, expected_radius);
        assert!(
            result.report().losses.is_empty(),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_applies_declared_real_significance_to_offset_surface_indicators() {
    for (components, decoded) in [
        (("0", "0", ".9999995"), true),
        (("0", "0", ".99999949"), false),
        (("0", "0", ".9999999D0"), false),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_plane_file_with_indicator(
                    components.0,
                    components.1,
                    components.2,
                    2.0,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id.0 == "iges:model:surface#D3");
        assert_eq!(offset, decoded, "{components:?}");
        if !decoded {
            assert!(result.report().losses.iter().any(|loss| {
                loss.message
                    .contains("offset indicator is not a unit vector")
                    || loss.message.contains("not the support normal")
            }));
        }
    }
}

#[test]
fn decode_rejects_a_unit_offset_indicator_that_is_not_the_designated_normal() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(offset_plane_file_with_indicator(".6", ".8", "0", 2.0)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "iges:model:surface#D3"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("not the support normal")));
}

#[test]
fn decode_projects_a_bspline_surface_with_u_major_control_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((nurbs.u_degree(), nurbs.v_degree()), (1, 1));
    assert_eq!((nurbs.u_count(), nurbs.v_count()), (2, 2));
    assert_eq!(
        nurbs.control_points(),
        [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ]
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(nurbs, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_degree_zero_bspline_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(degree_zero_nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (0, 0));
    assert_eq!((surface.u_count(), surface.v_count()), (1, 1));
    assert_eq!(surface.u_knots(), [0.0, 1.0]);
    assert_eq!(surface.v_knots(), [0.0, 1.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(Point3::new(1.0, 2.0, 3.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_multispan_degree_zero_bspline_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(multispan_degree_zero_nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (0, 0));
    assert_eq!((surface.u_count(), surface.v_count()), (2, 1));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5),
        Some(Point3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 1.5, 0.5),
        Some(Point3::new(4.0, 5.0, 6.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_enforces_type128_closure_flags_in_iges_4_and_5_0() {
    for global in [
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,6,0;".as_slice(),
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;".as_slice(),
    ] {
        let invalid = IgesCodec
            .decode(
                &mut Cursor::new(type128_surface_with_closure(
                    global,
                    1,
                    0,
                    "0,0,0,1,0,0,0,1,0,1,0,0",
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(invalid.ir().model.surfaces.is_empty());
        assert!(invalid
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("U-closed surface flag")));

        for (closed_u, closed_v, poles) in [
            (1, 0, "0,0,0,0,0,0,0,1,0,0,1,0"),
            (0, 1, "0,0,0,1,0,0,0,0,0,1,0,0"),
        ] {
            let valid = IgesCodec
                .decode(
                    &mut Cursor::new(type128_surface_with_closure(
                        global, closed_u, closed_v, poles,
                    )),
                    &DecodeOptions::default(),
                )
                .unwrap();
            assert_eq!(valid.ir().model.surfaces.len(), 1);
            assert!(!valid
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("closed surface")));
        }
    }
}

#[test]
fn rational_boundary_comparison_accepts_projectively_scaled_curves() {
    let first = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        Some(vec![1.0, 1.0]),
        false,
    )
    .expect("valid rational boundary");
    let mut scaled = first.clone();
    scaled.weights_mut().unwrap().fill(2.0);
    assert_eq!(
        homogeneous_curve_boundary_matches(&first, &scaled, [0.0, 1.0], 0.0),
        Some(true)
    );

    scaled.control_points_mut()[1].x = 1.1;
    assert_eq!(
        homogeneous_curve_boundary_matches(&first, &scaled, [0.0, 1.0], 0.0),
        Some(false)
    );
}

#[test]
fn decode_applies_rational_surface_weight_declaration_in_iges_4_and_5_0() {
    for global in [
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,6,0;".as_slice(),
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;".as_slice(),
    ] {
        for (weights, projected, expected_message) in [
            (
                "1,1,1,1",
                false,
                "rational surface has equal weights but PROP3 declares rational",
            ),
            ("1,0.99,1,1", true, ""),
        ] {
            let parameters = format!(
                "128,1,1,1,1,0,0,0,0,0,0,0,1,1,0,0,1,1,{weights},0,0,0,1,0,0,1,0,1,1,0,1,0,1,0,1;"
            );
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(owned_test_file_with_global_and_line_fonts(
                        &[OwnedTestEntity {
                            entity_type: 128,
                            form: 0,
                            label: "SURFACE".into(),
                            status: "00000000",
                            parameters,
                        }],
                        global,
                        &[(1, 1)],
                    )),
                    &DecodeOptions::default(),
                )
                .unwrap();

            assert_eq!(result.ir().model.surfaces.len(), usize::from(projected));
            if projected {
                let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
                    &result.ir().model.surfaces[0].geometry
                else {
                    panic!("expected a NURBS surface carrier");
                };
                assert_eq!(
                    surface.weights(),
                    Some([1.0, 1.0, 0.99, 1.0].as_slice())
                );
            } else {
                assert!(result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.message.contains(expected_message)));
            }
        }
    }
}
