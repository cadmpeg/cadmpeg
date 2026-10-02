// SPDX-License-Identifier: Apache-2.0
//! Borrow spreadsheet rows and format bounded coordinates through the serializer.

use super::{CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetDimension, SpreadsheetRange};
use crate::features::ParameterId;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

struct ColumnLabel(u32);

impl std::fmt::Display for ColumnLabel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut column = self.0;
        let mut digits = [b'A'; 7];
        let mut start = digits.len();
        while column > 0 {
            column -= 1;
            start = start.checked_sub(1).ok_or(std::fmt::Error)?;
            digits[start] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"[cadmpeg_core::decode::index_from_u32(column % 26)];
            column /= 26;
        }
        formatter.write_str(std::str::from_utf8(&digits[start..]).map_err(|_| std::fmt::Error)?)
    }
}

impl std::fmt::Display for CellAddress {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{}{}", ColumnLabel(self.col), self.row) }
}

impl Serialize for CellAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.collect_str(self) }
}

impl Serialize for SpreadsheetCell {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> { address: CellAddress, parameter: &'a ParameterId }
        Wire { address: self.address, parameter: &self.parameter }.serialize(serializer)
    }
}

impl Serialize for SpreadsheetRange {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire { start: CellAddress, end: CellAddress }
        Wire { start: self.start, end: self.end }.serialize(serializer)
    }
}

struct Dimensions<'a> { dimensions: &'a [SpreadsheetDimension], columns: bool }

struct DimensionName { index: u32, column: bool }

impl std::fmt::Display for DimensionName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.column { std::fmt::Display::fmt(&ColumnLabel(self.index), formatter) } else { std::fmt::Display::fmt(&self.index, formatter) }
    }
}

impl Serialize for DimensionName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.collect_str(self) }
}

impl Serialize for Dimensions<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire { name: DimensionName, pixels: u32 }
        let mut sequence = serializer.serialize_seq(None)?;
        for dimension in self.dimensions {
            sequence.serialize_element(&Wire { name: DimensionName { index: dimension.index.get(), column: self.columns }, pixels: dimension.pixels })?;
        }
        sequence.end()
    }
}

impl Serialize for Spreadsheet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let length = 3 + usize::from(!self.column_widths.is_empty()) + usize::from(!self.row_heights.is_empty()) + usize::from(!self.merged_ranges.is_empty()) + usize::from(self.native_ref.is_some());
        let mut wire = serializer.serialize_struct("Spreadsheet", length)?;
        wire.serialize_field("id", &self.id)?;
        wire.serialize_field("feature", &self.feature)?;
        wire.serialize_field("cells", &self.cells)?;
        if !self.column_widths.is_empty() { wire.serialize_field("column_widths", &Dimensions { dimensions: &self.column_widths, columns: true })?; }
        if !self.row_heights.is_empty() { wire.serialize_field("row_heights", &Dimensions { dimensions: &self.row_heights, columns: false })?; }
        if !self.merged_ranges.is_empty() { wire.serialize_field("merged_ranges", &self.merged_ranges)?; }
        if let Some(native_ref) = &self.native_ref { wire.serialize_field("native_ref", native_ref)?; }
        wire.end()
    }
}
