// SPDX-License-Identifier: Apache-2.0
//! Display colors preceding complete linked or target-index row frames.

use super::{rmfastload_target_object_id, PartColorDefinition, RmFastLoadObjectId};
use crate::container::Container;
use crate::om::color::PaletteIndex;
use crate::om::column_row::{LinkedRow, TargetRow};
use serde::{Deserialize, Serialize};

mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "wire::EncodingWire", into = "wire::EncodingWire")]
pub(crate) enum RmDisplayColorAssignmentEncoding {
    Linked(LinkedRow<(), u64>),
    Target(TargetRow<(), u64>),
}

impl RmDisplayColorAssignmentEncoding {
    fn offset(&self) -> u64 {
        match self {
            Self::Linked(row) => row.offset(),
            Self::Target(row) => row.offset(),
        }
    }
}

/// The color token and its following row share one checked position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayColorFrame {
    encoding: RmDisplayColorAssignmentEncoding,
    color_index: PaletteIndex,
}

impl DisplayColorFrame {
    pub(crate) fn new(
        encoding: RmDisplayColorAssignmentEncoding,
        color_index: PaletteIndex,
    ) -> Option<Self> {
        encoding.offset().checked_sub(
            u64::from(color_index.display_byte_len())
                + crate::om::column_row::ROW_SUFFIX.len() as u64,
        )?;
        Some(Self {
            encoding,
            color_index,
        })
    }
    pub(crate) fn encoding(&self) -> &RmDisplayColorAssignmentEncoding {
        &self.encoding
    }
    pub(crate) fn offset(&self) -> u64 {
        self.encoding.offset() - u64::from(self.color_index.display_byte_len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::RmDisplayColorAssignmentWire",
    into = "wire::RmDisplayColorAssignmentWire"
)]
pub(crate) struct RmDisplayColorAssignment {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) frame: DisplayColorFrame,
    pub(crate) target_object_id: Option<String>,
    pub(crate) color_definition: String,
    pub(crate) source_entry: String,
}

/// Decode explicit display-color assignments from `RMFastLoad` linked rows.
pub fn rm_display_color_assignments(
    container: &Container,
    color_definitions: &[PartColorDefinition],
    object_ids: &[RmFastLoadObjectId],
) -> Vec<RmDisplayColorAssignment> {
    let mut candidates = Vec::new();
    for (entry, section) in container
        .om_sections()
        .into_iter()
        .filter(|(entry, _)| entry.name == "/Root/FastLoad/RMFastLoad")
    {
        let Some(record_area) = section.record_area else {
            continue;
        };
        let record_area_offset = record_area.offset;
        let record_area = record_area.bytes;
        let source_base =
            entry.file_span().map_or(0, |(offset, _)| offset) + record_area_offset as u64;
        for row in crate::om::column_row::scan::linked_rows(record_area) {
            let Some(color) =
                crate::om::column_row::scan::preceding_color(record_area, row.offset())
            else {
                continue;
            };
            let mut matches = color_definitions
                .iter()
                .filter(|definition| definition.color_index == color);
            let Some(definition) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let target_object_id =
                rmfastload_target_object_id(object_ids, row.target_index().atom.value());
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let Some(frame) =
                DisplayColorFrame::new(RmDisplayColorAssignmentEncoding::Linked(row), color)
            else {
                continue;
            };
            candidates.push((
                frame,
                target_object_id,
                definition.id.clone(),
                entry.name.clone(),
            ));
        }
        for row in crate::om::column_row::scan::target_rows(record_area) {
            let Some(color) =
                crate::om::column_row::scan::preceding_color(record_area, row.offset())
            else {
                continue;
            };
            let mut matches = color_definitions
                .iter()
                .filter(|definition| definition.color_index == color);
            let Some(definition) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let target_object_id =
                rmfastload_target_object_id(object_ids, row.target_index().atom.value());
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let Some(frame) =
                DisplayColorFrame::new(RmDisplayColorAssignmentEncoding::Target(row), color)
            else {
                continue;
            };
            candidates.push((
                frame,
                target_object_id,
                definition.id.clone(),
                entry.name.clone(),
            ));
        }
    }
    candidates.sort_by_key(|(frame, _, _, _)| frame.offset());
    candidates
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (frame, target_object_id, color_definition, source_entry))| {
                RmDisplayColorAssignment {
                    id: format!("nx:rm-display-color-assignments:assignment#{ordinal}"),
                    ordinal: ordinal as u32,
                    frame,
                    target_object_id,
                    color_definition,
                    source_entry,
                }
            },
        )
        .collect()
}
