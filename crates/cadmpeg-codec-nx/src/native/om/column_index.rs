// SPDX-License-Identifier: Apache-2.0
//! Contiguous target-index bounds owned by a complete column-row sequence.

use serde::{Deserialize, Serialize};

/// Complete target and linked row lists with a nonnegative descending index range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ColumnIndexRowsWire", into = "ColumnIndexRowsWire")]
pub(crate) struct ColumnIndexRows {
    target_rows: Vec<String>,
    linked_rows: Vec<String>,
    first_target_index: u32,
}

impl ColumnIndexRows {
    pub(crate) fn new(
        first_target_index: u32,
        target_rows: Vec<String>,
        linked_rows: Vec<String>,
    ) -> Result<Self, &'static str> {
        if target_rows.is_empty() || linked_rows.is_empty() {
            return Err("ColumnIndexRows.target_rows and linked_rows must both be nonempty");
        }
        let count = target_rows
            .len()
            .checked_add(linked_rows.len())
            .and_then(|count| u32::try_from(count).ok())
            .ok_or("ColumnIndexRows row count exceeds u32")?;
        if first_target_index < count {
            return Err("ColumnIndexRows.first_target_index is smaller than the row count");
        }
        Ok(Self {
            target_rows,
            linked_rows,
            first_target_index,
        })
    }

    pub(crate) fn target_rows(&self) -> &[String] {
        &self.target_rows
    }

    pub(crate) fn linked_rows(&self) -> &[String] {
        &self.linked_rows
    }

    pub(crate) fn last_target_index(&self) -> u32 {
        self.first_target_index - (self.target_rows.len() + self.linked_rows.len()) as u32
    }
}

#[derive(Serialize, Deserialize)]
struct ColumnIndexRowsWire {
    target_rows: Vec<String>,
    linked_rows: Vec<String>,
    first_target_index: u32,
    last_target_index: u32,
}

impl From<ColumnIndexRows> for ColumnIndexRowsWire {
    fn from(value: ColumnIndexRows) -> Self {
        let last_target_index = value.last_target_index();
        Self {
            target_rows: value.target_rows,
            linked_rows: value.linked_rows,
            first_target_index: value.first_target_index,
            last_target_index,
        }
    }
}

impl TryFrom<ColumnIndexRowsWire> for ColumnIndexRows {
    type Error = &'static str;
    fn try_from(wire: ColumnIndexRowsWire) -> Result<Self, Self::Error> {
        let rows = Self::new(wire.first_target_index, wire.target_rows, wire.linked_rows)?;
        if wire.last_target_index != rows.last_target_index() {
            return Err(
                "ColumnIndexRows.last_target_index disagrees with first_target_index and row count",
            );
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_row_bounds_derive_the_last_index_and_reject_disagreement() {
        let wire = r#"{"target_rows":["target"],"linked_rows":["linked"],"first_target_index":4,"last_target_index":2}"#;
        let rows: ColumnIndexRows = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&rows).unwrap(), wire);
        assert_eq!(rows.first_target_index, 4);
        assert_eq!(rows.last_target_index(), 2);
        assert!(
            serde_json::from_str::<ColumnIndexRows>(&wire.replace("index\":2", "index\":3"))
                .is_err()
        );
        assert!(ColumnIndexRows::new(1, vec!["target".into()], vec!["linked".into()]).is_err());
        assert!(ColumnIndexRows::new(4, vec!["target".into()], Vec::new()).is_err());
    }
}
