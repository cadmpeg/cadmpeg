// SPDX-License-Identifier: Apache-2.0
//! Display colors preceding complete linked or target-index row frames.

use super::{rmfastload_target_object_id, PartColorDefinition, RmFastLoadObjectId};
use crate::container::Container;
use crate::om::color::PaletteIndex;
use crate::om::column_row::{LinkedRow, TargetRow};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

mod borrowed_wires;
mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "wire::EncodingWire")]
pub(in crate::native) enum RmDisplayColorAssignmentEncoding {
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
pub(in crate::native) struct DisplayColorFrame {
    encoding: RmDisplayColorAssignmentEncoding,
    color_index: PaletteIndex,
}

impl DisplayColorFrame {
    pub(in crate::native) fn new(
        encoding: RmDisplayColorAssignmentEncoding,
        color_index: PaletteIndex,
    ) -> Option<Self> {
        encoding.offset().checked_sub(
            u64::from(color_index.display_byte_len())
                + cadmpeg_core::decode::u64_from_index(crate::om::column_row::ROW_SUFFIX.len()),
        )?;
        Some(Self {
            encoding,
            color_index,
        })
    }
    pub(in crate::native) fn encoding(&self) -> &RmDisplayColorAssignmentEncoding {
        &self.encoding
    }
    pub(in crate::native) fn offset(&self) -> u64 {
        self.encoding.offset() - u64::from(self.color_index.display_byte_len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "wire::RmDisplayColorAssignmentWire")]
pub(in crate::native) struct RmDisplayColorAssignment {
    pub(in crate::native) id: String,
    pub(in crate::native) ordinal: u32,
    pub(in crate::native) frame: DisplayColorFrame,
    pub(in crate::native) target_object_id: Option<String>,
    pub(in crate::native) color_definition: String,
    pub(in crate::native) source_entry: String,
}

fn push_assignment(
    ctx: &DecodeContext<'_>,
    assignments: &mut Vec<RmDisplayColorAssignment>,
    frame: DisplayColorFrame,
    target_object_id: Option<String>,
    color_definition: &str,
    source_entry: &str,
) -> Result<(), CodecError> {
    ctx.reserve_retained_vec(assignments, 1, "NX display color assignments")?;
    assignments.push(RmDisplayColorAssignment {
        id: String::new(),
        ordinal: 0,
        frame,
        target_object_id,
        color_definition: ctx
            .copy_retained_text(color_definition, "retain NX display color text")?,
        source_entry: ctx.copy_retained_text(source_entry, "retain NX display color text")?,
    });
    Ok(())
}

fn assignment_id(ctx: &DecodeContext<'_>, ordinal: usize) -> Result<String, CodecError> {
    let mut value = ordinal;
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    let length = "nx:rm-display-color-assignments:assignment#"
        .len()
        .checked_add(digits)
        .ok_or_else(|| ctx.refuse_codec_limit("NX display color identity length", 0, 1))?;
    let mut id = ctx.retained_string(length, "retain NX display color identity")?;
    write!(id, "nx:rm-display-color-assignments:assignment#{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit("write NX display color identity", 0, 1))?;
    Ok(id)
}

fn finalize_assignments(
    ctx: &DecodeContext<'_>,
    mut assignments: Vec<RmDisplayColorAssignment>,
) -> Result<Vec<RmDisplayColorAssignment>, CodecError> {
    let sort_bytes = assignments
        .len()
        .checked_mul(std::mem::size_of::<RmDisplayColorAssignment>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX display color sort bytes", 0, 1))?;
    let sort_reservation = ctx.reserve_scoped(
        u64_from_index(sort_bytes),
        "sort NX display color assignments",
    )?;
    let sort_work = assignments
        .len()
        .checked_mul(assignments.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX display color sort work", 0, 1))?;
    ctx.charge_work(
        u64_from_index(sort_work),
        "sort NX display color assignments",
    )?;
    assignments.sort_by_key(|assignment| assignment.frame.offset());
    drop(sort_reservation);
    for (ordinal, assignment) in assignments.iter_mut().enumerate() {
        assignment.ordinal = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX display color ordinal", 0, 1))?;
        assignment.id = assignment_id(ctx, ordinal)?;
    }
    Ok(assignments)
}

/// Decode explicit display-color assignments from `RMFastLoad` linked rows.
pub(in crate::native) fn rm_display_color_assignments(
    ctx: &DecodeContext<'_>,
    container: &Container,
    color_definitions: &[PartColorDefinition],
    object_ids: &[RmFastLoadObjectId],
) -> Result<Vec<RmDisplayColorAssignment>, CodecError> {
    let mut assignments = Vec::new();
    for (entry, section) in container
        .om_sections(ctx)?
        .into_iter()
        .filter(|(entry, _)| entry.name == "/Root/FastLoad/RMFastLoad")
    {
        let Some(record_area) = section.record_area else {
            continue;
        };
        let record_area_offset = record_area.offset;
        let record_area = record_area.bytes;
        let source_base = entry.file_span().map_or(0, |(offset, _)| offset)
            + cadmpeg_core::decode::u64_from_index(record_area_offset);
        for row in crate::om::column_row::scan::linked_rows(ctx, record_area)? {
            let Some(color) =
                crate::om::column_row::scan::preceding_color(record_area, row.offset())
            else {
                continue;
            };
            ctx.charge_work(
                u64_from_index(color_definitions.len()),
                "match NX display color definition",
            )?;
            let mut matches = color_definitions
                .iter()
                .filter(|definition| definition.color_index == color);
            let Some(definition) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let target_index = row.target_index().atom.value();
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let Some(frame) =
                DisplayColorFrame::new(RmDisplayColorAssignmentEncoding::Linked(row), color)
            else {
                continue;
            };
            let target_object_id = rmfastload_target_object_id(ctx, object_ids, target_index)?;
            push_assignment(
                ctx,
                &mut assignments,
                frame,
                target_object_id,
                &definition.id,
                &entry.name,
            )?;
        }
        for row in crate::om::column_row::scan::target_rows(ctx, record_area)? {
            let Some(color) =
                crate::om::column_row::scan::preceding_color(record_area, row.offset())
            else {
                continue;
            };
            ctx.charge_work(
                u64_from_index(color_definitions.len()),
                "match NX display color definition",
            )?;
            let mut matches = color_definitions
                .iter()
                .filter(|definition| definition.color_index == color);
            let Some(definition) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            let target_index = row.target_index().atom.value();
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let Some(frame) =
                DisplayColorFrame::new(RmDisplayColorAssignmentEncoding::Target(row), color)
            else {
                continue;
            };
            let target_object_id = rmfastload_target_object_id(ctx, object_ids, target_index)?;
            push_assignment(
                ctx,
                &mut assignments,
                frame,
                target_object_id,
                &definition.id,
                &entry.name,
            )?;
        }
    }
    finalize_assignments(ctx, assignments)
}

#[cfg(test)]
mod admission_tests {
    use super::{finalize_assignments, push_assignment, RmDisplayColorAssignment};
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    const ROW: &str = r#"{"id":"nx:rm-display-color-assignments:assignment#0","ordinal":0,"encoding":{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7},"color_index":128,"color_definition":"definition","raw_color_index":[128,128],"source_entry":"entry","source_offset":8,"row_source_offset":10}"#;

    #[test]
    fn display_color_assignment_refuses_collection_limit() {
        let row: RmDisplayColorAssignment = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            push_assignment(ctx, &mut Vec::new(), row.frame, None, "definition", "entry")
                .unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems));
    }

    #[test]
    fn display_color_assignment_refuses_retained_limit() {
        let row: RmDisplayColorAssignment = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            push_assignment(ctx, &mut Vec::new(), row.frame, None, "definition", "entry")
                .unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes));
    }

    #[test]
    fn display_color_assignment_preserves_definition_and_source() {
        let row: RmDisplayColorAssignment = serde_json::from_str(ROW).unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            |_| {},
            |ctx| {
                let mut assignments = Vec::new();
                push_assignment(
                    ctx,
                    &mut assignments,
                    row.frame,
                    None,
                    "definition",
                    "entry",
                )
                .unwrap();
                assert_eq!(assignments[0].color_definition, "definition");
                assert_eq!(assignments[0].source_entry, "entry");
            },
        );
    }

    #[test]
    fn display_color_finalization_refuses_scoped_limit() {
        let row: RmDisplayColorAssignment = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_materialized_bytes = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            finalize_assignments(ctx, vec![row]).unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn display_color_finalization_refuses_work_limit() {
        let row: RmDisplayColorAssignment = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_work_units = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            finalize_assignments(ctx, vec![row]).unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits));
    }
}
