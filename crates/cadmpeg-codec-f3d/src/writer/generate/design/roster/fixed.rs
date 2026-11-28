// SPDX-License-Identifier: Apache-2.0
//! The records of a Base Feature design that have the same bytes in every
//! design apart from their indexed headers and references (f3d.md §3.1,
//! "Fixed records of the roster"), each written from its layout template.

use cadmpeg_core::CodecError;

use super::super::frames::{Bind, DesignRecords};
use super::segment::SegmentKind;
use super::{Context, AGGREGATE, ASSET_MANAGER, ROOT_INSTANCE};
use crate::layout;

/// The ancestor type of the paired headers of most fixed records.
const COLLECTION: &str = "834C9DEF-4C39-4587-A08D-A5BD1267B7B4";
/// The ancestor type of the paired headers of records 38, 41, 178 and 194.
const COMMON_COLLECTION: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
/// The ancestor type of the paired header of record 162.
pub(super) const DATA_MODEL_OBJECT: &str = "CB844AB6-240D-4fc9-9C9F-3679DC896D6F";
/// The ancestor type of the paired header of record 42.
const PROPERTY_OWNER: &str = "63D6AD42-466A-40B7-9CCD-EB21BC500EF6";
/// The ancestor type of the paired header of record 39.
const COMMON_COLLECTION_ITEM: &str = "B5B50E5B-EDC8-4309-82A3-C53B3E82033D";
/// The ACT naming-space link.
pub(super) const ACT_NAMING_LINK: u32 = 4;
/// The unit-system records, in roster order.
pub(super) const UNIT_SYSTEMS: [u32; 6] = [47, 67, 87, 107, 124, 141];

/// The basenames of the configuration-table and configuration-rule members
/// that records 185 and 186 name.
pub(super) fn configuration_basenames(ctx: &Context<'_>) -> [String; 2] {
    if let Some(names) = &ctx.configuration_names {
        return names.clone();
    }
    [
        format!(
            "DesignConfigurationTable.{}.dsgcfg",
            ctx.values.lower_guid("configuration-table")
        ),
        format!(
            "DesignConfigurationRule.{}.dsgcfgrule",
            ctx.values.lower_guid("configuration-rule")
        ),
    ]
}

