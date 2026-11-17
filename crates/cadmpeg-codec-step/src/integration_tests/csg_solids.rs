// SPDX-License-Identifier: Apache-2.0
//! CSG primitives and swept-area solids (ISO 10303-42 §6.4) decode to design
//! features and to derived bodies whose measured and tessellated volumes,
//! areas, and centroids equal their closed forms.

use std::f64::consts::PI;

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::PrimitiveShape;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::test_support::round_trip::measured;
use cadmpeg_measure::value::Quantity;

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;

/// Relative allowance of a measured value against its closed form.
const MEASURE_RELATIVE: f64 = 1.0e-9;

/// Largest distance, in millimetres, between a test triangle and its surface.
const TESSELLATION_CHORD: f64 = 1.0e-3;

/// Relative allowance of a tessellated area against its closed form at
/// [`TESSELLATION_CHORD`].
const TESSELLATION_RELATIVE: f64 = 1.0e-3;

/// The product, units, context, and CSG shape representation that list
/// `items`, followed by `geometry`. Lengths are millimetres and plane angles
/// degrees.
fn exchange(items: &str, geometry: &str) -> String {
    format!(
        "#1=APPLICATION_CONTEXT('csg');\n\
         #2=PRODUCT_CONTEXT('',#1,'mechanical');\n\
         #3=PRODUCT('part','part','',(#2));\n\
         #4=PRODUCT_DEFINITION_FORMATION('','',#3);\n\
         #5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');\n\
         #6=PRODUCT_DEFINITION('design','',#4,#5);\n\
         #7=PRODUCT_DEFINITION_SHAPE('','',#6);\n\
         #8=SHAPE_DEFINITION_REPRESENTATION(#7,#20);\n\
         #10=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #11=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #12=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);\n\
         #13=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433),#11);\n\
         #14=(CONVERSION_BASED_UNIT('DEGREE',#13) NAMED_UNIT(#12) PLANE_ANGLE_UNIT());\n\
         #15=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#10,#14)) REPRESENTATION_CONTEXT('model','3D'));\n\
         #20=CSG_SHAPE_REPRESENTATION('',({items}),#15);\n\
         #21=DIRECTION('',(0.,0.,1.));\n\
         #22=DIRECTION('',(1.,0.,0.));\n\
         #23=DIRECTION('',(0.,1.,0.));\n\
         {geometry}"
    )
}

fn decode(items: &str, geometry: &str) -> CadIr {
    decode_inline(&exchange(items, geometry)).ir().clone()
}

/// The measured volume, area, and centroid of the one body of `ir`.
fn body_measures(ir: &CadIr) -> (f64, f64, [f64; 3]) {
    let report = measured("csg", ir).expect("measure");
    let [body] = report.bodies().as_slice() else {
        panic!("one body row, got {}", report.bodies().len());
    };
    let scalar = |quantity: Option<&Quantity<cadmpeg_measure::value::Scalar>>| {
        quantity
            .and_then(Quantity::measured)
            .map(|value| value.value().value.get())
            .expect("measured")
    };
    let centroid = body
        .centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .map(|value| value.value().value.get())
        .expect("centroid");
    (
        scalar(body.volume().as_ref()),
        scalar(body.area().as_ref()),
        [centroid.x, centroid.y, centroid.z],
    )
}

