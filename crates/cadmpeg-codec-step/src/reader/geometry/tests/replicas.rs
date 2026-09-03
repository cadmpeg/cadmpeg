// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::{model_curve_point_by_id, model_surface_point_by_id};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::export::is_rigid_transform;
use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::test_support::decode_inline;
use crate::{write_step, StepCodec, StepSchema, StepWriteOptions};

#[test]
fn rigid_transform_rejects_reflections() {
    assert!(!is_rigid_transform(&[
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
}

#[test]
fn placement_reference_is_projected_and_angular_trims_use_context_units() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.017453292519943295),#2);
#4=(CONVERSION_BASED_UNIT('degree',#3) NAMED_UNIT(*) PLANE_ANGLE_UNIT());
#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,1.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=CIRCLE('',#13,2.);
#15=TRIMMED_CURVE('',#14,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);
#16=GEOMETRIC_CURVE_SET('',(#15));
#17=SHAPE_REPRESENTATION('',(#16),#5);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("circle");
    let CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    } = circle.geometry
    else {
        panic!("decoded carrier is not a circle")
    };
    let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
    assert!(dot.abs() < 1.0e-12);
    assert!(result
        .ir()
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range: [start, end],
                ..
            } if start.abs() < 1.0e-12 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
        )));
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("LINE #14 parameter scale did not resolve")
    }));
}

#[test]
fn omitted_placement_reference_uses_the_first_projected_axis() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.6,0.8,0.));
#3=AXIS2_PLACEMENT_3D('',#1,#2,$);
#4=CIRCLE('',#3,2.);
#5=GEOMETRIC_CURVE_SET('',(#4));
#6=SHAPE_REPRESENTATION('',(#5),$);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#4")
        .expect("circle");
    let CurveGeometry::Circle { ref_direction, .. } = circle.geometry else {
        panic!("decoded carrier is not a circle");
    };
    assert!((ref_direction.x - 0.8).abs() < 1.0e-12);
    assert!((ref_direction.y + 0.6).abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
}

#[test]
fn near_parallel_omitted_reference_uses_a_stable_projected_axis() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(-1.,0.0000000612905015206,0.0000000692801624183));
#12=AXIS2_PLACEMENT_3D('',#10,#11,$);
#13=CIRCLE('',#12,2.);
#14=GEOMETRIC_CURVE_SET('',(#13));
#15=SHAPE_REPRESENTATION('',(#14),#3);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#13")
        .expect("circle");
    let CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    } = circle.geometry
    else {
        panic!("decoded carrier is not a circle");
    };
    let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
    assert!(ref_direction.y > 0.999_999_999);
    assert!(dot.abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn placement_reference_witness_covers_default_axes_and_invalid_parallel_input() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc06_placement_reference.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode placement reference witness");

    let expected = [
        ("#6", (0.8, -0.6, 0.0)),
        ("#9", (0.0, 1.0, 0.0)),
        ("#12", (0.0, 1.0, 0.0)),
    ];
    for (source_id, (x, y, z)) in expected {
        let curve = decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == format!("step:data:curve{source_id}"))
            .expect("witness circle");
        let CurveGeometry::Circle { ref_direction, .. } = curve.geometry else {
            panic!("witness carrier is not a circle");
        };
        assert!((ref_direction.x - x).abs() < 1.0e-12);
        assert!((ref_direction.y - y).abs() < 1.0e-12);
        assert!((ref_direction.z - z).abs() < 1.0e-12);
    }

    let near_axis = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#15")
        .expect("near-axis witness circle");
    let CurveGeometry::Circle { ref_direction, .. } = near_axis.geometry else {
        panic!("near-axis witness carrier is not a circle");
    };
    assert!(ref_direction.y > 0.999_999_999);

    let parallel_reference = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#18")
        .expect("parallel-reference witness circle");
    let CurveGeometry::Circle { ref_direction, .. } = parallel_reference.geometry else {
        panic!("parallel-reference witness carrier is not a circle");
    };
    assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
    assert!(ref_direction.y.abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PlacementReferenceInferred.kind()
            && loss.message.contains("AXIS2_PLACEMENT_3D #17")
    }));
}