/// A fixed record: its index, layout template, unstated slots and binds.
type FixedRecord<'a> = (u32, &'a [u8], &'a [(usize, usize)], &'a [(usize, Bind<'a>)]);

pub(super) fn write(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    let collection = ctx.class(COLLECTION)?;
    let common = ctx.class(COMMON_COLLECTION)?;
    let object = ctx.class(DATA_MODEL_OBJECT)?;
    let owner = ctx.class(PROPERTY_OWNER)?;
    let item = ctx.class(COMMON_COLLECTION_ITEM)?;
    let aggregate = Bind::Ref(AGGREGATE);
    let [table, rule] = configuration_basenames(ctx);
    let units = UNIT_SYSTEMS.map(Bind::Ref);
    let records: &[FixedRecord<'_>] = &[
        (
            2,
            &layout::base_feature_zero_record::TEMPLATE,
            &layout::base_feature_zero_record::UNSTATED,
            &[],
        ),
        (
            4,
            &layout::base_feature_components_root::TEMPLATE,
            &layout::base_feature_components_root::UNSTATED,
            &[(layout::base_feature_components_root::AGGREGATE, aggregate)],
        ),
        (
            5,
            &layout::base_feature_aggregate_link_25::TEMPLATE,
            &layout::base_feature_aggregate_link_25::UNSTATED,
            &[(layout::base_feature_aggregate_link_25::AGGREGATE, aggregate)],
        ),
        (
            6,
            &layout::base_feature_component_naming_space::TEMPLATE,
            &layout::base_feature_component_naming_space::UNSTATED,
            &[
                (
                    layout::base_feature_component_naming_space::ACT_NAMING_SPACE_LINK,
                    Bind::Cross {
                        target: ACT_NAMING_LINK,
                        segment: SegmentKind::Act.id(),
                    },
                ),
                (
                    layout::base_feature_component_naming_space::AGGREGATE,
                    aggregate,
                ),
            ],
        ),
        (
            10,
            &layout::base_feature_sketch_collection::TEMPLATE,
            &layout::base_feature_sketch_collection::UNSTATED,
            &[
                (
                    layout::base_feature_sketch_collection::CURVE_COLLECTION,
                    Bind::Ref(22),
                ),
                (layout::base_feature_sketch_collection::AGGREGATE, aggregate),
            ],
        ),
        (
            11,
            &layout::base_feature_record_11::TEMPLATE,
            &layout::base_feature_record_11::UNSTATED,
            &[
                (layout::base_feature_record_11::RECORD_21, Bind::Ref(21)),
                (layout::base_feature_record_11::AGGREGATE, aggregate),
            ],
        ),
        (
            19,
            &layout::base_feature_alternate_owner_list_empty::TEMPLATE,
            &layout::base_feature_alternate_owner_list_empty::UNSTATED,
            &[(
                layout::base_feature_alternate_owner_list_empty::AGGREGATE,
                aggregate,
            )],
        ),
        (
            23,
            &layout::base_feature_paired_record_23::TEMPLATE,
            &layout::base_feature_paired_record_23::UNSTATED,
            &[
                (
                    layout::base_feature_paired_record_23::PAIRED_HEADER,
                    Bind::Paired(&collection),
                ),
                (layout::base_feature_paired_record_23::AGGREGATE, aggregate),
            ],
        ),
        (
            34,
            &layout::base_feature_record_34::TEMPLATE,
            &layout::base_feature_record_34::UNSTATED,
            &[
                (layout::base_feature_record_34::RECORD_39, Bind::Ref(39)),
                (
                    layout::base_feature_record_34::PAIRED_HEADER,
                    Bind::Paired(&collection),
                ),
                (layout::base_feature_record_34::AGGREGATE, aggregate),
            ],
        ),
        (
            36,
            &layout::base_feature_record_36::TEMPLATE,
            &layout::base_feature_record_36::UNSTATED,
            &[
                (
                    layout::base_feature_record_36::PAIRED_HEADER,
                    Bind::Paired(&collection),
                ),
                (layout::base_feature_record_36::AGGREGATE, aggregate),
            ],
        ),
        (
            38,
            &layout::base_feature_record_38::TEMPLATE,
            &layout::base_feature_record_38::UNSTATED,
            &[
                (
                    layout::base_feature_record_38::PAIRED_HEADER,
                    Bind::Paired(&common),
                ),
                (layout::base_feature_record_38::AGGREGATE, aggregate),
            ],
        ),
        (
            39,
            &layout::base_feature_record_39::TEMPLATE,
            &layout::base_feature_record_39::UNSTATED,
            &[
                (
                    layout::base_feature_record_39::PAIRED_HEADER,
                    Bind::Paired(&item),
                ),
                (layout::base_feature_record_39::AGGREGATE, aggregate),
            ],
        ),
        (
            40,
            &layout::base_feature_record_40::TEMPLATE,
            &layout::base_feature_record_40::UNSTATED,
            &[
                (
                    layout::base_feature_record_40::PAIRED_HEADER,
                    Bind::Paired(&collection),
                ),
                (layout::base_feature_record_40::AGGREGATE, aggregate),
            ],
        ),
        (
            41,
            &layout::base_feature_record_41::TEMPLATE,
            &layout::base_feature_record_41::UNSTATED,
            &[
                (
                    layout::base_feature_record_41::PAIRED_HEADER,
                    Bind::Paired(&common),
                ),
                (layout::base_feature_record_41::RECORD_42, Bind::Ref(42)),
                (layout::base_feature_record_41::AGGREGATE, aggregate),
            ],
        ),
        (
            42,
            &layout::base_feature_record_42::TEMPLATE,
            &layout::base_feature_record_42::UNSTATED,
            &[
                (
                    layout::base_feature_record_42::ASSET_MANAGER,
                    Bind::Ref(ASSET_MANAGER),
                ),
                (
                    layout::base_feature_record_42::PAIRED_HEADER,
                    Bind::Paired(&owner),
                ),
                (layout::base_feature_record_42::RECORD_41, Bind::Ref(41)),
            ],
        ),
        (
            44,
            &layout::base_feature_component_instances_root::TEMPLATE,
            &layout::base_feature_component_instances_root::UNSTATED,
            &[(
                layout::base_feature_component_instances_root::ROOT_INSTANCE,
                Bind::Ref(ROOT_INSTANCE),
            )],
        ),
        (
            45,
            &layout::base_feature_visual_analyses::TEMPLATE,
            &layout::base_feature_visual_analyses::UNSTATED,
            &[
                (
                    layout::base_feature_visual_analyses::RECORD_0_A,
                    Bind::Ref(0),
                ),
                (
                    layout::base_feature_visual_analyses::RECORD_0_B,
                    Bind::Ref(0),
                ),
                (
                    layout::base_feature_visual_analyses::PAIRED_HEADER,
                    Bind::Paired(&collection),
                ),
            ],
        ),
        (
            46,
            &layout::base_feature_unit_system_root::TEMPLATE,
            &layout::base_feature_unit_system_root::UNSTATED,
            &[
                (layout::base_feature_unit_system_root::CM_MKS, units[0]),
                (layout::base_feature_unit_system_root::MM_MKS, units[1]),
                (layout::base_feature_unit_system_root::M_MKS, units[2]),
                (
                    layout::base_feature_unit_system_root::INCH_IMPERIAL,
                    units[3],
                ),
                (layout::base_feature_unit_system_root::IMPERIAL, units[4]),
                (layout::base_feature_unit_system_root::CUSTOM, units[5]),
            ],
        ),
        (
            161,
            &layout::base_feature_named_tracked_entity_set::TEMPLATE,
            &layout::base_feature_named_tracked_entity_set::UNSTATED,
            &[],
        ),
        (
            162,
            &layout::base_feature_record_162::TEMPLATE,
            &layout::base_feature_record_162::UNSTATED,
            &[
                (
                    layout::base_feature_record_162::PAIRED_HEADER,
                    Bind::Paired(&object),
                ),
                (layout::base_feature_record_162::AGGREGATE, aggregate),
            ],
        ),
        (
            177,
            &layout::base_feature_working_model_placeholder_root::TEMPLATE,
            &layout::base_feature_working_model_placeholder_root::UNSTATED,
            &[(
                layout::base_feature_working_model_placeholder_root::PAIRED_HEADER,
                Bind::Paired(&collection),
            )],
        ),
        (
            178,
            &layout::base_feature_record_178::TEMPLATE,
            &layout::base_feature_record_178::UNSTATED,
            &[
                (
                    layout::base_feature_record_178::PAIRED_HEADER,
                    Bind::Paired(&common),
                ),
                (layout::base_feature_record_178::AGGREGATE, aggregate),
            ],
        ),
        (
            184,
            &layout::base_feature_ogs_serializer::TEMPLATE,
            &layout::base_feature_ogs_serializer::UNSTATED,
            &[],
        ),
        (
            185,
            &layout::base_feature_configuration_table_trigger::TEMPLATE,
            &layout::base_feature_configuration_table_trigger::UNSTATED,
            &[(
                layout::base_feature_configuration_table_trigger::BASENAME,
                Bind::Units(&table),
            )],
        ),
        (
            186,
            &layout::base_feature_configuration_rule_trigger::TEMPLATE,
            &layout::base_feature_configuration_rule_trigger::UNSTATED,
            &[(
                layout::base_feature_configuration_rule_trigger::BASENAME,
                Bind::Units(&rule),
            )],
        ),
        (
            194,
            &layout::base_feature_record_194::TEMPLATE,
            &layout::base_feature_record_194::UNSTATED,
            &[
                (
                    layout::base_feature_record_194::PAIRED_HEADER,
                    Bind::Paired(&common),
                ),
                (layout::base_feature_record_194::AGGREGATE, aggregate),
            ],
        ),
        (
            196,
            &layout::base_feature_snapshot_map_companion::TEMPLATE,
            &layout::base_feature_snapshot_map_companion::UNSTATED,
            &[],
        ),
    ];
    for (index, template, unstated, binds) in records {
        // A design with `Sketch` items writes the sketch collection with them.
        if *index == super::SKETCHES && !ctx.roster.sketches.is_empty() {
            continue;
        }
        // A design with `WorkPlane` items writes the construction collection
        // with them.
        if *index == super::planes::CONSTRUCTION_COLLECTION && !ctx.roster.planes.is_empty() {
            continue;
        }
        ctx.templated(out, *index, template, unstated, binds)?;
    }
    for index in [12, 37] {
        ctx.templated(
            out,
            index,
            &layout::base_feature_flagged_aggregate_link_42::TEMPLATE,
            &layout::base_feature_flagged_aggregate_link_42::UNSTATED,
            &[(
                layout::base_feature_flagged_aggregate_link_42::AGGREGATE,
                aggregate,
            )],
        )?;
    }
    ctx.templated(
        out,
        13,
        &layout::base_feature_flagged_aggregate_link_46::TEMPLATE,
        &layout::base_feature_flagged_aggregate_link_46::UNSTATED,
        &[(
            layout::base_feature_flagged_aggregate_link_46::AGGREGATE,
            aggregate,
        )],
    )?;
    for index in [14, 16, 20, 21, 22] {
        ctx.templated(
            out,
            index,
            &layout::base_feature_aggregate_link_41::TEMPLATE,
            &layout::base_feature_aggregate_link_41::UNSTATED,
            &[(layout::base_feature_aggregate_link_41::AGGREGATE, aggregate)],
        )?;
    }
    ctx.templated(
        out,
        15,
        &layout::base_feature_aggregate_link_45::TEMPLATE,
        &layout::base_feature_aggregate_link_45::UNSTATED,
        &[(layout::base_feature_aggregate_link_45::AGGREGATE, aggregate)],
    )?;
    for index in [32, 33] {
        ctx.templated(
            out,
            index,
            &layout::base_feature_paired_aggregate_link_55::TEMPLATE,
            &layout::base_feature_paired_aggregate_link_55::UNSTATED,
            &[
                (
                    layout::base_feature_paired_aggregate_link_55::PAIRED_HEADER,
                    Bind::Paired(&collection),
                ),
                (
                    layout::base_feature_paired_aggregate_link_55::AGGREGATE,
                    aggregate,
                ),
            ],
        )?;
    }
    for index in 173..=176 {
        ctx.templated(
            out,
            index,
            &layout::base_feature_archive_entry_name::TEMPLATE,
            &layout::base_feature_archive_entry_name::UNSTATED,
            &[],
        )?;
    }
    Ok(())
}
