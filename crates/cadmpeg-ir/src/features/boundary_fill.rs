// SPDX-License-Identifier: Apache-2.0

use crate::features::selections::{BodySelection, SelectionReference};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;

/// A selected region of a boundary fill.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BoundaryFillCell {
    /// Cell selected through a body region.
    BodyRegion(BodySelection),
    /// Cell selected through a source record.
    Recorded {
        /// Source cell identity. Omitted for an empty source slot.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cell_record"
        )]
        record: Option<SelectionReference>,
    },
}

cadmpeg_core::named_optional_field!(deserialize_cell_record, SelectionReference, "record");
