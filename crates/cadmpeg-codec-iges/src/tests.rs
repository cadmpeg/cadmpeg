// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

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

use std::collections::BTreeMap;
use std::io::Cursor;

use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

pub(crate) use crate::test_support::*;

#[test]
fn blank_directory_status_defaults_to_zero_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "        ",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn right_justified_directory_status_supplies_leading_zero_groups() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "STATUS".into(),
                status: "     201",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let entity = &result.ir().native.namespace("iges").unwrap().arenas["entities"][0];

    assert_eq!(entity.fields()["blank_status"], 0);
    assert_eq!(entity.fields()["subordinate_status"], 0);
    assert_eq!(entity.fields()["use_flag"], 2);
    assert_eq!(entity.fields()["hierarchy_status"], 1);
}

#[test]
fn directory_status_rejects_embedded_or_trailing_blanks() {
    for status in ["0000 201", "0000020 "] {
        let error = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "STATUS".into(),
                    status,
                    parameters: "116,1,2,3,0;".into(),
                }])),
                &DecodeOptions::default(),
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("status number is neither blank nor a right-justified decimal integer"));
    }
}

#[test]
fn directed_cycle_detection_handles_long_branching_graphs_iteratively() {
    let mut graph = (1..=100_000_u32)
        .map(|sequence| (sequence, vec![sequence + 1]))
        .collect::<BTreeMap<_, _>>();
    graph.entry(50_000).or_default().push(100_001);
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::directed_cycle(
        1,
        &mut visited,
        |sequence| graph.get(&sequence).cloned().unwrap_or_default()
    ));
    assert_eq!(visited.len(), 100_001);

    graph.insert(100_001, vec![50_000]);
    assert!(crate::entities::directed_cycle(
        1,
        &mut std::collections::BTreeSet::new(),
        |sequence| graph.get(&sequence).cloned().unwrap_or_default()
    ));
}

#[test]
fn inspect_parses_alternate_delimiters_and_cross_card_hollerith() {
    let product = "p".repeat(70);
    let global = format!(
        "1H^^1H!^70H{product}^8Hpart.igs^7Hcadmpeg^3H0.1^32^38^6^308^15^0H^1.0^2^2HMM^1^1.0^15H20260714.000000^0.001^1000.0^6Hauthor^3Horg^11^0^0H^0H!"
    );
    let bytes = fixed_ascii_with_global(global.as_bytes());

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"parameter_delimiter=^".into()));
    assert!(summary.notes.contains(&"record_delimiter=!".into()));
    assert!(summary.notes.contains(&format!("sender_product={product}")));
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(summary.notes.contains(&"units=MM".into()));
}

#[test]
fn global_defaults_apply_only_to_omitted_fields() {
    let global =
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,,,2HIN,1,1.0,1Hd,0,1,1Ha,1Ho,,0,0H,0H;";
    let bytes = fixed_ascii_with_global(global);
    let scan = crate::card::scan(&bytes).unwrap();
    let parsed = crate::global::parse(&scan).unwrap();

    assert_eq!(parsed.model_scale(), 1.0);
    assert_eq!(parsed.units_flag(), 1);
    assert_eq!(parsed.version_flag(), 3);
    assert_eq!(parsed.minimum_resolution_mm(), 0.0);
}

#[test]
fn malformed_global_integer_does_not_select_its_default() {
    let global = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2.,2HMM,1,1.0,1Hd,0.001,1,1Ha,1Ho,11,0,0H,0H;";
    let error = IgesCodec
        .inspect(
            &mut Cursor::new(fixed_ascii_with_global(global)),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "malformed container: IGES Global: field 14 (units flag) is not an integer"
    );
}

