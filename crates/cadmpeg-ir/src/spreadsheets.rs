// SPDX-License-Identifier: Apache-2.0
//! Neutral spreadsheet structure and layout.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::{FeatureId, ParameterId};

/// Stable spreadsheet identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SpreadsheetId(
    #[serde(serialize_with = "crate::schema::serialize_reference_id")] pub String,
);

/// One used spreadsheet cell and its A1 address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpreadsheetCell {
    /// One-based row and column.
    pub address: CellAddress,
    /// Parameter that stores the cell expression and value.
    pub parameter: ParameterId,
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
#[serde(from = "SpreadsheetWire", into = "SpreadsheetWire")]
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
struct SpreadsheetWire {
    id: SpreadsheetId,
    feature: FeatureId,
    cells: Vec<ParameterId>,
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
            cells: sheet.cells.into_iter().map(|cell| cell.parameter).collect(),
            column_widths: sheet
                .column_widths
                .into_iter()
                .map(|dimension| SpreadsheetDimensionWire {
                    name: column_label(dimension.index),
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

impl From<SpreadsheetWire> for Spreadsheet {
    fn from(wire: SpreadsheetWire) -> Self {
        Self {
            id: wire.id,
            feature: wire.feature,
            cells: wire
                .cells
                .into_iter()
                .map(|parameter| SpreadsheetCell {
                    address: CellAddress::new(1, 1).expect("one-based origin"),
                    parameter,
                })
                .collect(),
            column_widths: wire
                .column_widths
                .into_iter()
                .filter_map(|wire| {
                    column_index(&wire.name).map(|index| SpreadsheetDimension {
                        index,
                        pixels: wire.pixels,
                    })
                })
                .collect(),
            row_heights: wire
                .row_heights
                .into_iter()
                .filter_map(|wire| {
                    wire.name
                        .parse::<u32>()
                        .ok()
                        .filter(|row| *row > 0)
                        .map(|index| SpreadsheetDimension {
                            index,
                            pixels: wire.pixels,
                        })
                })
                .collect(),
            merged_ranges: wire.merged_ranges,
            native_ref: wire.native_ref,
        }
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
    pub index: u32,
    /// Display size in source UI pixels.
    pub pixels: u32,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