/// The total area of the tessellations of every face of `ir`.
fn tessellated_area(ir: &CadIr) -> f64 {
    let options = cadmpeg_tessellate::Options {
        chord: TESSELLATION_CHORD,
        ..cadmpeg_tessellate::Options::default()
    };
    let mut tessellator = cadmpeg_tessellate::Tessellator::new(ir, options);
    let mut total = 0.0;
    for face in &ir.geometry.faces {
        let mesh = tessellator.face(&face.id).expect("face tessellates");
        for triangle in &mesh.triangles {
            let [first, second, third] = triangle.map(|index| mesh.positions[index]);
            let along = [second.x - first.x, second.y - first.y, second.z - first.z];
            let across = [third.x - first.x, third.y - first.y, third.z - first.z];
            let normal = [
                along[1] * across[2] - along[2] * across[1],
                along[2] * across[0] - along[0] * across[2],
                along[0] * across[1] - along[1] * across[0],
            ];
            total += (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt()
                / 2.0;
        }
    }
    total
}

fn assert_relative(label: &str, computed: f64, expected: f64, relative: f64) {
    let allowed = relative * expected.abs().max(1.0);
    assert!(
        (computed - expected).abs() <= allowed,
        "{label}: computed {computed}, expected {expected}, allowed {allowed}"
    );
}

/// Measure, then tessellate, one derived body against its closed forms.
fn assert_solid(label: &str, ir: &CadIr, volume: f64, area: f64, centroid: [f64; 3]) {
    let validation = cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate");
    let errors: Vec<_> = validation
        .findings
        .iter()
        .filter(|finding| finding.severity == cadmpeg_ir::report::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{label}: {errors:#?}");
    let (measured_volume, measured_area, measured_centroid) = body_measures(ir);
    assert_relative(label, measured_volume, volume, MEASURE_RELATIVE);
    assert_relative(label, measured_area, area, MEASURE_RELATIVE);
    for axis in 0..3 {
        assert_relative(
            label,
            measured_centroid[axis],
            centroid[axis],
            MEASURE_RELATIVE,
        );
    }
    assert_relative(label, tessellated_area(ir), area, TESSELLATION_RELATIVE);
}

#[test]
fn a_block_spans_the_positive_octant_of_its_placement() {
    let ir = decode(
        "#30",
        "#30=BLOCK('block',#31,2.,3.,4.);\n\
         #31=AXIS2_PLACEMENT_3D('',#32,#21,#23);\n\
         #32=CARTESIAN_POINT('',(1.,2.,3.));\n",
    );
    assert_solid("block", &ir, 24.0, 52.0, [1.0 - 1.5, 2.0 + 1.0, 3.0 + 2.0]);
}

#[test]
fn a_cylinder_stands_on_its_base_along_its_axis() {
    let ir = decode(
        "#30",
        "#30=RIGHT_CIRCULAR_CYLINDER('cylinder',#31,5.,2.);\n\
         #31=AXIS1_PLACEMENT('',#32,#22);\n\
         #32=CARTESIAN_POINT('',(1.,0.,0.));\n",
    );
    assert_solid("cylinder", &ir, 20.0 * PI, 28.0 * PI, [3.5, 0.0, 0.0]);
}

/// The truncated face of radius 1 lies at the placement; the base of radius
/// `1 + 3 tan 45° = 4` lies 3 back along the axis.
#[test]
fn a_cone_widens_from_its_placement_against_its_axis() {
    let ir = decode(
        "#30",
        "#30=RIGHT_CIRCULAR_CONE('cone',#31,3.,1.,45.);\n\
         #31=AXIS1_PLACEMENT('',#32,#21);\n\
         #32=CARTESIAN_POINT('',(0.,0.,3.));\n",
    );
    let (base, top, height) = (4.0_f64, 1.0_f64, 3.0_f64);
    let volume = PI * height / 3.0 * (base * base + base * top + top * top);
    let slant = (height * height + (base - top) * (base - top)).sqrt();
    let area = PI * (base + top) * slant + PI * (base * base + top * top);
    let rise = height * (base * base + 2.0 * base * top + 3.0 * top * top)
        / (4.0 * (base * base + base * top + top * top));
    assert_solid("frustum", &ir, volume, area, [0.0, 0.0, rise]);
}

#[test]
fn a_cone_of_zero_radius_has_its_apex_at_the_placement() {
    let ir = decode(
        "#30",
        "#30=RIGHT_CIRCULAR_CONE('cone',#31,3.,0.,45.);\n\
         #31=AXIS1_PLACEMENT('',#32,#21);\n\
         #32=CARTESIAN_POINT('',(0.,0.,0.));\n",
    );
    let area = PI * 3.0 * 18.0_f64.sqrt() + PI * 9.0;
    assert_solid("pointed cone", &ir, 9.0 * PI, area, [0.0, 0.0, -2.25]);
}

#[test]
fn a_sphere_and_a_torus_have_their_closed_forms() {
    let sphere = decode(
        "#30",
        "#30=SPHERE('sphere',2.,#32);\n\
         #32=CARTESIAN_POINT('',(1.,1.,1.));\n",
    );
    assert_solid(
        "sphere",
        &sphere,
        32.0 * PI / 3.0,
        16.0 * PI,
        [1.0, 1.0, 1.0],
    );
    let torus = decode(
        "#30",
        "#30=TORUS('torus',#31,5.,1.);\n\
         #31=AXIS1_PLACEMENT('',#32,#22);\n\
         #32=CARTESIAN_POINT('',(0.,0.,0.));\n",
    );
    assert_solid(
        "torus",
        &torus,
        10.0 * PI * PI,
        20.0 * PI * PI,
        [0.0, 0.0, 0.0],
    );
}

#[test]
fn a_wedge_extrudes_its_trapezoid() {
    let ir = decode(
        "#30",
        "#30=RIGHT_ANGULAR_WEDGE('wedge',#31,4.,3.,2.,1.);\n\
         #31=AXIS2_PLACEMENT_3D('',#32,#21,#22);\n\
         #32=CARTESIAN_POINT('',(0.,0.,0.));\n",
    );
    let slant = (9.0_f64 + 9.0).sqrt();
    let area = 2.0 * 7.5 + 2.0 * (4.0 + 3.0 + 1.0 + slant);
    let centroid_x = (4.0 * 4.0 + 4.0 * 1.0 + 1.0) / (3.0 * 5.0);
    let centroid_y = 3.0 * (4.0 + 2.0 * 1.0) / (3.0 * 5.0);
    assert_solid("wedge", &ir, 15.0, area, [centroid_x, centroid_y, 1.0]);
}

/// The square `[0, 2]²` of the plane `z = 0`, bounded by four trimmed
/// lines, extruded 3 along `+z`.
#[test]
fn an_extruded_area_sweeps_its_boundary() {
    let ir = decode(
        "#30",
        "#30=EXTRUDED_AREA_SOLID('extrusion',#31,#21,3.);\n\
         #31=CURVE_BOUNDED_SURFACE('',#32,(#34),.F.);\n\
         #32=PLANE('',#33);\n\
         #33=AXIS2_PLACEMENT_3D('',#40,#21,#22);\n\
         #34=BOUNDARY_CURVE('',(#35,#36,#37,#38),.F.);\n\
         #35=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#50);\n\
         #36=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#51);\n\
         #37=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#52);\n\
         #38=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#53);\n\
         #40=CARTESIAN_POINT('',(0.,0.,0.));\n\
         #41=CARTESIAN_POINT('',(2.,0.,0.));\n\
         #42=CARTESIAN_POINT('',(2.,2.,0.));\n\
         #43=CARTESIAN_POINT('',(0.,2.,0.));\n\
         #44=VECTOR('',#22,1.);\n\
         #45=VECTOR('',#23,1.);\n\
         #46=LINE('',#40,#44);\n\
         #47=LINE('',#41,#45);\n\
         #48=LINE('',#43,#44);\n\
         #49=LINE('',#40,#45);\n\
         #50=TRIMMED_CURVE('',#46,(#40),(#41),.T.,.CARTESIAN.);\n\
         #51=TRIMMED_CURVE('',#47,(#41),(#42),.T.,.CARTESIAN.);\n\
         #52=TRIMMED_CURVE('',#48,(#42),(#43),.F.,.CARTESIAN.);\n\
         #53=TRIMMED_CURVE('',#49,(#43),(#40),.F.,.CARTESIAN.);\n",
    );
    assert_solid("extruded square", &ir, 12.0, 32.0, [1.0, 1.0, 1.5]);
}

/// The disc of radius 1 about `(3, 0, 0)` in the plane `y = 0`, turned a
/// quarter turn about `+z`: by Pappus' theorems its volume is the disc's
/// area times the arc `3 · π/2` its centroid travels, and its side area the
/// circumference times that arc.
#[test]
fn a_revolved_area_obeys_pappus() {
    let ir = decode(
        "#30",
        "#30=REVOLVED_AREA_SOLID('revolution',#31,#36,90.);\n\
         #31=CURVE_BOUNDED_SURFACE('',#32,(#34),.F.);\n\
         #32=PLANE('',#33);\n\
         #33=AXIS2_PLACEMENT_3D('',#40,#23,#22);\n\
         #34=BOUNDARY_CURVE('',(#35),.F.);\n\
         #35=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#38);\n\
         #36=AXIS1_PLACEMENT('',#41,#21);\n\
         #37=CIRCLE('',#39,1.);\n\
         #38=TRIMMED_CURVE('',#37,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(360.)),.T.,.PARAMETER.);\n\
         #39=AXIS2_PLACEMENT_3D('',#40,#23,#22);\n\
         #40=CARTESIAN_POINT('',(3.,0.,0.));\n\
         #41=CARTESIAN_POINT('',(0.,0.,0.));\n",
    );
    let path = 3.0 * PI / 2.0;
    let volume = PI * path;
    let area = 2.0 * PI * path + 2.0 * PI;
    let centroid = 3.0 * (2.0 / PI) * (1.0 + 1.0 / 36.0);
    assert_solid(
        "quarter torus",
        &ir,
        volume,
        area,
        [centroid, centroid, 0.0],
    );
}

#[test]
fn each_primitive_is_a_feature_whose_current_state_holds_its_body() {
    let ir = decode(
        "#30",
        "#30=BLOCK('block',#31,2.,3.,4.);\n\
         #31=AXIS2_PLACEMENT_3D('',#32,#21,#22);\n\
         #32=CARTESIAN_POINT('',(0.,0.,0.));\n",
    );
    let [feature] = ir.design.features.as_slice() else {
        panic!("one feature");
    };
    let FeatureOperation::Primitive(primitive) = &feature.definition.operation else {
        panic!("a primitive");
    };
    let PrimitiveShape::Box { size } = &primitive.shape else {
        panic!("a box");
    };
    assert_eq!(size.len(), 3);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let [state] = scope.current.as_slice() else {
        panic!("one current state");
    };
    assert_eq!(
        state.at,
        Position::After(Member::Feature(feature.id.clone()))
    );
    assert_eq!(state.bodies, vec![ir.geometry.bodies[0].id.clone()]);
    assert_eq!(
        ir.model.product_definitions[0].bodies,
        vec![ir.geometry.bodies[0].id.clone()]
    );
}

#[test]
fn a_derived_body_is_annotated_derived() {
    let decoded = decode_inline(&exchange(
        "#30",
        "#30=SPHERE('sphere',2.,#32);\n\
         #32=CARTESIAN_POINT('',(0.,0.,0.));\n",
    ));
    let body = decoded.ir().geometry.bodies[0].id.as_str().to_owned();
    let (_, _, fidelity) = decoded.into_parts();
    let note = fidelity
        .annotations
        .exactness()
        .get(&body)
        .expect("an exactness note");
    assert_eq!(note.entity(), cadmpeg_ir::Exactness::Derived);
}

/// A block less a cylinder: three features, the combine a cut of the block
/// by the cylinder, and no derived body.
#[test]
fn a_boolean_tree_has_features_and_no_body() {
    let decoded = decode_inline(&exchange(
        "#33",
        "#30=BLOCK('block',#31,2.,2.,2.);\n\
         #31=AXIS2_PLACEMENT_3D('',#32,#21,#22);\n\
         #32=CARTESIAN_POINT('',(0.,0.,0.));\n\
         #34=RIGHT_CIRCULAR_CYLINDER('hole',#35,2.,0.5);\n\
         #35=AXIS1_PLACEMENT('',#36,#21);\n\
         #36=CARTESIAN_POINT('',(1.,1.,0.));\n\
         #37=BOOLEAN_RESULT('',.DIFFERENCE.,#30,#34);\n\
         #33=CSG_SOLID('solid',#37);\n",
    ));
    let ir = decoded.ir();
    assert!(ir.geometry.bodies.is_empty());
    assert_eq!(ir.design.features.len(), 3);
    let FeatureOperation::Combine(combine) = &ir.design.features[2].definition.operation else {
        panic!("the last feature is the combine");
    };
    assert_eq!(
        combine.op,
        Some(cadmpeg_ir::design::options::BooleanOp::Cut)
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::CsgBooleanUnevaluated.kind()));
}

/// A quarter disc of radius 2 in the plane `y = 0`, its straight edge on
/// the z axis stated 10⁻⁹ off the axis, turned a full turn about `+z`: the
/// profile point lies on the axis and the solid is the hemisphere. The arc
/// runs clockwise from parameter 0 to 270 degrees.
#[test]
fn a_revolved_profile_point_beside_the_axis_lies_on_it() {
    let ir = decode(
        "#30",
        "#30=REVOLVED_AREA_SOLID('dome',#31,#36,360.);\n\
         #31=CURVE_BOUNDED_SURFACE('',#32,(#34),.F.);\n\
         #32=PLANE('',#33);\n\
         #33=AXIS2_PLACEMENT_3D('',#40,#23,#22);\n\
         #34=BOUNDARY_CURVE('',(#35,#37,#38),.F.);\n\
         #35=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#50);\n\
         #37=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#51);\n\
         #38=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#52);\n\
         #36=AXIS1_PLACEMENT('',#40,#21);\n\
         #40=CARTESIAN_POINT('',(0.,0.,0.));\n\
         #41=CARTESIAN_POINT('',(2.,0.,0.));\n\
         #42=CARTESIAN_POINT('',(1.E-9,0.,2.));\n\
         #43=VECTOR('',#22,1.);\n\
         #44=VECTOR('',#21,1.);\n\
         #45=LINE('',#40,#43);\n\
         #46=LINE('',#42,#44);\n\
         #47=AXIS2_PLACEMENT_3D('',#40,#23,#22);\n\
         #48=CIRCLE('',#47,2.);\n\
         #50=TRIMMED_CURVE('',#45,(#40),(#41),.T.,.CARTESIAN.);\n\
         #51=TRIMMED_CURVE('',#48,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(270.)),.F.,.PARAMETER.);\n\
         #52=TRIMMED_CURVE('',#46,(#42),(#40),.T.,.CARTESIAN.);\n",
    );
    assert_solid(
        "hemisphere",
        &ir,
        16.0 * PI / 3.0,
        12.0 * PI,
        [0.0, 0.0, 0.75],
    );
}
