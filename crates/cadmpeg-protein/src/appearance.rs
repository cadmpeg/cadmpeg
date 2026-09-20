// SPDX-License-Identifier: Apache-2.0
//! Neutral projection of Protein texture assets and material property names.

use cadmpeg_ir::appearance::{BumpMap, TextureMap2d, TextureRef};

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
pub fn texture_asset(record: &crate::DecodedRecord) -> (Option<TextureAsset>, usize) {
    if !matches!(
        record.schema.as_str(),
        "UnifiedBitmapSchema" | "BumpMapSchema"
    ) {
        return (None, 0);
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
    let mut distance = |suffix: &str, default| match distance_property(record, suffix) {
        Ok(Some(value)) => value,
        Ok(None) => default,
        Err(_) => {
            untyped_distance_properties += 1;
            default
        }
    };
    let mapping = TextureMap2d {
        map_channel: integer_property(record, "MapChannel").unwrap_or(1),
        uvw_source: integer_property(record, "MapChannel_UVWSource_Advanced").unwrap_or(0),
        u_offset: float_property(record, "UOffset").unwrap_or(0.0),
        v_offset: float_property(record, "VOffset").unwrap_or(0.0),
        u_scale: float_property(record, "UScale").unwrap_or(1.0),
        v_scale: float_property(record, "VScale").unwrap_or(1.0),
        rotation: float_property(record, "WAngle").unwrap_or(0.0).to_radians(),
        repeat_u: boolean_property(record, "URepeat").unwrap_or(true),
        repeat_v: boolean_property(record, "VRepeat").unwrap_or(true),
        real_world_offset_x: distance("RealWorldOffsetX", 0.0),
        real_world_offset_y: distance("RealWorldOffsetY", 0.0),
        real_world_scale_x: distance("RealWorldScaleX", 0.0),
        real_world_scale_y: distance("RealWorldScaleY", 0.0),
    };
    let bump = (record.schema == "BumpMapSchema").then(|| BumpMap {
        normal_map: integer_property(record, "bumpmap_Type") == Some(1),
        depth: distance("bumpmap_Depth", 0.0),
        normal_scale: float_property(record, "bumpmap_NormalScale").unwrap_or(1.0),
    });
    let texture = TextureAsset {
        asset_guid: record.guid.clone(),
        schema: record.schema.clone(),
        paths,
        urn,
        mapping,
        bump,
    };
    (Some(texture), untyped_distance_properties)
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

fn float_property(record: &crate::DecodedRecord, suffix: &str) -> Option<f64> {
    match property_with_suffix(record, suffix)? {
        crate::property::PropertyValue::Float(value) => Some(*value),
        _ => None,
    }
}

fn boolean_property(record: &crate::DecodedRecord, suffix: &str) -> Option<bool> {
    match property_with_suffix(record, suffix)? {
        crate::property::PropertyValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

fn distance_property(record: &crate::DecodedRecord, suffix: &str) -> Result<Option<f64>, u32> {
    let Some(crate::property::PropertyValue::Distance { unit, value }) =
        property_with_suffix(record, suffix)
    else {
        return Ok(None);
    };
    match *unit {
        0x2016 => Ok(Some(*value * 25.4)),
        0x200e => Ok(Some(*value)),
        0x200d => Ok(Some(*value * 10.0)),
        unit => Err(unit),
    }
}

#[cfg(test)]
mod tests;
