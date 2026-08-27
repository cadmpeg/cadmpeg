// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::{model_surface_partials_by_id, model_surface_point_by_id};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::loss::StepLossCode;
use crate::test_support::decode_inline;
use crate::{write_step, StepCodec, StepSchema, StepWriteOptions};

#[test]
fn rectangular_trimmed_surface_preserves_basis_ranges_and_senses() {
    let source = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));\
#3=CARTESIAN_POINT('',(0.,0.,0.));\
#4=DIRECTION('',(0.,0.,1.));\
#5=DIRECTION('',(1.,0.,0.));\
#6=AXIS2_PLACEMENT_3D('',#3,#4,#5);\
#7=PLANE('',#6);\
#8=RECTANGULAR_TRIMMED_SURFACE('trim',#7,3.,1.,4.,2.,.F.,.F.);\
#9=GEOMETRIC_SET('',(#8));\
#10=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#9),#2);";
    let decoded = decode_inline(source);
    let trimmed = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#8")
        .expect("trimmed surface carrier");
    assert!(matches!(trimmed.geometry, SurfaceGeometry::Plane { .. }));
    let procedural = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface.as_str() == "step:data:surface#8")
        .expect("trimmed surface construction");
    assert!(matches!(
        &procedural.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
            support,
            parameter_ranges: [[3.0, 1.0], [4.0, 2.0]],
            u_sense: Some(false),
            v_sense: Some(false),
        } if support.as_str() == "step:data:surface#7"
    ));
    let index = ModelIndex::new(decoded.ir());
    let trimmed_id = SurfaceId("step:data:surface#8".into());
    assert_eq!(
        model_surface_point_by_id(&index, &trimmed_id, 0.0, 0.0),
        Some(Point3::new(3.0, 4.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &trimmed_id, 2.0, 2.0),
        Some(Point3::new(1.0, 2.0, 0.0))
    );
    let partials = model_surface_partials_by_id(&index, &trimmed_id, 1.0, 1.0)
        .expect("trimmed surface partials");
    assert_eq!(partials.du, Vector3::new(-1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, -1.0, 0.0));

    let mut output = Vec::new();
    let report = write_step(
        decoded.ir(),
        &mut output,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("write trimmed surface");
    let text = String::from_utf8(output.clone()).expect("UTF-8 STEP");
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("procedural surface definition(s) and")
    }));

    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed surface");
    let round_trip = round_trip
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            matches!(
                &surface.definition,
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
                    parameter_ranges: [[3.0, 1.0], [4.0, 2.0]],
                    u_sense: Some(false),
                    v_sense: Some(false),
                    ..
                }
            )
        })
        .expect("round-trip trimmed surface construction");
    assert!(matches!(
        &round_trip.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
            parameter_ranges: [[3.0, 1.0], [4.0, 2.0]],
            u_sense: Some(false),
            v_sense: Some(false),
            ..
        }
    ));
}

#[test]
fn rectangular_trimmed_surface_unwraps_cyclic_basis_parameters() {
    let source = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CYLINDRICAL_SURFACE('',#4,2.);
#6=RECTANGULAR_TRIMMED_SURFACE('trim',#5,5.5,.5,1.,3.,.T.,.T.);
#7=GEOMETRIC_SET('',(#6));
#8=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#7),#9);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));";
    let decoded = decode_inline(source);
    let construction = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface.as_str() == "step:data:surface#6")
        .expect("cyclic trimmed surface construction");
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
        parameter_ranges,
        u_sense: Some(true),
        v_sense: Some(true),
        ..
    } = &construction.definition
    else {
        panic!(
            "unexpected cyclic trimmed definition: {:?}",
            construction.definition
        );
    };
    assert!((parameter_ranges[0][0] - 5.5).abs() < 1.0e-12);
    assert!((parameter_ranges[0][1] - (0.5 + std::f64::consts::TAU)).abs() < 1.0e-12);
    let index = ModelIndex::new(decoded.ir());
    let point = model_surface_point_by_id(
        &index,
        &SurfaceId("step:data:surface#6".into()),
        parameter_ranges[0][1] - parameter_ranges[0][0],
        1.0,
    )
    .expect("cyclic trimmed endpoint");
    assert!((point.x - 2.0 * 0.5_f64.cos()).abs() < 1.0e-12);
    assert!((point.y - 2.0 * 0.5_f64.sin()).abs() < 1.0e-12);
    assert!((point.z - 2.0).abs() < 1.0e-12);
}

