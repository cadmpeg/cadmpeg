// SPDX-License-Identifier: Apache-2.0
//! The Browser segment of a Base Feature design: the browser root, the
//! component node and the browser entries of the Design records the
//! browser shows (f3d.md §3.1, "Browser segment of a Base Feature design").

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;

use super::super::frames::{Bind, DesignRecords};
use super::fixed::DATA_MODEL_OBJECT;
use super::segment::{Segment, SegmentKind, TypeTable};
use super::{count, AGGREGATE, NAMING_SPACE, RECORD_23, ROOT_INSTANCE};
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

const NODE_BASE: &str = "6AE9DE8C-A6F6-40CB-B13D-4091AEA4968D";
const FOLDER_BASE: &str = "F4EEA2F1-81A8-4171-A828-D7940E1B525E";
const ADD_IN: &str = "BrowserAddInIdentifier";

/// `(type row, records)` of every row of the Browser type table, in the
/// order f3d.md states one valid table.
const ROWS: [(TypeRow, &[u32]); 41] = [
    (
        row(
            "42A5CADA-9BA3-46AB-800F-E1D821F1836A",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[3],
    ),
    (
        row(
            NODE_BASE,
            Some("01CAE6B2-DF44-4569-A7E8-70486A17F4BD"),
            0,
            ADD_IN,
        ),
        &[],
    ),
    (
        row(
            "01CAE6B2-DF44-4569-A7E8-70486A17F4BD",
            Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
            1,
            ADD_IN,
        ),
        &[],
    ),
    (
        row("98542EB9-A4F2-4137-A808-DBB5B3CD6159", None, 0, ""),
        &[],
    ),
    (
        row(
            "EE6D333C-314E-4825-9FE0-FBEA4018BD0D",
            Some(NODE_BASE),
            1,
            "Fusion",
        ),
        &[2],
    ),
    (
        row(
            "AD60A48F-855F-44F4-94F3-809F40CD4DAA",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[4],
    ),
    (
        row(
            "5BB3CC2D-15CD-406F-BEB4-2506D3250FA1",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[5],
    ),
    (
        row(
            "E165F0A1-0A0B-4E55-BE9F-C53AC3DBA409",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[6],
    ),
    (
        row(
            "75CB256B-FDDE-44D6-AE08-4DB583875AB7",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[7],
    ),
    (
        row(
            "192BB6B4-DA9B-4FD4-98C5-6BC67F4C67C4",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[8],
    ),
    (
        row(
            "B3B2A88D-4CDD-4C17-B769-713E339D80A4",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[9],
    ),
    (
        row(
            "ACA1DF11-A07B-4004-A07C-7ACFF54BDA7E",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[10],
    ),
    (
        row(
            "4C26A3DB-5071-4E35-AACE-547B55BFE707",
            Some(FOLDER_BASE),
            0,
            ADD_IN,
        ),
        &[11],
    ),
    (row(FOLDER_BASE, Some(NODE_BASE), 0, ADD_IN), &[]),
    (
        row(
            "5137338A-EEB8-489D-BF3A-6708BF003909",
            Some(FOLDER_BASE),
            0,
            ADD_IN,
        ),
        &[12],
    ),
    (
        row(
            "0DDFD0D0-9109-4164-B307-068BCF8FE0A5",
            Some(FOLDER_BASE),
            0,
            ADD_IN,
        ),
        &[13],
    ),
    (
        row(
            "9200EB53-D06E-4811-953A-9094AD3818E7",
            Some(FOLDER_BASE),
            0,
            ADD_IN,
        ),
        &[14],
    ),
    (
        row(
            "80DD69FA-8942-4D8B-9E6C-F7F9D730EA80",
            Some("FEE3F7C7-CEBE-4ED4-9946-453E6298C64E"),
            0,
            "Fusion",
        ),
        &[15, 16],
    ),
    (
        row(
            "FEE3F7C7-CEBE-4ED4-9946-453E6298C64E",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[],
    ),
    (
        row(
            "21042350-3D57-4270-B0EC-BB9125633E6B",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[17],
    ),
    (
        row(
            "82167150-660F-4308-89D7-836E444C404B",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[18],
    ),
    (
        row(
            "2B7BBC49-FCFC-4958-BAC2-7B09D1C9B98C",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[19],
    ),
    (
        row(
            "2688AD35-7069-4806-B8F0-9BA7462D77A3",
            Some("E483BFF5-54AB-43A2-8032-1E296D254BBF"),
            0,
            "Fusion",
        ),
        &[20],
    ),
    (
        row(
            "E483BFF5-54AB-43A2-8032-1E296D254BBF",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[],
    ),
    (
        row(
            "DA174979-046B-4658-833D-5F5B8EAA9665",
            Some("802C57C2-EA37-4FAC-A94E-422726AA01FC"),
            0,
            "Fusion",
        ),
        &[21],
    ),
    (
        row(
            "802C57C2-EA37-4FAC-A94E-422726AA01FC",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[],
    ),
    (
        row(
            "5DFC9F26-73BD-4B58-9BC7-51207A348BD1",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[22],
    ),
    (
        row(
            "C619506D-F0AF-47DB-8BB8-54E52A2107E3",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[23],
    ),
    (
        row(
            "1BDA73B0-EEFF-41DF-B575-58B448934FF8",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[24],
    ),
    (
        row(
            "2C50B57E-96DC-415E-AB7A-46FC2F7F378D",
            Some(NODE_BASE),
            0,
            ADD_IN,
        ),
        &[25],
    ),
    (
        row(
            "8F27D836-CBEE-4E17-ABA6-4E2FDBAB3FC4",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[26],
    ),
    (
        row(
            "E650F5D6-7846-49CC-BDE8-C817111BC32F",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[27],
    ),
    (
        row(
            "D6CCC7E7-C73C-4A4A-8681-FF6A397CF8F7",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[28],
    ),
    (
        row(
            "97E7C313-78ED-4711-B12C-50CCF0FF8E1E",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[29],
    ),
    (
        row(
            "3B7744B1-B605-4BAA-B1DF-E7535A6E9DBE",
            Some(NODE_BASE),
            0,
            "Fusion",
        ),
        &[30],
    ),
    (
        row(
            "33F72249-4935-47EB-8282-B2C3D93546CE",
            Some("A510106D-7CDB-4FC7-9504-6C4137A8A917"),
            0,
            "Fusion",
        ),
        &[0],
    ),
    (
        row(
            "A510106D-7CDB-4FC7-9504-6C4137A8A917",
            Some("155685E8-D562-4A1A-9302-A80F34CEC3C3"),
            0,
            ADD_IN,
        ),
        &[],
    ),
    (
        row(
            "155685E8-D562-4A1A-9302-A80F34CEC3C3",
            Some(DATA_MODEL_OBJECT),
            1,
            ADD_IN,
        ),
        &[],
    ),
    (
        row(
            DATA_MODEL_OBJECT,
            Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
            1,
            "DataModel",
        ),
        &[],
    ),
    (
        row(
            "DBD7EE26-FC31-4777-A1D2-A72EB3C65DB9",
            Some("CA22AE6F-CCEF-42D6-91E1-487F6A4EEC3B"),
            0,
            "Fusion",
        ),
        &[1],
    ),
    (
        row(
            "CA22AE6F-CCEF-42D6-91E1-487F6A4EEC3B",
            Some(NODE_BASE),
            1,
            ADD_IN,
        ),
        &[],
    ),
];

/// The browser root.
const ROOT: u32 = 0;
/// The component node.
const COMPONENT_NODE: u32 = 1;
/// The document-settings folder.
const SETTINGS_FOLDER: u32 = 2;
/// The relationships entry, the parent of entries 11 to 14.
const RELATIONSHIPS: u32 = 10;
/// The document-structure entry.
const DOCUMENT_STRUCTURE: u32 = 29;
/// The Browser records.
const RECORDS: u32 = 31;
/// The key of the browser root and its closing name.
const ROOT_KEY: &str = "PersistBrowserRootId";
/// The kind name of the document-structure entry.
const DOCUMENT_STRUCTURE_KIND: &str = "Na::DocumentStructureTypeBrowserEntry";

/// A browser entry: the Browser record, the Design record it shows, its
/// position among the entries of that Design record, its parent, and its
/// kind name.
struct Entry {
    record: u32,
    shown: u32,
    position: u32,
    parent: u32,
    kind: &'static str,
}

const fn entry(record: u32, shown: u32, parent: u32) -> Entry {
    Entry {
        record,
        shown,
        position: 0,
        parent,
        kind: "",
    }
}

const fn relationship(record: u32, position: u32, kind: &'static str) -> Entry {
    Entry {
        record,
        shown: 13,
        position,
        parent: if position == 0 {
            COMPONENT_NODE
        } else {
            RELATIONSHIPS
        },
        kind,
    }
}

/// Every browser entry except record 29.
const ENTRIES: [Entry; 28] = [
    entry(2, 0, COMPONENT_NODE),
    entry(3, 163, COMPONENT_NODE),
    entry(4, 20, COMPONENT_NODE),
    entry(5, 15, COMPONENT_NODE),
    entry(6, 24, COMPONENT_NODE),
    entry(7, 45, COMPONENT_NODE),
    entry(8, 16, COMPONENT_NODE),
    entry(9, 12, COMPONENT_NODE),
    relationship(10, 0, "Ns::Comp::RelationshipsBrowserEntry"),
    relationship(11, 1, "JointsBrowserEntry"),
    relationship(12, 2, "Ns::Comp::RigidGroupsBrowserEntry"),
    relationship(13, 3, "TangentRelationshipsBrowserEntry"),
    relationship(14, 4, "Ns::Comp::ConstraintsBrowserEntry"),
    entry(15, 7, COMPONENT_NODE),
    entry(16, 193, COMPONENT_NODE),
    entry(17, 34, COMPONENT_NODE),
    entry(18, 37, COMPONENT_NODE),
    entry(19, 36, COMPONENT_NODE),
    entry(20, 10, COMPONENT_NODE),
    entry(21, 11, COMPONENT_NODE),
    entry(22, 41, COMPONENT_NODE),
    entry(23, 19, COMPONENT_NODE),
    entry(24, 21, COMPONENT_NODE),
    entry(25, 22, COMPONENT_NODE),
    entry(26, 33, COMPONENT_NODE),
    entry(27, 178, COMPONENT_NODE),
    entry(28, 46, SETTINGS_FOLDER),
    entry(30, 2, SETTINGS_FOLDER),
];

/// The Browser segment's type table and records.
pub(super) struct Browser {
    table: TypeTable,
    records: DesignRecords,
}

impl Browser {
    pub(super) fn new() -> Result<Self, CodecError> {
        let records = ROWS
            .iter()
            .flat_map(|(row, records)| records.iter().map(|record| (*record, *row)))
            .collect::<Vec<_>>();
        let known = ROWS.iter().map(|(row, _)| *row).collect::<Vec<_>>();
        let table = TypeTable::new(&records, &known)?;
        let class_of = |record: u32| {
            let (row, _) = ROWS
                .iter()
                .find(|(_, records)| records.contains(&record))
                .ok_or_else(|| CodecError::malformed(format_args!("Browser record {record}")))?;
            table.class(row.guid)
        };
        let design = |target: u32| Bind::Cross {
            target,
            segment: SegmentKind::Design.id(),
        };
        let mut out = DesignRecords::default();
        for entry in &ENTRIES {
            let children = ENTRIES
                .iter()
                .map(|child| child.record)
                .chain([DOCUMENT_STRUCTURE])
                .filter(|child| {
                    ENTRIES
                        .iter()
                        .find(|candidate| candidate.record == *child)
                        .map_or(entry.record == SETTINGS_FOLDER, |candidate| {
                            candidate.parent == entry.record
                        })
                })
                .collect::<BTreeSet<_>>();
            let mut record = DesignRecords::default();
            record.templated(
                &class_of(entry.record)?,
                entry.record,
                &layout::base_feature_browser_entry::TEMPLATE,
                &layout::base_feature_browser_entry::UNSTATED,
                &[
                    (
                        layout::base_feature_browser_entry::DESIGN_RECORD,
                        design(entry.shown),
                    ),
                    (
                        layout::base_feature_browser_entry::DESIGN_RECORD_REPEAT,
                        design(entry.shown),
                    ),
                    (
                        layout::base_feature_browser_entry::PARENT,
                        Bind::Ref(entry.parent),
                    ),
                    (
                        layout::base_feature_browser_entry::BROWSER_ROOT,
                        Bind::Ref(ROOT),
                    ),
                ],
            )?;
            let mut bytes = record.bytes;
            let at = |offset: usize| offset..offset + 4;
            bytes[at(layout::base_feature_browser_entry::POSITION)]
                .copy_from_slice(&entry.position.to_le_bytes());
            bytes[at(layout::base_feature_browser_entry::CHILD_COUNT)]
                .copy_from_slice(&count(children.len())?.to_le_bytes());
            bytes[at(layout::base_feature_browser_entry::KIND_LENGTH)]
                .copy_from_slice(&count(entry.kind.len())?.to_le_bytes());
            let mut tail = bytes.split_off(layout::base_feature_browser_entry::TERMINATOR);
            bytes.extend_from_slice(entry.kind.as_bytes());
            bytes.append(&mut tail);
            let mut listed = DesignRecords::default();
            for child in &children {
                listed.reference(*child);
            }
            let mut rest =
                bytes.split_off(layout::base_feature_browser_entry::DESIGN_RECORD_REPEAT);
            bytes.extend_from_slice(&listed.bytes);
            bytes.append(&mut rest);
            out.primaries.push((entry.record, out.bytes.len()));
            out.raw(&bytes);
        }
        out.templated(
            &class_of(DOCUMENT_STRUCTURE)?,
            DOCUMENT_STRUCTURE,
            &layout::base_feature_browser_document_structure_entry::TEMPLATE,
            &layout::base_feature_browser_document_structure_entry::UNSTATED,
            &[
                (
                    layout::base_feature_browser_document_structure_entry::DESIGN_RECORD,
                    design(NAMING_SPACE),
                ),
                (
                    layout::base_feature_browser_document_structure_entry::EXTRA_DESIGN_RECORD,
                    design(NAMING_SPACE),
                ),
                (
                    layout::base_feature_browser_document_structure_entry::PARENT,
                    Bind::Ref(SETTINGS_FOLDER),
                ),
                (
                    layout::base_feature_browser_document_structure_entry::BROWSER_ROOT,
                    Bind::Ref(ROOT),
                ),
            ],
        )?;

        let mut children = DesignRecords::default();
        for child in ENTRIES
            .iter()
            .filter(|entry| entry.parent == COMPONENT_NODE)
        {
            children.reference(child.record);
        }
        out.templated(
            &class_of(COMPONENT_NODE)?,
            COMPONENT_NODE,
            &layout::base_feature_browser_component_node::TEMPLATE,
            &layout::base_feature_browser_component_node::UNSTATED,
            &[
                (
                    layout::base_feature_browser_component_node::AGGREGATE,
                    design(AGGREGATE),
                ),
                (
                    layout::base_feature_browser_component_node::NAMING_SPACE,
                    design(NAMING_SPACE),
                ),
                (
                    layout::base_feature_browser_component_node::RECORD_23,
                    design(RECORD_23),
                ),
                (
                    layout::base_feature_browser_component_node::ROOT_INSTANCE,
                    design(ROOT_INSTANCE),
                ),
                (
                    layout::base_feature_browser_component_node::ACT_INSTANCE_ROOT,
                    Bind::Cross {
                        target: super::component::ACT_INSTANCE_ROOT,
                        segment: SegmentKind::Act.id(),
                    },
                ),
                (
                    layout::base_feature_browser_component_node::DESIGN_AGGREGATE,
                    design(AGGREGATE),
                ),
                (
                    layout::base_feature_browser_component_node::DESIGN_NAMING_SPACE,
                    design(NAMING_SPACE),
                ),
                (
                    layout::base_feature_browser_component_node::DESIGN_RECORD_23,
                    design(RECORD_23),
                ),
                (
                    layout::base_feature_browser_component_node::CHILDREN,
                    Bind::Bytes(&children.bytes),
                ),
                (
                    layout::base_feature_browser_component_node::ROOT_INSTANCE_REPEAT,
                    design(ROOT_INSTANCE),
                ),
                (
                    layout::base_feature_browser_component_node::BROWSER_ROOT,
                    Bind::Ref(ROOT),
                ),
                (
                    layout::base_feature_browser_component_node::BROWSER_ROOT_REPEAT,
                    Bind::Ref(ROOT),
                ),
            ],
        )?;

        let mut pairs = DesignRecords::default();
        pairs.cross_segment_reference(u64::from(ROOT_INSTANCE), SegmentKind::Design.id());
        pairs.reference(COMPONENT_NODE);
        for entry in ENTRIES.iter().filter(|entry| entry.shown != 13) {
            pairs.cross_segment_reference(u64::from(entry.shown), SegmentKind::Design.id());
            pairs.reference(entry.record);
        }
        out.templated(
            &class_of(ROOT)?,
            ROOT,
            &layout::base_feature_browser_root_prefix::TEMPLATE,
            &layout::base_feature_browser_root_prefix::UNSTATED,
            &[
                (
                    layout::base_feature_browser_root_prefix::ROOT_INSTANCE,
                    design(ROOT_INSTANCE),
                ),
                (
                    layout::base_feature_browser_root_prefix::ENTRY_PAIRS,
                    Bind::Bytes(&pairs.bytes),
                ),
                (
                    layout::base_feature_browser_root_prefix::COMPONENT_NODE,
                    Bind::Ref(COMPONENT_NODE),
                ),
            ],
        )?;
        out.u32(2)
            .byte(0)
            .u32(1)
            .lp_ascii(DOCUMENT_STRUCTURE_KIND)?;
        out.reference(DOCUMENT_STRUCTURE)
            .cross_segment_reference(13, SegmentKind::Design.id());
        let relationships = ENTRIES
            .iter()
            .filter(|entry| entry.shown == 13)
            .collect::<Vec<_>>();
        out.u32(count(relationships.len())?);
        for entry in relationships {
            out.lp_ascii(entry.kind)?.reference(entry.record);
        }
        out.header(&table.class(DATA_MODEL_OBJECT)?, ROOT)
            .zeros(4)
            .lp_ascii(ROOT_KEY)?
            .byte(0);
        Ok(Self {
            table,
            records: out.sorted()?,
        })
    }

    pub(super) fn segment(&self) -> Segment<'_> {
        Segment {
            kind: SegmentKind::Browser,
            table: &self.table,
            records: &self.records,
            named: vec![ROOT],
            next_entity: u64::from(RECORDS),
        }
    }
}
