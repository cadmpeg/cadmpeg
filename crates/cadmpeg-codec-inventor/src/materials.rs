// SPDX-License-Identifier: Apache-2.0
//! Protein appearance catalog projection without inferred topology bindings.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;
use cadmpeg_protein::appearance::{
    is_physical_schema, neutral_property_name, texture_asset, TextureAssetResult,
};

use crate::protein::ProteinInstanceRecords;

const NO_ASSET_LIB_ID: &str = "00000000-0000-0000-0000-000000000000";

pub(crate) struct MaterialCatalog {
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) duplicate_guids: Vec<String>,
    pub(crate) untyped_distance_properties: usize,
}

pub(crate) fn project_catalog(
    ctx: &DecodeContext<'_>,
    instances: &[ProteinInstanceRecords],
    admitted_entities: &mut u64,
) -> Result<MaterialCatalog, CodecError> {
    let record_count = instances
        .iter()
        .try_fold(0_usize, |total, instance| {
            total.checked_add(instance.records.len())
        })
        .ok_or_else(|| CodecError::Malformed("Protein record count overflows".into()))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(record_count),
        "Inventor material record references",
    )?;
    let records = instances
        .iter()
        .flat_map(|instance| instance.records.iter())
        .collect::<Vec<_>>();
    let mut guid_counts: HashMap<&str, usize> = HashMap::new();
    for record in &records {
        let guid = record.guid.as_str();
        ctx.admit_hash_map_entry(&mut guid_counts, &guid, "Inventor material GUID counts")?;
        match guid_counts.entry(guid) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = entry
                    .get()
                    .checked_add(1)
                    .ok_or_else(|| CodecError::Malformed("Protein GUID count overflows".into()))?;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(1_usize);
            }
        }
    }
    let mut duplicate_guids = Vec::new();
    for (guid, count) in &guid_counts {
        if *count > 1 {
            ctx.charge_collection_items(1, "Inventor duplicate material GUIDs")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(guid.len()),
                "Inventor duplicate material GUID",
            )?;
            duplicate_guids.push((*guid).to_owned());
        }
    }
    ctx.stable_sort_by(
        &mut duplicate_guids,
            |value| value,
            Ord::cmp,
        "Inventor duplicate material GUID sort",
    )?;

    let mut textures = BTreeMap::new();
    let mut untyped_distance_properties = 0_usize;
    for record in records
        .iter()
        .filter(|record| guid_counts.get(record.guid.as_str()) == Some(&1))
    {
        let texture = match texture_asset(ctx, record)? {
            TextureAssetResult::NotTexture => continue,
            TextureAssetResult::UnknownDistanceUnit { count } => {
                untyped_distance_properties = untyped_distance_properties
                    .checked_add(count)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "untyped material distance count overflows".into(),
                        )
                    })?;
                continue;
            }
            TextureAssetResult::Usable(texture) => texture,
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(texture.asset_guid.len()),
            "Inventor material texture key",
        )?;
        ctx.insert_btree_map(
            &mut textures,
            texture.asset_guid.clone(),
            texture,
            "Inventor material texture catalog",
        )?;
    }
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
                    ctx.charge_retained(
                        cadmpeg_core::decode::u64_from_index(neutral_property_name(id).len()),
                        "Inventor appearance property name",
                    )?;
                    ctx.insert_btree_map(
                        &mut properties,
                        neutral_property_name(id).to_owned(),
                        *value,
                        "Inventor appearance properties",
                    )?;
                }
                for guid in property.connections() {
                    if let Some(texture) = textures.get(guid) {
                        connected.push(texture.to_ref(ctx, id)?);
                    }
                }
            }
            ctx.stable_sort_by(&mut connected, |value| &value.asset_guid, Ord::cmp, "Inventor appearance texture sort")?;
        ctx.stable_sort_by(&mut connected, |value| &value.slot, Ord::cmp, "Inventor appearance texture sort")?;
            let base_color = [
                "generic_diffuse",
                "opaque_albedo",
                "surface_albedo",
                "common_Tint_color",
            ]
            .into_iter()
            .find_map(|id| color_property(record, id));
            ctx.charge_collection_items(1, "Inventor neutral appearances")?;
            let next_entity = admitted_entities
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("Inventor entity count overflows".into()))?;
            ctx.admit_entities(
                next_entity,
                admitted_entities,
                "Inventor neutral appearance",
            )?;
            for value in [&record.base, &record.guid, &record.schema] {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(value.len()),
                    "Inventor appearance field",
                )?;
            }
            if !is_physical_schema(&record.schema) {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(record.guid.len()),
                    "Inventor visual GUID",
                )?;
            }
            if !record.asset_lib_id.is_empty() && record.asset_lib_id != NO_ASSET_LIB_ID {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(record.asset_lib_id.len()),
                    "Inventor appearance library ID",
                )?;
            }
            appearances.push(Appearance {
                id: appearance_id(ctx, instance_ordinal, record.ordinal)?,
                name: Some(record.base.clone()),
                asset_guid: Some(record.guid.clone()),
                library_id: library_id(&record.asset_lib_id),
                visual_guid: (!is_physical_schema(&record.schema)).then(|| record.guid.clone()),
                physical_token: None,
                schema: Some(record.schema.clone()),
                category: None,
                base_color,
                properties: cadmpeg_core::text::named_entries_for_decode(
                    ctx,
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
        untyped_distance_properties,
    })
}

