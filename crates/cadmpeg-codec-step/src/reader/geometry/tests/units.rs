// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

const EPS_LINEAR_UNCERTAINTY: f64 = 1.0e-12;
/// The `LENGTH_MEASURE(1.E-07)` each context declares, in millimetres.
const DECLARED_CONTEXT_UNCERTAINTY_MM: f64 = 1.0e-7;

/// Assert one ambiguous linear-uncertainty note that names `values` in
/// ascending order and the kept `default_linear`.
fn assert_ambiguous_length_uncertainty(
    losses: &[cadmpeg_ir::report::LossNote],
    values: &[f64],
    default_linear: f64,
) {
    let ambiguous = losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::UncertaintyLengthAmbiguous.kind())
        .collect::<Vec<_>>();
    assert_eq!(ambiguous.len(), 1, "{losses:#?}");
    assert_eq!(ambiguous[0].severity, cadmpeg_ir::Severity::Warning);
    // The note names each distinct candidate the file declares and the
    // substituted default.
    let listed = values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        ambiguous[0]
            .message
            .contains(&format!("values in millimetres ({listed})")),
        "{}",
        ambiguous[0].message
    );
    assert!(
        ambiguous[0]
            .message
            .contains(&format!("keeps the default {default_linear:?}")),
        "{}",
        ambiguous[0].message
    );
}

#[test]
fn unresolvable_length_unit_reports_an_error_loss() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('unresolvable length unit'),'2;1');FILE_NAME('unit','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*));#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));#4=CARTESIAN_POINT('',(1.,2.,3.));#5=SHAPE_REPRESENTATION('',(#4),#3);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode bare named length unit");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.message
                .starts_with("the document length unit did not resolve")
        })
        .expect("unresolved length unit loss");
    assert_eq!(loss.code, StepLossCode::DocumentLengthUnitUnresolved.kind());
    assert_eq!(loss.severity, cadmpeg_ir::Severity::Error);
    assert_eq!(
        loss.message,
        "the document length unit did not resolve; coordinates are unscaled and reported as millimetres"
    );
}

fn assert_unscoped_cadir_fallback_point(
    first_length_unit: u64,
    second_length_unit: u64,
    expected_x: f64,
    expect_unresolved_loss: bool,
) {
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('document fallback units'),'2;1');FILE_NAME('document-fallback-units','2026-08-15T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{first_length_unit},#4)) REPRESENTATION_CONTEXT('first','3D'));#6=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{second_length_unit},#4)) REPRESENTATION_CONTEXT('second','3D'));#7=CARTESIAN_POINT('unscoped',(1.,0.,0.));#8=REPRESENTATION_CONTEXT('unscoped','3D');#9=SHAPE_REPRESENTATION('unscoped',(#7),#8);ENDSEC;END-ISO-10303-21;"
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode unscoped document point");
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#7")
        .expect("unscoped document point");
    assert_eq!(point.position.x, expected_x);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == StepLossCode::DocumentLengthUnitUnresolved.kind() }),
        expect_unresolved_loss
    );
}

#[test]
fn conflicting_cadir_fallback_units_are_order_independent() {
    assert_unscoped_cadir_fallback_point(1, 3, 1.0, true);
    assert_unscoped_cadir_fallback_point(3, 1, 1.0, true);
}

#[test]
fn equivalent_context_units_define_cadir_fallback_scale() {
    assert_unscoped_cadir_fallback_point(1, 1, 1.0, false);
    assert_unscoped_cadir_fallback_point(3, 3, 25.4, false);
}

#[test]
fn one_unscoped_unit_record_can_supply_cadir_fallback_scale() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('unscoped unit'),'2;1');FILE_NAME('unscoped-unit','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));#2=CARTESIAN_POINT('unscoped',(1.,0.,0.));#3=GEOMETRIC_REPRESENTATION_CONTEXT(3);#4=SHAPE_REPRESENTATION('unscoped',(#2),#3);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode unscoped unit");
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#2")
        .expect("unscoped point");
    assert_eq!(point.position.x, 10.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::DocumentLengthUnitUnresolved.kind() }));
}

