// SPDX-License-Identifier: Apache-2.0
//! The `DesignConfigurationRoot` record of a generated Design stream, which
//! names the row of the document's active configuration (cadir-v11.md
//! §5.11, f3d.md §1.2).

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::value_unwritable;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::presentation::{GeneratedDesignRegistry, TypeRow};
use super::records::{encode_configuration_root, EncodedDesignBulkStream};
use crate::records::configuration::DesignConfiguration;

/// The type the record is registered under when the source registers none
/// for it (f3d-open-items.md DR-1063).
const ROOT_TYPE: TypeRow = TypeRow {
    guid: "30429158-16B8-8973-B14B-E2137EDD5764",
    base: None,
    version: 0,
    module: "Fusion",
};

/// The row the written root record names.
pub(super) struct ConfigurationRoot {
    configuration: String,
    table_id: String,
    row_id: String,
    /// The record index of the source's root record.
    source: Option<u32>,
}

/// The root record of the configuration that `target` states active, among
/// the rows of the table documents in `documents`, and the losses of the
/// active configurations it does not write as active.
///
/// The active configurations are the design scopes' active configurations,
/// or, where no scope names one, the earlier arena's active rows. A Design
/// stream holds one root record, so a second active row is written
/// inactive.
pub(super) fn plan(
    target: &CadIr,
    documents: &[DesignConfiguration],
) -> (Option<ConfigurationRoot>, Vec<LossNote>) {
    let mut active = target
        .design
        .scopes
        .iter()
        .filter_map(|scope| scope.active_configuration.as_ref())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    if active.is_empty() {
        active = target
            .model
            .configurations
            .iter()
            .filter(|configuration| configuration.active)
            .map(|configuration| configuration.id.to_string())
            .collect();
    }
    let source = documents
        .iter()
        .find_map(DesignConfiguration::active_row)
        .map(|row| row.record_index);
    let mut rows = documents
        .iter()
        .flat_map(|document| {
            document.tables().iter().flat_map(move |table| {
                table.rows.iter().map(move |row| ConfigurationRoot {
                    configuration: crate::ids::neutral_configuration_id(
                        document.entry_name(),
                        &row.id,
                    )
                    .to_string(),
                    table_id: table.id.clone(),
                    row_id: row.id.clone(),
                    source,
                })
            })
        })
        .filter(|root| active.contains(&root.configuration));
    let root = rows.next();
    let others = rows.map(|root| root.configuration).collect::<Vec<_>>();
    let unmatched = active
        .into_iter()
        .filter(|id| {
            root.as_ref().is_none_or(|root| &root.configuration != id) && !others.contains(id)
        })
        .collect::<Vec<_>>();
    let losses = value_unwritable(
        "F3D",
        "writes no active row for these active configurations: no written configuration table holds their row",
        unmatched,
    )
    .into_iter()
    .chain(value_unwritable(
        "F3D",
        "writes these active configurations inactive: the Design stream holds one DesignConfigurationRoot record, which names another row",
        others,
    ))
    .collect();
    (root, losses)
}

impl ConfigurationRoot {
    /// Append the record to `bulk` under the source's type of the record,
    /// else under [`ROOT_TYPE`], and return the loss of a type the source
    /// does not state.
    pub(super) fn write(
        &self,
        registry: &mut GeneratedDesignRegistry,
        bulk: &mut EncodedDesignBulkStream,
    ) -> Result<Option<LossNote>, CodecError> {
        let record_index = registry.allocate()?;
        let stated = match self.source {
            Some(source) => registry.register_in_place_of(source, record_index)?,
            None => None,
        };
        let (class_tag, loss) = match stated {
            Some(class_tag) => (class_tag, None),
            None => (
                registry.register(ROOT_TYPE, &[record_index])?,
                value_unwritable(
                    "F3D",
                    &format!(
                        "writes the MetaStream type row of the DesignConfigurationRoot record that names the active row of these configurations as type {}, with no base type, version {} and add-in {}: f3d-open-items.md DR-1063 leaves that type row open",
                        ROOT_TYPE.guid, ROOT_TYPE.version, ROOT_TYPE.module
                    ),
                    [self.configuration.as_str()],
                ),
            ),
        };
        encode_configuration_root(bulk, &class_tag, record_index, &self.table_id, &self.row_id)?;
        Ok(loss)
    }
}

#[cfg(test)]
mod tests;
