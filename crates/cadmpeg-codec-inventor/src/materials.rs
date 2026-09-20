// SPDX-License-Identifier: Apache-2.0
//! Protein appearance catalog projection without inferred topology bindings.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;
use cadmpeg_protein::appearance::{is_physical_schema, neutral_property_name, texture_asset};

use crate::protein::ProteinInstanceRecords;

const NO_ASSET_LIB_ID: &str = "00000000-0000-0000-0000-000000000000";

pub(crate) struct MaterialCatalog {
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) duplicate_guids: Vec<String>,
}

pub(crate) fn project_catalog(
    instances: &[ProteinInstanceRecords],
) -> Result<MaterialCatalog, cadmpeg_core::CodecError> {
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
        .filter_map(|record| texture_asset(record).0)
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
                if let Some(cadmpeg_protein::property::PropertyValue::Float(value)) =
                    property.value()
                {
                    properties.insert(neutral_property_name(id).to_owned(), *value);
                }
                for guid in property.connections() {
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
                id: AppearanceId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "protein", "appearance"),
                    cadmpeg_ir::ids::IdentityKey::from(instance_ordinal).dash(record.ordinal),
                ),
                name: Some(record.base.clone()),
                asset_guid: Some(record.guid.clone()),
                library_id: library_id(&record.asset_lib_id),
                visual_guid: (!is_physical_schema(&record.schema)).then(|| record.guid.clone()),
                physical_token: None,
                schema: Some(record.schema.clone()),
                category: None,
                base_color,
                properties: cadmpeg_core::text::named_entries(
                    format_args!(
                        "inventor:protein:appearance#{instance_ordinal}-{}",
                        record.ordinal
                    ),
                    properties,
                )?,
                textures: connected,
            });
        }
    }
    Ok(MaterialCatalog {
        appearances,
        duplicate_guids,
    })
}

fn library_id(value: &str) -> Option<String> {
    (!value.is_empty() && value != NO_ASSET_LIB_ID).then(|| value.to_owned())
}

fn color_property(record: &cadmpeg_protein::DecodedRecord, id: &str) -> Option<Color> {
    let cadmpeg_protein::property::PropertyValue::Color([r, g, b, a]) =
        record
            .properties
            .get(id)
            .and_then(|property| property.value())?
    else {
        return None;
    };
    let values = [*r, *g, *b, *a];
    values
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .then(|| {
            Color::new(
                values[0] as f32,
                values[1] as f32,
                values[2] as f32,
                values[3] as f32,
            )
        })
        .flatten()
}

#[cfg(test)]
mod tests {
    use cadmpeg_protein::property::{DecodedProperty, PropertyValue};
    use cadmpeg_protein::DecodedRecord;

    use super::project_catalog;
    use crate::protein::ProteinInstanceRecords;
    use std::collections::BTreeMap;

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
                    content: cadmpeg_protein::property::PropertyContent::Value {
                        value: PropertyValue::Color([0.0, 0.25, 1.0, 1.0]),
                        connections: vec!["duplicate-texture".into()],
                    },
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
        let catalog = project_catalog(&instances).expect("the fixture states named properties");

        assert_eq!(catalog.appearances.len(), 1);
        assert_eq!(
            catalog.appearances[0]
                .base_color
                .expect("valid catalog color")
                .b(),
            1.0
        );
        assert!(catalog.appearances[0].textures.is_empty());
        assert_eq!(catalog.duplicate_guids, ["duplicate-texture"]);
    }
}
