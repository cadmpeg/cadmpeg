// SPDX-License-Identifier: Apache-2.0
//! Protein appearance catalog projection without inferred topology bindings.

use std::collections::BTreeMap;

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
    let mut records_storage = ctx.reserve_scoped(0, "Inventor material record references")?;
    let mut instances_iter = ctx.admit_iter(instances, "Inventor material instances")?;
    let mut instance_records: Option<
        cadmpeg_core::decode::scan::AdmittedIter<
            std::slice::Iter<'_, cadmpeg_protein::DecodedRecord>,
        >,
    > = None;
    let records = std::iter::from_fn(move || {
        (|| -> Result<Option<&cadmpeg_protein::DecodedRecord>, CodecError> {
            loop {
                ctx.charge_work(1, "flatten Inventor material records")?;
                if let Some(records) = instance_records.as_mut() {
                    if let Some(record) = records.next() {
                        return Ok(Some(record));
                    }
                }
                instance_records = None;
                let Some(instance) = instances_iter.next() else {
                    return Ok(None);
                };
                instance_records =
                    Some(ctx.admit_iter(&instance.records, "Inventor material instance records")?);
            }
        })()
        .transpose()
    });
    let records = records_storage
        .with_storage(|| ctx.try_collect_vec(records, "Inventor material record references"))?;
    let mut guid_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut guid_counts_storage = ctx.reserve_scoped(0, "Inventor material GUID counts")?;
    for record in ctx.admit_iter(&records, "Inventor material GUID count records")? {
        let guid = record.guid.as_str();
        guid_counts_storage.with_storage(|| {
            if let Some(count) =
                ctx.get_mut_btree_map(&mut guid_counts, guid, "Inventor material GUID counts")?
            {
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| CodecError::Malformed("Protein GUID count overflows".into()))?;
            } else {
                ctx.insert_btree_map(
                    &mut guid_counts,
                    guid,
                    1_usize,
                    "Inventor material GUID counts",
                )?;
            }
            Ok::<_, CodecError>(())
        })?;
    }
    let mut duplicate_guids = Vec::new();
    for (guid, count) in ctx.admit_iter(&guid_counts, "Inventor duplicate material GUID scan")? {
        if *count > 1 {
            ctx.push_vec(
                &mut duplicate_guids,
                ctx.copy_retained_text(guid, "Inventor duplicate material GUID")?,
                "Inventor duplicate material GUIDs",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut duplicate_guids,
        |value| value,
        Ord::cmp,
        "Inventor duplicate material GUID sort",
    )?;

    let mut textures_storage = ctx.reserve_scoped(0, "Inventor material texture catalog")?;
    let mut textures = BTreeMap::new();
    let mut untyped_distance_properties = 0_usize;
    for record in ctx.admit_iter(&records, "Inventor material texture records")? {
        if ctx.get_btree_map(
            &guid_counts,
            record.guid.as_str(),
            "Inventor unique material GUID lookup",
        )? != Some(&1)
        {
            continue;
        }
        let texture = match textures_storage.with_storage(|| texture_asset(ctx, record))? {
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
        textures_storage.with_storage(|| {
            let key =
                ctx.copy_retained_text(&texture.asset_guid, "Inventor material texture key")?;
            ctx.insert_btree_map(
                &mut textures,
                key,
                texture,
                "Inventor material texture catalog",
            )?;
            Ok::<_, CodecError>(())
        })?;
    }
    let mut appearances = Vec::new();
    for (instance_ordinal, instance) in ctx
        .admit_iter(instances, "Inventor appearance instances")?
        .enumerate()
    {
        for record in ctx.admit_iter(&instance.records, "Inventor appearance records")? {
            if matches!(
                record.schema.as_str(),
                "UnifiedBitmapSchema" | "BumpMapSchema"
            ) {
                continue;
            }
            let mut property_values = BTreeMap::new();
            let mut properties_storage = ctx.reserve_scoped(0, "Inventor appearance properties")?;
            let mut connected = Vec::new();
            for (id, property) in
                ctx.admit_iter(&record.properties, "Inventor appearance properties")?
            {
                if let Some(cadmpeg_protein::property::PropertyValue::Float(value)) =
                    property.value()
                {
                    properties_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut property_values,
                            neutral_property_name(id),
                            *value,
                            "Inventor appearance properties",
                        )
                    })?;
                }
                for guid in ctx.admit_iter(
                    property.connections(),
                    "Inventor appearance texture connections",
                )? {
                    if let Some(texture) =
                        ctx.get_btree_map(&textures, guid, "Inventor connected texture lookup")?
                    {
                        ctx.push_vec(
                            &mut connected,
                            texture.to_ref(ctx, id)?,
                            "Inventor appearance connected textures",
                        )?;
                    }
                }
            }
            ctx.stable_sort_by(
                &mut connected,
                |value| &value.asset_guid,
                Ord::cmp,
                "Inventor appearance texture sort",
            )?;
            ctx.stable_sort_by(
                &mut connected,
                |value| &value.slot,
                Ord::cmp,
                "Inventor appearance texture sort",
            )?;
            let mut base_color = None;
            for id in [
                "generic_diffuse",
                "opaque_albedo",
                "surface_albedo",
                "common_Tint_color",
            ] {
                base_color = color_property(ctx, record, id)?;
                if base_color.is_some() {
                    break;
                }
            }
            let next_entity = admitted_entities
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("Inventor entity count overflows".into()))?;
            ctx.admit_entities(
                next_entity,
                admitted_entities,
                "Inventor neutral appearance",
            )?;
            let name = ctx.copy_retained_text(&record.base, "Inventor appearance field")?;
            let asset_guid = ctx.copy_retained_text(&record.guid, "Inventor appearance field")?;
            let schema = ctx.copy_retained_text(&record.schema, "Inventor appearance field")?;
            let visual_guid = if is_physical_schema(&record.schema) {
                None
            } else {
                Some(ctx.copy_retained_text(&record.guid, "Inventor visual GUID")?)
            };
            let library_id = library_id(ctx, &record.asset_lib_id)?;
            let id = appearance_id(ctx, instance_ordinal, record.ordinal)?;
            let named_properties = {
                let (mut named_properties, mut named_properties_reservation) =
                    ctx.temporary_vec(0, "Inventor appearance named property entries")?;
                for (name, value) in ctx.admit_iter(
                    &property_values,
                    "Inventor appearance named property source",
                )? {
                    let name = ctx.copy_retained_text(name, "Inventor appearance property name")?;
                    ctx.push_scoped_vec(
                        &mut named_properties_reservation,
                        &mut named_properties,
                        (name, *value),
                        "Inventor appearance named property entries",
                    )?;
                }
                cadmpeg_core::text::named_entries_for_decode(
                    ctx,
                    format_args!(
                        "inventor:protein:appearance#{instance_ordinal}-{}",
                        record.ordinal
                    ),
                    named_properties,
                )?
            };
            drop(property_values);
            drop(properties_storage);
            ctx.push_vec(
                &mut appearances,
                Appearance {
                    id,
                    name: Some(name),
                    asset_guid: Some(asset_guid),
                    library_id,
                    visual_guid,
                    physical_token: None,
                    schema: Some(schema),
                    category: None,
                    base_color,
                    properties: named_properties,
                    textures: connected,
                },
                "Inventor neutral appearances",
            )?;
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
    let (instance_key, instance_key_storage) = ctx.format_scoped(
        format_args!("{instance_ordinal}"),
        "retain Inventor appearance instance key",
    )?;
    let (record_key, record_key_storage) = ctx.format_scoped(
        format_args!("{record_ordinal}"),
        "retain Inventor appearance record key",
    )?;
    let key_len = instance_key
        .len()
        .checked_add(record_key.len())
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("Inventor appearance key length", u64::MAX - 1, u64::MAX)
        })?;
    let key_len = cadmpeg_core::decode::u64_from_index(key_len);
    let (key_text, key_storage) = ctx.format_scoped(
        format_args!("{instance_key}-{record_key}"),
        "retain Inventor appearance key",
    )?;
    let key_work = key_len.checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit("validate Inventor appearance key", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_work(key_work, "validate Inventor appearance key")?;
    let key = cadmpeg_ir::ids::IdentityKey::try_new(key_text)
        .map_err(|_| CodecError::malformed("Inventor appearance key is invalid"))?;
    drop(instance_key);
    drop(instance_key_storage);
    drop(record_key);
    drop(record_key_storage);
    let namespace = cadmpeg_ir::identity_namespace!("inventor", "protein", "appearance");
    let namespace_len = namespace
        .format()
        .len()
        .checked_add(namespace.scope().len())
        .and_then(|len| len.checked_add(namespace.kind().len()))
        .and_then(|len| len.checked_add(2))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "Inventor appearance namespace length",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    let id_len = namespace_len
        .checked_add(key.as_str().len())
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("Inventor appearance id length", u64::MAX - 1, u64::MAX)
        })?;
    let id_text = ctx.format_retained(
        format_args!(
            "{}:{}:{}#{}",
            namespace.format(),
            namespace.scope(),
            namespace.kind(),
            key
        ),
        "retain Inventor appearance id",
    )?;
    let namespace_scan = cadmpeg_core::decode::u64_from_index(namespace_len);
    let key_scan = cadmpeg_core::decode::u64_from_index(key.as_str().len());
    let id_scan = cadmpeg_core::decode::u64_from_index(id_len);
    let identity_work = namespace_scan
        .checked_add(1)
        .and_then(|work| work.checked_add(key_scan))
        .and_then(|work| work.checked_add(id_scan))
        .and_then(|work| work.checked_add(namespace_scan))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("validate Inventor appearance id", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(identity_work, "validate Inventor appearance id")?;
    let appearance_id = AppearanceId::mint(id_text)
        .map_err(|_| CodecError::malformed("Inventor appearance id is invalid"))?;
    drop(key);
    drop(key_storage);
    Ok(appearance_id)
}