#[test]
fn rectangular_trimmed_surface_unwraps_both_periodic_directions_and_senses() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc05_periodic_trim.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode periodic trim witness");

    let expected = [
        (
            "step:data:surface#8",
            [[5.5, 0.5 + std::f64::consts::TAU], [0.5, 4.5]],
            true,
            true,
        ),
        (
            "step:data:surface#9",
            [
                [5.5, 0.5 + std::f64::consts::TAU],
                [0.5 + std::f64::consts::TAU, 4.5],
            ],
            true,
            false,
        ),
        (
            "step:data:surface#10",
            [
                [0.5 + std::f64::consts::TAU, 5.5],
                [5.5, 0.5 + std::f64::consts::TAU],
            ],
            false,
            true,
        ),
        (
            "step:data:surface#11",
            [[4.5, 0.5], [4.5, 0.5]],
            false,
            false,
        ),
    ];
    for (surface_id, expected_ranges, expected_u_sense, expected_v_sense) in expected {
        let construction = decoded
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|surface| surface.surface.as_str() == surface_id)
            .expect("periodic trimmed surface construction");
        assert!(matches!(
            &construction.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
                parameter_ranges,
                u_sense: Some(u_sense),
                v_sense: Some(v_sense),
                ..
            } if parameter_ranges
                .iter()
                .flatten()
                .zip(expected_ranges.iter().flatten())
                .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12)
                && *u_sense == expected_u_sense
                && *v_sense == expected_v_sense
        ));
    }

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn rectangular_trimmed_surface_keeps_topology_pcurves_in_local_uv_space() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#28=PLANE('',#27);",
        "#58=PLANE('',#27);\n#28=RECTANGULAR_TRIMMED_SURFACE('',#58,0.,10.,0.,10.,.T.,.T.);",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode trimmed sheet");
    let face = decoded.ir().model.faces.first().expect("trimmed face");
    assert_eq!(face.surface.as_str(), "step:data:surface#28");
    let construction = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface == face.surface)
        .expect("trimmed face construction");
    assert!(matches!(
        &construction.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
            support,
            parameter_ranges: [[0.0, 10.0], [0.0, 10.0]],
            u_sense: Some(true),
            v_sense: Some(true),
        } if support.as_str() == "step:data:surface#58"
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn catia_cartesian_trim_points_resolve_on_nurbs_curve() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#2,#3),.UNSPECIFIED.,.U.,.U.,(3,3),(0.,2.),.UNSPECIFIED.);
#5=TRIMMED_CURVE('',#4,(#1),(#3),.T.,.CARTESIAN.);
#6=COMPOSITE_CURVE_SEGMENT(.DISCONTINUOUS.,.T.,#5);
#7=COMPOSITE_CURVE('',(#6),.U.);
#8=GEOMETRIC_SET('NONE',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert_eq!(result.ir().model.curves.len(), 3);
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss.message.contains("UNKNOWN periodicity")
            && loss.message.contains("#4")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn line_numeric_trim_uses_vector_magnitude_and_length_unit() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=CARTESIAN_POINT('',(2.,0.,0.));
#12=DIRECTION('',(1.,0.,0.));
#13=VECTOR('',#12,2.);
#14=LINE('',#10,#13);
#15=TRIMMED_CURVE('',#14,(#11),(PARAMETER_VALUE(1.)),.T.,.UNSPECIFIED.);
#16=GEOMETRIC_CURVE_SET('',(#15));
#17=SHAPE_REPRESENTATION('',(#16),#2);",
    );
    assert!(result
        .ir()
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range: [start, end],
                ..
            } if (start - 2.0).abs() < 1.0e-12 && (end - 2.0).abs() < 1.0e-12
        )));
}

#[test]
fn trimmed_curve_prefers_the_parameter_value_under_parameter_master() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#30=CARTESIAN_POINT('',(1.,0.,0.));
#31=CARTESIAN_POINT('',(0.,-1.,0.));
#40=TRIMMED_CURVE('',#24,(#30,PARAMETER_VALUE(0.)), (#31,PARAMETER_VALUE(4.712388980)),.T.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("parameter-master trimmed curve");
    assert_eq!(parameter_range[0], 0.0);
    assert!((parameter_range[1] - 3.0 * std::f64::consts::PI / 2.0).abs() < 1.0e-9);
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("fell back to a Cartesian trim selector")
    }));
}

#[test]
fn trimmed_curve_prefers_the_point_under_cartesian_master() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#30=CARTESIAN_POINT('',(1.,0.,0.));
#31=CARTESIAN_POINT('',(0.,-1.,0.));
#40=TRIMMED_CURVE('',#24,(#30,PARAMETER_VALUE(0.)), (#31,PARAMETER_VALUE(4.712388980)),.T.,.CARTESIAN.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("Cartesian-master trimmed curve");
    assert_eq!(parameter_range[0], 0.0);
    assert!((parameter_range[1] - 3.0 * std::f64::consts::PI / 2.0).abs() < 1.0e-12);
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("fell back to a parameter trim selector")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn trimmed_curve_opposed_sense_retains_the_periodic_branch() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#40=TRIMMED_CURVE('',#24,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.5707963267948966)),.F.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("opposed-sense trimmed curve");
    assert!((parameter_range[0] - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!((parameter_range[1] - std::f64::consts::TAU).abs() < 1.0e-12);
    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { sense, .. }
                if curve.id.as_str() == "step:construction:trimmed_curve#40" && !sense
        )
    }));
    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("write opposed-sense trimmed curve");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains(".F.,.PARAMETER."));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn trimmed_curve_forward_sense_wraps_a_closed_basis() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#40=TRIMMED_CURVE('',#24,(PARAMETER_VALUE(5.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("forward trimmed curve");
    assert!((parameter_range[0] - 5.0).abs() < 1.0e-12);
    assert!((parameter_range[1] - (1.0 + std::f64::consts::TAU)).abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn trimmed_curve_reports_a_fallback_when_the_preferred_form_is_absent() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#30=CARTESIAN_POINT('',(1.,0.,0.));
#31=CARTESIAN_POINT('',(0.,-1.,0.));
#40=TRIMMED_CURVE('',#24,(#30), (#31,PARAMETER_VALUE(4.712388980)),.T.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == StepLossCode::DecodeWarning.kind()
                    && loss.message.contains("TRIMMED_CURVE #40")
                    && loss.message.contains("Cartesian trim selector")
            })
            .count(),
        1
    );
}