fn appearance_id(
    ctx: &DecodeContext<'_>,
    instance_ordinal: usize,
    record_ordinal: u64,
) -> Result<AppearanceId, CodecError> {
    ctx.charge_formatted_retained(
        format_args!("{instance_ordinal}"),
        "retain Inventor appearance instance key",
    )?;
    ctx.charge_formatted_retained(
        format_args!("{record_ordinal}"),
        "retain Inventor appearance record key",
    )?;
    let instance_key = cadmpeg_ir::ids::IdentityKey::from(instance_ordinal);
    let record_key = cadmpeg_ir::ids::IdentityKey::from(record_ordinal);
    let key_len = instance_key
        .as_str()
        .len()
        .checked_add(record_key.as_str().len())
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("Inventor appearance key length", u64::MAX - 1, u64::MAX)
        })?;
    let key_len = cadmpeg_core::decode::u64_from_index(key_len);
    ctx.charge_retained(key_len, "retain Inventor appearance key")?;
    let key = instance_key.dash(record_key);
    let namespace = cadmpeg_ir::identity_namespace!("inventor", "protein", "appearance");
    let id_len = namespace
        .format()
        .len()
        .checked_add(namespace.scope().len())
        .and_then(|len| len.checked_add(namespace.kind().len()))
        .and_then(|len| len.checked_add(key.as_str().len()))
        .and_then(|len| len.checked_add(4))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("Inventor appearance id length", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_len),
        "retain Inventor appearance id",
    )?;
    Ok(AppearanceId::compose(&namespace, key))
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
    let components = [r.get(), g.get(), b.get(), a.get()].map(|value| {
        if (0.0..=1.0).contains(&value) {
            cadmpeg_core::convert::f32_from_f64(value)
                .and_then(cadmpeg_ir::scalar::UnitBinary32::new)
        } else {
            None
        }
    });
    let [Some(r), Some(g), Some(b), Some(a)] = components else {
        return None;
    };
    Some(Color::from_unit_binary32([r, g, b, a]))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_protein::property::{DecodedProperty, PropertyValue};
    use cadmpeg_protein::DecodedRecord;

    use super::{appearance_id, project_catalog};
    use crate::protein::ProteinInstanceRecords;
    use std::collections::BTreeMap;

    #[test]
    fn appearance_identity_refuses_retained_limits_before_each_copy() {
        let arena = DecodeArena::new();
        let bytes = b"fixture";
        let (service, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("service context");
        let id = appearance_id(&service, 0, 1).expect("identity admitted");
        assert_eq!(id.as_str(), "inventor:protein:appearance#0-1");
        for (cap, operation) in [
            (0, "retain Inventor appearance instance key"),
            (1, "retain Inventor appearance record key"),
            (3, "retain Inventor appearance key"),
            (
                4 + cadmpeg_core::decode::u64_from_index(id.as_str().len()) - 1,
                "retain Inventor appearance id",
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (limited, _) =
                DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("limited context");
            assert!(matches!(
                appearance_id(&limited, 0, 1),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
            ));
        }
    }

    fn project_fixture(
        instances: &[ProteinInstanceRecords],
    ) -> Result<super::MaterialCatalog, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
        let mut admitted_entities = 0;
        project_catalog(&ctx, instances, &mut admitted_entities)
    }

    fn one_float_appearance() -> [ProteinInstanceRecords; 1] {
        [ProteinInstanceRecords {
            entry_name: "AssetData/InstanceProperties.bin".into(),
            records: vec![DecodedRecord {
                ordinal: 0,
                logical_offset: 0,
                schema: "GenericSchema".into(),
                guid: "appearance".into(),
                base: "Matte".into(),
                asset_lib_id: String::new(),
                properties: BTreeMap::from([(
                    "generic_roughness".into(),
                    DecodedProperty {
                        value_offset: 0,
                        content: cadmpeg_protein::property::PropertyContent::Value {
                            value: PropertyValue::Float(
                                cadmpeg_ir::scalar::FiniteReal::new(0.5).expect("finite"),
                            ),
                            connections: Vec::new(),
                        },
                    },
                )]),
            }],
            rejected: Vec::new(),
        }]
    }

    fn one_connected_texture() -> [ProteinInstanceRecords; 1] {
        let texture_guid = "texture";
        [ProteinInstanceRecords {
            entry_name: "AssetData/InstanceProperties.bin".into(),
            records: vec![
                DecodedRecord {
                    ordinal: 0,
                    logical_offset: 0,
                    schema: "GenericSchema".into(),
                    guid: "appearance".into(),
                    base: "Paint".into(),
                    asset_lib_id: String::new(),
                    properties: BTreeMap::from([(
                        "generic_diffuse".into(),
                        DecodedProperty {
                            value_offset: 0,
                            content: cadmpeg_protein::property::PropertyContent::Value {
                                value: PropertyValue::Color([0.0, 0.25, 1.0, 1.0].map(|value| {
                                    cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")
                                })),
                                connections: vec![texture_guid.into()],
                            },
                        },
                    )]),
                },
                DecodedRecord {
                    ordinal: 1,
                    logical_offset: 0,
                    schema: "UnifiedBitmapSchema".into(),
                    guid: texture_guid.into(),
                    base: "Bitmap".into(),
                    asset_lib_id: String::new(),
                    properties: BTreeMap::new(),
                },
            ],
            rejected: Vec::new(),
        }]
    }

    #[test]
    fn protein_color_admits_source_range_before_binary32_narrowing() {
        let mut record = one_connected_texture()[0].records[0].clone();
        assert_eq!(
            super::color_property(&record, "generic_diffuse"),
            cadmpeg_ir::topology::Color::new(0.0, 0.25, 1.0, 1.0)
        );
        for invalid in [1.0 + f64::EPSILON, -f64::EPSILON, f64::NAN, f64::INFINITY] {
            let Some(invalid_component) = cadmpeg_ir::scalar::FiniteReal::new(invalid) else {
                assert!(
                    !invalid.is_finite(),
                    "non-finite source is refused before a color can exist"
                );
                continue;
            };
            let property = record
                .properties
                .get_mut("generic_diffuse")
                .expect("color property");
            let cadmpeg_protein::property::PropertyContent::Value { value, .. } =
                &mut property.content
            else {
                panic!("color value property");
            };
            *value = PropertyValue::Color([
                invalid_component,
                cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("finite"),
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ONE,
            ]);
            assert!(super::color_property(&record, "generic_diffuse").is_none());
        }
    }

    #[test]
    fn inventor_texture_catalog_refuses_before_insertion() {
        let instances = [ProteinInstanceRecords {
            entry_name: "AssetData/InstanceProperties.bin".into(),
            records: vec![one_connected_texture()[0].records[1].clone()],
            rejected: Vec::new(),
        }];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            project_catalog(&ctx, &instances, &mut 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Inventor material texture catalog"
        ));
    }

    #[test]
    fn inventor_connection_refuses_before_texture_copy() {
        let instances = one_connected_texture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 5;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            project_catalog(&ctx, &instances, &mut 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein appearance texture"
        ));
        assert_eq!(
            project_fixture(&instances)
                .expect("connected appearance fits service profile")
                .appearances[0]
                .textures
                .len(),
            1
        );
    }

    #[test]
    fn inventor_property_map_refuses_before_insertion() {
        let instances = one_float_appearance();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            project_catalog(&ctx, &instances, &mut 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Inventor appearance properties"
        ));
        assert_eq!(
            project_fixture(&instances)
                .expect("appearance fits the service profile")
                .appearances
                .len(),
            1
        );
    }

    #[test]
    fn inventor_appearance_entity_refuses_before_creation() {
        let instances = one_float_appearance();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            project_catalog(&ctx, &instances, &mut 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "Inventor neutral appearance"
        ));
        assert_eq!(
            project_fixture(&instances)
                .expect("appearance fits the service profile")
                .appearances
                .len(),
            1
        );
    }

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
                        value: PropertyValue::Color([0.0, 0.25, 1.0, 1.0].map(|value| {
                            cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")
                        })),
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
        let catalog = project_fixture(&instances).expect("the fixture states named properties");

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

    #[test]
    fn unknown_texture_unit_omits_connected_texture_and_counts_loss() {
        let texture_guid = "unknown-unit-texture";
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
                        value: PropertyValue::Color([0.0, 0.25, 1.0, 1.0].map(|value| {
                            cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")
                        })),
                        connections: vec![texture_guid.into()],
                    },
                },
            )]),
        };
        let texture = DecodedRecord {
            ordinal: 1,
            logical_offset: 0,
            schema: "UnifiedBitmapSchema".into(),
            guid: texture_guid.into(),
            base: "Texture".into(),
            asset_lib_id: String::new(),
            properties: BTreeMap::from([(
                "texture_RealWorldScaleX".into(),
                DecodedProperty {
                    value_offset: 0,
                    content: cadmpeg_protein::property::PropertyContent::Value {
                        value: PropertyValue::Distance {
                            unit: 0x0002_1008,
                            value: cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite"),
                        },
                        connections: Vec::new(),
                    },
                },
            )]),
        };
        let instances = [ProteinInstanceRecords {
            entry_name: "AssetData/InstanceProperties.bin".into(),
            records: vec![color, texture],
            rejected: Vec::new(),
        }];
        let catalog = project_fixture(&instances).expect("catalog projects the valid appearance");
        assert_eq!(catalog.untyped_distance_properties, 1);
        assert!(catalog.appearances[0].textures.is_empty());
    }
}
