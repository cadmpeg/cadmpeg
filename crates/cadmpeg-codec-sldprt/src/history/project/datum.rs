// SPDX-License-Identifier: Apache-2.0
//! Datum, curve, helix, and wrap projection.

use super::{copy_projected_feature_id, parameter_literal, property_literal, property_value};
use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        CurveProjectionDirection, CurveProjectionDirectionState, DatumPlaneReference,
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef, WrapMode,
    },
    scalar::Angle,
};
use std::collections::HashMap;

use crate::history::literals::{
    parse_angle_rad, parse_bool, parse_dimension_length_mm, parse_length_mm, parse_point3_mm,
    parse_positive_length_mm, parse_valid_direction, parse_vector3, valid_direction,
    valid_plane_frame,
};

const DATUM_OPERATION: &str = "retain SLDPRT datum and curve reference";

fn copy_reference_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.copy_retained_text(text, DATUM_OPERATION)
}

/// A point property; `None` when it is absent or malformed.
fn point(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<Option<cadmpeg_ir::features::FinitePoint3>, CodecError> {
    Ok(property_literal(ctx, feature, name)?.and_then(parse_point3_mm))
}

/// A vector property; `None` when it is absent or malformed.
fn vector(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<Option<cadmpeg_ir::math::Vector3>, CodecError> {
    Ok(property_literal(ctx, feature, name)?.and_then(parse_vector3))
}

/// A real-number parameter literal.
fn real(ctx: &DecodeContext<'_>, feature: &Feature, name: &str) -> Result<Option<f64>, CodecError> {
    let Some(value) = ctx.get_btree_map(&feature.parameters, name, super::FEATURE_LITERAL)? else {
        return Ok(None);
    };
    let value = ctx.trim_text(value, super::FEATURE_LITERAL)?;
    Ok(ctx.parse_text::<f64>(value, super::FEATURE_LITERAL)?.ok())
}

pub(super) fn project_datum_plane(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let origin = require!(point(ctx, feature, "Origin")?);
    let normal = require!(vector(ctx, feature, "Normal")?);
    let u_axis = require!(vector(ctx, feature, "UAxis")?);
    Ok((|| {
        valid_plane_frame(normal, u_axis).then_some(FeatureDefinition::Operation(
            FeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::from_parts(
                    origin,
                    cadmpeg_ir::features::FeatureDirection3::new(normal)?,
                    cadmpeg_ir::features::FeatureDirection3::new(u_axis)?,
                )?,
            },
        ))
    })())
}

pub(in crate::history) fn project_offset_plane(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &super::NeutralByKey<'_, '_>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let distance =
        require!(parameter_literal(ctx, feature, "D1")?.and_then(parse_dimension_length_mm));
    let resolved_frame = || -> Result<_, CodecError> {
        let origin = require!(point(ctx, feature, "ReferenceFaceOrigin")?);
        let normal = require!(vector(ctx, feature, "ReferenceFaceNormal")?);
        let u_axis = require!(vector(ctx, feature, "ReferenceFaceUAxis")?);
        Ok((|| {
            cadmpeg_ir::features::FeatureSupportPlaneFrame::from_parts(
                origin,
                cadmpeg_ir::features::FeatureDirection3::new(normal)?,
                cadmpeg_ir::features::FeatureDirection3::new(u_axis)?,
            )
        })())
    };
    let named = match property_value(ctx, feature, "Reference")? {
        Some(source) => Some(source),
        None => property_value(ctx, feature, "Plane")?,
    };
    let referenced = match named {
        Some(source) => ctx
            .get_hash_map(by_source, source, "look up SLDPRT hash key")?
            .copied(),
        None => None,
    };
    let reference = if let Some(reference) = referenced {
        Some(DatumPlaneReference::Feature {
            feature: copy_projected_feature_id(ctx, reference)?,
        })
    } else if let Some(frame) = resolved_frame()? {
        Some(DatumPlaneReference::ResolvedPlane { frame })
    } else if let Some(native) = property_value(ctx, feature, "ReferenceFaceNative")? {
        Some(DatumPlaneReference::Face {
            face: FaceSelection::Native(copy_reference_text(ctx, native)?),
        })
    } else {
        None
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DatumOffsetPlane {
            reference,
            distance,
        },
    )))
}

pub(super) fn project_datum_axis(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let origin = require!(point(ctx, feature, "Origin")?);
    let direction = require!(vector(ctx, feature, "Direction")?);
    Ok((|| {
        valid_direction(direction).then_some(FeatureDefinition::Operation(
            FeatureOperation::DatumAxis {
                origin,
                direction: cadmpeg_ir::features::FeatureDirection3::new(direction)?,
            },
        ))
    })())
}

pub(super) fn project_datum_point(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    Ok(point(ctx, feature, "Position")?.map(|position| {
        FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position,
            construction: None,
        })
    }))
}

