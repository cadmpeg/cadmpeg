// SPDX-License-Identifier: Apache-2.0
//! Neutral projection of Protein texture assets and material property names.

use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{BumpMap, TextureMap2d, TextureRef};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};

#[derive(Clone, PartialEq)]
/// A decoded texture awaiting its appearance-property slot.
pub struct TextureAsset {
    /// Identity of the source texture asset.
    pub asset_guid: String,
    schema: String,
    paths: Vec<String>,
    urn: Option<String>,
    mapping: TextureMap2d,
    bump: Option<BumpMap>,
}

impl TextureAsset {
    /// Bind this texture to an appearance property.
    pub fn into_ref(self, slot: String) -> TextureRef {
        TextureRef {
            asset_guid: self.asset_guid,
            slot,
            schema: self.schema,
            paths: self.paths,
            urn: self.urn,
            mapping: self.mapping,
            bump: self.bump,
        }
    }
}

/// Project a bitmap or bump asset and count distances with unknown units.
/// Unknown distances use zero; the caller decides whether to retain the asset.
/// A stated float or distance that is not finite refuses the asset.
pub fn texture_asset(
    record: &crate::DecodedRecord,
) -> Result<(Option<TextureAsset>, usize), CodecError> {
    if !matches!(
        record.schema.as_str(),
        "UnifiedBitmapSchema" | "BumpMapSchema"
    ) {
        return Ok((None, 0));
    }
    let paths = record
        .properties
        .iter()
        .find_map(|(id, property)| {
            (id.ends_with("_Bitmap"))
                .then(|| property.value())
                .flatten()
                .and_then(|value| match value {
                    crate::property::PropertyValue::TextureUri(paths) => Some(paths.clone()),
                    _ => None,
                })
        })
        .unwrap_or_default();
    let urn = record.properties.iter().find_map(|(id, property)| {
        (id.ends_with("_Bitmap_urn"))
            .then(|| property.value())
            .flatten()
            .and_then(|value| match value {
                crate::property::PropertyValue::String(value) if !value.is_empty() => {
                    Some(value.clone())
                }
                _ => None,
            })
    });
    let mut untyped_distance_properties = 0usize;
    let mut distance = |suffix: &str| match distance_property(record, suffix) {
        Ok(value) => Ok(value.unwrap_or(Length::ZERO)),
        Err(DistanceError::UnknownUnit(_)) => {
            untyped_distance_properties += 1;
            Ok(Length::ZERO)
        }
        Err(DistanceError::NonFinite) => Err(CodecError::malformed(format_args!(
            "Protein asset {} distance {suffix} is non-finite after millimetre conversion",
            record.guid
        ))),
    };
    let real = |suffix: &str, default| finite_float_property(record, suffix, default);
    let mapping = TextureMap2d {
        map_channel: integer_property(record, "MapChannel").unwrap_or(1),
        uvw_source: integer_property(record, "MapChannel_UVWSource_Advanced").unwrap_or(0),
        u_offset: real("UOffset", FiniteReal::ZERO)?,
        v_offset: real("VOffset", FiniteReal::ZERO)?,
        u_scale: real("UScale", FiniteReal::ONE)?,
        v_scale: real("VScale", FiniteReal::ONE)?,
        // A finite angle in degrees is finite in radians: the factor is below one.
        rotation: Angle::new(real("WAngle", FiniteReal::ZERO)?.get().to_radians()).ok_or_else(
            || {
                CodecError::malformed(format_args!(
                    "Protein asset {} property WAngle is non-finite in radians",
                    record.guid
                ))
            },
        )?,
        repeat_u: boolean_property(record, "URepeat").unwrap_or(true),
        repeat_v: boolean_property(record, "VRepeat").unwrap_or(true),
        real_world_offset_x: distance("RealWorldOffsetX")?,
        real_world_offset_y: distance("RealWorldOffsetY")?,
        real_world_scale_x: distance("RealWorldScaleX")?,
        real_world_scale_y: distance("RealWorldScaleY")?,
    };
    let bump = if record.schema == "BumpMapSchema" {
        Some(BumpMap {
            normal_map: integer_property(record, "bumpmap_Type") == Some(1),
            depth: distance("bumpmap_Depth")?,
            normal_scale: real("bumpmap_NormalScale", FiniteReal::ONE)?,
        })
    } else {
        None
    };
    let texture = TextureAsset {
        asset_guid: record.guid.clone(),
        schema: record.schema.clone(),
        paths,
        urn,
        mapping,
        bump,
    };
    Ok((Some(texture), untyped_distance_properties))
}

fn property_with_suffix<'a>(
    record: &'a crate::DecodedRecord,
    suffix: &str,
) -> Option<&'a crate::property::PropertyValue> {
    let qualified_suffix = format!("_{suffix}");
    record
        .properties
        .iter()
        .find(|(id, _)| *id == suffix || id.ends_with(&qualified_suffix))
        .and_then(|(_, property)| property.value())
}

/// Map Protein property names to the neutral material vocabulary.
pub fn neutral_property_name(id: &str) -> &str {
    match id {
        "generic_reflectivity_at_0deg" => "reflectivity_at_0deg",
        "generic_refraction_index" | "transparent_refraction_index" => "refraction_index",
        _ => id,
    }
}

/// Whether a schema describes physical rather than visual properties.
pub fn is_physical_schema(schema: &str) -> bool {
    schema == "PhysMatSchema" || schema.starts_with("Structural") || schema.starts_with("Thermal")
}

fn integer_property(record: &crate::DecodedRecord, suffix: &str) -> Option<u32> {
    match property_with_suffix(record, suffix)? {
        crate::property::PropertyValue::Integer(value) => Some(*value),
        _ => None,
    }
}

/// The float property the record states under `suffix`, admitted finite, or
/// `default` when it states none.
fn finite_float_property(
    record: &crate::DecodedRecord,
    suffix: &str,
    default: FiniteReal,
) -> Result<FiniteReal, CodecError> {
    let Some(crate::property::PropertyValue::Float(value)) = property_with_suffix(record, suffix)
    else {
        return Ok(default);
    };
    finite_scalar(record, suffix, *value)
}

/// A float the record states under `property`, admitted finite.
pub fn finite_scalar(
    record: &crate::DecodedRecord,
    property: &str,
    value: f64,
) -> Result<FiniteReal, CodecError> {
    FiniteReal::new(value).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "Protein asset {} property {property} is non-finite",
            record.guid
        ))
    })
}

fn boolean_property(record: &crate::DecodedRecord, suffix: &str) -> Option<bool> {
    match property_with_suffix(record, suffix)? {
        crate::property::PropertyValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
enum DistanceError {
    UnknownUnit(u32),
    NonFinite,
}

fn distance_property(
    record: &crate::DecodedRecord,
    suffix: &str,
) -> Result<Option<Length>, DistanceError> {
    let Some(crate::property::PropertyValue::Distance { unit, value }) =
        property_with_suffix(record, suffix)
    else {
        return Ok(None);
    };
    let factor = match *unit {
        0x2016 => 25.4,
        0x200e => 1.0,
        0x200d => 10.0,
        unit => return Err(DistanceError::UnknownUnit(unit)),
    };
    Length::new(value * factor)
        .map(Some)
        .ok_or(DistanceError::NonFinite)
}

#[cfg(test)]
mod tests;
