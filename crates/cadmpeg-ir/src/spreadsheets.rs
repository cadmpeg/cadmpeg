// SPDX-License-Identifier: Apache-2.0
//! Neutral spreadsheet structure and layout.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::num::NonZeroU32;

use crate::features::{FeatureId, ParameterId};

crate::ids::id_type!(
    /// Stable spreadsheet identity.
    SpreadsheetId, compose
);

/// One used spreadsheet cell and its A1 address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpreadsheetCellWire", into = "SpreadsheetCellWire")]
pub struct SpreadsheetCell {
    /// One-based row and column.
    pub address: CellAddress,
    /// Parameter that stores the cell expression and value.
    pub parameter: ParameterId,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpreadsheetCellWire {
    /// One-based row and column.
    address: String,
    /// Parameter that stores the cell expression and value.
    parameter: ParameterId,
}

impl From<SpreadsheetCell> for SpreadsheetCellWire {
    fn from(cell: SpreadsheetCell) -> Self {
        Self {
            address: cell.address.a1(),
            parameter: cell.parameter,
        }
    }
}

impl TryFrom<SpreadsheetCellWire> for SpreadsheetCell {
    type Error = String;

    fn try_from(wire: SpreadsheetCellWire) -> Result<Self, Self::Error> {
        let address = CellAddress::parse(&wire.address)
            .ok_or_else(|| format!("invalid cell address {}", wire.address))?;
        Ok(Self {
            address,
            parameter: wire.parameter,
        })
    }
}

/// One-based spreadsheet coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CellAddress {
    row: u32,
    col: u32,
}

impl CellAddress {
    /// Build a one-based address.
    #[must_use]
    pub fn new(row: u32, col: u32) -> Option<Self> {
        (row > 0 && col > 0).then_some(Self { row, col })
    }

    /// Parse an A1 address such as `B12`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let split = value.find(|character: char| character.is_ascii_digit())?;
        let col = column_index(&value[..split])?;
        let row = value[split..].parse::<u32>().ok()?;
        Self::new(row, col)
    }

    /// One-based row number.
    #[must_use]
    pub const fn row(self) -> u32 {
        self.row
    }

    /// One-based column number.
    #[must_use]
    pub const fn col(self) -> u32 {
        self.col
    }

    /// A1 spelling of this address.
    #[must_use]
    pub fn a1(self) -> String {
        format!("{}{}", column_label(self.col), self.row)
    }
}

/// One sheet and its ordered cell/layout state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SpreadsheetWire", into = "SpreadsheetWire")]
pub struct Spreadsheet {
    /// Globally unique sheet id.
    pub id: SpreadsheetId,
    /// Feature-tree node owning this sheet.
    pub feature: FeatureId,
    /// Used cells in persistence order.
    cells: Vec<SpreadsheetCell>,
    /// Non-default column widths.
    column_widths: Vec<SpreadsheetDimension>,
    /// Non-default row heights.
    row_heights: Vec<SpreadsheetDimension>,
    /// Merged rectangular ranges.
    merged_ranges: Vec<SpreadsheetRange>,
    /// Full-fidelity source sheet record.
    pub native_ref: Option<String>,
}

impl Spreadsheet {
    /// Build a sheet with distinct cells and dimensions and disjoint merged ranges.
    pub fn new(
        id: SpreadsheetId,
        feature: FeatureId,
        cells: Vec<SpreadsheetCell>,
        column_widths: Vec<SpreadsheetDimension>,
        row_heights: Vec<SpreadsheetDimension>,
        merged_ranges: Vec<SpreadsheetRange>,
        native_ref: Option<String>,
    ) -> Result<Self, String> {
        let mut parameters = HashSet::new();
        let mut addresses = HashSet::new();
        for cell in &cells {
            if !parameters.insert(&cell.parameter) {
                return Err("spreadsheet repeats a cell identity".into());
            }
            if !addresses.insert(cell.address) {
                return Err("spreadsheet cell address is repeated".into());
            }
        }
        for (name, dimensions) in [
            ("column_widths", &column_widths),
            ("row_heights", &row_heights),
        ] {
            let mut indices = HashSet::new();
            for dimension in dimensions {
                if !indices.insert(dimension.index) {
                    return Err(format!("{name} repeats an index"));
                }
            }
        }
        for (index, range) in merged_ranges.iter().enumerate() {
            if !addresses.contains(&range.start()) {
                return Err("merged range start has no cell".into());
            }
            if merged_ranges[..index]
                .iter()
                .any(|other| ranges_overlap(other, range))
            {
                return Err("merged ranges overlap".into());
            }
        }
        Ok(Self {
            id,
            feature,
            cells,
            column_widths,
            row_heights,
            merged_ranges,
            native_ref,
        })
    }

    /// Used cells in persistence order.
    #[must_use]
    pub fn cells(&self) -> &[SpreadsheetCell] {
        &self.cells
    }

    /// Non-default column widths.
    #[must_use]
    pub fn column_widths(&self) -> &[SpreadsheetDimension] {
        &self.column_widths
    }

