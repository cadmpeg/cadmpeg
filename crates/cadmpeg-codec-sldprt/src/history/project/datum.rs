// SPDX-License-Identifier: Apache-2.0
//! Datum, curve, helix, and wrap projection.

use crate::records::Feature;
use cadmpeg_ir::{
    features::{
        CurveProjectionDirection, CurveProjectionDirectionState, DatumPlaneReference, Dimension,
        FaceSelection, FeatureDefinition, FeatureId, FeatureOperation, PathRef, PlanarProfileRef,
        WrapMode,
    },
    scalar::Angle,
};
use std::collections::HashMap;

use crate::history::literals::{
    parse_bool, parse_point3_mm, parse_valid_direction, parse_vector3, valid_direction,
    valid_plane_frame,
};

pub(super) fn project_datum_plane(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let normal = parse_vector3(feature.properties.get("Normal")?)?;
    let u_axis = parse_vector3(feature.properties.get("UAxis")?)?;
    valid_plane_frame(normal, u_axis).then_some(FeatureDefinition::Operation(
        FeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::from_parts(
                origin,
                cadmpeg_ir::features::FeatureDirection3::new(normal)?,
                cadmpeg_ir::features::FeatureDirection3::new(u_axis)?,
            )?,
        },
    ))
}

pub(in crate::history) fn project_offset_plane(
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
) -> Option<FeatureDefinition> {
    let distance = super::parameter_dimension(feature, &["D1"])?;
    let resolved_frame = || {
        cadmpeg_ir::features::FeatureSupportPlaneFrame::from_parts(
            parse_point3_mm(feature.properties.get("ReferenceFaceOrigin")?)?,
            cadmpeg_ir::features::FeatureDirection3::new(parse_vector3(
                feature.properties.get("ReferenceFaceNormal")?,
            )?)?,
            cadmpeg_ir::features::FeatureDirection3::new(parse_vector3(
                feature.properties.get("ReferenceFaceUAxis")?,
            )?)?,
        )
    };
    let reference = if let Some(reference) = feature
        .properties
        .get("Reference")
        .or_else(|| feature.properties.get("Plane"))
        .and_then(|source| by_source.get(source.as_str()))
    {
        Some(DatumPlaneReference::Feature {
            feature: reference.clone(),
        })
    } else if let Some(frame) = resolved_frame() {
        Some(DatumPlaneReference::ResolvedPlane { frame })
    } else {
        feature
            .properties
            .get("ReferenceFaceNative")
            .map(|native| DatumPlaneReference::Face {
                face: FaceSelection::Native(native.to_owned()),
            })
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::DatumOffsetPlane {
            reference,
            distance,
        },
    ))
}

pub(super) fn project_datum_axis(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let direction = parse_vector3(feature.properties.get("Direction")?)?;
    valid_direction(direction).then_some(FeatureDefinition::Operation(
        FeatureOperation::DatumAxis {
            origin,
            direction: cadmpeg_ir::features::FeatureDirection3::new(direction)?,
        },
    ))
}

pub(super) fn project_datum_point(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::Operation(FeatureOperation::DatumPoint {
        position: parse_point3_mm(feature.properties.get("Position")?)?,
        construction: None,
    }))
}

pub(super) fn project_datum_coordinate_system(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let x_axis = parse_vector3(feature.properties.get("XAxis")?)?;
    let y_axis = parse_vector3(feature.properties.get("YAxis")?)?;
    let z_axis = parse_vector3(feature.properties.get("ZAxis")?)?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::DatumCoordinateSystem {
            frame: cadmpeg_ir::features::FeatureCoordinateFrame::from_parts(
                cadmpeg_ir::features::FeatureUnitPlaneFrame::from_parts(
                    origin,
                    cadmpeg_ir::units::UnitVector3::new(x_axis)?,
                    cadmpeg_ir::units::UnitVector3::new(y_axis)?,
                )?,
                cadmpeg_ir::units::UnitVector3::new(z_axis)?,
            )?,
        },
    ))
}

pub(super) fn project_equation_curve(feature: &Feature) -> Option<FeatureDefinition> {
    let (parameter, x_expression, y_expression, z_expression, start, end) = (|| {
        Some((
            feature.properties.get("Parameter")?.trim(),
            feature.properties.get("XEquation")?.trim(),
            feature.properties.get("YEquation")?.trim(),
            feature.properties.get("ZEquation")?.trim(),
            feature
                .properties
                .get("Start")?
                .trim()
                .parse::<f64>()
                .ok()?,
            feature.properties.get("End")?.trim().parse::<f64>().ok()?,
        ))
    })()?;
    cadmpeg_ir::features::FeatureEquationCurve::new(
        parameter.to_owned(),
        x_expression.to_owned(),
        y_expression.to_owned(),
        z_expression.to_owned(),
        super::parameter_dimension(feature, &["Start"])
            .or_else(|| cadmpeg_ir::scalar::FiniteReal::new(start).map(Into::into))?,
        super::parameter_dimension(feature, &["End"])
            .or_else(|| cadmpeg_ir::scalar::FiniteReal::new(end).map(Into::into))?,
    )
    .map(|curve| FeatureDefinition::Operation(FeatureOperation::EquationCurve { curve }))
}