#[test]
fn real_significance_fields_are_required_and_positive() {
    for (global, field) in [
        (
            b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,,308,15,0H,1.0,2,2HMM,1,1.0,1Hd,0.001,1,1Ha,1Ho,11,0,0H,0H;".as_slice(),
            9,
        ),
        (
            b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,0,0H,1.0,2,2HMM,1,1.0,1Hd,0.001,1,1Ha,1Ho,11,0,0H,0H;".as_slice(),
            11,
        ),
    ] {
        let error = IgesCodec
            .inspect(
                &mut Cursor::new(fixed_ascii_with_global(global)),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap_err();

        assert!(
            error.to_string().contains(&format!("field {field}")),
            "{error}"
        );
    }
}

#[test]
fn other_units_require_an_exact_supported_standard_name() {
    let global = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,3,2Hmm,1,1.0,1Hd,0.001,1,1Ha,1Ho,11,0,0H,0H;";
    let error = IgesCodec
        .inspect(
            &mut Cursor::new(fixed_ascii_with_global(global)),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("field 15 (units name) is not a supported standard unit name"));
}

#[test]
fn minimum_resolution_is_required_and_cannot_be_negative() {
    for (resolution, expected) in [
        ("", "field 19 (minimum resolution) has no value"),
        (
            "-0.001",
            "field 19 (minimum resolution) must be finite and nonnegative",
        ),
    ] {
        let global = format!(
            "1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,1Hd,{resolution},1,1Ha,1Ho,11,0,0H,0H;"
        );
        let error = IgesCodec
            .inspect(
                &mut Cursor::new(fixed_ascii_with_global(global.as_bytes())),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn non_utf8_global_identifiers_are_preserved_as_exact_hex_attributes() {
    let mut bytes = point_file();
    let product = bytes
        .windows(9)
        .position(|window| window == b"7Hproduct")
        .expect("sender product");
    bytes[product + 5] = 0xff;
    let file_name = bytes
        .windows(10)
        .position(|window| window == b"8Hpart.igs")
        .expect("native file name");
    bytes[file_name + 4] = 0xfe;

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    assert_eq!(attributes["sender_product_bytes_hex"], "70726fff756374");
    assert_eq!(attributes["native_file_name_bytes_hex"], "7061fe742e696773");
    assert!(!attributes.contains_key("sender_product"));
    assert!(!attributes.contains_key("native_file_name"));
}

#[test]
fn inspect_reports_directory_entity_and_form_census() {
    let bytes = point_file();

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"entities=1".into()));
    assert!(summary.notes.contains(&"entity.116.form.0=1".into()));
    assert!(summary.notes.contains(&"parameter_records=1".into()));
    assert!(summary.notes.contains(&"parameter_tokens=4".into()));
}

#[test]
fn decode_treats_subordinate_switch_three_as_physically_dependent() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().geometry_transferred);
    assert!(result.report().losses.is_empty());
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["directions"].len(), 1);
    let direction_fields = native.arenas["directions"][0].fields();
    let components = direction_fields["components"].as_array().unwrap();
    assert_eq!(components[0], 2.0);
    assert_eq!(components[1], -3.0);
    assert_eq!(components[2], 4.0);
    assert_eq!(
        native.arenas["directions"][0].fields()["physically_dependent"],
        true
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_concatenates_ordered_composite_curve_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected a concatenated NURBS cache");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(nurbs.control_points.len(), 3);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &nurbs.knots, &nurbs.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn composite_join_uses_global_resolution_and_reports_degradation() {
    let within_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.000_999)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let within_curve = within_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .expect("Type 102 curve within the Global resolution");
    assert!(matches!(
        within_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
    ));
    assert!(within_resolution.report().losses.is_empty());

    let outside_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let outside_curve = outside_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .expect("degraded Type 102 curve");
    let cadmpeg_ir::geometry::CurveGeometry::Composite { segments, .. } = &outside_curve.geometry
    else {
        panic!("expected retained native Type 102 carrier")
    };
    assert_eq!(
        segments[1].transition,
        cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous
    );
    assert!(outside_resolution.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)
            && loss.message.contains("Global minimum resolution")
    }));
    let validation = cadmpeg_ir::validate_neutral(
        outside_resolution.ir(),
        outside_resolution.report().losses.clone(),
    );
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_concatenates_exact_circular_arc_and_line_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_analytic_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected an exact quadratic composite cache");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 5);
    assert_eq!(
        nurbs.weights.as_ref().unwrap()[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_converts_heterogeneous_composite_curve_children_to_an_exact_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(heterogeneous_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected an exact heterogeneous composite carrier");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 5);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_mixed_degree_composite_pcurve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_degree_composite_pcurve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D7")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("expected an elevated cubic composite cache");
    };
    assert_eq!(nurbs.degree, 3);
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge
                .curve
                .as_ref()
                .is_some_and(|id| id.0 == "iges:model:curve#D7"))
            .and_then(|edge| edge.param_range),
        Some([0.0, 2.0])
    );
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D11")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 1);
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(matches!(
        result.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree: 3, .. }
    ));
    assert_eq!(result.ir().model.pcurves[0].fit_tolerance, None);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_composite_curve_with_an_inconsistent_parametric_spline_child() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .expect("composite curve should be projected after its spline child");
    assert!(matches!(
        composite.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
    ));
    assert_eq!(result.report().losses.len(), 1);
    assert!(result.report().losses[0]
        .message
        .contains("terminal derivative block disagrees with the last polynomial"));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
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
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
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
    assert!(
        ruled.report().losses.is_empty(),
        "{:#?}",
        ruled.report().losses
    );
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
    assert_eq!(surface.v_degree, 2);
    assert_eq!(surface.weights.as_ref().unwrap().len(), 6);
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
    assert_eq!(surface.control_points[0].x, 11.0);
    let procedural = &result.ir().model.procedural_surfaces[0];
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
        directrix,
        axis_origin,
        ..
    } = &procedural.definition
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
        .find(|surface| surface.surface.0 == "iges:model:surface#D1")
        .expect("Type 128 parameter-domain record");
    assert_eq!(
        procedural.record_bounds,
        Some([Some(0.2), Some(0.8), Some(-1.0), Some(1.0)])
    );
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Exact {
        parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges { ranges },
        ..
    } = &procedural.definition
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
fn decode_rejects_file_duplicate_drawing_sheet_ids() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(duplicate_drawing_sheet_ids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss
                .message
                .contains("property value layout, attachment, or owner kind is invalid"))
            .count(),
        2,
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_orthographic_and_perspective_views() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let views = &result.ir().native.namespace("iges").unwrap().arenas["views"];
    assert_eq!(views.len(), 3);
    assert_eq!(views[0].fields()["projection"], "orthographic_parallel");
    assert!(views[0].fields()["scale"].is_null());
    assert_eq!(
        views[0].fields()["clipping_planes"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(views[1].fields()["projection"], "perspective");
    assert_eq!(views[1].fields()["view_plane_normal"][2], 1.0);
    assert_eq!(views[1].fields()["center_of_projection"][2], 10.0);
    assert_eq!(views[1].fields()["clipping_window"][0], -2.0);
    assert_eq!(views[1].fields()["depth_clipping"], 3);
    assert_eq!(views[2].fields()["view_plane_normal"][2], 1.0e-200);
    assert_eq!(views[2].fields()["view_up"][1], 1.0e-200);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_applies_defaults_and_accepts_zero_text_box_dimensions() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(defaulted_text_and_view_fields_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_view_visibility_and_display_overrides() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_visibility_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let visibility = &result.ir().native.namespace("iges").unwrap().arenas["view_visibility"];
    assert_eq!(visibility.len(), 2);
    assert_eq!(visibility[0].fields()["form"], 3);
    assert_eq!(
        visibility[0].fields()["displays"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert!(visibility[0].fields()["displays"][0]["line_font"].is_null());
    assert_eq!(visibility[1].fields()["form"], 4);
    assert_eq!(visibility[1].fields()["displays"][0]["line_font"], 1);
    assert_eq!(visibility[1].fields()["displays"][0]["color"], 2);
    assert_eq!(visibility[1].fields()["displays"][0]["line_weight"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_ordered_segmented_view_display() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(segmented_view_visibility_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let segmented =
        &result.ir().native.namespace("iges").unwrap().arenas["segmented_visibility"][0];
    assert_eq!(segmented.fields()["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(segmented.fields()["blocks"][0]["breakpoint"], 0.5);
    assert_eq!(segmented.fields()["blocks"][0]["color"]["kind"], "omitted");
    assert_eq!(segmented.fields()["blocks"][1]["breakpoint"], 1.0);
    assert_eq!(segmented.fields()["blocks"][1]["color"]["value"], 2);
    assert_eq!(segmented.fields()["blocks"][1]["line_font"]["value"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_drawing_view_placement_annotations_and_sheet_properties() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(drawing_with_properties_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let drawing = &result.ir().native.namespace("iges").unwrap().arenas["drawings"][0];
    assert_eq!(drawing.fields()["form"], 1);
    assert_eq!(
        drawing.fields()["views"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert_eq!(drawing.fields()["views"][0]["origin"][0], 10.0);
    assert_eq!(drawing.fields()["views"][0]["rotation"], 0.5);
    assert_eq!(
        drawing.fields()["annotations"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(drawing.fields()["size"][0], 210.0);
    assert_eq!(drawing.fields()["size"][1], 297.0);
    assert_eq!(drawing.fields()["units_flag"], 2);
    assert_eq!(drawing.fields()["units_name"][0], 77);
    assert_eq!(drawing.fields()["name"][0], 68);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_view_list_with_required_back_pointers() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let view_list = result.ir().native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "view_list")
        .unwrap();
    assert_eq!(view_list.fields()["declared_visible_count"], 1);
    assert_eq!(view_list.fields()["view"], "iges:entity:directory#1");
    assert_eq!(
        view_list.fields()["visible_entities"][0],
        "iges:entity:directory#5"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );

    let missing = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(missing.report().losses.iter().any(|loss| {
        loss.message.contains("entity type 402 form 6")
            && loss.message.contains("predefined associativity")
    }));
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
            result.ir().model.procedural_surfaces[0].definition
        else {
            panic!("expected an offset dependency");
        };
        assert_eq!(distance, expected_z);
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
        .all(|surface| surface.id.0 != "iges:model:surface#D3"));
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
    assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
    assert_eq!(
        nurbs.control_points,
        vec![
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
fn fixed_ascii_5_1_and_5_2_decode_under_the_supported_profile() {
    for (encoded_version, version_name) in [(b"09", "5.1"), (b"10", "5.2")] {
        let mut bytes = point_file();
        let version = bytes
            .windows(b",11,0,".len())
            .position(|window| window == b",11,0,")
            .unwrap();
        bytes[version + 1..version + 3].copy_from_slice(encoded_version);

        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(bytes.clone()),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(summary
            .notes
            .contains(&format!("iges_version={version_name}")));
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["iges_version"],
            version_name
        );
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(
            result.report().losses.is_empty(),
            "{version_name}: {:#?}",
            result.report().losses
        );
        assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
    }
}

mod writer;

#[path = "integration_tests.rs"]
mod integration_tests;