#[test]
fn parallel_axis_reference_direction_is_reported_and_inferred() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(0.,0.,2.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CIRCLE('',#4,2.);
#6=GEOMETRIC_CURVE_SET('',(#5));
#7=SHAPE_REPRESENTATION('',(#6),$);",
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PlacementReferenceInferred.kind()
            && loss.message.contains("AXIS2_PLACEMENT_3D #4")
    }));
}

#[test]
fn trimmed_curve_replica_keeps_parent_parameterization_for_both_selectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,2.);
#6=LINE('',#1,#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#8=CURVE_REPLICA('',#6,#7);
#9=TRIMMED_CURVE('',#8,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#10=CARTESIAN_POINT('',(6.,0.,0.));
#11=CARTESIAN_POINT('',(12.,0.,0.));
#12=TRIMMED_CURVE('',#8,(#10),(#11),.T.,.CARTESIAN.);
#13=GEOMETRIC_CURVE_SET('',(#9,#12));
#14=SHAPE_REPRESENTATION('',(#13),$);",
    );

    for (curve_id, expected) in [("#9", [2.0, 4.0]), ("#12", [2.0, 4.0])] {
        let construction_id =
            StepIdentity::construction("trimmed_curve", curve_id.trim_start_matches('#'));
        assert!(result.ir().model.procedural_curves.iter().any(|curve| {
            curve.id.as_str() == construction_id
                && matches!(
                    curve.definition(),
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                        parameter_range,
                        ..
                    } if *parameter_range == expected
                )
        }));
    }

    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if curve.curve.as_str() == "step:data:curve#8"
                    && source.as_str() == "step:data:curve#6"
        )
    }));
    let index = ModelIndex::new(result.ir());
    assert_eq!(
        model_curve_point_by_id(&index, &CurveId("step:data:curve#9".into()), 0.0,),
        Some(Point3::new(6.0, 0.0, 0.0))
    );
    assert_eq!(
        model_curve_point_by_id(&index, &CurveId("step:data:curve#9".into()), 2.0,),
        Some(Point3::new(12.0, 0.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write trimmed replica");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("TRIMMED_CURVE"));
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed replica");
    assert!(round_trip.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if source.as_str().starts_with("step:data:curve#")
        )
    }));
}

#[test]
fn transformed_curves_and_surfaces_round_trip_through_step_replicas() {
    let transform = Transform {
        rows: [
            [0.0, -2.0, 0.0, 10.0],
            [2.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let curve_geometry = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    let surface_geometry = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    let mut source = CadIr::empty(Units::default());
    source.model.curves.push(Curve {
        id: CurveId("transformed-curve".into()),
        geometry: curve_geometry.clone(),
        source_object: None,
    });
    source.model.surfaces.push(Surface {
        id: SurfaceId("transformed-surface".into()),
        geometry: surface_geometry.clone(),
        source_object: None,
    });

    let mut output = Vec::new();
    write_step(
        &source,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write replicas");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_3D"));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode replicas");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == curve_geometry));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.geometry == surface_geometry));
}

#[test]
fn surface_replica_dependencies_resolve_before_trimmed_surfaces() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#6=PLANE('',#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,2.,#4);
#8=SURFACE_REPLICA('',#9,#7);
#9=SURFACE_REPLICA('',#6,#7);
#10=RECTANGULAR_TRIMMED_SURFACE('',#8,0.,1.,0.,1.,.T.,.T.);
#11=GEOMETRIC_SET('',(#10));
#12=SHAPE_REPRESENTATION('',(#11),#13);
#13=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#10"
            && matches!(surface.geometry, SurfaceGeometry::Transformed { .. })
    }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            surface.surface.as_str() == "step:data:surface#10"
                && matches!(
                    surface.definition(),
                    cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
                        support,
                        parameter_ranges: [[0.0, 1.0], [0.0, 1.0]],
                        u_sense: Some(true),
                        v_sense: Some(true),
                    } if support.as_str() == "step:data:surface#8"
                )
        }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("RECTANGULAR_TRIMMED_SURFACE #10 has invalid or unresolved")
    }));

    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if surface.surface.as_str() == "step:data:surface#8"
                        && source.as_str() == "step:data:surface#9"
            )
        }));
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_surface_point_by_id(&index, &SurfaceId("step:data:surface#10".into()), 0.0, 0.0,),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &SurfaceId("step:data:surface#10".into()), 1.0, 1.0,),
        Some(Point3::new(4.0, 4.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write trimmed surface replica");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed surface replica");
    assert!(round_trip
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if source.as_str().starts_with("step:data:surface#")
            )
        }));
}

