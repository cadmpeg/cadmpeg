// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::CadIr;

use crate::export::Builder;
use crate::test_support::{decode_inline, export};
use crate::StepSchema;

#[test]
fn defaulted_spline_curve_subtypes_derive_knot_vectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=QUASI_UNIFORM_CURVE('quasi',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#5=UNIFORM_CURVE('uniform',1,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#6=BEZIER_CURVE('bezier',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#7=(BOUNDED_CURVE() B_SPLINE_CURVE(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.) QUASI_UNIFORM_CURVE() RATIONAL_B_SPLINE_CURVE((1.,.5,1.)) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('rational'));
#8=GEOMETRIC_SET('',(#4,#5,#6,#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let nurbs = |id: &str| {
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| match &curve.geometry {
                CurveGeometry::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing NURBS curve {id}"))
    };
    assert_eq!(
        nurbs("step:data:curve#4").knots,
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(nurbs("step:data:curve#5").knots, [-1.0, 0.0, 1.0, 2.0, 3.0]);
    assert_eq!(
        nurbs("step:data:curve#6").knots,
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    let rational = nurbs("step:data:curve#7");
    assert_eq!(rational.knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(rational.weights.as_deref(), Some(&[1.0, 0.5, 1.0][..]));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn defaulted_spline_surface_subtypes_derive_axis_knot_vectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=CARTESIAN_POINT('',(0.,1.,0.));
#5=CARTESIAN_POINT('',(1.,1.,0.));
#6=CARTESIAN_POINT('',(2.,1.,0.));
#10=QUASI_UNIFORM_SURFACE('quasi',1,1,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#11=UNIFORM_SURFACE('uniform',1,2,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#12=BEZIER_SURFACE('bezier',1,2,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#13=GEOMETRIC_SET('',(#10,#11,#12));
#14=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#13),#15);
#15=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let nurbs = |id: &str| {
        result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == id)
            .and_then(|surface| match &surface.geometry {
                SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing NURBS surface {id}"))
    };
    assert_eq!(nurbs("step:data:surface#10").u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs("step:data:surface#10").v_knots,
        [0.0, 0.0, 1.0, 2.0, 2.0]
    );
    assert_eq!(nurbs("step:data:surface#11").u_knots, [-1.0, 0.0, 1.0, 2.0]);
    assert_eq!(
        nurbs("step:data:surface#11").v_knots,
        [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0]
    );
    assert_eq!(nurbs("step:data:surface#12").u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs("step:data:surface#12").v_knots,
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_rational_quasi_uniform_surface_decodes_with_weight_grid() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(0.,1.,0.));
#4=CARTESIAN_POINT('',(1.,1.,0.));
#5=CARTESIAN_POINT('',(0.,2.,0.));
#6=CARTESIAN_POINT('',(1.,2.,0.));
#7=(BOUNDED_SURFACE() B_SPLINE_SURFACE(2,1,((#1,#2),(#3,#4),(#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.) QUASI_UNIFORM_SURFACE() RATIONAL_B_SPLINE_SURFACE(((1.,.5),(1.,.5),(1.,1.))) SURFACE());
#8=GEOMETRIC_SET('',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#7")
        .expect("complex rational surface");
    let SurfaceGeometry::Nurbs(nurbs) = &surface.geometry else {
        panic!("complex rational surface is not NURBS")
    };
    assert_eq!(nurbs.u_knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs.weights.as_deref(),
        Some(&[1.0, 0.5, 1.0, 0.5, 1.0, 1.0][..])
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn excessive_nurbs_degree_is_rejected_before_knot_allocation() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=B_SPLINE_CURVE_WITH_KNOTS('',4294967295,(#1,#2),.UNSPECIFIED.,.F.,.F.,(4294967298),(0.),.UNSPECIFIED.);",
    );
    assert!(result.ir().model.curves.is_empty());
}

#[test]
fn deferred_curve_dependencies_resolve_independent_of_record_order() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=VECTOR('',#2,1.);
#4=LINE('',#1,#3);
#5=OFFSET_CURVE_3D('',#7,1.,.F.,#2);
#6=GEOMETRIC_SET('',(#5));
#7=OFFSET_CURVE_3D('',#4,2.,.F.,#2);
#8=SHAPE_REPRESENTATION('',(#6),#9);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#5"));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#7"));
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("OFFSET_CURVE_3D #5 has no decoded basis curve")
    }));
}

#[test]
fn deferred_surface_dependencies_resolve_independent_of_record_order() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=PLANE('',#4);
#6=OFFSET_SURFACE('',#7,1.,.F.);
#7=OFFSET_SURFACE('',#5,2.,.F.);
#8=GEOMETRIC_SET('',(#6));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#7"));
    assert_eq!(result.ir().model.bodies.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unknown_recursive_curve_dependency_is_refused_without_panicking() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
    };

    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId("unknown".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("composite".into()),
        geometry: CurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: CurveId("unknown".into()),
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }],
            self_intersect: Some(false),
        },
        source_object: None,
    });
    let output = export(&ir);
    assert!(!output.contains("COMPOSITE_CURVE("));
    let mut builder = Builder::new(&ir, StepSchema::Ap242Edition3);
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.active_curves.is_empty());
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.active_curves.is_empty());
}
