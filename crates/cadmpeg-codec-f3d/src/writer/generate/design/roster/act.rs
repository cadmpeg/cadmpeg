// SPDX-License-Identifier: Apache-2.0
//! The ACT segment of a Base Feature design: the ACT table, the component
//! and instance records the Design stream names across segments, and one
//! aspect entry for each of them and for each body-reference record (f3d.md
//! §3.1, "ACT segment of a Base Feature design").

use cadmpeg_core::CodecError;

use super::super::frames::{Bind, DesignRecords};
use super::bodies::geometry_guid;
use super::segment::{Segment, SegmentKind, TypeTable};
use super::{count, Context, ACT_COMPONENT, AGGREGATE, NAMING_SPACE, ROOT_INSTANCE};
use crate::layout;
use crate::writer::generate::presentation::TypeRow;

const fn row(
    guid: &'static str,
    base: Option<&'static str>,
    version: u32,
    module: &'static str,
) -> TypeRow {
    TypeRow {
        guid,
        base,
        version,
        module,
    }
}

const TABLE: TypeRow = row(
    "8EF12734-4D22-4F6A-8A80-1462E6B40A13",
    Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
    1,
    "ACT",
);
const COMPONENTS_ROOT: TypeRow = row(
    "596618BE-CD74-4BC9-A627-2DA799696C2D",
    Some("5EAF2F0F-4AEE-4f20-B569-119A7BD1F36D"),
    0,
    "CommonData",
);
const COMPONENT: TypeRow = row(
    "D956D02C-2806-4E57-8B2E-EFA323A5C2B8",
    Some("63D6AD42-466A-40B7-9CCD-EB21BC500EF6"),
    0,
    "CommonData",
);
const ASPECT_ENTRY: TypeRow = row(
    "ABC8C0E3-50FB-4E8C-AFF3-116759A36073",
    Some("19E286C3-1C58-4F13-8050-3F00EC2F1562"),
    0,
    "ACT",
);
const NAMING_LINK: TypeRow = row(
    "F01D16CF-95D5-4239-A488-00448AC88023",
    Some("A7AEA631-985B-4DD1-8CE2-DE2C-14B54081"),
    0,
    "CommonData",
);
const INSTANCES_ROOT: TypeRow = row(
    "F3E2665C-F3E1-4BA7-8676-8B90579B56D3",
    Some("5EAF2F0F-4AEE-4f20-B569-119A7BD1F36D"),
    1,
    "CommonData",
);
const INSTANCE_ROOT: TypeRow = row(
    "96E1B59A-1989-4493-AA92-67E6072135F6",
    Some("63D6AD42-466A-40B7-9CCD-EB21BC500EF6"),
    0,
    "CommonData",
);

/// The rows the ACT type table lists with an empty entity-ID run.
const EMPTY_ROWS: [TypeRow; 11] = [
    row("98542EB9-A4F2-4137-A808-DBB5B3CD6159", None, 0, ""),
    row(
        "5EAF2F0F-4AEE-4f20-B569-119A7BD1F36D",
        Some("E47053DB-8AF6-4521-81D6-523964C0C97A"),
        1,
        "CommonData",
    ),
    row(
        "E47053DB-8AF6-4521-81D6-523964C0C97A",
        Some("CB844AB6-240D-4fc9-9C9F-3679DC896D6F"),
        0,
        "DataModel",
    ),
    row(
        "CB844AB6-240D-4fc9-9C9F-3679DC896D6F",
        Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
        1,
        "DataModel",
    ),
    row(
        "63D6AD42-466A-40B7-9CCD-EB21BC500EF6",
        Some("CB844AB6-240D-4fc9-9C9F-3679DC896D6F"),
        0,
        "CommonData",
    ),
    row(
        "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
        Some("834C9DEF-4C39-4587-A08D-A5BD1267B7B4"),
        0,
        "CommonData",
    ),
    row(
        "834C9DEF-4C39-4587-A08D-A5BD1267B7B4",
        Some("CB844AB6-240D-4fc9-9C9F-3679DC896D6F"),
        2,
        "DataModel",
    ),
    row(
        "19E286C3-1C58-4F13-8050-3F00EC2F1562",
        Some("4624CDED-E181-493B-BC82-3372738D683C"),
        0,
        "ACT",
    ),
    row(
        "4624CDED-E181-493B-BC82-3372738D683C",
        Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
        1,
        "ACT",
    ),
    row("3AFBE085-BFA1-4DA6-BE71-45A6C61ECB99", None, 1, ""),
    row("70BD8285-AEDC-4E05-9632-B8B57FEA1E42", None, 0, ""),
];

/// The ACT table.
const ACT_TABLE: u32 = 0;
/// The components root.
const ACT_COMPONENTS_ROOT: u32 = 1;
/// The aspect entry of the component record.
const COMPONENT_ASPECTS: u32 = 3;
/// The instances root.
const ACT_INSTANCES_ROOT: u32 = 5;
/// The aspect entry of the instance root.
const INSTANCE_ASPECTS: u32 = 7;
/// The aspect entries of the first body's design and parameter
/// body-reference records.
const FIRST_BODY_ASPECTS: [u32; 2] = [8, 10];
/// The first aspect entry of a further body.
const FURTHER_BODY_ASPECTS: u32 = 12;
/// The ACT records that store an LP-ASCII name after four zero bytes.
const NAMED: [u32; 4] = [0, 1, 5, 6];

