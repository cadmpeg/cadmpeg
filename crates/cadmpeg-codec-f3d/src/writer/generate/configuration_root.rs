// SPDX-License-Identifier: Apache-2.0
//! The `DesignConfigurationRoot` record of a generated Design stream, which
//! names the row of the document's active configuration (cadir-v11.md
//! §5.11, f3d.md §1.2).

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::capacity::{TextCapacity, TextUnit};
use cadmpeg_ir::codec::write::{record_residual_unavailable, value_unwritable};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::presentation::{GeneratedDesignRegistry, TypeRow};
use super::records::{encode_configuration_root, EncodedDesignBulkStream};
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
/// losses of the active configurations it does not write as active.
pub(super) struct Plan {
    root: Option<ConfigurationRoot>,
    /// The record index of the source's root record.
    source: Option<u32>,
    losses: Vec<LossNote>,
}

/// The row the written root record names.
struct ConfigurationRoot {
    configuration: String,
    /// The position of the row's table document.
    document: usize,
    /// The position of the row's table in the document's `tables` array.
    table: usize,
    table_id: WrittenId,
    row_id: WrittenId,
}

/// An id as the document states it and as the record writes it.
struct WrittenId {
    stated: String,
    written: String,
}

impl WrittenId {
    fn new(stated: &str, written: &str) -> Self {
        Self {
            stated: stated.to_owned(),
            written: written.to_owned(),
        }
    }

    fn is_renamed(&self) -> bool {
        self.written != self.stated
    }
}

/// A candidate row of the root record.
struct Candidate<'a> {
    configuration: String,
    document: usize,
    table: &'a ConfigurationTable,
    row: &'a str,
}

/// Plan the root record of the configuration that `target` states active,
/// among the rows of the populated table of `documents`, the one table whose
/// rows the neutral configurations project.
///
/// A design scope's active configuration takes precedence over the row that
/// the earlier arena states active. A Design stream holds one root record, so
/// a second active row is written inactive.
pub(super) fn plan(target: &CadIr, documents: &[DesignConfiguration]) -> Plan {
    let scoped = target
        .design
        .scopes
        .iter()
        .filter_map(|scope| scope.active_configuration.as_ref())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    let flagged = target
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .map(|configuration| configuration.id.to_string())
        .collect::<BTreeSet<_>>();
    let rows = documents
        .iter()
        .enumerate()
        .flat_map(|(index, document)| {
            document
                .tables()
                .iter()
                .map(move |table| (index, document, table))
        })
        .find(|(_, _, table)| !table.rows.is_empty())
        .map(|(index, document, table)| {
            table
                .rows
                .iter()
                .map(|row| Candidate {
                    configuration: crate::ids::neutral_configuration_id(
                        document.entry_name(),
                        &row.id,
                    )
                    .to_string(),
                    document: index,
                    table,
                    row: &row.id,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
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
    let unheld = documents.iter().filter_map(|document| {
        let active = document.active_row()?;
        let held = document.tables().iter().any(|table| {
            table.id == active.table_id && table.rows.iter().any(|row| row.id == active.row_id)
        });
        (!held).then(|| {
            format!(
                "{} table id {} row id {}",
                document.id(),
                active.table_id,
                active.row_id
            )
        })
    });
    let mut losses = Vec::new();
    losses.extend(value_unwritable(
        "F3D",
        "writes no active row for these active configurations: no written configuration table holds their row",
        unmatched,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes no active row for these configuration table documents: their DesignConfigurationRoot record names a row that no table of the document holds",
        unheld,
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
    Plan {
        root,
        source: documents
            .iter()
            .find_map(DesignConfiguration::active_row)
            .map(|row| row.record_index),
        losses,
    }
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
    let other_tables = || {
        documents
            .iter()
            .enumerate()
            .flat_map(|(index, document)| document.tables().iter().map(move |table| (index, table)))
            .filter(|&(index, table)| (index, table.position) != (row.document, row.table.position))
    };
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
            let unique = unique(
                TABLE_ID,
                &row.table.id,
                other_tables().count(),
                |candidate| other_tables().any(|(_, table)| table.id == candidate),
            );
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
    let other_rows = || row.table.rows.iter().filter(|other| other.id != row.row);
    let written_row_id = if other_rows().any(|other| other.id == row_id.text) {
        let unique = unique(ROW_ID, row.row, other_rows().count(), |candidate| {
            other_rows().any(|other| other.id == candidate)
        });
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
    Some(ConfigurationRoot {
        table_id: WrittenId::new(&row.table.id, &written_table_id),
        row_id: WrittenId::new(row.row, &written_row_id),
        configuration: row.configuration,
        document: row.document,
        table: row.table.position,
    })
}

/// The first id `stated` fitted to `capacity` with the suffix `-n`, for `n`
/// from 1, that `taken` does not hold, among `others` taken ids.
fn unique(
    capacity: TextCapacity,
    stated: &str,
    others: usize,
    taken: impl Fn(&str) -> bool,
) -> String {
    (1..=others + 1)
        .map(|ordinal| {
            let suffix = format!("-{ordinal}");
            let width = capacity
                .width
                .checked_sub(suffix.len())
                .expect("an id capacity is wider than a decimal suffix");
            format!(
                "{}{suffix}",
                TextCapacity { width, ..capacity }.fit(stated).text
            )
        })
        .find(|candidate| !taken(candidate))
        .expect("one of `others + 1` distinct suffixed ids is free")
}

impl Plan {
    /// The bytes of `configuration`, the document at `index`, with the ids
    /// the root record cuts or renames written as the record writes them.
    pub(super) fn payload(
        &self,
        index: usize,
        configuration: &DesignConfiguration,
    ) -> Result<Vec<u8>, CodecError> {
        let Some(root) = self
            .root
            .as_ref()
            .filter(|root| root.document == index)
            .filter(|root| root.table_id.is_renamed() || root.row_id.is_renamed())
        else {
            return encode_configuration_payload(configuration);
        };
        let mut payload = configuration.payload().clone();
        rename_ids(
            &mut payload,
            root.table,
            (&root.table_id.stated, &root.table_id.written),
            (&root.row_id.stated, &root.row_id.written),
        );
        serde_json::to_vec(&payload).map_err(|error| {
            CodecError::malformed(format_args!(
                "cannot encode F3D configuration JSON {}: {error}",
                configuration.entry_name()
            ))
        })
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
                        "writes the MetaStream type row of the DesignConfigurationRoot record that names the active row of these configurations as the type {} the writer generates, with base type {}, version {} and add-in {}: f3d-open-items.md DR-1063 leaves that type row open",
                        ROOT_TYPE.guid,
                        ROOT_TYPE.base.unwrap_or_default(),
                        ROOT_TYPE.version,
                        ROOT_TYPE.module
                    ),
                    [root.configuration.as_str()],
                ));
                registry.register(ROOT_TYPE, &[record_index])?
            }
        };
        encode_configuration_root(
            bulk.get_or_insert_with(Default::default),
            &class_tag,
            record_index,
            &root.table_id.written,
            &root.row_id.written,
        )?;
        Ok(losses)
    }
}

#[cfg(test)]
mod tests;
