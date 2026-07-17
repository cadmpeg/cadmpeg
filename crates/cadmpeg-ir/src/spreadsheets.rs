// SPDX-License-Identifier: Apache-2.0
//! Neutral spreadsheet structure and layout.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::{FeatureId, ParameterId};

/// Stable spreadsheet identity.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct SpreadsheetId(pub String);

/// One sheet and its ordered cell/layout state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Spreadsheet {
    /// Globally unique sheet id.
    pub id: SpreadsheetId,
    /// Feature-tree node owning this sheet.
    pub feature: FeatureId,
    /// Used cells in persistence order.
    pub cells: Vec<ParameterId>,
    /// Non-default column widths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub column_widths: Vec<SpreadsheetDimension>,
    /// Non-default row heights.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_heights: Vec<SpreadsheetDimension>,
    /// Merged rectangular ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged_ranges: Vec<SpreadsheetRange>,
    /// Full-fidelity source sheet record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One explicitly sized spreadsheet row or column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SpreadsheetDimension {
    /// Row number or column label exactly as persisted.
    pub name: String,
    /// Display size in source UI pixels.
    pub pixels: u32,
}

/// Inclusive rectangular spreadsheet range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SpreadsheetRange {
    /// Top-left cell address.
    pub start: String,
    /// Bottom-right cell address.
    pub end: String,
}