/// The six aspects, in the order every aspect list stores them.
const ASPECTS: [&str; 6] = [
    "Appearance",
    "Geometry",
    "PhysicalMaterial",
    "Properties",
    "StructureChange",
    "Transform",
];

/// One aspect entry: its record, its key, and the GUID of each aspect it
/// lists.
struct AspectEntry {
    record: u32,
    key: String,
    guids: Vec<String>,
}

/// The ACT segment's type table and records.
pub(super) struct Act {
    table: TypeTable,
    records: DesignRecords,
    next_entity: u64,
}

impl Act {
    pub(super) fn new(ctx: &Context<'_>) -> Result<Self, CodecError> {
        let entries = aspect_entries(ctx)?;
        let mut rows = vec![
            (ACT_TABLE, TABLE),
            (ACT_COMPONENTS_ROOT, COMPONENTS_ROOT),
            (ACT_COMPONENT, COMPONENT),
            (super::fixed::ACT_NAMING_LINK, NAMING_LINK),
            (ACT_INSTANCES_ROOT, INSTANCES_ROOT),
            (super::component::ACT_INSTANCE_ROOT, INSTANCE_ROOT),
        ];
        rows.extend(entries.iter().map(|entry| (entry.record, ASPECT_ENTRY)));
        let table = TypeTable::new(&rows, &EMPTY_ROWS)?;
        let class = |row: TypeRow| table.class(row.guid);
        let mut out = DesignRecords::default();

        let mut keyed = entries.iter().collect::<Vec<_>>();
        keyed.sort_by(|a, b| a.key.as_bytes().cmp(b.key.as_bytes()));
        let geometry = ctx.values.lower_guid("act-table-Geometry");
        let aspect_guids = ASPECTS.map(|aspect| match aspect {
            "Geometry" => geometry.clone(),
            other => ctx.values.lower_guid(&format!("act-table-{other}")),
        });
        out.templated(
            &class(TABLE)?,
            ACT_TABLE,
            &layout::base_feature_act_table_prefix::TEMPLATE,
            &layout::base_feature_act_table_prefix::UNSTATED,
            &[(
                layout::base_feature_act_table_prefix::ENTRY_COUNT,
                Bind::U32(count(keyed.len())?),
            )],
        )?;
        for entry in &keyed {
            out.reference(entry.record).lp_utf16(&entry.key)?;
        }
        out.lp_utf16(crate::manifest::GENERATED_ASSET_GUID)?
            .lp_utf16(&geometry)?
            .u32(0)
            .u32(count(ASPECTS.len())?);
        for (aspect, guid) in ASPECTS.iter().zip(&aspect_guids) {
            out.lp_ascii(aspect)?.lp_utf16(guid)?;
        }

        out.templated(
            &class(COMPONENTS_ROOT)?,
            ACT_COMPONENTS_ROOT,
            &layout::base_feature_act_components_root::TEMPLATE,
            &layout::base_feature_act_components_root::UNSTATED,
            &[(
                layout::base_feature_act_components_root::COMPONENT_RECORD,
                Bind::Ref(ACT_COMPONENT),
            )],
        )?;
        let design = |target: u32| Bind::Cross {
            target,
            segment: SegmentKind::Design.id(),
        };
        let name = &ctx.values.name;
        out.templated(
            &class(COMPONENT)?,
            ACT_COMPONENT,
            &layout::base_feature_act_component_record_prefix::TEMPLATE,
            &layout::base_feature_act_component_record_prefix::UNSTATED,
            &[
                (
                    layout::base_feature_act_component_record_prefix::NAMING_SPACE_LINK,
                    Bind::Ref(super::fixed::ACT_NAMING_LINK),
                ),
                (
                    layout::base_feature_act_component_record_prefix::KEY_CODE_UNITS,
                    Bind::Units(&format!("1_{ACT_COMPONENT}")),
                ),
                (
                    layout::base_feature_act_component_record_prefix::DESIGN_COMPONENT_AGGREGATE,
                    design(AGGREGATE),
                ),
                (
                    layout::base_feature_act_component_record_prefix::COMPONENT_NAME_LENGTH,
                    Bind::U32(count(name.encode_utf16().count())?),
                ),
            ],
        )?;
        for unit in name.encode_utf16() {
            out.raw(&unit.to_le_bytes());
        }
        out.byte(0).reference(super::component::ACT_INSTANCE_ROOT);

        out.templated(
            &class(NAMING_LINK)?,
            super::fixed::ACT_NAMING_LINK,
            &layout::base_feature_act_naming_space_link::TEMPLATE,
            &layout::base_feature_act_naming_space_link::UNSTATED,
            &[
                (
                    layout::base_feature_act_naming_space_link::DESIGN_NAMING_SPACE,
                    design(NAMING_SPACE),
                ),
                (
                    layout::base_feature_act_naming_space_link::COMPONENT_RECORD,
                    Bind::Ref(ACT_COMPONENT),
                ),
            ],
        )?;
        let instance_root = super::component::ACT_INSTANCE_ROOT;
        out.templated(
            &class(INSTANCES_ROOT)?,
            ACT_INSTANCES_ROOT,
            &layout::base_feature_act_instances_root::TEMPLATE,
            &layout::base_feature_act_instances_root::UNSTATED,
            &[
                (
                    layout::base_feature_act_instances_root::INSTANCE_ROOT,
                    Bind::Ref(instance_root),
                ),
                (
                    layout::base_feature_act_instances_root::INSTANCE_ROOT_REPEAT,
                    Bind::Ref(instance_root),
                ),
            ],
        )?;
        out.templated(
            &class(INSTANCE_ROOT)?,
            instance_root,
            &layout::base_feature_act_instance_root::TEMPLATE,
            &layout::base_feature_act_instance_root::UNSTATED,
            &[
                (
                    layout::base_feature_act_instance_root::INSTANCE_ROOT_SELF,
                    Bind::Ref(instance_root),
                ),
                (
                    layout::base_feature_act_instance_root::COMPONENT_RECORD,
                    Bind::Ref(ACT_COMPONENT),
                ),
                (
                    layout::base_feature_act_instance_root::KEY_CODE_UNITS,
                    Bind::Units(&format!("1_{instance_root}")),
                ),
                (
                    layout::base_feature_act_instance_root::DESIGN_ROOT_INSTANCE,
                    design(ROOT_INSTANCE),
                ),
                (
                    layout::base_feature_act_instance_root::COMPONENT_RECORD_REPEAT,
                    Bind::Ref(ACT_COMPONENT),
                ),
            ],
        )?;
        for entry in &entries {
            out.templated(
                &class(ASPECT_ENTRY)?,
                entry.record,
                &layout::base_feature_act_aspect_entry_prefix::TEMPLATE,
                &layout::base_feature_act_aspect_entry_prefix::UNSTATED,
                &[(
                    layout::base_feature_act_aspect_entry_prefix::ASPECT_COUNT,
                    Bind::U32(count(entry.guids.len())?),
                )],
            )?;
            for (aspect, guid) in ASPECTS.iter().zip(&entry.guids) {
                out.lp_ascii(aspect)?.lp_utf16(guid)?;
            }
            out.lp_utf16(&entry.key)?;
        }
        let highest = rows.iter().map(|&(index, _)| index).max().unwrap_or(0);
        Ok(Self {
            table,
            records: out.sorted()?,
            next_entity: u64::from(highest) + 2,
        })
    }

