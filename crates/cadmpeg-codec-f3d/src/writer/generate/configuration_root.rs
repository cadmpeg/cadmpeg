// SPDX-License-Identifier: Apache-2.0
//! The `DesignConfigurationRoot` record of a generated Design stream, which
//! names the row of the document's active configuration (cadir-v11.md
//! §5.11, f3d.md §1.2), and the ids it renames in the configuration table
//! documents written beside it.

use std::borrow::Cow;
use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::capacity::{TextCapacity, TextUnit};
use cadmpeg_ir::codec::write::{record_residual_unavailable, value_unwritable};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::DesignConfiguration as Configuration;
use cadmpeg_ir::report::loss::LossNote;

use super::configuration_table::Written;
use super::presentation::{GeneratedDesignRegistry, TypeRow};
use super::records::{encode_configuration_root, EncodedDesignBulkStream};
use crate::design::configurations::root_document;
use crate::records::configuration::table::{rename_ids, ConfigurationTable};
use crate::records::configuration::{encode_configuration_payload, DesignConfiguration, ID_WIDTH};

/// The type the record is registered under when the source registers none
/// for it (f3d-open-items.md DR-1063). The writer generates the type GUID,
/// `deterministic_guid("type", "DesignConfigurationRoot")`. The base type,
/// version and add-in are those of the configuration-table and
/// configuration-rule serializer triggers (f3d.md §3.1, "Records of a
/// sketch-and-extrude design").
const ROOT_TYPE: TypeRow = TypeRow {
    guid: "30429158-16B8-8973-B14B-E2137EDD5764",
    base: Some("130A0711-4E92-4FCD-AADE-B9C82238BB27"),
    version: 0,
    module: "Fusion",
};

/// The table id of the record: UTF-16 text of 1 to
/// [`ID_WIDTH`] code units (f3d.md §1.2).
pub(in crate::writer) const TABLE_ID: TextCapacity = TextCapacity::new(
    "DesignConfigurationRoot table id",
    TextUnit::Utf16CodeUnit,
    ID_WIDTH,
);

/// The row id of the record, of the width of [`TABLE_ID`].
pub(in crate::writer) const ROW_ID: TextCapacity = TextCapacity::new(
    "DesignConfigurationRoot row id",
    TextUnit::Utf16CodeUnit,
    ID_WIDTH,
);

/// The root record to write, the source's root record it replaces, and the
/// losses of what the write states otherwise than the configurations.
pub(super) struct Plan {
    root: Option<ConfigurationRoot>,
    /// The record index of the source's root record.
    source: Option<u32>,
    losses: Vec<LossNote>,
}

/// The row the written root record names.
struct ConfigurationRoot {
    /// The active configuration whose row the record names, or the
    /// configuration table document whose stated row it names.
    subject: String,
    table_id: String,
    row_id: String,
    /// The table whose ids the record writes renamed in its document.
    renamed: Option<RenamedTable>,
}

/// A table whose ids the root record writes renamed.
struct RenamedTable {
    /// The position of the table's document.
    document: usize,
    /// The position of the table in the document's `tables` array.
    table: usize,
    /// The table id as the document states it.
    table_id: String,
    /// The row id as the document states it.
    row_id: String,
}

/// A candidate row of the root record.
struct Candidate<'a> {
    configuration: String,
    document: usize,
    table: &'a ConfigurationTable,
    row: &'a str,
}

