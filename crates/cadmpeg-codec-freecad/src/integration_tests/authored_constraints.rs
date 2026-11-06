// SPDX-License-Identifier: Apache-2.0

use crate::FcstdCodec;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, ParameterValue, SketchFeatureBinding,
};
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::test_support::round_trip::measured;
use serde_json::{json, Value};
use std::io::Cursor;

const LENGTH_TOLERANCE: f64 = 1.0e-12;
const ANGLE_TOLERANCE: f64 = 1.0e-12;
const DIRECTION_TOLERANCE: f64 = 1.0e-12;
const RATIO_TOLERANCE: f64 = 1.0e-12;
const WEIGHT_TOLERANCE: f64 = 1.0e-12;
const KNOT_TOLERANCE: f64 = 1.0e-12;
const MEASURE_LENGTH_TOLERANCE: f64 = 1.0e-6;
const MEASURE_BBOX_TOLERANCE: f64 = 1.0e-6;

#[test]
fn authored_constraint_families_keep_operands_and_values() {
    let data = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/sketch_constraints.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    assert_eq!(model.sketches.len(), 7);
    assert_eq!(model.sketch_entities.len(), 43);
    assert_eq!(model.sketch_constraints.len(), 32);
    assert_eq!(model.parameters.len(), 9);
    for sketch in &model.sketches {
        let feature = model
            .features
            .iter()
            .find(|feature| feature.name == sketch.name)
            .unwrap();
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(id)),
            }) if id == &sketch.id
        ));
        assert!(!feature.evaluation.outputs().is_empty());
        let (origin, normal, u_axis) = sketch.placement.resolved().unwrap();
        for actual in origin.coordinates() {
            assert!(actual.get().abs() < LENGTH_TOLERANCE);
        }
        for (actual, expected) in normal.components().into_iter().zip([0.0, 0.0, 1.0]) {
            assert!((actual.get() - expected).abs() < DIRECTION_TOLERANCE);
        }
        for (actual, expected) in u_axis.components().into_iter().zip([1.0, 0.0, 0.0]) {
            assert!((actual.get() - expected).abs() < DIRECTION_TOLERANCE);
        }
    }
    let sketch = |name: &str| {
        model
            .sketches
            .iter()
            .find(|s| s.name.as_deref() == Some(name))
            .unwrap()
    };
    let entity = |name: &str, i: usize| {
        let s = sketch(name);
        let suffix = format!("#{name}:{i}");
        model
            .sketch_entities
            .iter()
            .find(|e| e.sketch == s.id && e.id().as_str().ends_with(&suffix))
            .unwrap()
            .id()
            .as_str()
            .to_owned()
    };
    let parameter = |name: &str, i: usize| format!("fcstd:design:parameter#{name}:constraint:{i}");
    let loc = |kind: &str, id: String| json!({"kind":kind,"entity":id});
    let mut cases: Vec<(&str, usize, Vec<Value>)> = Vec::new();
    let n = "GeometricRelations";
    cases.push((n,15,vec![
        json!({"kind":"horizontal","entity":entity(n,1)}),
        json!({"kind":"vertical","entity":entity(n,2)}),
        json!({"kind":"parallel","first":entity(n,3),"second":entity(n,4)}),
        json!({"kind":"perpendicular","first":entity(n,5),"second":entity(n,6)}),
        json!({"kind":"tangent","first":entity(n,7),"second":entity(n,8)}),
        json!({"kind":"equal","first":entity(n,14),"second":entity(n,15)}),
        json!({"kind":"coincident_loci","loci":[loc("end",entity(n,9)),loc("start",entity(n,10))]}),
        json!({"kind":"point_on_object","point":loc("start",entity(n,11)),"entity":entity(n,12)}),
        json!({"kind":"fixed","entity":entity(n,13)}),
    ]));
    let n = "DimensionalRelations";
    cases.push((n,9,vec![
        json!({"kind":"distance","entities":[entity(n,1)],"parameter":parameter(n,1)}),
        json!({"kind":"distance_loci","first":loc("start",entity(n,2)),"second":loc("start",entity(n,3)),"parameter":parameter(n,2)}),
        json!({"kind":"horizontal_distance","first":loc("start",entity(n,4)),"second":loc("end",entity(n,4)),"parameter":parameter(n,3)}),
        json!({"kind":"vertical_distance","first":loc("start",entity(n,5)),"second":loc("end",entity(n,5)),"parameter":parameter(n,4)}),
        json!({"kind":"angle","first":entity(n,6),"second":entity(n,7),"parameter":parameter(n,5)}),
        json!({"kind":"radius","entity":entity(n,8),"parameter":parameter(n,6)}),
        json!({"kind":"diameter","entity":entity(n,9),"parameter":parameter(n,7)}),
    ]));
    let n = "SymmetryRelation";
    cases.push((n,3,vec![json!({"kind":"symmetric","first":loc("start",entity(n,1)),"second":loc("start",entity(n,2)),"axis":entity(n,3)})]));
    let n = "InactiveRelation";
    cases.push((
        n,
        1,
        vec![json!({"kind":"horizontal","entity":entity(n,1)})],
    ));
    let n = "InternalAlignmentRelations";
    let alignment = |n: &str, helper, kind: &str| json!({"kind":"internal_alignment","helper":entity(n,helper),"parent":entity(n,1),"alignment":{"alignment":kind}});
    cases.push((
        n,
        5,
        vec![
            alignment(n, 2, "ellipse_major_diameter"),
            alignment(n, 3, "ellipse_minor_diameter"),
            alignment(n, 4, "ellipse_focus1"),
            alignment(n, 5, "ellipse_focus2"),
        ],
    ));
    let n = "OpticalRelation";
    cases.push((n,3,vec![json!({"kind":"snells_law","incident":loc("end",entity(n,1)),"refracted":loc("start",entity(n,2)),"interface":entity(n,3),"parameter":parameter(n,1)})]));
    let n = "SplineWeightRelation";
    let spline_alignment = |helper, kind: &str, index| json!({"kind":"internal_alignment","helper":entity(n,helper),"parent":entity(n,1),"alignment":{"alignment":kind,"index":index}});
    cases.push((
        n,
        7,
        vec![
            spline_alignment(2, "bspline_control_point", 0),
            json!({"kind":"weight","entity":entity(n,2),"parameter":parameter(n,2)}),
            spline_alignment(3, "bspline_control_point", 1),
            spline_alignment(4, "bspline_control_point", 2),
            json!({"kind":"equal","first":entity(n,4),"second":entity(n,2)}),
            spline_alignment(5, "bspline_control_point", 3),
            json!({"kind":"equal","first":entity(n,5),"second":entity(n,2)}),
            spline_alignment(6, "bspline_knot_point", 0),
            spline_alignment(7, "bspline_knot_point", 1),
        ],
    ));
    for (name, entity_count, expected) in cases {
        let s = sketch(name);
        assert_eq!(
            model
                .sketch_entities
                .iter()
                .filter(|e| e.sketch == s.id)
                .count(),
            entity_count,
            "{name}"
        );
        let constraints = model
            .sketch_constraints
            .iter()
            .filter(|c| c.sketch == s.id)
            .collect::<Vec<_>>();
        assert_eq!(constraints.len(), expected.len(), "{name}");
        for (index, expected) in expected.iter().enumerate() {
            let suffix = format!("#{name}:{}", index + 1);
            let constraint = constraints
                .iter()
                .find(|c| c.id.as_str().ends_with(&suffix))
                .unwrap();
            assert_eq!(
                serde_json::to_value(constraint.definition.kind()).unwrap(),
                *expected,
                "{name} constraint {}",
                index + 1
            );
            assert_eq!(
                constraint.active,
                Some(name != "InactiveRelation"),
                "{name}"
            );
            assert_eq!(constraint.driving, Some(true), "{name}");
        }
    }
    for (name, index, expected, kind, tolerance) in [
        ("DimensionalRelations", 1, 9.0, "length", LENGTH_TOLERANCE),
        ("DimensionalRelations", 2, 11.0, "length", LENGTH_TOLERANCE),
        ("DimensionalRelations", 3, 8.0, "length", LENGTH_TOLERANCE),
        ("DimensionalRelations", 4, 4.0, "length", LENGTH_TOLERANCE),
        (
            "DimensionalRelations",
            5,
            std::f64::consts::FRAC_PI_4,
            "angle",
            ANGLE_TOLERANCE,
        ),
        ("DimensionalRelations", 6, 4.0, "length", LENGTH_TOLERANCE),
        ("DimensionalRelations", 7, 8.0, "length", LENGTH_TOLERANCE),
        ("OpticalRelation", 1, 1.25, "real", RATIO_TOLERANCE),
        ("SplineWeightRelation", 2, 1.0, "real", WEIGHT_TOLERANCE),
    ] {
        let id = parameter(name, index);
        let value = &model
            .parameters
            .iter()
            .find(|p| p.id.as_str() == id)
            .unwrap()
            .value;
        let actual = match (kind, value) {
            ("length", Some(ParameterValue::Length(v))) => v.get(),
            ("angle", Some(ParameterValue::Angle(v))) => v.get(),
            ("real", Some(ParameterValue::Real(v))) => v.get(),
            _ => panic!("{name} parameter {index}"),
        };
        assert!(
            (actual - expected).abs() < tolerance,
            "{name} parameter {index}"
        );
    }
    let spline_id = entity("SplineWeightRelation", 1);
    let spline = model
        .sketch_entities
        .iter()
        .find(|e| e.id().as_str() == spline_id)
        .unwrap();
    assert!(!spline.construction);
    let SketchGeometryDefinition::Nurbs { curve } = spline.geometry.definition() else {
        panic!("spline")
    };
    assert_eq!(curve.degree(), 3);
    assert!(!curve.periodic());
    assert_eq!(curve.knots().as_slice().len(), 8);
    for (actual, expected) in curve
        .knots()
        .as_slice()
        .iter()
        .zip([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
    {
        assert!((actual - expected).abs() < KNOT_TOLERANCE);
    }
    for (actual, expected) in curve
        .pole_rows()
        .weights()
        .unwrap()
        .into_iter()
        .zip([1.0, 1.2, 1.0, 1.0])
    {
        assert!((actual - expected).abs() < WEIGHT_TOLERANCE);
    }
    for (point, expected) in
        curve
            .control_points()
            .iter()
            .zip([[0.0, 0.0], [4.0, 8.0], [8.0, 7.0], [12.0, 2.0]])
    {
        for (actual, expected) in point.coordinates().into_iter().zip(expected) {
            assert!((actual.get() - expected).abs() < LENGTH_TOLERANCE);
        }
    }
}

#[test]
fn authored_constraint_sketches_keep_saved_geometry() {
    let data = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/sketch_constraints.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let entity = |name: &str, index: usize| {
        let sketch = model
            .sketches
            .iter()
            .find(|s| s.name.as_deref() == Some(name))
            .unwrap();
        let suffix = format!("#{name}:{index}");
        {
            let item = model
                .sketch_entities
                .iter()
                .find(|e| e.sketch == sketch.id && e.id().as_str().ends_with(&suffix))
                .unwrap();
            let helper =
                matches!(name, "InternalAlignmentRelations" | "SplineWeightRelation") && index > 1;
            assert_eq!(item.construction, helper, "{name} entity {index}");
            item
        }
    };
    for &(name, index, expected) in LINESEGMENT_GEOMETRY {
        let SketchGeometryDefinition::Line { start, end } =
            entity(name, index).geometry.definition()
        else {
            panic!("{name} line {index}")
        };
        for (actual, expected) in [start.get().u, start.get().v, end.get().u, end.get().v]
            .into_iter()
            .zip(expected)
        {
            assert!(
                (actual - expected).abs() < LENGTH_TOLERANCE,
                "{name} line {index}"
            );
        }
    }
    for &(name, index, expected) in CIRCLE_GEOMETRY {
        let SketchGeometryDefinition::Circle { center, radius } =
            entity(name, index).geometry.definition()
        else {
            panic!("{name} circle {index}")
        };
        for (actual, expected) in [center.get().u, center.get().v, radius.get()]
            .into_iter()
            .zip(expected)
        {
            assert!(
                (actual - expected).abs() < LENGTH_TOLERANCE,
                "{name} circle {index}"
            );
        }
    }
    for &(name, index, expected) in GEOMPOINT_GEOMETRY {
        let SketchGeometryDefinition::Point { position } =
            entity(name, index).geometry.definition()
        else {
            panic!("{name} point {index}")
        };
        for (actual, expected) in [position.get().u, position.get().v]
            .into_iter()
            .zip(expected)
        {
            assert!(
                (actual - expected).abs() < LENGTH_TOLERANCE,
                "{name} point {index}"
            );
        }
    }
    for &(name, index, expected) in ELLIPSE_GEOMETRY {
        let SketchGeometryDefinition::Ellipse {
            center,
            radii,
            major_angle,
            bounds,
        } = entity(name, index).geometry.definition()
        else {
            panic!("{name} ellipse {index}")
        };
        assert!(bounds.is_none());
        let [u, v, major, minor, angle] = expected;
        for (actual, expected) in [
            center.get().u,
            center.get().v,
            radii.major().get(),
            radii.minor().get(),
        ]
        .into_iter()
        .zip([u, v, major, minor])
        {
            assert!(
                (actual - expected).abs() < LENGTH_TOLERANCE,
                "{name} ellipse {index}"
            );
        }
        assert!(
            (major_angle.get() - angle).abs() < ANGLE_TOLERANCE,
            "{name} ellipse {index}"
        );
    }
}
const LINESEGMENT_GEOMETRY: &[(&str, usize, [f64; 4])] = &[
    ("GeometricRelations", 1, [0.0, 0.0, 10.0, 0.0]),
    ("GeometricRelations", 2, [15.0, 0.0, 15.0, 10.0]),
    (
        "GeometricRelations",
        3,
        [20.0, 0.0, 30.0, 3.999_999_999_999_999_6],
    ),
    ("GeometricRelations", 4, [20.0, 8.0, 30.0, 12.0]),
    (
        "GeometricRelations",
        5,
        [35.0, 0.0, 45.0, 3.999_999_999_999_999_6],
    ),
    ("GeometricRelations", 6, [39.0, -5.0, 35.0, 5.0]),
    (
        "GeometricRelations",
        7,
        [
            50.011_794_536_369_5,
            -0.363_461_244_864_965_2,
            59.967_351_517_734_06,
            2.058_809_666_393_605_7,
        ],
    ),
    (
        "GeometricRelations",
        8,
        [
            59.997_898_689_119_474,
            2.066_242_050_618_975,
            65.022_955_256_776_97,
            3.288_880_660_800_522_5,
        ],
    ),
    ("GeometricRelations", 9, [70.0, 0.0, 80.0, 3.0]),
    ("GeometricRelations", 10, [80.0, 3.0, 90.0, 6.0]),
    (
        "GeometricRelations",
        11,
        [95.0, 0.0, 105.0, 3.999_999_999_999_999_6],
    ),
    (
        "GeometricRelations",
        12,
        [95.0, 0.0, 105.0, -3.999_999_999_999_999_6],
    ),
    (
        "GeometricRelations",
        13,
        [110.0, 0.0, 120.0, 3.999_999_999_999_999_6],
    ),
    (
        "DimensionalRelations",
        1,
        [-0.062_257_748_326_953_2, 0.0, 8.0, 4.0],
    ),
    (
        "DimensionalRelations",
        2,
        [15.106_777_054_548_552, 0.031_622_776_601_683_8, 18.0, 3.0],
    ),
    (
        "DimensionalRelations",
        3,
        [24.936_754_446_796_634, 4.968_377_223_398_316, 28.0, 9.0],
    ),
    ("DimensionalRelations", 4, [35.0, 0.0, 43.0, 4.0]),
    ("DimensionalRelations", 5, [50.0, 0.0, 58.0, 4.0]),
    ("DimensionalRelations", 6, [65.0, 0.0, 75.0, 0.0]),
    (
        "DimensionalRelations",
        7,
        [65.0, 0.0, 72.0, 7.000_000_000_000_001],
    ),
    ("SymmetryRelation", 1, [-8.0, 3.0, -6.0, 5.0]),
    ("SymmetryRelation", 2, [8.0, 3.0, 6.0, 5.0]),
    ("SymmetryRelation", 3, [0.0, -10.0, 0.0, 10.0]),
    ("InactiveRelation", 1, [0.0, 0.0, 10.0, 0.0]),
    ("InternalAlignmentRelations", 2, [12.0, 0.0, -12.0, 0.0]),
    ("InternalAlignmentRelations", 3, [0.0, 5.0, 0.0, -5.0]),
    (
        "OpticalRelation",
        1,
        [
            -10.041_197_527_567_036,
            -4.916_827_177_867_217_5,
            0.041_197_527_567_035_8,
            -0.083_172_822_132_782_5,
        ],
    ),
    (
        "OpticalRelation",
        2,
        [
            0.121_227_646_125_801_1,
            -0.287_633_877_610_994,
            7.878_772_353_874_199,
            7.136_414_264_868_549,
        ],
    ),
    (
        "OpticalRelation",
        3,
        [
            -11.999_973_848_754_916,
            0.037_597_603_995_338_3,
            11.999_973_848_754_916,
            -0.037_597_603_995_338_3,
        ],
    ),
];

const CIRCLE_GEOMETRY: &[(&str, usize, [f64; 3])] = &[
    ("GeometricRelations", 14, [55.0, 0.0, 5.0]),
    ("GeometricRelations", 15, [55.0, 12.0, 5.0]),
    ("DimensionalRelations", 8, [85.0, 0.0, 4.0]),
    ("DimensionalRelations", 9, [100.0, 0.0, 4.0]),
    ("SplineWeightRelation", 2, [0.0, 0.0, 1.0]),
    ("SplineWeightRelation", 3, [4.0, 8.0, 1.2]),
    ("SplineWeightRelation", 4, [8.0, 7.0, 1.0]),
    ("SplineWeightRelation", 5, [12.0, 2.0, 1.0]),
];

const GEOMPOINT_GEOMETRY: &[(&str, usize, [f64; 2])] = &[
    (
        "InternalAlignmentRelations",
        4,
        [10.908_712_114_635_714, 0.0],
    ),
    (
        "InternalAlignmentRelations",
        5,
        [-10.908_712_114_635_714, 0.0],
    ),
    ("SplineWeightRelation", 6, [0.0, 0.0]),
    ("SplineWeightRelation", 7, [12.0, 2.0]),
];

const ELLIPSE_GEOMETRY: &[(&str, usize, [f64; 5])] =
    &[("InternalAlignmentRelations", 1, [0.0, 0.0, 12.0, 5.0, -0.0])];

#[test]
fn authored_symmetry_constraint_sketch_has_source_derived_measures() {
    let data = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/sketch_constraints.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("SymmetryRelation"))
        .unwrap();
    let [body_id] = feature.evaluation.outputs().as_slice() else {
        panic!("one symmetry sketch body")
    };
    let report = measured("sketch_constraints", decoded.ir()).unwrap();
    let body = report
        .bodies()
        .iter()
        .find(|body| body.body() == body_id.as_str())
        .unwrap();
    let length = body
        .length()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    let expected_length = 20.0 + 4.0 * std::f64::consts::SQRT_2;
    assert!(
        (length - expected_length).abs() < MEASURE_LENGTH_TOLERANCE,
        "symmetry sketch length {length}"
    );
    let bbox = body.bbox().measured().unwrap().value();
    for (actual, expected) in bbox
        .min
        .coordinates()
        .into_iter()
        .chain(bbox.max.coordinates())
        .zip([-8.0, -10.0, 0.0, 8.0, 10.0, 0.0])
    {
        assert!(
            (actual.get() - expected).abs() < MEASURE_BBOX_TOLERANCE,
            "symmetry sketch bbox"
        );
    }
}
