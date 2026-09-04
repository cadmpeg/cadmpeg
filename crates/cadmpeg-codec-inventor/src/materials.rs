// SPDX-License-Identifier: Apache-2.0
//! Protein appearance catalog projection without inferred topology bindings.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::appearance::{Appearance, BumpMap, TextureMap2d, TextureRef};
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;

use crate::protein::ProteinInstanceRecords;

const NO_ASSET_LIB_ID: &str = "00000000-0000-0000-0000-000000000000";

pub(crate) struct MaterialCatalog {
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) duplicate_guids: Vec<String>,
}

pub(crate) fn project_catalog(instances: &[ProteinInstanceRecords]) -> MaterialCatalog {
    let records = instances
        .iter()
        .flat_map(|instance| instance.records.iter())
        .collect::<Vec<_>>();
    let mut guid_counts = HashMap::new();
    for record in &records {
        *guid_counts.entry(record.guid.as_str()).or_insert(0_usize) += 1;
    }
    let mut duplicate_guids = guid_counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(guid, _)| (*guid).to_owned())
        .collect::<Vec<_>>();
    duplicate_guids.sort();

    let textures = records
        .iter()
        .filter(|record| guid_counts.get(record.guid.as_str()) == Some(&1))
        .filter_map(|record| texture_asset(record))
        .map(|texture| (texture.asset_guid.clone(), texture))
        .collect::<BTreeMap<_, _>>();
    let mut appearances = Vec::new();
    for (instance_ordinal, instance) in instances.iter().enumerate() {
        for record in &instance.records {
            if matches!(
                record.schema.as_str(),
                "UnifiedBitmapSchema" | "BumpMapSchema"
            ) {
                continue;
            }
            let mut properties = BTreeMap::new();
            let mut connected = Vec::new();
            for (id, property) in &record.properties {
                if let cadmpeg_protein::PropertyValue::Float(value) = property.value {
                    properties.insert(neutral_property_name(id).to_owned(), value);
                }
                for guid in &property.connections {
                    if let Some(texture) = textures.get(guid) {
                        connected.push(texture.clone().into_ref(id.clone()));
                    }
                }
            }
            connected.sort_by(|left, right| {
                left.slot
                    .cmp(&right.slot)
                    .then_with(|| left.asset_guid.cmp(&right.asset_guid))
            });
            let base_color = [
                "generic_diffuse",
                "opaque_albedo",
                "surface_albedo",
                "common_Tint_color",
            ]
            .into_iter()
            .find_map(|id| color_property(record, id));
            appearances.push(Appearance {
                id: AppearanceId::mint(format!(
                    "inventor:protein:appearance#{instance_ordinal}-{}",
                    record.ordinal
                ))
                .expect("identity grammar"),
                name: Some(record.base.clone()),
                asset_guid: Some(record.guid.clone()),
                library_id: library_id(&record.asset_lib_id),
                visual_guid: (!is_physical_schema(&record.schema)).then(|| record.guid.clone()),
                physical_token: None,
                schema: Some(record.schema.clone()),
                category: None,
                base_color,
                properties,
                textures: connected,
            });
        }
    }
    MaterialCatalog {
        appearances,
        duplicate_guids,
    }
}

fn library_id(value: &str) -> Option<String> {
    (!value.is_empty() && value != NO_ASSET_LIB_ID).then(|| value.to_owned())
}

fn color_property(record: &cadmpeg_protein::DecodedRecord, id: &str) -> Option<Color> {
    let cadmpeg_protein::PropertyValue::Color([r, g, b, a]) =
        record.properties.get(id).map(|property| &property.value)?
    else {
        return None;
    };
    let values = [*r, *g, *b, *a];
    values
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .then_some(Color {
            r: values[0] as f32,
            g: values[1] as f32,
            b: values[2] as f32,
            a: values[3] as f32,
        })
}

#[derive(Clone, PartialEq)]
struct TextureAsset {
    asset_guid: String,
    schema: String,
    paths: Vec<String>,
    urn: Option<String>,
    mapping: TextureMap2d,
    bump: Option<BumpMap>,
}