#[test]
fn forward_replica_dependencies_resolve_to_nested_transforms() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
#9=CURVE_REPLICA('',#10,#8);
#10=CURVE_REPLICA('',#6,#8);
#11=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#12=PLANE('',#11);
#13=SURFACE_REPLICA('',#14,#8);
#14=SURFACE_REPLICA('',#12,#8);",
    );
    let transform = Transform {
        rows: [
            [2.0, 0.0, 0.0, 10.0],
            [0.0, 2.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let base_curve = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    let expected_curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Transformed {
            basis: Box::new(base_curve),
            transform,
        }),
        transform,
    };
    let expected_surface = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Transformed {
            basis: Box::new(SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            transform,
        }),
        transform,
    };
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#9" && curve.geometry == expected_curve));
    assert_eq!(
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#6")
            .and_then(|curve| curve.source_object.as_ref())
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#13"
            && surface.geometry == expected_surface));
    assert_eq!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#12")
            .and_then(|surface| surface.source_object.as_ref())
            .map(|source| source.object_id.as_str()),
        Some("#14")
    );
}

#[test]
fn cartesian_transformation_operator_derives_optional_axes() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(10.,20.,30.));
#2=CARTESIAN_TRANSFORMATION_OPERATOR_3D('', $,$,#1,$,$);
#3=CARTESIAN_POINT('',(0.,0.,0.));
#4=DIRECTION('',(1.,0.,0.));
#5=VECTOR('',#4,1.);
#6=LINE('',#3,#5);
#7=CURVE_REPLICA('',#6,#2);
#8=DIRECTION('',(1.,1.,0.));
#9=DIRECTION('',(0.,0.,1.));
#10=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#8,$,#3,2.,#9);
#11=CURVE_REPLICA('',#6,#10);
#12=GEOMETRIC_CURVE_SET('',(#7,#11));
#13=SHAPE_REPRESENTATION('',(#12),$);",
    );

    let transform_for = |id: &str| {
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| match &curve.geometry {
                CurveGeometry::Transformed { transform, .. } => Some(*transform),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing transformed curve {id}"))
    };
    let assert_rows = |actual: Transform, expected: [[f64; 4]; 4]| {
        for (row, values) in expected.iter().enumerate() {
            for (column, expected) in values.iter().enumerate() {
                assert!(
                    (actual.rows[row][column] - expected).abs() < 1.0e-12,
                    "matrix coefficient [{row}][{column}] was {}, expected {expected}",
                    actual.rows[row][column]
                );
            }
        }
    };

    assert_rows(
        transform_for("step:data:curve#7"),
        [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
    let root_two = 2.0_f64.sqrt();
    assert_rows(
        transform_for("step:data:curve#11"),
        [
            [root_two, -root_two, 0.0, 0.0],
            [root_two, root_two, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
}

#[test]
fn pcurve_replica_derives_orthogonal_two_dimensional_axes() {
    use cadmpeg_ir::geometry::PcurveGeometry;

    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);",
            "#55=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#71=DIRECTION('',(1.,1.));\n#72=CARTESIAN_TRANSFORMATION_OPERATOR_2D('',#71,$,#51,1.);\n#73=CURVE_REPLICA('',#54,#72);",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(0.7071067811865476,0.7071067811865476,0.));",
        )
        .replace(
            "#16=LINE('',#3,#13);",
            "#74=DIRECTION('',(0.7071067811865476,0.7071067811865476,0.));\n#75=VECTOR('',#74,1.);\n#16=LINE('',#3,#75);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pcurve replica");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("replica pcurve");
    let PcurveGeometry::Transformed { transform, .. } = &pcurve.geometry else {
        panic!("pcurve replica lost its transformation")
    };
    let root_two = 2.0_f64.sqrt();
    assert!((transform.rows[0][0] - 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows[0][1] + 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows[1][0] - 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows[1][1] - 1.0 / root_two).abs() < 1.0e-12);
}

#[test]
fn long_forward_curve_replica_chain_resolves_with_a_worklist() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
",
    );
    for id in 9..=264 {
        writeln!(records, "#{id}=CURVE_REPLICA('',#{},#8);", id + 1).expect("append curve replica");
    }
    records.push_str("#265=CURVE_REPLICA('',#6,#8);");

    let decoded = decode_inline(&records);
    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#9"
            && matches!(curve.geometry, CurveGeometry::Transformed { .. })
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("CURVE_REPLICA #9 has invalid or unresolved parent/operator")
    }));
}