#[test]
pub(crate) fn decode_transfers_placed_analytic_geometry_in_millimetres() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let bytes = include_bytes!("../../../../tests/fixtures/ap242_geometry.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed STEP geometry");

    assert_eq!(result.ir().model.points.len(), 1);
    let placed = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.0 == "step:data:point#3")
        .unwrap();
    assert_eq!(placed.position.x, 1.0);
    assert_eq!(placed.position.y, 2.0);
    assert_eq!(placed.position.z, 3.0);
    assert_eq!(result.ir().model.curves.len(), 9);
    assert!(result.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#45"
            && matches!(curve.geometry, CurveGeometry::Composite { .. })
    }));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Line { origin, direction }
            if origin.x == 1.0 && origin.y == 2.0 && origin.z == 3.0
                && direction.x == 0.0 && direction.y == 0.0 && direction.z == 1.0
    )));
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #51")));
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
            } if start == 0.0 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
        )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse { major_radius, minor_radius, .. }
            if major_radius == 6.0 && minor_radius == 2.0
    )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.degree == 2
                && nurbs.knots == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
                && nurbs.weights.as_deref() == Some(&[1.0, 0.5, 1.0][..])
    )));
    assert_eq!(result.ir().model.surfaces.len(), 10);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Curve(_)
        )));
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Surface(_)
        )));
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Point(_)
        )));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("STYLED_ITEM #43")));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("STYLED_ITEM #52")));
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.source_entity_id.as_deref() == Some("#47"))
            .count(),
        2
    );
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            &binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Source { source_id } if source_id == "#6"
        )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if curve.id.as_str() == "step:data:curve#48"
                && nurbs.degree == 1
                && nurbs.knots == [0.0, 0.0, 1.0, 2.0, 2.0]
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane { origin, normal, .. }
            if origin.x == 1.0 && origin.y == 2.0 && origin.z == 3.0 && normal.z == 1.0
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.u_degree == 1
                && nurbs.v_degree == 1
                && nurbs.u_count == 2
                && nurbs.v_count == 2
                && nurbs.u_knots == [0.0, 0.0, 1.0, 1.0]
                && nurbs.v_knots == [0.0, 0.0, 1.0, 1.0]
                && nurbs.weights.as_deref() == Some(&[1.0, 1.0, 1.0, 0.75][..])
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { radius, .. } if radius == 5.0
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { radius, ratio, half_angle, .. }
            if radius == 5.0 && ratio == 1.0 && half_angle == 0.25
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { radius, .. } if radius == 5.0
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus { major_radius, minor_radius, .. }
            if major_radius == 8.0 && minor_radius == 2.0
    )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, radius, .. }
            if center.x == 1.0 && center.y == 2.0 && center.z == 3.0 && radius == 4.0
    )));
    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.procedural_curves.len(), 3);
    let cartesian_trim = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:construction:trimmed_curve#29")
        .expect("Cartesian trimmed curve");
    assert!(matches!(
        cartesian_trim.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
            parameter_range: [start, end],
            ..
        } if start == 0.0 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));
    let (source, parameter_range) = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                source,
                parameter_range,
                ..
            } => Some((source, *parameter_range)),
            _ => None,
        })
        .expect("trimmed curve was not retained as a subset construction");
    assert_eq!(source.as_str(), "step:data:curve#8");
    assert_eq!(parameter_range, [0.0, std::f64::consts::FRAC_PI_2]);
    assert!(result
        .ir()
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::SpatialOffset {
                distance: 1.0,
                self_intersect: None,
                ..
            }
        )));
    assert_eq!(result.ir().model.procedural_surfaces.len(), 4);
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::DegenerateTorus {
                select_outer: true
            }
        )));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::LinearSweep { direction, .. }
                if direction.z == 2.0
        )));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::AxisRevolution { axis_direction, .. }
                if axis_direction.z == 1.0
        )));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::ParallelOffset {
                distance: 0.5,
                self_intersect: Some(false),
                ..
            }
        )));
}

#[test]
pub(crate) fn decode_conical_apex_and_context_plane_angle_units() {
    let bytes = include_bytes!("../../../../tests/fixtures/ap242_degree_cone.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode degree cone");

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { radius, half_angle, .. }
            if radius == 0.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12
    )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(
        validation
            .findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::CarrierReachability),
        "{:#?}",
        validation.findings
    );
}