pub(super) fn project_datum_coordinate_system(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let origin = require!(point(ctx, feature, "Origin")?);
    let x_axis = require!(vector(ctx, feature, "XAxis")?);
    let y_axis = require!(vector(ctx, feature, "YAxis")?);
    let z_axis = require!(vector(ctx, feature, "ZAxis")?);
    Ok((|| {
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
    })())
}

pub(super) fn project_equation_curve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let trimmed = |name| -> Result<_, CodecError> {
        property_value(ctx, feature, name)?
            .map(|value| ctx.trim_text(value, super::FEATURE_LITERAL))
            .transpose()
    };
    let parameter = require!(trimmed("Parameter")?);
    let x_expression = require!(trimmed("XEquation")?);
    let y_expression = require!(trimmed("YEquation")?);
    let z_expression = require!(trimmed("ZEquation")?);
    let start = require!(trimmed("Start")?);
    let start = require!(ctx.parse_text::<f64>(start, super::FEATURE_LITERAL)?.ok());
    let end = require!(trimmed("End")?);
    let end = require!(ctx.parse_text::<f64>(end, super::FEATURE_LITERAL)?.ok());
    let (curve, storage) = ctx.with_scoped_storage("retain SLDPRT equation curve", || {
        Ok::<_, CodecError>(cadmpeg_ir::features::FeatureEquationCurve::new(
            copy_reference_text(ctx, parameter)?,
            copy_reference_text(ctx, x_expression)?,
            copy_reference_text(ctx, y_expression)?,
            copy_reference_text(ctx, z_expression)?,
            start,
            end,
        ))
    })?;
    let Some(curve) = curve else {
        return Ok(None);
    };
    storage.commit()?;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::EquationCurve { curve },
    )))
}

/// The native record a source names, or the source text itself.
fn native_source<'s>(
    ctx: &DecodeContext<'_>,
    native_by_source: &HashMap<&str, &'s str>,
    source: &'s str,
) -> Result<&'s str, CodecError> {
    Ok(ctx
        .get_hash_map(native_by_source, source, "look up SLDPRT hash key")?
        .copied()
        .unwrap_or(source))
}

pub(super) fn project_projected_curve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let source = require!(property_value(ctx, feature, "Source")?);
    let source = native_source(ctx, native_by_source, source)?;
    let direction = match property_literal(ctx, feature, "Direction")? {
        Some(value) => CurveProjectionDirection::Vector(require!(parse_valid_direction(value))),
        None => CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
    };
    let target_faces = require!(property_value(ctx, feature, "TargetFaces")?);
    let bidirectional = property_value(ctx, feature, "Bidirectional")?
        .and_then(parse_bool)
        .unwrap_or(false);
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::ProjectedCurve {
            source: PathRef::Native(copy_reference_text(ctx, source)?),
            target_faces: FaceSelection::Native(copy_reference_text(ctx, target_faces)?),
            direction,
            bidirectional: Some(bidirectional),
        },
    )))
}

pub(super) fn project_composite_curve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    const OPERATION: &str = "project SLDPRT composite curve segments";
    let segment_text = require!(property_value(ctx, feature, "Segments")?);
    let mut closed = None;
    let mut segments = Vec::new();
    let mut characters = segment_text.char_indices();
    while let Some(start) = ctx.find_map(
        &mut characters,
        |(offset, character)| Ok((character != ';' && !character.is_whitespace()).then_some(offset)),
        OPERATION,
    )? {
        if closed.is_none() {
            closed = Some(require!(
                property_value(ctx, feature, "Closed")?.map_or(Some(false), parse_bool)
            ));
        }
        let delimiter = ctx.find_map(
            &mut characters,
            |(offset, character)| Ok((character == ';').then_some(offset)),
            OPERATION,
        )?;
        let end = delimiter.unwrap_or(segment_text.len());
        let source = ctx.trim_text(&segment_text[start..end], OPERATION)?;
        let source = native_source(ctx, native_by_source, source)?;
        let segment = PathRef::Native(copy_reference_text(ctx, source)?);
        ctx.push_vec(&mut segments, segment, OPERATION)?;
        if delimiter.is_none() {
            break;
        }
    }
    let Some(closed) = closed else {
        return Ok(None);
    };
    Ok(segments.try_into().ok().map(|segments| {
        FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, closed })
    }))
}

pub(super) fn project_helix(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let axis_origin = require!(point(ctx, feature, "AxisOrigin")?);
    let axis_direction =
        require!(property_literal(ctx, feature, "AxisDirection")?.and_then(parse_valid_direction));
    let radius =
        require!(parameter_literal(ctx, feature, "Radius")?.and_then(parse_positive_length_mm));
    let pitch = require!(parameter_literal(ctx, feature, "Pitch")?.and_then(parse_length_mm));
    let revolutions = require!(
        real(ctx, feature, "Revolutions")?.and_then(cadmpeg_ir::scalar::PositiveReal::new)
    );
    let clockwise = property_value(ctx, feature, "Clockwise")?
        .and_then(parse_bool)
        .unwrap_or(false);
    let start_angle = match parameter_literal(ctx, feature, "StartAngle")? {
        Some(value) => require!(parse_angle_rad(value)),
        None => Angle::ZERO,
    };
    let pitch = require!(cadmpeg_ir::scalar::NonZeroLength::try_from(pitch).ok());
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Helix {
            axis_origin,
            axis_direction,
            radius,
            shape: cadmpeg_ir::features::HelixShape::Cylindrical { pitch },
            revolutions,
            start_angle,
            clockwise,
            segment_turns: None,
            construction_style: None,
        },
    )))
}