pub(super) fn project_projected_curve(
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Option<FeatureDefinition> {
    let (source, target_faces, direction, bidirectional) = (|| {
        let source = feature.properties.get("Source")?;
        let source = native_by_source
            .get(source.as_str())
            .copied()
            .unwrap_or(source);
        let direction = match feature.properties.get("Direction") {
            Some(value) => CurveProjectionDirection::Vector(parse_valid_direction(value)?),
            None => CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
        };
        Some((
            source,
            feature.properties.get("TargetFaces")?,
            direction,
            feature
                .properties
                .get("Bidirectional")
                .and_then(|value| parse_bool(value))
                .unwrap_or(false),
        ))
    })()?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::ProjectedCurve {
            source: PathRef::Native(source.to_owned()),
            target_faces: FaceSelection::Native(target_faces.to_owned()),
            direction,
            bidirectional: Some(bidirectional),
        },
    ))
}

pub(super) fn project_composite_curve(
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Option<FeatureDefinition> {
    let segment_text = feature.properties.get("Segments")?;
    let mut segments = Vec::new();
    for source in segment_text
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let source = native_by_source.get(source).copied().unwrap_or(source);
        let segment = PathRef::Native(source.to_owned());
        segments.push(segment);
    }
    if segments.is_empty() {
        return None;
    }
    let closed = feature
        .properties
        .get("Closed")
        .map_or(Some(false), |value| parse_bool(value))?;
    segments.try_into().ok().map(|segments| {
        FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, closed })
    })
}

pub(super) fn project_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let axis_origin = parse_point3_mm(feature.properties.get("AxisOrigin")?)?;
    let axis_direction = parse_valid_direction(feature.properties.get("AxisDirection")?)?;
    let radius = super::parameter_dimension(feature, &["Radius"])?;
    let pitch = super::parameter_dimension(feature, &["Pitch"])?;
    let revolutions = super::parameter_dimension(feature, &["Revolutions"])?;
    let clockwise = feature
        .properties
        .get("Clockwise")
        .and_then(|value| parse_bool(value))
        .unwrap_or(false);
    let start_angle = super::parameter_dimension(feature, &["StartAngle"])
        .unwrap_or(Dimension::Literal(Angle::ZERO));
    Some(FeatureDefinition::Operation(FeatureOperation::Helix {
        axis_origin,
        axis_direction,
        radius,
        shape: cadmpeg_ir::features::HelixShape::Cylindrical {
            law: cadmpeg_ir::features::HelixLaw::PitchRevolutions { pitch, revolutions },
        },
        start_angle,
        clockwise,
        segment_turns: None,
        construction_style: None,
    }))
}

pub(super) fn project_native_axis_helix(_feature: &Feature) -> Option<FeatureDefinition> {
    // The observed D3/D4/D5 records contain height, pitch and turns without a
    // decoded authoring-law discriminator. Selecting an independent pair from
    // numerical agreement would invent intent. Native projection retains all
    // three canonical parameter identities and reports the unresolved law.
    None
}

pub(super) fn project_wrap(
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Option<FeatureDefinition> {
    let (profile, face, mode) = (|| {
        let profile = feature.properties.get("Profile")?;
        let profile = native_by_source
            .get(profile.as_str())
            .copied()
            .unwrap_or(profile);
        let face = feature.properties.get("Face")?;
        let mode_name = feature.properties.get("Mode")?;
        let mode = if mode_name.eq_ignore_ascii_case("emboss") {
            WrapMode::Emboss {
                depth: super::parameter_dimension(feature, &["Depth"])?,
            }
        } else if mode_name.eq_ignore_ascii_case("deboss") {
            WrapMode::Deboss {
                depth: super::parameter_dimension(feature, &["Depth"])?,
            }
        } else if mode_name.eq_ignore_ascii_case("scribe") {
            WrapMode::Scribe
        } else {
            return None;
        };
        Some((profile, face, mode))
    })()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Wrap {
        profile: PlanarProfileRef::Native(profile.to_owned()),
        face: FaceSelection::Native(face.to_owned()),
        mode,
    }))
}

#[cfg(test)]
mod tests {
    use super::{project_helix, project_native_axis_helix};
    use cadmpeg_core::text::NonBlankString;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    fn parameter(feature: &mut crate::records::Feature, name: &str, value: &str) {
        feature.parameters.insert(
            NonBlankString::new(name.to_owned()).expect("nonblank test parameter name"),
            value.to_owned(),
        );
    }