#[test]
pub(crate) fn decode_resolves_conversion_units_and_linear_uncertainty() {
    let bytes = include_bytes!("../../../../tests/fixtures/ap242_conversion_units.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode conversion-based units");

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 50.8);
    assert!((result.ir().tolerances.linear - 0.0254).abs() < 1e-12);
}

#[test]
fn decode_selects_a_length_uncertainty_after_an_angular_measure() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('mixed uncertainty'),'2;1');\nFILE_NAME('mixed-uncertainty','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\n#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=UNCERTAINTY_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.5),#4,'angle_accuracy','');\n#6=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.002),#3,'distance_accuracy_value','');\n#7=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#5,#6)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3,#4)) REPRESENTATION_CONTEXT('model','3D'));\n#8=CARTESIAN_POINT('two inches',(2.,0.,0.));\n#9=SHAPE_REPRESENTATION('construction points',(#8),#7);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode mixed uncertainty units");

    assert!((result.ir().tolerances.linear - 0.0508).abs() < 1e-12);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::UncertaintyLengthAmbiguous.kind() }));
}

#[test]
fn decode_prefers_named_length_uncertainty_when_several_lengths_are_present() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('named uncertainty'),'2;1');\nFILE_NAME('named-uncertainty','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'manufacturing_accuracy','');\n#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'distance_accuracy_value','');\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));\n#6=CARTESIAN_POINT('point',(1.,0.,0.));\n#7=SHAPE_REPRESENTATION('construction points',(#6),#5);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode named uncertainty");

    assert!((result.ir().tolerances.linear - 0.2).abs() < 1e-12);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::UncertaintyLengthAmbiguous.kind() }));
}

#[test]
fn decode_named_uncertainty_selection_is_order_independent() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('ordered uncertainty'),'2;1');FILE_NAME('ordered-uncertainty','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'manufacturing_accuracy','');#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'distance_accuracy_value','');#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));#6=CARTESIAN_POINT('point',(1.,0.,0.));#7=SHAPE_REPRESENTATION('construction points',(#6),#5);ENDSEC;END-ISO-10303-21;";
    for source in [
        source.to_owned(),
        source.replace(
            "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3))",
            "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#3,#2))",
        ),
    ] {
        let result = StepCodec::default()
            .decode(
                &mut Cursor::new(source.as_bytes()),
                &DecodeOptions::default(),
            )
            .expect("decode ordered uncertainty");
        assert!((result.ir().tolerances.linear - 0.2).abs() < EPS_LINEAR_UNCERTAINTY);
        assert!(result
            .report()
            .losses
            .iter()
            .all(|loss| { loss.code != StepLossCode::UncertaintyLengthAmbiguous.kind() }));
    }
}

#[test]
fn decode_uses_uncertainty_name_not_description() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('uncertainty labels'),'2;1');FILE_NAME('uncertainty-labels','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'manufacturing_accuracy','distance_accuracy_value');#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'other_accuracy','');#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));#6=CARTESIAN_POINT('point',(1.,0.,0.));#7=SHAPE_REPRESENTATION('construction points',(#6),#5);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode uncertainty labels");

    assert_eq!(
        result.ir().tolerances.linear,
        cadmpeg_ir::units::Tolerances::default().linear
    );
    assert_ambiguous_length_uncertainty(
        &result.report().losses,
        &[0.1, 0.2],
        cadmpeg_ir::units::Tolerances::default().linear,
    );
}

#[test]
fn decode_keeps_representation_uncertainty_scoped_to_native_source() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('scoped uncertainty'),'2;1');FILE_NAME('scoped-uncertainty','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'distance_accuracy_value','global');#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));#5=CARTESIAN_POINT('point',(1.,0.,0.));#6=SHAPE_REPRESENTATION('global',(#5),#4);#7=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'representation_accuracy','scoped');#8=(REPRESENTATION('scoped',(#5),#4) UNCERTAINTY_ASSIGNED_REPRESENTATION((#7)));ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode scoped uncertainty");

    assert!((result.ir().tolerances.linear - 0.1).abs() < EPS_LINEAR_UNCERTAINTY);
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| {
            record.id.0.ends_with("#8")
                && record.id.0.contains("uncertainty_assigned_representation")
        }));
}

