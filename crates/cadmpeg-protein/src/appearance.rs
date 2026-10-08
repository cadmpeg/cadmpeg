// SPDX-License-Identifier: Apache-2.0
//! Neutral projection of Protein texture assets and material property names.

use cadmpeg_core::decode::DecodeContext;
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
    if !matches!(
        record.schema.as_str(),
        "UnifiedBitmapSchema" | "BumpMapSchema"
    ) {
        return Ok(TextureAssetResult::NotTexture);
    }
    let mut distances = [Length::ZERO; 5];
    let mut unknown_count = 0_usize;
    for (index, suffix) in [
        "RealWorldOffsetX",
        "RealWorldOffsetY",
        "RealWorldScaleX",
        "RealWorldScaleY",
        "bumpmap_Depth",
    ]
    .into_iter()
    .enumerate()
    {
        if index == 4 && record.schema != "BumpMapSchema" {
            break;
        }
        match distance_property(ctx, record, suffix)? {
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
        return Ok(TextureAssetResult::UnknownDistanceUnit {
            count: unknown_count,
        });
    }
    // The suffix tests compare fixed literals; each search pays for the
    // properties it visits.
    let source_paths = ctx.find_map(
        &record.properties,
        |(id, property)| {
            Ok(match property.value() {
                Some(crate::property::PropertyValue::TextureUri(paths))
                    if id.ends_with("_Bitmap") =>
                {
                    Some(paths)
                }
                _ => None,
            })
        },
        "Protein texture bitmap search",
    )?;
    let paths = match source_paths {
        Some(paths) => ctx.try_collect_vec(
            paths
                .iter()
                .map(|path| ctx.copy_retained_text(path, "Protein texture path")),
            "Protein texture paths",
        )?,
        None => Vec::new(),
    };
    let urn = ctx
        .find_map(
            &record.properties,
            |(id, property)| {
                Ok(match property.value() {
                    Some(crate::property::PropertyValue::String(value))
                        if !value.is_empty() && id.ends_with("_Bitmap_urn") =>
                    {
                        Some(value)
                    }
                    _ => None,
                })
            },
            "Protein texture URN search",
        )?
        .map(|urn| ctx.copy_retained_text(urn, "Protein texture URN"))
        .transpose()?;
    let mapping = TextureMap2d {
        map_channel: integer_property(ctx, record, "MapChannel")?.unwrap_or(1),
        uvw_source: integer_property(ctx, record, "MapChannel_UVWSource_Advanced")?.unwrap_or(0),
        u_offset: finite_float_property(ctx, record, "UOffset", FiniteReal::ZERO)?,
        v_offset: finite_float_property(ctx, record, "VOffset", FiniteReal::ZERO)?,
        u_scale: finite_float_property(ctx, record, "UScale", FiniteReal::ONE)?,
        v_scale: finite_float_property(ctx, record, "VScale", FiniteReal::ONE)?,
        // A finite angle in degrees is finite in radians: the factor is below one.
        rotation: Angle::new(
            finite_float_property(ctx, record, "WAngle", FiniteReal::ZERO)?
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
        repeat_u: boolean_property(ctx, record, "URepeat")?.unwrap_or(true),
        repeat_v: boolean_property(ctx, record, "VRepeat")?.unwrap_or(true),
        real_world_offset_x: distances[0],
        real_world_offset_y: distances[1],
        real_world_scale_x: distances[2],
        real_world_scale_y: distances[3],
    };
    let bump = if record.schema == "BumpMapSchema" {
        Some(BumpMap {
            normal_map: integer_property(ctx, record, "bumpmap_Type")? == Some(1),
            depth: distances[4],
            normal_scale: finite_float_property(
                ctx,
                record,
                "bumpmap_NormalScale",
                FiniteReal::ONE,
            )?,
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

/// The value of the first property named `suffix` or `<prefix>_<suffix>`.
fn property_with_suffix<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a crate::DecodedRecord,
    suffix: &str,
) -> Result<Option<&'a crate::property::PropertyValue>, CodecError> {
    Ok(ctx
        .find_map(
            &record.properties,
            |(id, property)| {
                let named = ctx
                    .strip_suffix(id.as_str(), suffix, "Protein property suffix comparison")?
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('_'));
                Ok(named.then(|| property.value()))
            },
            "Protein property suffix search",
        )?
        .flatten())
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

fn integer_property(
    ctx: &DecodeContext<'_>,
    record: &crate::DecodedRecord,
    suffix: &str,
) -> Result<Option<u32>, CodecError> {
    Ok(match property_with_suffix(ctx, record, suffix)? {
        Some(crate::property::PropertyValue::Integer(value)) => Some(*value),
        _ => None,
    })
}

/// The admitted finite float stated under `suffix`, or `default` when absent.
fn finite_float_property(
    ctx: &DecodeContext<'_>,
    record: &crate::DecodedRecord,
    suffix: &str,
    default: FiniteReal,
) -> Result<FiniteReal, CodecError> {
    Ok(match property_with_suffix(ctx, record, suffix)? {
        Some(crate::property::PropertyValue::Float(value)) => *value,
        _ => default,
    })
}

fn boolean_property(
    ctx: &DecodeContext<'_>,
    record: &crate::DecodedRecord,
    suffix: &str,
) -> Result<Option<bool>, CodecError> {
    Ok(match property_with_suffix(ctx, record, suffix)? {
        Some(crate::property::PropertyValue::Boolean(value)) => Some(*value),
        _ => None,
    })
}

#[derive(Debug, PartialEq)]
enum DistanceError {
    UnknownUnit(u32),
    NonFinite,
}

fn distance_property(
    ctx: &DecodeContext<'_>,
    record: &crate::DecodedRecord,
    suffix: &str,
) -> Result<Result<Option<Length>, DistanceError>, CodecError> {
    let Some(crate::property::PropertyValue::Distance { unit, value }) =
        property_with_suffix(ctx, record, suffix)?
    else {
        return Ok(Ok(None));
    };
    let factor = match *unit {
        0x2016 => 25.4,
        0x200e => 1.0,
        0x200d => 10.0,
        unit => return Ok(Err(DistanceError::UnknownUnit(unit))),
    };
    Ok(Length::new(value.get() * factor)
        .map(Some)
        .ok_or(DistanceError::NonFinite))
}

#[cfg(test)]
mod tests;