#[test]
fn long_forward_offset_surface_chain_resolves_with_a_worklist() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=PLANE('',#4);
",
    );
    for id in 6..=261 {
        writeln!(records, "#{id}=OFFSET_SURFACE('',#{},1.,.F.);", id + 1)
            .expect("append offset surface");
    }
    records.push_str("#262=OFFSET_SURFACE('',#5,1.,.F.);\n#263=GEOMETRIC_SET('',(#6));");

    let decoded = decode_inline(&records);
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("OFFSET_SURFACE #6 has invalid or unresolved support parameters")
    }));
}

#[test]
fn replicas_retain_bounded_parent_relations() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=TRIMMED_CURVE('',#6,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#9=CURVE_REPLICA('',#7,#8);
#10=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#11=PLANE('',#10);
#12=RECTANGULAR_TRIMMED_SURFACE('',#11,1.,2.,3.,4.,.T.,.T.);
#13=SURFACE_REPLICA('',#12,#8);
#14=GEOMETRIC_SET('',(#9,#13));
#15=SHAPE_REPRESENTATION('',(#14),#16);
#16=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));",
    );

    assert!(decoded.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if curve.curve.as_str() == "step:data:curve#9"
                    && source.as_str() == "step:data:curve#7"
        )
    }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if surface.surface.as_str() == "step:data:surface#13"
                        && source.as_str() == "step:data:surface#12"
            )
        }));
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_curve_point_by_id(&index, &CurveId("step:data:curve#9".into()), 0.0,),
        Some(Point3::new(3.0, 0.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &SurfaceId("step:data:surface#13".into()), 0.0, 0.0,),
        Some(Point3::new(3.0, 9.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write replicas of bounded parents");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("TRIMMED_CURVE"));
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
}

#[test]
fn mapped_representation_dag_is_memoized() {
    let depth = 32_u64;
    let mut records = String::from(
        "#1=APPLICATION_CONTEXT('');\n\
#2=PRODUCT('p','p','',());\n\
#3=PRODUCT_DEFINITION_FORMATION('','',#2);\n\
#4=PRODUCT_DEFINITION('','',#3,#1);\n\
#5=PRODUCT_DEFINITION_SHAPE('','',#4);\n\
#6=SHAPE_DEFINITION_REPRESENTATION(#5,#100);\n",
    );
    for level in 0..depth {
        let representation = 100 + level;
        let next = representation + 1;
        let map = 1_000 + level;
        let first = 2_000 + level * 2;
        let second = first + 1;
        write!(
            records,
            "#{representation}=SHAPE_REPRESENTATION('',(#{first},#{second}),$);\n\
#{map}=REPRESENTATION_MAP($,#{next});\n\
#{first}=MAPPED_ITEM('',#{map},$);\n\
#{second}=MAPPED_ITEM('',#{map},$);\n"
        )
        .expect("write mapped representation fixture");
    }
    write!(
        records,
        "#{}=SHAPE_REPRESENTATION('',(#9000),$);\n\
#9000=MANIFOLD_SOLID_BREP('',#9001);\n\
#9001=CLOSED_SHELL('',(#9002));\n\
#9002=ADVANCED_FACE('',(#9003),#9004,.T.);\n\
#9003=FACE_OUTER_BOUND('',#9005,.T.);\n\
#9005=VERTEX_LOOP('',#9006);\n\
#9006=VERTEX_POINT('',#9007);\n\
#9007=CARTESIAN_POINT('',(0.,0.,0.));\n\
#9004=PLANE('',#9008);\n\
#9008=AXIS2_PLACEMENT_3D('',#9007,$,$);",
        100 + depth
    )
    .expect("write terminal representation fixture");

    let result = decode_inline(&records);
    assert_eq!(result.ir().model.product_definitions.len(), 1);
    assert_eq!(result.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        result.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#9000"
    );
}