#[test]
fn decode_reports_ambiguous_length_uncertainty() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ambiguous uncertainty'),'2;1');\nFILE_NAME('ambiguous-uncertainty','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'first_accuracy','');\n#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'second_accuracy','');\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));\n#6=CARTESIAN_POINT('point',(1.,0.,0.));\n#7=SHAPE_REPRESENTATION('construction points',(#6),#5);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode ambiguous uncertainty");

    let default_linear = cadmpeg_ir::units::Tolerances::default().linear;
    assert!((result.ir().tolerances.linear - default_linear).abs() < EPS_LINEAR_UNCERTAINTY);
    assert_ambiguous_length_uncertainty(&result.report().losses, &[0.1, 0.2], default_linear);
}

#[test]
fn decode_resolves_agreeing_context_uncertainties_without_ambiguity() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('agreeing context uncertainty'),'2;1');FILE_NAME('agreeing-context-uncertainty','2026-08-18T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#1,'distance_accuracy_value','confusion accuracy');#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('first','3D'));#5=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#6=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#7=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#5,'distance_accuracy_value','confusion accuracy');#8=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#7)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#5,#6)) REPRESENTATION_CONTEXT('second','3D'));#9=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#10=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#11=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#9,'distance_accuracy_value','confusion accuracy');#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#11)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#9,#10)) REPRESENTATION_CONTEXT('third','3D'));#13=CARTESIAN_POINT('first point',(1.,0.,0.));#14=SHAPE_REPRESENTATION('first',(#13),#4);#15=CARTESIAN_POINT('second point',(2.,0.,0.));#16=SHAPE_REPRESENTATION('second',(#15),#8);#17=CARTESIAN_POINT('third point',(3.,0.,0.));#18=SHAPE_REPRESENTATION('third',(#17),#12);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode agreeing context uncertainty");

    assert!(
        (result.ir().tolerances.linear - DECLARED_CONTEXT_UNCERTAINTY_MM).abs()
            < EPS_LINEAR_UNCERTAINTY
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| { loss.code != StepLossCode::UncertaintyLengthAmbiguous.kind() }));
}

#[test]
fn decode_reports_distinct_context_uncertainties_as_ambiguous() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('distinct context uncertainty'),'2;1');FILE_NAME('distinct-context-uncertainty','2026-08-18T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'distance_accuracy_value','');#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('coarse','3D'));#5=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'distance_accuracy_value','');#6=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#5)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('fine','3D'));#7=CARTESIAN_POINT('coarse point',(1.,0.,0.));#8=SHAPE_REPRESENTATION('coarse',(#7),#4);#9=CARTESIAN_POINT('fine point',(2.,0.,0.));#10=SHAPE_REPRESENTATION('fine',(#9),#6);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode distinct context uncertainty");

    let default_linear = cadmpeg_ir::units::Tolerances::default().linear;
    assert!((result.ir().tolerances.linear - default_linear).abs() < EPS_LINEAR_UNCERTAINTY);
    assert_ambiguous_length_uncertainty(&result.report().losses, &[0.1, 0.2], default_linear);
}

#[test]
fn decode_does_not_let_a_named_context_uncertainty_mask_another_context() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('masked context uncertainty'),'2;1');FILE_NAME('masked-context-uncertainty','2026-08-18T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'distance_accuracy_value','');#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('named','3D'));#5=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'first_accuracy','');#6=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'second_accuracy','');#7=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#5,#6)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('unnamed','3D'));#8=CARTESIAN_POINT('named point',(1.,0.,0.));#9=SHAPE_REPRESENTATION('named',(#8),#4);#10=CARTESIAN_POINT('unnamed point',(2.,0.,0.));#11=SHAPE_REPRESENTATION('unnamed',(#10),#7);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode masked context uncertainty");

    // The named measure of the first context does not answer for the second
    // context, which has no named measure and contributes both of its length
    // measures.
    let default_linear = cadmpeg_ir::units::Tolerances::default().linear;
    assert!((result.ir().tolerances.linear - default_linear).abs() < EPS_LINEAR_UNCERTAINTY);
    assert_ambiguous_length_uncertainty(&result.report().losses, &[0.1, 0.2], default_linear);
}