    /// Non-default row heights.
    #[must_use]
    pub fn row_heights(&self) -> &[SpreadsheetDimension] {
        &self.row_heights
    }

    /// Merged rectangular ranges.
    #[must_use]
    pub fn merged_ranges(&self) -> &[SpreadsheetRange] {
        &self.merged_ranges
    }
}

fn ranges_overlap(left: &SpreadsheetRange, right: &SpreadsheetRange) -> bool {
    left.start().row() <= right.end().row()
        && right.start().row() <= left.end().row()
        && left.start().col() <= right.end().col()
        && right.start().col() <= left.end().col()
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpreadsheetWire {
    /// Globally unique sheet id.
    id: SpreadsheetId,
    /// Feature-tree node owning this sheet.
    feature: FeatureId,
    /// Used cells in persistence order.
    cells: Vec<SpreadsheetCell>,
    /// Non-default column widths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    column_widths: Vec<SpreadsheetDimensionWire>,
    /// Non-default row heights.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    row_heights: Vec<SpreadsheetDimensionWire>,
    /// Merged rectangular ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    merged_ranges: Vec<SpreadsheetRange>,
    /// Full-fidelity source sheet record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    native_ref: Option<String>,
}

impl From<Spreadsheet> for SpreadsheetWire {
    fn from(sheet: Spreadsheet) -> Self {
        Self {
            id: sheet.id,
            feature: sheet.feature,
            cells: sheet.cells,
            column_widths: sheet
                .column_widths
                .into_iter()
                .map(|dimension| SpreadsheetDimensionWire {
                    name: column_label(dimension.index.get()),
                    pixels: dimension.pixels,
                })
                .collect(),
            row_heights: sheet
                .row_heights
                .into_iter()
                .map(|dimension| SpreadsheetDimensionWire {
                    name: dimension.index.to_string(),
                    pixels: dimension.pixels,
                })
                .collect(),
            merged_ranges: sheet.merged_ranges,
            native_ref: sheet.native_ref,
        }
    }
}

impl TryFrom<SpreadsheetWire> for Spreadsheet {
    type Error = String;

    fn try_from(wire: SpreadsheetWire) -> Result<Self, Self::Error> {
        let column_widths = wire
            .column_widths
            .into_iter()
            .map(|wire| {
                let index = column_index(&wire.name)
                    .and_then(NonZeroU32::new)
                    .ok_or_else(|| format!("column_widths name is invalid: {}", wire.name))?;
                Ok(SpreadsheetDimension {
                    index,
                    pixels: wire.pixels,
                })
            })
            .collect::<Result<_, String>>()?;
        let row_heights = wire
            .row_heights
            .into_iter()
            .map(|wire| {
                let index = wire
                    .name
                    .parse::<NonZeroU32>()
                    .map_err(|_| format!("row_heights name is invalid: {}", wire.name))?;
                Ok(SpreadsheetDimension {
                    index,
                    pixels: wire.pixels,
                })
            })
            .collect::<Result<_, String>>()?;
        Self::new(
            wire.id,
            wire.feature,
            wire.cells,
            column_widths,
            row_heights,
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

/// One explicitly sized spreadsheet row or column.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpreadsheetDimension {
    /// One-based row or column index.
    pub index: NonZeroU32,
    /// Display size in source UI pixels.
    pub pixels: u32,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpreadsheetDimensionWire {
    name: String,
    pixels: u32,
}

/// Inclusive rectangular spreadsheet range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpreadsheetRangeWire", into = "SpreadsheetRangeWire")]
pub struct SpreadsheetRange {
    start: CellAddress,
    end: CellAddress,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpreadsheetRangeWire {
    /// Top-left cell.
    start: String,
    /// Bottom-right cell.
    end: String,
}

impl SpreadsheetRange {
    /// Build a range whose start is strictly upper-left of end.
    pub fn new(start: CellAddress, end: CellAddress) -> Option<Self> {
        (start.row <= end.row && start.col <= end.col && start != end)
            .then_some(Self { start, end })
    }

    /// Top-left cell.
    #[must_use]
    pub const fn start(&self) -> CellAddress {
        self.start
    }

    /// Bottom-right cell.
    #[must_use]
    pub const fn end(&self) -> CellAddress {
        self.end
    }

    /// Whether `address` lies inside this inclusive range.
    #[must_use]
    pub fn contains(&self, address: CellAddress) -> bool {
        (self.start.row..=self.end.row).contains(&address.row)
            && (self.start.col..=self.end.col).contains(&address.col)
    }
}

impl From<SpreadsheetRange> for SpreadsheetRangeWire {
    fn from(range: SpreadsheetRange) -> Self {
        Self {
            start: range.start.a1(),
            end: range.end.a1(),
        }
    }
}

impl TryFrom<SpreadsheetRangeWire> for SpreadsheetRange {
    type Error = String;

    fn try_from(wire: SpreadsheetRangeWire) -> Result<Self, Self::Error> {
        let start = CellAddress::parse(&wire.start)
            .ok_or_else(|| format!("invalid merged range start {}", wire.start))?;
        let end = CellAddress::parse(&wire.end)
            .ok_or_else(|| format!("invalid merged range end {}", wire.end))?;
        Self::new(start, end).ok_or_else(|| {
            format!(
                "merged range {}..{} is empty or reversed",
                wire.start, wire.end
            )
        })
    }
}

fn column_index(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    value.bytes().try_fold(0_u32, |index, byte| {
        index
            .checked_mul(26)?
            .checked_add(u32::from(byte - b'A' + 1))
    })
}

fn column_label(mut column: u32) -> String {
    let mut label = Vec::new();
    while column > 0 {
        column -= 1;
        label.push(b'A' + (column % 26) as u8);
        column /= 26;
    }
    label.reverse();
    String::from_utf8(label).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn spreadsheet_wire_rejects_invalid_dimensions_without_dropping_them() {
        for (field, names) in [
            ("column_widths", vec!["", "0", "a", "A1", "ZZZZZZZZZZ"]),
            ("row_heights", vec!["", "0", "-1", "A", "4294967296"]),
        ] {
            for name in names {
                let mut value = serde_json::json!({"id": "synthetic:test:spreadsheet#sheet", "feature": "synthetic:test:feature#feature", "cells": []});
                value[field] = serde_json::json!([{"name": name, "pixels": 10}]);
                let error =
                    serde_json::from_value::<Spreadsheet>(value).expect_err("invalid dimension");
                assert!(error.to_string().contains(field));
            }
        }
        let value = serde_json::json!({"id": "synthetic:test:spreadsheet#sheet", "feature": "synthetic:test:feature#feature", "cells": [], "column_widths": [{"name": "AA", "pixels": 0}], "row_heights": [{"name": "4294967295", "pixels": 0}]});
        let sheet: Spreadsheet = serde_json::from_value(value.clone()).expect("valid dimensions");
        assert_eq!(sheet.column_widths[0].index.get(), 27);
        assert_eq!(sheet.row_heights[0].index.get(), u32::MAX);
        assert_eq!(
            serde_json::to_value(sheet).expect("serialize dimensions"),
            value
        );
    }

    use super::{CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetId};
    use crate::features::{FeatureId, ParameterId};

    #[test]
    fn spreadsheet_round_trip_preserves_b2_address() {
        let sheet = Spreadsheet::new(
            SpreadsheetId::mint("synthetic:test:spreadsheet#sheet").unwrap(),
            FeatureId::mint("synthetic:test:feature#feature").unwrap(),
            vec![SpreadsheetCell {
                address: CellAddress::new(2, 2).unwrap(),
                parameter: ParameterId::mint("synthetic:test:parameter#parameter").unwrap(),
            }],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
        let json = serde_json::to_string(&sheet).unwrap();
        let decoded: Spreadsheet = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, sheet);
    }

    fn base_sheet() -> serde_json::Value {
        serde_json::json!({
            "id": "synthetic:test:spreadsheet#sheet",
            "feature": "synthetic:test:feature#feature",
            "cells": [
                {"address": "A1", "parameter": "synthetic:test:parameter#one"},
                {"address": "B1", "parameter": "synthetic:test:parameter#two"}
            ]
        })
    }

    fn rejects(value: serde_json::Value, message: &str) {
        let error = serde_json::from_value::<Spreadsheet>(value).expect_err(message);
        assert!(error.to_string().contains(message), "{error}");
    }

    #[test]
    fn spreadsheet_wire_rejects_duplicate_cell_identity() {
        let mut value = base_sheet();
        value["cells"][1]["parameter"] = value["cells"][0]["parameter"].clone();
        rejects(value, "spreadsheet repeats a cell identity");
    }

    #[test]
    fn spreadsheet_wire_rejects_duplicate_cell_address() {
        let mut value = base_sheet();
        value["cells"][1]["address"] = value["cells"][0]["address"].clone();
        rejects(value, "spreadsheet cell address is repeated");
    }

    #[test]
    fn spreadsheet_wire_rejects_duplicate_column_index() {
        let mut value = base_sheet();
        value["column_widths"] = serde_json::json!([
            {"name": "A", "pixels": 10}, {"name": "A", "pixels": 20}
        ]);
        rejects(value, "column_widths repeats an index");
    }

    #[test]
    fn spreadsheet_wire_rejects_duplicate_row_index() {
        let mut value = base_sheet();
        value["row_heights"] = serde_json::json!([
            {"name": "1", "pixels": 10}, {"name": "1", "pixels": 20}
        ]);
        rejects(value, "row_heights repeats an index");
    }

    #[test]
    fn spreadsheet_wire_rejects_missing_merge_start_cell() {
        let mut value = base_sheet();
        value["merged_ranges"] = serde_json::json!([
            {"start": "C1", "end": "D1"}
        ]);
        rejects(value, "merged range start has no cell");
    }

    #[test]
    fn spreadsheet_wire_rejects_overlapping_merges() {
        let mut value = base_sheet();
        value["merged_ranges"] = serde_json::json!([
            {"start": "A1", "end": "B2"},
            {"start": "B1", "end": "C2"}
        ]);
        rejects(value, "merged ranges overlap");
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
