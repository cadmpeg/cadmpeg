// SPDX-License-Identifier: Apache-2.0
//! Spreadsheets: the sheets that own some of a scope's parameters.
//!
//! A sheet is a feature whose operation is `Spreadsheet(s)`, naming its one
//! record `s` here. Each used cell holds its address and the parameter that
//! holds the cell's content as its expression.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

use crate::spreadsheets::serialization::Dimensions;
use crate::spreadsheets::{
    check_layout, column_widths, row_heights, SpreadsheetCell, SpreadsheetDimension,
    SpreadsheetDimensionWire, SpreadsheetId, SpreadsheetRange,
};

/// One sheet and its ordered cell and layout state. The feature whose
/// operation names the sheet owns it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "SpreadsheetWire", into = "SpreadsheetWire")]
pub struct Spreadsheet {
    /// The sheet's identity.
    pub id: SpreadsheetId,
    cells: Vec<SpreadsheetCell>,
    column_widths: Vec<SpreadsheetDimension>,
    row_heights: Vec<SpreadsheetDimension>,
    merged_ranges: Vec<SpreadsheetRange>,
    /// The sheet's native record.
    pub native_ref: Option<String>,
}

impl Spreadsheet {
    /// Builds a sheet with distinct cells and dimensions and disjoint merged
    /// ranges, each starting at a cell.
    ///
    /// # Errors
    ///
    /// Names the repeated cell, address, or index, or the merged range that
    /// starts at no cell or overlaps another.
    pub fn new(
        id: SpreadsheetId,
        cells: Vec<SpreadsheetCell>,
        column_widths: Vec<SpreadsheetDimension>,
        row_heights: Vec<SpreadsheetDimension>,
        merged_ranges: Vec<SpreadsheetRange>,
        native_ref: Option<String>,
    ) -> Result<Self, String> {
        check_layout(&cells, &column_widths, &row_heights, &merged_ranges)?;
        Ok(Self {
            id,
            cells,
            column_widths,
            row_heights,
            merged_ranges,
            native_ref,
        })
    }

    /// The used cells, in persistence order.
    #[must_use]
    pub fn cells(&self) -> &[SpreadsheetCell] {
        &self.cells
    }

    /// The column widths that differ from the default.
    #[must_use]
    pub fn column_widths(&self) -> &[SpreadsheetDimension] {
        &self.column_widths
    }

    /// The row heights that differ from the default.
    #[must_use]
    pub fn row_heights(&self) -> &[SpreadsheetDimension] {
        &self.row_heights
    }

    /// The merged rectangular ranges.
    #[must_use]
    pub fn merged_ranges(&self) -> &[SpreadsheetRange] {
        &self.merged_ranges
    }
}

impl Serialize for Spreadsheet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let length = 5 + usize::from(self.native_ref.is_some());
        let mut wire = serializer.serialize_struct("Spreadsheet", length)?;
        wire.serialize_field("id", &self.id)?;
        wire.serialize_field("cells", &self.cells)?;
        wire.serialize_field("column_widths", &Dimensions::columns(&self.column_widths))?;
        wire.serialize_field("row_heights", &Dimensions::rows(&self.row_heights))?;
        wire.serialize_field("merged_ranges", &self.merged_ranges)?;
        if let Some(native_ref) = &self.native_ref {
            wire.serialize_field("native_ref", native_ref)?;
        }
        wire.end()
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "Spreadsheet"))]
#[serde(deny_unknown_fields)]
struct SpreadsheetWire {
    /// The sheet's identity.
    id: SpreadsheetId,
    /// The used cells, in persistence order.
    cells: Vec<SpreadsheetCell>,
    /// The column widths that differ from the default, each named by its
    /// column letters.
    column_widths: Vec<SpreadsheetDimensionWire>,
    /// The row heights that differ from the default, each named by its row
    /// number.
    row_heights: Vec<SpreadsheetDimensionWire>,
    /// The merged rectangular ranges.
    merged_ranges: Vec<SpreadsheetRange>,
    /// The sheet's native record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    native_ref: Option<String>,
}

impl TryFrom<SpreadsheetWire> for Spreadsheet {
    type Error = String;

    fn try_from(wire: SpreadsheetWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.id,
            wire.cells,
            column_widths(wire.column_widths)?,
            row_heights(wire.row_heights)?,
            wire.merged_ranges,
            wire.native_ref,
        )
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Spreadsheet {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Spreadsheet".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::Spreadsheet").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SpreadsheetWire::json_schema(generator)
    }
}

cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