pub(super) fn project_native_axis_helix(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let axial_rise =
        require!(parameter_literal(ctx, feature, "D3")?.and_then(parse_dimension_length_mm));
    let pitch =
        require!(parameter_literal(ctx, feature, "D4")?.and_then(parse_dimension_length_mm));
    let revolutions =
        require!(real(ctx, feature, "D5")?.and_then(cadmpeg_ir::scalar::PositiveReal::new));
    let start_angle = require!(parameter_literal(ctx, feature, "D7")?.and_then(parse_angle_rad));
    let clockwise = property_value(ctx, feature, "Clockwise")?
        .and_then(parse_bool)
        .unwrap_or(false);
    let axis_native_ref = cadmpeg_core::text::NonBlankString::for_decode(
        ctx,
        copy_reference_text(ctx, &feature.id)?,
        "validate nonblank text",
    )?;
    Ok(axis_native_ref.map(|axis_native_ref| {
        FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis {
            axis_native_ref,
            axial_rise,
            pitch,
            revolutions,
            start_angle,
            clockwise,
        })
    }))
}

pub(super) fn project_wrap(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let profile = require!(property_value(ctx, feature, "Profile")?);
    let profile = native_source(ctx, native_by_source, profile)?;
    let face = require!(property_value(ctx, feature, "Face")?);
    let mode_name = require!(property_value(ctx, feature, "Mode")?);
    let depth = || -> Result<_, CodecError> {
        Ok(parameter_literal(ctx, feature, "Depth")?.and_then(parse_positive_length_mm))
    };
    let mode = if mode_name.eq_ignore_ascii_case("emboss") {
        WrapMode::Emboss {
            depth: require!(depth()?),
        }
    } else if mode_name.eq_ignore_ascii_case("deboss") {
        WrapMode::Deboss {
            depth: require!(depth()?),
        }
    } else if mode_name.eq_ignore_ascii_case("scribe") {
        WrapMode::Scribe
    } else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Wrap {
        profile: PlanarProfileRef::Native(copy_reference_text(ctx, profile)?),
        face: FaceSelection::Native(copy_reference_text(ctx, face)?),
        mode,
    })))
}

#[cfg(test)]
mod tests {
    use super::{project_helix, project_native_axis_helix};
    use cadmpeg_core::text::NonBlankString;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    fn parameter(feature: &mut crate::records::Feature, name: &str, value: &str) {
        feature.parameters.insert(
            NonBlankString::try_from(name.to_owned()).expect("nonblank test parameter name"),
            value.to_owned(),
        );
    }

    fn property(feature: &mut crate::records::Feature, name: &str, value: &str) {
        feature.properties.insert(
            NonBlankString::try_from(name.to_owned()).expect("nonblank test property name"),
            value.to_owned(),
        );
    }

    #[test]
    fn helix_revolutions_are_admitted_once_into_the_operation() {
        let mut feature = crate::history::tests::feature("helix", None, 1);
        property(&mut feature, "AxisOrigin", "0mm,0mm,0mm");
        property(&mut feature, "AxisDirection", "0,0,1");
        parameter(&mut feature, "Radius", "2mm");
        parameter(&mut feature, "Pitch", "1mm");
        parameter(&mut feature, "Revolutions", "2.5");

        assert!(matches!(
            project_helix(&cadmpeg_test_support::service_decode_context(), &feature).unwrap(),
            Some(FeatureDefinition::Operation(FeatureOperation::Helix { revolutions, .. }))
                if revolutions.get() == 2.5
        ));
        parameter(&mut feature, "Revolutions", "0");
        assert!(
            project_helix(&cadmpeg_test_support::service_decode_context(), &feature)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn native_axis_helix_revolutions_are_admitted_once_into_the_operation() {
        let mut feature = crate::history::tests::feature("helix", None, 1);
        parameter(&mut feature, "D3", "2mm");
        parameter(&mut feature, "D4", "1mm");
        parameter(&mut feature, "D5", "2.5");
        parameter(&mut feature, "D7", "0rad");

        assert!(matches!(
            project_native_axis_helix(&cadmpeg_test_support::service_decode_context(), &feature).unwrap(),
            Some(FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis { revolutions, .. }))
                if revolutions.get() == 2.5
        ));
        parameter(&mut feature, "D5", "0");
        assert!(project_native_axis_helix(
            &cadmpeg_test_support::service_decode_context(),
            &feature
        )
        .unwrap()
        .is_none());
    }
}