impl TextureAsset {
    fn into_ref(self, slot: String) -> TextureRef {
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

fn texture_asset(record: &cadmpeg_protein::DecodedRecord) -> Option<TextureAsset> {
    if !matches!(
        record.schema.as_str(),
        "UnifiedBitmapSchema" | "BumpMapSchema"
    ) {
        return None;
    }
    let paths = record
        .properties
        .iter()
        .find_map(|(id, property)| {
            id.ends_with("_Bitmap")
                .then_some(&property.value)
                .and_then(|value| match value {
                    cadmpeg_protein::PropertyValue::TextureUri(paths) => Some(paths.clone()),
                    _ => None,
                })
        })
        .unwrap_or_default();
    let urn = record.properties.iter().find_map(|(id, property)| {
        id.ends_with("_Bitmap_urn")
            .then_some(&property.value)
            .and_then(|value| match value {
                cadmpeg_protein::PropertyValue::String(value) if !value.is_empty() => {
                    Some(value.clone())
                }
                _ => None,
            })
    });
    Some(TextureAsset {
        asset_guid: record.guid.clone(),
        schema: record.schema.clone(),
        paths,
        urn,
        mapping: TextureMap2d {
            map_channel: integer_property(record, "MapChannel").unwrap_or(1),
            uvw_source: integer_property(record, "MapChannel_UVWSource_Advanced").unwrap_or(0),
            u_offset: float_property(record, "UOffset").unwrap_or(0.0),
            v_offset: float_property(record, "VOffset").unwrap_or(0.0),
            u_scale: float_property(record, "UScale").unwrap_or(1.0),
            v_scale: float_property(record, "VScale").unwrap_or(1.0),
            rotation: float_property(record, "WAngle").unwrap_or(0.0).to_radians(),
            repeat_u: boolean_property(record, "URepeat").unwrap_or(true),
            repeat_v: boolean_property(record, "VRepeat").unwrap_or(true),
            real_world_offset_x: distance_property(record, "RealWorldOffsetX").unwrap_or(0.0),
            real_world_offset_y: distance_property(record, "RealWorldOffsetY").unwrap_or(0.0),
            real_world_scale_x: distance_property(record, "RealWorldScaleX").unwrap_or(0.0),
            real_world_scale_y: distance_property(record, "RealWorldScaleY").unwrap_or(0.0),
        },
        bump: (record.schema == "BumpMapSchema").then(|| BumpMap {
            normal_map: integer_property(record, "bumpmap_Type") == Some(1),
            depth: distance_property(record, "bumpmap_Depth").unwrap_or(0.0),
            normal_scale: float_property(record, "bumpmap_NormalScale").unwrap_or(1.0),
        }),
    })
}

fn property_with_suffix<'a>(
    record: &'a cadmpeg_protein::DecodedRecord,
    suffix: &str,
) -> Option<&'a cadmpeg_protein::PropertyValue> {
    let qualified_suffix = format!("_{suffix}");
    record
        .properties
        .iter()
        .find(|(id, _)| *id == suffix || id.ends_with(&qualified_suffix))
        .map(|(_, property)| &property.value)
}

fn integer_property(record: &cadmpeg_protein::DecodedRecord, suffix: &str) -> Option<u32> {
    match property_with_suffix(record, suffix)? {
        cadmpeg_protein::PropertyValue::Integer(value) => Some(*value),
        _ => None,
    }
}

fn float_property(record: &cadmpeg_protein::DecodedRecord, suffix: &str) -> Option<f64> {
    match property_with_suffix(record, suffix)? {
        cadmpeg_protein::PropertyValue::Float(value) => Some(*value),
        _ => None,
    }
}

fn boolean_property(record: &cadmpeg_protein::DecodedRecord, suffix: &str) -> Option<bool> {
    match property_with_suffix(record, suffix)? {
        cadmpeg_protein::PropertyValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

fn distance_property(record: &cadmpeg_protein::DecodedRecord, suffix: &str) -> Option<f64> {
    let cadmpeg_protein::PropertyValue::Distance { unit, value } =
        property_with_suffix(record, suffix)?
    else {
        return None;
    };
    match *unit {
        0x2016 => Some(*value * 25.4),
        0x200e => Some(*value),
        0x200d => Some(*value * 10.0),
        _ => None,
    }
}

fn neutral_property_name(id: &str) -> &str {
    match id {
        "generic_reflectivity_at_0deg" => "reflectivity_at_0deg",
        "generic_refraction_index" | "transparent_refraction_index" => "refraction_index",
        _ => id,
    }
}

fn is_physical_schema(schema: &str) -> bool {
    schema == "PhysMatSchema" || schema.starts_with("Structural") || schema.starts_with("Thermal")
}

#[cfg(test)]
mod tests {
    use cadmpeg_protein::{DecodedProperty, DecodedRecord, PropertyValue};

    use super::*;

    #[test]
    fn catalog_projects_assets_and_refuses_ambiguous_texture_guids() {
        let color = DecodedRecord {
            ordinal: 0,
            logical_offset: 0,
            schema: "GenericSchema".into(),
            guid: "appearance".into(),
            base: "Blue".into(),
            asset_lib_id: String::new(),
            properties: BTreeMap::from([(
                "generic_diffuse".into(),
                DecodedProperty {
                    value_offset: 0,
                    value: PropertyValue::Color([0.0, 0.25, 1.0, 1.0]),
                    connections: vec!["duplicate-texture".into()],
                },
            )]),
        };
        let texture = || DecodedRecord {
            ordinal: 1,
            logical_offset: 0,
            schema: "UnifiedBitmapSchema".into(),
            guid: "duplicate-texture".into(),
            base: "Texture".into(),
            asset_lib_id: String::new(),
            properties: BTreeMap::new(),
        };
        let instances = [ProteinInstanceRecords {
            entry_name: "AssetData/InstanceProperties.bin".into(),
            records: vec![color, texture(), texture()],
            rejected: Vec::new(),
        }];
        let catalog = project_catalog(&instances);

        assert_eq!(catalog.appearances.len(), 1);
        assert_eq!(
            catalog.appearances[0]
                .base_color
                .expect("valid catalog color")
                .b,
            1.0
        );
        assert!(catalog.appearances[0].textures.is_empty());
        assert_eq!(catalog.duplicate_guids, ["duplicate-texture"]);
    }
}
