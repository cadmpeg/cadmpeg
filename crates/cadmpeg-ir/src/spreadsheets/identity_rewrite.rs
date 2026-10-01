// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetDimension, SpreadsheetRange};

rewrite_scalar!(CellAddress);
rewrite_record!(Spreadsheet, []; {id, feature, cells, column_widths, row_heights, merged_ranges, native_ref});
rewrite_record!(SpreadsheetCell, []; {address, parameter});
rewrite_record!(SpreadsheetDimension, []; {index, pixels});
rewrite_record!(SpreadsheetRange, []; {start, end});
