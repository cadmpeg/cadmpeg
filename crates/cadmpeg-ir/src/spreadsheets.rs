// SPDX-License-Identifier: Apache-2.0
//! Neutral spreadsheet structure and layout.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use crate::features::{FeatureId, ParameterId};

crate::ids::id_type!(
    /// Stable spreadsheet identity.
    SpreadsheetId
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
    address: String,
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
    pub cells: Vec<SpreadsheetCell>,
    /// Non-default column widths.
    pub column_widths: Vec<SpreadsheetDimension>,
    /// Non-default row heights.
    pub row_heights: Vec<SpreadsheetDimension>,
    /// Merged rectangular ranges.
    pub merged_ranges: Vec<SpreadsheetRange>,
    /// Full-fidelity source sheet record.
    pub native_ref: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpreadsheetWire {
    id: SpreadsheetId,
    feature: FeatureId,
    cells: Vec<SpreadsheetCell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    column_widths: Vec<SpreadsheetDimensionWire>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    row_heights: Vec<SpreadsheetDimensionWire>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    merged_ranges: Vec<SpreadsheetRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
        Ok(Self {
            id: wire.id,
            feature: wire.feature,
            cells: wire.cells,
            column_widths: wire
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
                .collect::<Result<_, String>>()?,
            row_heights: wire
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
                .collect::<Result<_, String>>()?,
            merged_ranges: wire.merged_ranges,
            native_ref: wire.native_ref,
        })
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
    start: String,
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

    use super::*;

    #[test]
    fn spreadsheet_round_trip_preserves_b2_address() {
        let sheet = Spreadsheet {
            id: SpreadsheetId::mint("synthetic:test:spreadsheet#sheet").unwrap(),
            feature: FeatureId::mint("synthetic:test:feature#feature").unwrap(),
            cells: vec![SpreadsheetCell {
                address: CellAddress::new(2, 2).unwrap(),
                parameter: ParameterId::mint("synthetic:test:parameter#parameter").unwrap(),
            }],
            column_widths: Vec::new(),
            row_heights: Vec::new(),
            merged_ranges: Vec::new(),
            native_ref: None,
        };
        let json = serde_json::to_string(&sheet).unwrap();
        let decoded: Spreadsheet = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, sheet);
    }
}