    pub(super) fn segment(&self) -> Segment<'_> {
        Segment {
            kind: SegmentKind::Act,
            table: &self.table,
            records: &self.records,
            named: NAMED.to_vec(),
            next_entity: self.next_entity,
        }
    }
}

/// The aspect entries: the six-aspect entries of the component record and
/// the instance root, and a four-aspect entry for each body-reference
/// record.
fn aspect_entries(ctx: &Context<'_>) -> Result<Vec<AspectEntry>, CodecError> {
    let fresh =
        |record: u32, aspect: &str| ctx.values.lower_guid(&format!("act-{record}-{aspect}"));
    let material = fresh(COMPONENT_ASPECTS, "PhysicalMaterial");
    let component = ASPECTS
        .iter()
        .map(|aspect| match *aspect {
            "PhysicalMaterial" | "Transform" => material.clone(),
            other => fresh(COMPONENT_ASPECTS, other),
        })
        .collect();
    let shared = fresh(INSTANCE_ASPECTS, "Instance");
    let instance = ASPECTS
        .iter()
        .map(|aspect| match *aspect {
            "Properties" => fresh(INSTANCE_ASPECTS, "Properties"),
            _ => shared.clone(),
        })
        .collect();
    let mut entries = vec![
        AspectEntry {
            record: COMPONENT_ASPECTS,
            key: format!("1_{ACT_COMPONENT}"),
            guids: component,
        },
        AspectEntry {
            record: INSTANCE_ASPECTS,
            key: format!("1_{}", super::component::ACT_INSTANCE_ROOT),
            guids: instance,
        },
    ];
    let mut next = FURTHER_BODY_ASPECTS;
    for (position, body) in ctx.roster.bodies.iter().enumerate() {
        for (side, reference) in [body.design, body.parameter].into_iter().enumerate() {
            let record = match FIRST_BODY_ASPECTS.get(side).filter(|_| position == 0) {
                Some(record) => *record,
                None => {
                    let record = next;
                    next = next
                        .checked_add(1)
                        .ok_or_else(|| CodecError::NotImplemented("ACT record index".into()))?;
                    record
                }
            };
            let geometry = geometry_guid(ctx, reference);
            entries.push(AspectEntry {
                record,
                key: format!("0_{reference}"),
                guids: ASPECTS[..4]
                    .iter()
                    .map(|aspect| match *aspect {
                        "Geometry" => geometry.clone(),
                        other => fresh(record, other),
                    })
                    .collect(),
            });
        }
    }
    Ok(entries)
}