/// Plan the root record of the configuration of `configurations` that
/// `target` states active, among the rows of the table whose rows the
/// written documents project. With no such configuration, the record is the
/// one a `source` table document states, when that record names a row of a
/// table that the configurations do not project (f3d.md §1.2) and reads back
/// as the document's row. `projected` is the position of the document and of
/// the table whose rows `source` projects.
///
/// A design scope's active configuration takes precedence over the row that
/// the earlier arena states active. A Design stream holds one root record, so
/// a second active row is written inactive.
pub(super) fn plan(
    target: &CadIr,
    source: &[DesignConfiguration],
    projected: Option<(usize, usize)>,
    written: &Written<'_>,
    configurations: &[Configuration],
) -> Plan {
    let documents = written.documents.as_ref();
    let scoped = target
        .design
        .scopes
        .iter()
        .filter_map(|scope| scope.active_configuration.as_ref())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    let flagged = configurations
        .iter()
        .filter(|configuration| configuration.active)
        .map(|configuration| configuration.id.to_string())
        .collect::<BTreeSet<_>>();
    let rows = crate::design::configurations::table_at(documents, written.projection.table)
        .map(|(index, table)| {
            table.rows.iter().map(move |row| {
                let id =
                    crate::ids::neutral_configuration_id(documents[index].entry_name(), &row.id)
                        .to_string();
                Candidate {
                    configuration: written.generated.get(&id).cloned().unwrap_or(id),
                    document: index,
                    table,
                    row: &row.id,
                }
            })
        })
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut losses = Vec::new();
    let matched = rows
        .iter()
        .map(|row| row.configuration.clone())
        .collect::<BTreeSet<_>>();
    let covered = rows.iter().any(|row| scoped.contains(&row.configuration));
    let mut scope_rows = Vec::new();
    let mut flagged_rows = Vec::new();
    let mut overridden = Vec::new();
    for row in rows {
        match (
            scoped.contains(&row.configuration),
            flagged.contains(&row.configuration),
        ) {
            (true, _) => scope_rows.push(row),
            (false, true) if covered => overridden.push(row.configuration),
            (false, true) => flagged_rows.push(row),
            (false, false) => {}
        }
    }
    let mut candidates = scope_rows.into_iter().chain(flagged_rows);
    let first = candidates.next();
    let inactive = candidates.map(|row| row.configuration).collect::<Vec<_>>();
    let unmatched = scoped
        .iter()
        .chain(&flagged)
        .filter(|id| !matched.contains(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    losses.extend(value_unwritable(
        "F3D",
        "writes no active row for these active configurations: no written configuration table holds their row",
        unmatched,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these configurations inactive, which the earlier arena states active: a design scope states another row of their table active",
        overridden,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these active configurations inactive: the Design stream holds one DesignConfigurationRoot record, which names another row",
        inactive,
    ));
    let root = first.and_then(|row| root(row, documents, &mut losses));
    let root = stated_root(source, projected, root, &mut losses);
    Plan {
        root,
        source: source
            .iter()
            .find_map(DesignConfiguration::active_row)
            .map(|row| row.record_index),
        losses,
    }
}

/// `root`, else the root record that a table document states, when that
/// record names a row of a table that the configurations do not project
/// (f3d.md §1.2) and reads back as the document's row. A stated row of the
/// projected table is the configurations' own. Each other stated row that
/// the write drops is charged. `projected` is the position of the document
/// and of the table whose rows the configurations project.
fn stated_root(
    documents: &[DesignConfiguration],
    projected: Option<(usize, usize)>,
    mut root: Option<ConfigurationRoot>,
    losses: &mut Vec<LossNote>,
) -> Option<ConfigurationRoot> {
    let mut holds_rows = Vec::new();
    let mut displaced = Vec::new();
    let mut unread = Vec::new();
    for (index, document) in documents.iter().enumerate() {
        let Some(active) = document.active_row() else {
            continue;
        };
        let entry = format!(
            "{} table id {} row id {}",
            document.id(),
            active.table_id,
            active.row_id
        );
        let fits = |capacity: TextCapacity, id: &str| !id.is_empty() && !capacity.fit(id).was_cut();
        let configured = document.tables().iter().find(|table| {
            table.id == active.table_id && Some((index, table.position)) == projected
        });
        if let Some(table) = configured {
            if !table.rows.iter().any(|row| row.id == active.row_id) {
                holds_rows.push(entry);
            }
        } else if root.is_some() {
            displaced.push(entry);
        } else if !fits(TABLE_ID, &active.table_id)
            || !fits(ROW_ID, &active.row_id)
            || root_document(documents, &active.table_id) != Some(index)
        {
            unread.push(entry);
        } else {
            root = Some(ConfigurationRoot {
                subject: document.id(),
                table_id: active.table_id.clone(),
                row_id: active.row_id.clone(),
                renamed: None,
            });
        }
    }
    for (reason, entries) in [
        (
            "the configurations project the table of its table id, which holds other rows",
            holds_rows,
        ),
        (
            "the Design stream holds one DesignConfigurationRoot record, which names another row",
            displaced,
        ),
        (
            "a record of these ids does not read back as the row of the document",
            unread,
        ),
    ] {
        losses.extend(value_unwritable(
            "F3D",
            &format!("writes no DesignConfigurationRoot record that names the row these configuration table documents state active: {reason}"),
            entries,
        ));
    }
    root
}

/// The root record that names `row`, with its ids fitted to [`TABLE_ID`]
/// and [`ROW_ID`]; `None` when an id is empty.
///
/// The decoder binds the record to the one table document that holds its
/// table id (f3d.md §1.2), so a table id that another document holds is
/// written unique. A row id whose cut names another row of the table is
/// written unique.
fn root(
    row: Candidate<'_>,
    documents: &[DesignConfiguration],
    losses: &mut Vec<LossNote>,
) -> Option<ConfigurationRoot> {
    let table_id = TABLE_ID.fit(&row.table.id);
    let row_id = ROW_ID.fit(row.row);
    if table_id.text.is_empty() || row_id.text.is_empty() {
        losses.extend(value_unwritable(
            "F3D",
            &format!("writes no DesignConfigurationRoot record for this active configuration: the record stores its table id and row id as UTF-16 text of 1 to {ID_WIDTH} units, and an id is empty"),
            [format!(
                "{} (table id of {} UTF-16 units, row id of {})",
                row.configuration, table_id.stated_units, row_id.stated_units
            )],
        ));
        return None;
    }
    let holder = documents
        .iter()
        .enumerate()
        .find(|&(index, document)| {
            index != row.document
                && document
                    .tables()
                    .iter()
                    .any(|table| table.id == table_id.text)
        })
        .map(|(_, document)| document.id());
    let written_table_id = match holder {
        Some(holder) => {
            let taken = documents
                .iter()
                .enumerate()
                .flat_map(|(index, document)| {
                    document.tables().iter().map(move |table| (index, table))
                })
                .filter(|&(index, table)| {
                    (index, table.position) != (row.document, row.table.position)
                })
                .map(|(_, table)| table.id.as_str())
                .collect::<BTreeSet<_>>();
            let unique = unique(&row.table.id, &taken);
            losses.extend(value_unwritable(
                "F3D",
                "writes the table id of this active configuration renamed, in the DesignConfigurationRoot record and in its configuration table document: another configuration table document holds the id, and the record names its table by id alone",
                [format!(
                    "{} table id {} written as {unique}, held by {holder}",
                    row.configuration, row.table.id
                )],
            ));
            unique
        }
        None => table_id.text.to_owned(),
    };
    let taken = row
        .table
        .rows
        .iter()
        .filter(|other| other.id != row.row)
        .map(|other| other.id.as_str())
        .collect::<BTreeSet<_>>();
    let written_row_id = if taken.contains(row_id.text) {
        let unique = unique(row.row, &taken);
        losses.extend(value_unwritable(
            "F3D",
            &format!("writes the row id of this active configuration renamed, in the DesignConfigurationRoot record and in its configuration table document: its cut to {ID_WIDTH} UTF-16 units names another row of its table"),
            [format!(
                "{} row id written as {unique}",
                row.configuration
            )],
        ));
        unique
    } else {
        row_id.text.to_owned()
    };
    losses.extend(value_unwritable(
        "F3D",
        &format!("writes the ids of this active configuration cut at a character boundary to {ID_WIDTH} UTF-16 units, in the DesignConfigurationRoot record and in its configuration table document"),
        [
            (written_table_id == table_id.text)
                .then(|| table_id.unwritable_entry(&row.configuration))
                .flatten(),
            (written_row_id == row_id.text)
                .then(|| row_id.unwritable_entry(&row.configuration))
                .flatten(),
        ]
        .into_iter()
        .flatten(),
    ));
    let renamed =
        (written_table_id != row.table.id || written_row_id != row.row).then(|| RenamedTable {
            document: row.document,
            table: row.table.position,
            table_id: row.table.id.clone(),
            row_id: row.row.to_owned(),
        });
    Some(ConfigurationRoot {
        subject: row.configuration,
        table_id: written_table_id,
        row_id: written_row_id,
        renamed,
    })
}

/// The longest suffix [`unique`] appends, of the largest `usize`.
const LONGEST_SUFFIX: &str = "-18446744073709551615";
const _: () = assert!(ID_WIDTH > LONGEST_SUFFIX.len());

/// The first id `stated` fitted to [`ID_WIDTH`] UTF-16 units with the suffix
/// `-n`, for `n` from 1, that `taken` does not hold. The text after the last
/// `-` of a candidate is `n`, so the candidates are distinct, and one of the
/// first `taken.len() + 1` is free.
fn unique(stated: &str, taken: &BTreeSet<&str>) -> String {
    let candidate = |ordinal: usize| {
        let suffix = format!("-{ordinal}");
        let capacity = TextCapacity::new(
            "DesignConfigurationRoot id",
            TextUnit::Utf16CodeUnit,
            ID_WIDTH - suffix.len(),
        );
        format!("{}{suffix}", capacity.fit(stated).text)
    };
    (1..=taken.len())
        .map(candidate)
        .find(|candidate| !taken.contains(candidate.as_str()))
        .unwrap_or_else(|| candidate(taken.len() + 1))
}

impl Plan {
    /// The bytes of `configuration`, the written document at `index`, with
    /// the ids the root record cuts or renames written as the record writes
    /// them.
    pub(super) fn payload(
        &self,
        index: usize,
        configuration: &DesignConfiguration,
    ) -> Result<Vec<u8>, CodecError> {
        let renamed = self
            .root
            .as_ref()
            .and_then(|root| Some((root, root.renamed.as_ref()?)))
            .filter(|(_, renamed)| renamed.document == index);
        let mut payload = Cow::Borrowed(configuration.payload());
        if let Some((root, renamed)) = renamed {
            rename_ids(
                payload.to_mut(),
                renamed.table,
                (&renamed.table_id, &root.table_id),
                (&renamed.row_id, &root.row_id),
            );
        }
        encode_configuration_payload(configuration.entry_name(), &payload)
    }

    /// Append the root record to `bulk` under the source's type of the
    /// record, else under [`ROOT_TYPE`]; return the plan's losses and the
    /// loss of a type the source does not state.
    pub(super) fn write(
        self,
        registry: &mut GeneratedDesignRegistry,
        bulk: &mut Option<EncodedDesignBulkStream>,
    ) -> Result<Vec<LossNote>, CodecError> {
        let mut losses = self.losses;
        let Some(root) = self.root else {
            return Ok(losses);
        };
        let record_index = registry.allocate()?;
        let stated = match self.source {
            Some(source) => registry.register_in_place_of(source, record_index, ROOT_TYPE.guid)?,
            None => None,
        };
        let class_tag = match stated {
            Some(class_tag) => class_tag,
            None => {
                losses.extend(record_residual_unavailable(
                    "F3D",
                    &format!(
                        "writes the MetaStream type row of the DesignConfigurationRoot record that names the active row of these configurations or configuration table documents as the type {} the writer generates, with base type {}, version {} and add-in {}: f3d-open-items.md DR-1063 leaves that type row open",
                        ROOT_TYPE.guid,
                        ROOT_TYPE.base.unwrap_or_default(),
                        ROOT_TYPE.version,
                        ROOT_TYPE.module
                    ),
                    [root.subject.as_str()],
                ));
                registry.register(ROOT_TYPE, &[record_index])?
            }
        };
        encode_configuration_root(
            bulk.get_or_insert_with(Default::default),
            &class_tag,
            record_index,
            &root.table_id,
            &root.row_id,
        )?;
        Ok(losses)
    }
}

#[cfg(test)]
mod tests;