    fn property(feature: &mut crate::records::Feature, name: &str, value: &str) {
        feature.properties.insert(
            NonBlankString::new(name.to_owned()).expect("nonblank test property name"),
            value.to_owned(),
        );
    }

    #[test]
    fn helix_authored_pair_uses_canonical_parameter_identities() {
        let mut feature = crate::history::tests::feature("helix", None, 1);
        property(&mut feature, "AxisOrigin", "0mm,0mm,0mm");
        property(&mut feature, "AxisDirection", "0,0,1");
        parameter(&mut feature, "Radius", "2mm");
        parameter(&mut feature, "Pitch", "1mm");
        parameter(&mut feature, "Revolutions", "2.5");
        let Some(FeatureDefinition::Operation(FeatureOperation::Helix {
            shape:
                cadmpeg_ir::features::HelixShape::Cylindrical {
                    law: cadmpeg_ir::features::HelixLaw::PitchRevolutions { pitch, revolutions },
                },
            ..
        })) = project_helix(&feature)
        else {
            panic!("named authored pair projects");
        };
        assert_eq!(
            pitch.parameter(),
            super::super::parameter_dimension::<cadmpeg_ir::scalar::NonZeroLength>(
                &feature,
                &["Pitch"]
            )
            .as_ref()
            .and_then(cadmpeg_ir::features::Dimension::parameter)
        );
        assert_eq!(
            revolutions.parameter(),
            super::super::parameter_dimension::<cadmpeg_ir::scalar::PositiveReal>(
                &feature,
                &["Revolutions"]
            )
            .as_ref()
            .and_then(cadmpeg_ir::features::Dimension::parameter)
        );
    }

    #[test]
    fn native_axis_helix_does_not_infer_independent_pair_from_matching_values() {
        let mut feature = crate::history::tests::feature("helix", None, 1);
        parameter(&mut feature, "D3", "2.5mm");
        parameter(&mut feature, "D4", "1mm");
        parameter(&mut feature, "D5", "2.5");
        parameter(&mut feature, "D7", "0rad");
        assert!(project_native_axis_helix(&feature).is_none());
        parameter(&mut feature, "D3", "7mm");
        assert!(project_native_axis_helix(&feature).is_none());
    }

    #[test]
    fn helix_revolutions_are_admitted_once_into_the_operation() {
        let mut feature = crate::history::tests::feature("helix", None, 1);
        property(&mut feature, "AxisOrigin", "0mm,0mm,0mm");
        property(&mut feature, "AxisDirection", "0,0,1");
        parameter(&mut feature, "Radius", "2mm");
        parameter(&mut feature, "Pitch", "1mm");
        parameter(&mut feature, "Revolutions", "2.5");

        let parameters = crate::history::parameters::project_parameters(
            &cadmpeg_test_support::service_decode_context(),
            &[crate::records::FeatureHistory {
                id: "history".into(),
                part_name: None,
                properties: std::collections::BTreeMap::new(),
                content: Vec::new(),
                configurations: Vec::new(),
                features: vec![feature.clone()],
            }],
        )
        .unwrap();

        assert!(matches!(
            project_helix(&feature),
            Some(FeatureDefinition::Operation(FeatureOperation::Helix { shape, .. }))
                if shape.law().and_then(|law| law.resolved_from(&parameters))
                    .is_some_and(|law| law.revolutions.get() == 2.5)
        ));
        parameter(&mut feature, "Revolutions", "0");
        assert!(project_helix(&feature).is_none());
    }

    #[test]
    fn native_axis_helix_revolutions_are_admitted_once_into_the_operation() {
        let mut feature = crate::history::tests::feature("helix", None, 1);
        parameter(&mut feature, "D3", "2mm");
        parameter(&mut feature, "D4", "1mm");
        parameter(&mut feature, "D5", "2.5");
        parameter(&mut feature, "D7", "0rad");

        let parameters = crate::history::parameters::project_parameters(
            &cadmpeg_test_support::service_decode_context(),
            &[crate::records::FeatureHistory {
                id: "history".into(),
                part_name: None,
                properties: std::collections::BTreeMap::new(),
                content: Vec::new(),
                configurations: Vec::new(),
                features: vec![feature.clone()],
            }],
        )
        .unwrap();

        assert!(matches!(
            project_native_axis_helix(&feature),
            Some(FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis { law, .. }))
                if law.resolved_from(&parameters)
                    .is_some_and(|law| law.revolutions.get() == 2.5)
        ));
        parameter(&mut feature, "D5", "0");
        assert!(project_native_axis_helix(&feature).is_none());
    }
}
