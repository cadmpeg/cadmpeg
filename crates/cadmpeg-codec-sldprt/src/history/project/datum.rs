// SPDX-License-Identifier: Apache-2.0
//! Datum, curve, helix, and wrap projection.

use super::copy_projected_feature_id;
use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        CurveProjectionDirection, CurveProjectionDirectionState, DatumPlaneReference,
        FaceSelection, FeatureDefinition, FeatureId, FeatureOperation, PathRef, PlanarProfileRef,
        WrapMode,
    },
    scalar::Angle,
};
use std::collections::HashMap;

use crate::history::literals::{
    parse_angle_rad, parse_bool, parse_dimension_length_mm, parse_length_mm, parse_point3_mm,
    parse_positive_length_mm, parse_valid_direction, parse_vector3, valid_direction,
    valid_plane_frame,
};

fn copy_reference_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(text.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "retain SLDPRT datum and curve reference",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(copy_work, "retain SLDPRT datum and curve reference")?;
    ctx.format_retained(
        format_args!("{text}"),
        "retain SLDPRT datum and curve reference",
    )
}

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
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(distance) = feature
        .parameters
        .get("D1")
        .and_then(|value| parse_dimension_length_mm(value))
    else {
        return Ok(None);
    };
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
        .or_else(|| feature.properties.get("Plane")).map(|source| {Ok::<_, cadmpeg_core::CodecError>(ctx.get_hash_map(&(by_source), source.as_str(), "look up SLDPRT hash key")?)}).transpose()?.flatten()
    {
        Some(DatumPlaneReference::Feature {
            feature: copy_projected_feature_id(ctx, reference)?,
        })
    } else if let Some(frame) = resolved_frame() {
        Some(DatumPlaneReference::ResolvedPlane { frame })
    } else if let Some(native) = feature.properties.get("ReferenceFaceNative") {
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

pub(super) fn project_equation_curve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((parameter, x_expression, y_expression, z_expression, start, end)) = (|| {
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
    })() else {
        return Ok(None);
    };
    Ok(cadmpeg_ir::features::FeatureEquationCurve::new(
        copy_reference_text(ctx, parameter)?,
        copy_reference_text(ctx, x_expression)?,
        copy_reference_text(ctx, y_expression)?,
        copy_reference_text(ctx, z_expression)?,
        start,
        end,
    )
    .map(|curve| FeatureDefinition::Operation(FeatureOperation::EquationCurve { curve })))
}

pub(super) fn project_projected_curve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((source, target_faces, direction, bidirectional)) = (|| -> Result<_, cadmpeg_core::CodecError> {
        let source = match feature.properties.get("Source") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let source = ctx.get_hash_map(&(native_by_source), source.as_str(), "look up SLDPRT hash key")?
            .copied()
            .unwrap_or(source);
        let direction = match feature.properties.get("Direction") {
            Some(value) => CurveProjectionDirection::Vector(match parse_valid_direction(value) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }),
            None => CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
        };
        Ok::<_, cadmpeg_core::CodecError>(Some((
            source,
            match feature.properties.get("TargetFaces") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) },
            direction,
            feature
                .properties
                .get("Bidirectional")
                .and_then(|value| parse_bool(value))
                .unwrap_or(false),
        )))
    })()? else {
        return Ok(None);
    };
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
    native_by_source: &HashMap<String, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(segment_text) = feature.properties.get("Segments") else {
        return Ok(None);
    };
    let mut segments = Vec::new();
    for source in segment_text
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        ctx.charge_work(1, "project SLDPRT composite curve segments")?;
        let source = ctx.get_hash_map(&(native_by_source), source, "look up SLDPRT hash key")?.copied().unwrap_or(source);
        let segment = PathRef::Native(copy_reference_text(ctx, source)?);
        ctx.reserve_vec(&mut segments, 1, "project SLDPRT composite curve segments")?;
        segments.push(segment);
    }
    if segments.is_empty() {
        return Ok(None);
    }
    let Some(closed) = feature
        .properties
        .get("Closed")
        .map_or(Some(false), |value| parse_bool(value))
    else {
        return Ok(None);
    };
    Ok(segments.try_into().ok().map(|segments| {
        FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, closed })
    }))
}

pub(super) fn project_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let axis_origin = parse_point3_mm(feature.properties.get("AxisOrigin")?)?;
    let axis_direction = parse_valid_direction(feature.properties.get("AxisDirection")?)?;
    let radius = parse_positive_length_mm(feature.parameters.get("Radius")?)?;
    let pitch = parse_length_mm(feature.parameters.get("Pitch")?)?;
    let revolutions = feature
        .parameters
        .get("Revolutions")?
        .trim()
        .parse::<f64>()
        .ok()
        .and_then(cadmpeg_ir::scalar::PositiveReal::new)?;
    let clockwise = feature
        .properties
        .get("Clockwise")
        .and_then(|value| parse_bool(value))
        .unwrap_or(false);
    let start_angle = match feature.parameters.get("StartAngle") {
        Some(value) => parse_angle_rad(value)?,
        None => Angle::ZERO,
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Helix {
        axis_origin,
        axis_direction,
        radius,
        shape: cadmpeg_ir::features::HelixShape::Cylindrical {
            pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(pitch).ok()?,
        },
        revolutions,
        start_angle,
        clockwise,
        segment_turns: None,
        construction_style: None,
    }))
}

pub(super) fn project_native_axis_helix(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((axial_rise, pitch, revolutions, start_angle, clockwise)) = (|| {
        let axial_rise = parse_dimension_length_mm(feature.parameters.get("D3")?)?;
        let pitch = parse_dimension_length_mm(feature.parameters.get("D4")?)?;
        let revolutions = feature
            .parameters
            .get("D5")?
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(cadmpeg_ir::scalar::PositiveReal::new)?;
        let start_angle = parse_angle_rad(feature.parameters.get("D7")?)?;
        let clockwise = feature
            .properties
            .get("Clockwise")
            .and_then(|value| parse_bool(value))
            .unwrap_or(false);
        Some((axial_rise, pitch, revolutions, start_angle, clockwise))
    })() else {
        return Ok(None);
    };
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
    native_by_source: &HashMap<String, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((profile, face, mode)) = (|| -> Result<_, cadmpeg_core::CodecError> {
        let profile = match feature.properties.get("Profile") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let profile = ctx.get_hash_map(&(native_by_source), profile.as_str(), "look up SLDPRT hash key")?
            .copied()
            .unwrap_or(profile);
        let face = match feature.properties.get("Face") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let mode_name = match feature.properties.get("Mode") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let mode = if mode_name.eq_ignore_ascii_case("emboss") {
            WrapMode::Emboss {
                depth: match parse_positive_length_mm(match feature.parameters.get("Depth") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) },
            }
        } else if mode_name.eq_ignore_ascii_case("deboss") {
            WrapMode::Deboss {
                depth: match parse_positive_length_mm(match feature.parameters.get("Depth") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) },
            }
        } else if mode_name.eq_ignore_ascii_case("scribe") {
            WrapMode::Scribe
        } else {
            return Ok::<_, cadmpeg_core::CodecError>(None);
        };
        Ok::<_, cadmpeg_core::CodecError>(Some((profile, face, mode)))
    })()? else {
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
            project_helix(&feature),
            Some(FeatureDefinition::Operation(FeatureOperation::Helix { revolutions, .. }))
                if revolutions.get() == 2.5
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
