// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;

use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SurfaceGeometry;

use cadmpeg_ir::math::Point2;

use crate::test_support::test_procedural_surfaces::trimmed_procedural_line_surface_of_revolution_file;
use crate::test_support::test_procedural_surfaces::trimmed_procedural_line_surface_of_revolution_file_with_global;
use crate::test_support::test_solids_and_structure::parametrically_bounded_plane_file;
use crate::test_support::test_surface_fixtures::bounded_plane_file;
use crate::test_support::test_surface_fixtures::bounded_plane_with_resolution_gap_file;
use crate::test_support::test_surface_fixtures::bounded_plane_with_significance_gap_file;
use crate::test_support::test_surface_fixtures::centimetre_bounded_plane_with_resolution_gap_file;
use crate::test_support::test_surface_fixtures::model_curve_only_trimmed_plane_file;
use crate::test_support::test_surface_fixtures::trimmed_circle_pcurve_file;
use crate::test_support::test_surface_fixtures::trimmed_plane_file;
use crate::IgesCodec;

const EPS_BOUNDARY_ENDPOINT_MATCH: f64 = 1.0e-9;

#[test]
fn decode_builds_a_parametrically_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametrically_bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(
        result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .map(|face| face.loop_role(&loop_.id))
            .unwrap_or_default(),
        cadmpeg_ir::topology::LoopBoundaryRole::Unspecified
    );
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(
        coedge.pcurves[0].pcurve.as_str(),
        "iges:model:pcurve#D9:0:0:0"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_an_ordered_multi_segment_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    assert_eq!(loop_.coedges().len(), 4);
    let senses = loop_
        .coedges()
        .iter()
        .map(|id| {
            result
                .ir()
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
                .sense
        })
        .collect::<Vec<_>>();
    assert_eq!(
        senses,
        vec![
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Reversed,
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Forward,
        ]
    );
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.owner_loop == loop_.id)
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_a_bounded_sheet_join_within_global_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_with_resolution_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .expect("bounded face within the declared resolution");
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .expect("bounded loop");
    assert_eq!(loop_.coedges().len(), 4);
    assert_eq!(
        face.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.001)
    );
    assert!(result
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.001)));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.001)));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_a_bounded_sheet_join_just_beyond_global_resolution() {
    let mut bytes = bounded_plane_file();
    let original = b"110,1,1,0,1,0,0;";
    let replacement = b"110,1,1,0,1,0.001001,0;";
    let start = bytes
        .windows(original.len())
        .position(|window| window == original)
        .expect("bounded-plane edge parameter record");
    let line_start = bytes[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let payload_end = line_start + 64;
    bytes[start..start + replacement.len()].copy_from_slice(replacement);
    bytes[start + replacement.len()..payload_end].fill(b' ');

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.id.as_str() != "iges:model:face#D13"));
    assert!(
        result.report().losses.iter().any(|loss| {
            loss.message
                .contains("boundary segments do not form a closed ring")
        }),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_converts_non_millimetre_resolution_before_sewing_a_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(centimetre_bounded_plane_with_resolution_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .expect("bounded face within the unit-converted resolution");
    assert_eq!(
        face.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.01)
    );
    assert!(result
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.01)));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.01)));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_sews_boundary_roundoff_with_declared_coordinate_significance() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_with_significance_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .expect("bounded face within one declared coordinate quantum");
    assert_eq!(
        face.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.01)
    );
    assert!(result
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.fit_tolerance().is_none()));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_valid_face_local_trimmed_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let sheet = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.as_str() == "iges:model:body#D9")
        .unwrap();
    assert_eq!(sheet.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap();
    assert_eq!(face.surface.as_str(), "iges:model:surface#D1");
    assert_eq!(face.loops.len(), 1);
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    assert_eq!(
        result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .map(|face| face.loop_role(&loop_.id))
            .unwrap_or_default(),
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(loop_.coedges().len(), 1);
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(coedge.radial_next, coedge.id);
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(
        coedge.pcurves[0].pcurve.as_str(),
        "iges:model:pcurve#D9:0:0:0"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_trimmed_sheet_from_a_native_circle_pcurve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_circle_pcurve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_maps_a_line_generatrix_pcurve_to_the_neutral_distance_parameter() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_procedural_line_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .unwrap();
    let SurfaceGeometry::Procedural { construction, .. } = &surface.geometry else {
        panic!("expected a procedural revolution surface");
    };
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)
        .unwrap();
    let ProceduralSurfaceDefinition::Revolution(definition_payload_0) = procedural.definition()
    else {
        panic!("expected a bounded procedural revolution");
    };
    let Some(parameter_interval) = &definition_payload_0
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        panic!("expected a bounded procedural revolution");
    };
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    let carrier_interval = procedural.record_bounds().unwrap().get();
    assert!(carrier_interval[1].is_some_and(|value| value > 3.0));

    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 1);
    let pcurve = result
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
        .unwrap();
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        panic!("expected a NURBS pcurve, got {:?}", pcurve.geometry);
    };
    let expected_u =
        (11.762_109_22_f64 - 6.814_348_186).hypot(-6.969_522_429_f64 - -2.592_356_749_f64) * 0.5;
    assert!((nurbs.control_points()[0].u - expected_u).abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
    assert!(nurbs.control_points()[0].v.abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_unscales_procedural_pcurve_coordinates_before_neutral_mapping() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_procedural_line_surface_of_revolution_file_with_global(
                b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,64,38,6,308,15,0H,1.0,1,4HINCH,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    let pcurve = result
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
        .unwrap();
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        panic!("expected a NURBS pcurve, got {:?}", pcurve.geometry);
    };
    let expected_u = (11.762_109_22_f64 - 6.814_348_186)
        .hypot(-6.969_522_429_f64 - -2.592_356_749_f64)
        * 0.5
        * 25.4;
    assert!((nurbs.control_points()[0].u - expected_u).abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
    assert!(nurbs.control_points()[0].v.abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn procedural_parameter_conversion_preserves_declared_endpoints() {
    let upper_u = 0.898_025_612_106_907_5;
    let mapped = super::super::source_parameter_point_to_neutral(
        Point2::new(25.4, 2.0 * 25.4),
        (upper_u, 0.0, 1.0, 0.0),
        25.4,
    );

    assert_eq!(mapped.u, upper_u);
    assert_eq!(mapped.v, 2.0);
}

#[test]
fn decode_builds_a_model_curve_only_trimmed_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(model_curve_only_trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert!(coedge.pcurves.is_empty());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
