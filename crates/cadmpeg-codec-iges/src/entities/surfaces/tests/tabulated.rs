// SPDX-License-Identifier: Apache-2.0
//! Exact tabulated surface carrier tests.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;

use crate::loss::IgesLossCode;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::test_surface_fixtures::{
    tabulated_cylinder_file, tabulated_hyperbola_file, tabulated_hyperbola_file_with_global,
};
use crate::test_support::test_tabulated_surfaces::{
    placed_tabulated_hyperbola_file, placed_tabulated_hyperbola_file_with_global,
    placed_tabulated_line_file, placed_tabulated_line_file_with_global,
};
use crate::IgesCodec;

#[test]
fn decode_solves_a_tabulated_cylinder_as_an_exact_extrusion() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact NURBS extrusion cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            surface,
            0.5,
            0.5
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload) =
        procedural.definition()
    else {
        panic!("expected an extrusion definition");
    };
    let directrix = definition_payload.directrix();
    assert_eq!(directrix.as_str(), "iges:model:curve#D1");
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
            .find(|surface| surface.id.as_str() == "iges:model:surface#D3")
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
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact extrusion definition");
        };
        let directrix = definition_payload_0.directrix();
        let Some(parameter_interval) = &definition_payload_0.parameter_interval() else {
            panic!("expected an exact extrusion definition");
        };
        let direction = definition_payload_0.direction();
        let Some(native_position) = &definition_payload_0.native_position() else {
            panic!("expected an exact extrusion definition");
        };
        assert_eq!(directrix.as_str(), "iges:model:curve#D1");
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
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_))
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let directrix_point = cadmpeg_ir::eval::decode::curve_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            directrix_geometry,
            parameter,
        )
        .expect("hyperbola directrix evaluates");
        let index =
            cadmpeg_ir::index::ModelIndex::build(result.ir(), cadmpeg_ir::index::StandardIndex);
        let surface_point = cadmpeg_ir::eval::model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface.id,
            parameter,
            1.0,
        )
        .expect("hyperbola tabulated surface evaluates");
        assert!(
            surface_point.distance(directrix_point.translated(direction.get(), 1.0))
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
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
            .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
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
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact placed extrusion definition");
        };
        let directrix = definition_payload_0.directrix();
        let Some(parameter_interval) = &definition_payload_0.parameter_interval() else {
            panic!("expected an exact placed extrusion definition");
        };
        let direction = definition_payload_0.direction();
        let Some(native_position) = &definition_payload_0.native_position() else {
            panic!("expected an exact placed extrusion definition");
        };
        assert_eq!(directrix.as_str(), "iges:model:curve#D5-placed-directrix");
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
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_))
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let directrix_point = cadmpeg_ir::eval::decode::curve_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            directrix_geometry,
            parameter,
        )
        .expect("placed hyperbola directrix evaluates");
        let index =
            cadmpeg_ir::index::ModelIndex::build(result.ir(), cadmpeg_ir::index::StandardIndex);
        let surface_point = cadmpeg_ir::eval::model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface.id,
            parameter,
            1.0,
        )
        .expect("placed hyperbola tabulated surface evaluates");
        assert!(
            surface_point.distance(directrix_point.translated(direction.get(), 1.0))
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
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
            .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
            .expect("placed NURBS tabulated surface");
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
            .expect("placed NURBS tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact placed NURBS extrusion definition");
        };
        let directrix = definition_payload_0.directrix();
        let direction = definition_payload_0.direction();
        let Some(native_position) = &definition_payload_0.native_position() else {
            panic!("expected an exact placed NURBS extrusion definition");
        };
        assert_eq!(directrix.as_str(), "iges:model:curve#D5-placed-directrix");
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
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
        ));
        assert_eq!(
            cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                directrix_geometry,
                0.5
            )
            .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(Point3::new(10.5, 20.0, 30.0))
        );
        assert_eq!(
            cadmpeg_ir::eval::decode::surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface.geometry,
                0.5,
                0.5
            )
            .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(Point3::new(10.5, 20.0, 31.0))
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

