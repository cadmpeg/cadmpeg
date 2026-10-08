// SPDX-License-Identifier: Apache-2.0
//! Neutral projection of Protein texture assets and material property names.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{BumpMap, TextureMap2d, TextureRef};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};

use crate::property::{DecodedProperty, PropertyValue};

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
    /// Compares all fields, admitting only visited text bytes and path slots.
    /// Fixed mapping fields and unequal collection lengths require no scan.
    pub fn equal_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        other: &Self,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        // Observe the original refusal even when a fixed-size comparison can
        // decide the result without visiting text or paths.
        ctx.charge_work(0, operation)?;
        if self.mapping != other.mapping
            || self.bump != other.bump
            || self.paths.len() != other.paths.len()
        {
            return Ok(false);
        }
        if !ctx.equal_bytes(self.asset_guid.as_bytes(), other.asset_guid.as_bytes(), operation)?
            || !ctx.equal_bytes(self.schema.as_bytes(), other.schema.as_bytes(), operation)?
        {
            return Ok(false);
        }
        match (&self.urn, &other.urn) {
            (Some(left), Some(right)) => {
                if !ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation)? {
                    return Ok(false);
                }
            }
            (None, None) => {}
            _ => return Ok(false),
        }
        if self.paths.is_empty() {
            return Ok(true);
        }
        ctx.all_by(
            self.paths.iter().zip(&other.paths),
            |(left, right)| ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation),
            operation,
        )
    }

    /// Bind this texture to an appearance property. The caller admits the
    /// collection slot that keeps the reference.
    pub fn to_ref(&self, ctx: &DecodeContext<'_>, slot: &str) -> Result<TextureRef, CodecError> {
        Ok(TextureRef {
            asset_guid: ctx
                .copy_retained_text(&self.asset_guid, "Protein appearance texture field")?,
            slot: ctx.copy_retained_text(slot, "Protein appearance texture field")?,
            schema: ctx.copy_retained_text(&self.schema, "Protein appearance texture field")?,
            paths: ctx.try_collect_vec(
                self.paths
                    .iter()
                    .map(|path| ctx.copy_retained_text(path, "Protein appearance texture path")),
                "Protein appearance texture paths",
            )?,
            urn: self
                .urn
                .as_deref()
                .map(|urn| ctx.copy_retained_text(urn, "Protein appearance texture URN"))
                .transpose()?,
            mapping: self.mapping.clone(),
            bump: self.bump.clone(),
        })
    }
}

/// Result of projecting one record as a texture asset.
pub enum TextureAssetResult {
    /// The record describes another asset type.
    NotTexture,
    /// Every stated distance has a length unit.
    Usable(TextureAsset),
    /// The source states distance units with no length conversion.
    UnknownDistanceUnit {
        /// Number of stated distance properties with unknown units.
        count: usize,
    },
}

