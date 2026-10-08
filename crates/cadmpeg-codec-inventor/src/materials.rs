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
    let mut guid_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut guid_counts_storage = ctx.reserve_scoped(0, "Inventor material GUID counts")?;
    let mut instance_steps = instances.iter();
    while let Some(instance) = ctx.next_charged(
        &mut instance_steps,
        "Inventor material GUID count instances",
    )? {
        let mut record_steps = instance.records.iter();
        while let Some(record) =
            ctx.next_charged(&mut record_steps, "Inventor material GUID count records")?
        {
            let guid = record.guid.as_str();
            guid_counts_storage.with_storage(|| {
                if let Some(count) =
                    ctx.get_mut_btree_map(&mut guid_counts, guid, "Inventor material GUID counts")?
                {
                    *count = count.checked_add(1).ok_or_else(|| {
                        CodecError::Malformed("Protein GUID count overflows".into())
                    })?;
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
    // The selected GUIDs retain the B-tree's ascending key order.
    let mut textures_storage = ctx.reserve_scoped(0, "Inventor material texture catalog")?;
    let mut textures = BTreeMap::new();
    let mut untyped_distance_properties = 0_usize;
    let mut instance_steps = instances.iter();
    while let Some(instance) =
        ctx.next_charged(&mut instance_steps, "Inventor material texture instances")?
    {
        let mut record_steps = instance.records.iter();
        while let Some(record) =
            ctx.next_charged(&mut record_steps, "Inventor material texture records")?
        {
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
            // The texture's GUID is its record's GUID; the catalog borrows it.
            textures_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut textures,
                    record.guid.as_str(),
                    texture,
                    "Inventor material texture catalog",
                )
            })?;
        }
    }
    drop((guid_counts, guid_counts_storage));
    let mut appearances = Vec::new();
    let mut instance_steps = instances.iter().enumerate();
    while let Some((instance_ordinal, instance)) =
        ctx.next_charged(&mut instance_steps, "Inventor appearance instances")?
    {
        let mut record_steps = instance.records.iter();
        while let Some(record) =
            ctx.next_charged(&mut record_steps, "Inventor appearance records")?
        {
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
                let mut connection_steps = property.connections().iter();
                while let Some(guid) = ctx.next_charged(
                    &mut connection_steps,
                    "Inventor appearance texture connections",
                )? {
                    if let Some(texture) = ctx.get_btree_map(
                        &textures,
                        guid.as_str(),
                        "Inventor connected texture lookup",
                    )? {
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
    // The literal prefix and two bounded decimal ordinals form a valid key.
    let instance_digits = usize::try_from(instance_ordinal.max(1).ilog10())
        .map_err(|_| CodecError::malformed("Inventor numeric value exceeds target range"))?
        + 1;
    let record_digits = usize::try_from(record_ordinal.max(1).ilog10())
        .map_err(|_| CodecError::malformed("Inventor numeric value exceeds target range"))?
        + 1;
    let mut id_text = ctx.retained_string(
        "inventor:protein:appearance#".len() + instance_digits + 1 + record_digits,
        "retain Inventor appearance id",
    )?;
    std::fmt::write(
        &mut id_text,
        format_args!("inventor:protein:appearance#{instance_ordinal}-{record_ordinal}"),
    )
    .map_err(|_| CodecError::malformed("cannot format Inventor appearance id"))?;
    AppearanceId::mint(id_text)
        .map_err(|_| CodecError::malformed("Inventor appearance id is invalid"))
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
    fn appearance_identity_needs_no_scoped_key_and_refuses_retained_id() {
        let arena = DecodeArena::new();
        let bytes = b"fixture";
        let (service, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("service context");
        let id = appearance_id(&service, 0, 1).expect("identity admitted");
        assert_eq!(id.as_str(), "inventor:protein:appearance#0-1");
        // The id is written whole: no temporary key text is needed.
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("limited context");
        assert_eq!(
            appearance_id(&limited, 0, 1)
                .expect("no scoped storage")
                .as_str(),
            id.as_str()
        );
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

    #[test]
    fn bounded_appearance_identity_admits_exact_storage_without_work() {
        for (instance, record, expected) in [
            (0, 0, "inventor:protein:appearance#0-0"),
            (9, 10, "inventor:protein:appearance#9-10"),
            (10, 99, "inventor:protein:appearance#10-99"),
            (
                100,
                u64::MAX,
                "inventor:protein:appearance#100-18446744073709551615",
            ),
        ] {
            for exact in [false, true] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes =
                    cadmpeg_core::decode::u64_from_index(expected.len()) - u64::from(!exact);
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("appearance identity context");
                let result = appearance_id(&ctx, instance, record);
                if exact {
                    assert_eq!(result.expect("exact identity storage").as_str(), expected);
                    ctx.finish_session()
                        .expect("bounded identity needs no work");
                } else {
                    let error = result.expect_err("identity storage refuses before formatting");
                    assert!(
                        matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "retain Inventor appearance id"
                            && limit.used == 0
                            && limit.additional == cadmpeg_core::decode::u64_from_index(expected.len()))
                    );
                    assert!(
                        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                        if matches!(&error, cadmpeg_core::CodecError::ResourceLimit(original) if original == &limit))
                    );
                }
            }
        }
    }

    #[test]
    fn material_catalog_refuses_only_the_next_source_step() {
        for count in [1_usize, 512] {
            let instances: Vec<_> = (0..count)
                .map(|_| ProteinInstanceRecords {
                    entry_name: String::new(),
                    records: Vec::new(),
                    rejected: Vec::new(),
                })
                .collect();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("material catalog context");
            let error = project_catalog(&ctx, &instances, &mut 0)
                .err()
                .expect("first instance step refuses");
            assert!(
                matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "Inventor material GUID count instances"
                    && limit.used == 0 && limit.additional == 1)
            );
            assert!(
                matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if matches!(&error, cadmpeg_core::CodecError::ResourceLimit(original) if original == &limit))
            );
        }
    }

    #[test]
    fn empty_material_catalog_admits_three_source_end_probes() {
        for allowance in [2_u64, 3] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_materialized_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty catalog context");
            let result = project_catalog(&ctx, &[], &mut 0);
            if allowance == 2 {
                let error = result.err().expect("appearance source end probe refuses");
                assert!(
                    matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.operation == "Inventor appearance instances"
                        && limit.used == 2 && limit.additional == 1)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if matches!(&error, cadmpeg_core::CodecError::ResourceLimit(original) if original == &limit))
                );
            } else {
                let catalog = result.expect("three flat source end probes fit");
                assert!(catalog.appearances.is_empty());
                assert!(catalog.duplicate_guids.is_empty());
                assert_eq!(catalog.untyped_distance_properties, 0);
                ctx.finish_session()
                    .expect("empty catalog needs no scratch");
            }
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
        // The texture's GUID count takes the one slot before the catalog entry.
        policy.limits.max_collection_items = 1;
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
        // Two GUID counts and one texture catalog entry precede the connection.
        policy.limits.max_collection_items = 3;
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
        // The appearance's GUID count takes the one slot before its property map.
        policy.limits.max_collection_items = 1;
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
    fn duplicate_material_guids_retain_tree_key_order() {
        let texture = |guid: &str| DecodedRecord {
            ordinal: 0,
            logical_offset: 0,
            schema: "UnifiedBitmapSchema".into(),
            guid: guid.into(),
            base: String::new(),
            asset_lib_id: String::new(),
            properties: BTreeMap::new(),
        };
        let instances = [ProteinInstanceRecords {
            entry_name: "AssetData/InstanceProperties.bin".into(),
            records: vec![texture("z"), texture("a"), texture("z"), texture("a")],
            rejected: Vec::new(),
        }];
        let catalog = project_fixture(&instances).expect("duplicate-only texture catalog");
        assert_eq!(catalog.duplicate_guids, ["a", "z"]);
        assert!(catalog.appearances.is_empty());
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