#[test]
fn decode_scales_geometry_by_its_representation_context() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('per representation units'),'2;1');\nFILE_NAME('per-representation-units','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\n#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('metric','3D'));\n#6=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3,#4)) REPRESENTATION_CONTEXT('inch','3D'));\n#7=CARTESIAN_POINT('metric point',(10.,0.,0.));\n#8=CARTESIAN_POINT('inch point',(1.,0.,0.));\n#9=SHAPE_REPRESENTATION('metric representation',(#7),#5);\n#10=SHAPE_REPRESENTATION('inch representation',(#8),#6);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode per-representation units");

    let metric = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#7")
        .expect("metric point");
    let inch = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#8")
        .expect("inch point");
    assert!((metric.position.x - 10.0).abs() < 1e-12);
    assert!((inch.position.x - 25.4).abs() < 1e-12);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::ConflictingRepresentationUnits.kind() }));
}

#[test]
fn mapped_target_items_use_their_target_representation_context_units() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('mapped units'),'2;1');FILE_NAME('mapped-units','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);
#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));
#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('source','3D'));
#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3)) REPRESENTATION_CONTEXT('target','3D'));
#6=CARTESIAN_POINT('source',(1.,0.,0.));
#7=SHAPE_REPRESENTATION('source',(#6),#4);
#8=CARTESIAN_POINT('target',(1.,0.,0.));
#9=AXIS2_PLACEMENT_3D('target',#8,$,$);
#10=REPRESENTATION_MAP(#6,#7);
#11=MAPPED_ITEM('mapped',#10,#9);
#12=SHAPE_REPRESENTATION('target',(#11),#5);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("parse mapped units");
    let mut losses = Vec::new();
    let scales = super::super::resolve_unit_scales(&exchange, 1.0, 1.0, &mut losses);
    assert_eq!(scales.length.get(&8), Some(&25.4));
    assert!(losses
        .iter()
        .all(|loss| { loss.code != StepLossCode::ConflictingRepresentationUnits.kind() }));
}

#[test]
fn indirect_representation_items_inherit_the_root_context_units() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('indirect units'),'2;1');FILE_NAME('indirect-units','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3)) REPRESENTATION_CONTEXT('model','3D'));#5=CARTESIAN_POINT('point',(1.,0.,0.));#6=DIRECTION('direction',(1.,0.,0.));#7=VECTOR('vector',#6,2.);#8=LINE('line',#5,#7);#9=SHAPE_REPRESENTATION('line',(#8),#4);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("parse indirect units");
    let mut losses = Vec::new();
    let scales = super::super::resolve_unit_scales(&exchange, 1.0, 1.0, &mut losses);
    assert_eq!(scales.length.get(&5), Some(&25.4));
    assert_eq!(scales.length.get(&8), Some(&25.4));
    assert!(losses
        .iter()
        .all(|loss| { loss.code != StepLossCode::ConflictingRepresentationUnits.kind() }));
}

#[test]
fn generic_representation_relationships_do_not_transfer_unit_contexts() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('related units'),'2;1');FILE_NAME('related-units','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3)) REPRESENTATION_CONTEXT('source','3D'));#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('related','3D'));#6=CARTESIAN_POINT('source',(1.,0.,0.));#7=SHAPE_REPRESENTATION('source',(#6),#4);#8=CARTESIAN_POINT('related',(1.,0.,0.));#9=SHAPE_REPRESENTATION('related',(#8),#5);#10=REPRESENTATION_RELATIONSHIP('related representations','',#7,#9);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("parse related units");
    let mut losses = Vec::new();
    let scales = super::super::resolve_unit_scales(&exchange, 1.0, 1.0, &mut losses);
    assert_eq!(scales.length.get(&6), Some(&25.4));
    assert!(losses
        .iter()
        .all(|loss| { loss.code != StepLossCode::ConflictingRepresentationUnits.kind() }));
}

#[test]
fn shared_representation_items_reject_conflicting_context_units() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('shared units'),'2;1');FILE_NAME('shared-units','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('metric','3D'));#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3)) REPRESENTATION_CONTEXT('inch','3D'));#6=CARTESIAN_POINT('shared',(1.,0.,0.));#7=SHAPE_REPRESENTATION('metric',(#6),#4);#8=SHAPE_REPRESENTATION('inch',(#6),#5);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("parse shared units");
    let mut losses = Vec::new();
    let scales = super::super::resolve_unit_scales(&exchange, 1.0, 1.0, &mut losses);
    assert_eq!(scales.length.get(&6), None);
    assert!(losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::ConflictingRepresentationUnits.kind() }));
}