/// Project a bitmap or bump asset without constructing a scale from an unknown unit.
/// A stated float or distance that is not finite refuses the asset.
pub fn texture_asset(
    ctx: &DecodeContext<'_>,
    record: &crate::DecodedRecord,
) -> Result<TextureAssetResult, CodecError> {
    ctx.charge_work(0, "Protein texture property selection")?;
    if !matches!(
        record.schema.as_str(),
        "UnifiedBitmapSchema" | "BumpMapSchema"
    ) {
        return Ok(TextureAssetResult::NotTexture);
    }
    // The grammar has thirteen bitmap fields and three additional bump fields.
    // Each suffix comparison visits at most the length of its fixed literal.
    const SUFFIXES: [&str; 16] = [
        "RealWorldOffsetX", "RealWorldOffsetY", "RealWorldScaleX", "RealWorldScaleY",
        "MapChannel", "MapChannel_UVWSource_Advanced", "UOffset", "VOffset",
        "UScale", "VScale", "WAngle", "URepeat", "VRepeat",
        "bumpmap_Depth", "bumpmap_Type", "bumpmap_NormalScale",
    ];
    let field_count = if record.schema == "BumpMapSchema" { 16 } else { 13 };
    let mut selected: [Option<&DecodedProperty>; 16] = [None; 16];
    let mut source_paths = None;
    let mut source_urn = None;
    for (id, property) in ctx.admit_iter(&record.properties, "Protein texture property selection")? {
        for (index, suffix) in SUFFIXES[..field_count].iter().enumerate() {
            if selected[index].is_none()
                && id.strip_suffix(*suffix)
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('_'))
            {
                // A named property wins even when its carrier is not the
                // expected one. A later matching name does not replace it.
                selected[index] = Some(property);
            }
        }
        if source_paths.is_none() && id.ends_with("_Bitmap") {
            if let Some(PropertyValue::TextureUri(paths)) = property.value() {
                source_paths = Some(paths);
            }
        }
        if source_urn.is_none() && id.ends_with("_Bitmap_urn") {
            if let Some(PropertyValue::String(urn)) = property.value() {
                if !urn.is_empty() {
                    source_urn = Some(urn);
                }
            }
        }
    }
    let [offset_x, offset_y, scale_x, scale_y, map_channel, uvw_source,
        u_offset, v_offset, u_scale, v_scale, w_angle, repeat_u, repeat_v,
        bump_depth, bump_type, normal_scale] = selected.map(|property| property.and_then(DecodedProperty::value));
    let mut distances = [Length::ZERO; 5];
    let mut unknown_count = 0_usize;
    for (index, (suffix, property)) in [
        ("RealWorldOffsetX", offset_x),
        ("RealWorldOffsetY", offset_y),
        ("RealWorldScaleX", scale_x),
        ("RealWorldScaleY", scale_y),
        ("bumpmap_Depth", bump_depth),
    ].into_iter().enumerate() {
        if index == 4 && record.schema != "BumpMapSchema" {
            break;
        }
        match distance_property(property) {
            Ok(Some(value)) => distances[index] = value,
            Ok(None) => {}
            Err(DistanceError::UnknownUnit(_)) => unknown_count += 1,
            Err(DistanceError::NonFinite) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!(
                        "Protein asset {} distance {suffix} is non-finite after millimetre conversion",
                        record.guid
                    ),
                    "Protein malformed detail",
                )?));
            }
        }
    }
    if unknown_count != 0 {
        return Ok(TextureAssetResult::UnknownDistanceUnit { count: unknown_count });
    }
    let paths = match source_paths {
        Some(paths) => ctx.try_collect_vec(
            paths
                .iter()
                .map(|path| ctx.copy_retained_text(path, "Protein texture path")),
            "Protein texture paths",
        )?,
        None => Vec::new(),
    };
    let urn = source_urn
        .map(|urn| ctx.copy_retained_text(urn, "Protein texture URN"))
        .transpose()?;
    let mapping = TextureMap2d {
        map_channel: integer_property(map_channel).unwrap_or(1),
        uvw_source: integer_property(uvw_source).unwrap_or(0),
        u_offset: finite_float_property(u_offset, FiniteReal::ZERO),
        v_offset: finite_float_property(v_offset, FiniteReal::ZERO),
        u_scale: finite_float_property(u_scale, FiniteReal::ONE),
        v_scale: finite_float_property(v_scale, FiniteReal::ONE),
        // A finite angle in degrees is finite in radians: the factor is below one.
        rotation: Angle::new(
            finite_float_property(w_angle, FiniteReal::ZERO)
                .get()
                .to_radians(),
        )
        .ok_or_else(|| {
            ctx.format_retained(
                format_args!(
                    "Protein asset {} property WAngle is non-finite in radians",
                    record.guid
                ),
                "Protein malformed detail",
            ).map(CodecError::Malformed).unwrap_or_else(|error| error)
        })?,
        repeat_u: boolean_property(repeat_u).unwrap_or(true),
        repeat_v: boolean_property(repeat_v).unwrap_or(true),
        real_world_offset_x: distances[0],
        real_world_offset_y: distances[1],
        real_world_scale_x: distances[2],
        real_world_scale_y: distances[3],
    };
    let bump = if record.schema == "BumpMapSchema" {
        Some(BumpMap {
            normal_map: integer_property(bump_type) == Some(1),
            depth: distances[4],
            normal_scale: finite_float_property(normal_scale, FiniteReal::ONE),
        })
    } else {
        None
    };
    let texture = TextureAsset {
        asset_guid: ctx.copy_retained_text(&record.guid, "Protein texture GUID")?,
        schema: ctx.copy_retained_text(&record.schema, "Protein texture schema")?,
        paths,
        urn,
        mapping,
        bump,
    };
    Ok(TextureAssetResult::Usable(texture))
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

fn integer_property(property: Option<&PropertyValue>) -> Option<u32> {
    match property {
        Some(PropertyValue::Integer(value)) => Some(*value),
        _ => None,
    }
}

fn finite_float_property(property: Option<&PropertyValue>, default: FiniteReal) -> FiniteReal {
    match property {
        Some(PropertyValue::Float(value)) => *value,
        _ => default,
    }
}

fn boolean_property(property: Option<&PropertyValue>) -> Option<bool> {
    match property {
        Some(PropertyValue::Boolean(value)) => Some(*value),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
enum DistanceError {
    UnknownUnit(u32),
    NonFinite,
}

fn distance_property(property: Option<&PropertyValue>) -> Result<Option<Length>, DistanceError> {
    let Some(PropertyValue::Distance { unit, value }) = property else {
        return Ok(None);
    };
    let factor = match *unit {
        0x2016 => 25.4,
        0x200e => 1.0,
        0x200d => 10.0,
        unit => return Err(DistanceError::UnknownUnit(unit)),
    };
    Length::new(value.get() * factor)
        .map(Some)
        .ok_or(DistanceError::NonFinite)
}

#[cfg(test)]
mod tests;