fn library_id(ctx: &DecodeContext<'_>, value: &str) -> Result<Option<String>, CodecError> {
    if !value.is_empty() && value != NO_ASSET_LIB_ID {
        Ok(Some(ctx.copy_retained_text(
            value,
            "Inventor appearance library ID",
        )?))
    } else {
        Ok(None)
    }
}

fn color_property(
    ctx: &DecodeContext<'_>,
    record: &cadmpeg_protein::DecodedRecord,
    id: &str,
) -> Result<Option<Color>, CodecError> {
    let Some(cadmpeg_protein::property::PropertyValue::Color([r, g, b, a])) = ctx
        .get_btree_map(
            &record.properties,
            id,
            "Inventor appearance color property lookup",
        )?
        .and_then(|property| property.value())
    else {
        return Ok(None);
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
        return Ok(None);
    };
    Ok(Some(Color::from_unit_binary32([r, g, b, a])))
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
    fn appearance_identity_refuses_scoped_keys_and_retained_id() {
        let arena = DecodeArena::new();
        let bytes = b"fixture";
        let (service, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("service context");
        let id = appearance_id(&service, 0, 1).expect("identity admitted");
        assert_eq!(id.as_str(), "inventor:protein:appearance#0-1");
        // Temporary key strings use scoped bytes; only the composed ID remains retained.
        for (cap, operation) in [
            (0, "retain Inventor appearance instance key"),
            (1, "retain Inventor appearance record key"),
            (3, "retain Inventor appearance key"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (limited, _) =
                DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("limited context");
            assert!(matches!(
                appearance_id(&limited, 0, 1),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::MaterializedBytes
                        && limit.operation == operation
            ));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index(id.as_str().len()) - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            appearance_id(&limited, 0, 1),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor appearance id"
        ));
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
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"fixture", &arena, &DecodePolicy::service())
            .expect("service context");
        assert_eq!(
            super::color_property(&ctx, &record, "generic_diffuse").expect("color lookup admitted"),
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
            assert!(super::color_property(&ctx, &record, "generic_diffuse")
                .expect("color lookup admitted")
                .is_none());
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
