// SPDX-License-Identifier: Apache-2.0
//! Active boundary planes whose source chain continues in visible geometry.

use crate::surface::{PlaneLocalSystem, SurfaceParameterRecord, SurfaceRow};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone)]
pub(crate) struct BoundaryPlane {
    pub(crate) row: SurfaceRow,
    pub(crate) parameter: SurfaceParameterRecord,
    pub(crate) local_system: Option<PlaneLocalSystem>,
}

impl BoundaryPlane {
    pub(crate) fn admitted(
        &self,
        peers: &[Self],
        visible: &[SurfaceRow],
        nonvisible: &[SurfaceRow],
        cylinders: &[crate::datum::DatumCylinder],
    ) -> bool {
        self.row.id != 0
            && self.row.feature_id != 0
            && self.row.kind == crate::surface::SurfaceKind::Plane
            && self.row.boundary_type == crate::surface::BoundaryType::Code00
            && self.row.next_surface != 0
            && self.parameter.surface_id == self.row.id
            && self.parameter.offset == self.row.offset
            && peers
                .iter()
                .filter(|plane| plane.row.id == self.row.id)
                .count()
                == 1
            && !nonvisible.iter().any(|row| row.id == self.row.id)
            && !cylinders.iter().any(|cylinder| cylinder.id == self.row.id)
            && self
                .local_system
                .as_ref()
                .is_some_and(PlaneLocalSystem::has_complete_frame)
            && crate::surface::unique_surface_row(visible, self.row.next_surface)
                .is_some_and(|next| next.feature_id == self.row.feature_id)
            && !visible.iter().any(|row| row.id == self.row.id)
    }

    pub(crate) fn shift_offsets(&mut self, base: usize) {
        self.row.offset += base;
        self.parameter.offset += base;
        self.parameter.body_offset += base;
        if let Some(frame) = &mut self.local_system {
            frame.offset += base;
            frame.row_offset += base;
        }
    }
}

pub(crate) fn scan(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Vec<BoundaryPlane>, CodecError> {
    let all_rows: Vec<_> = crate::surface::counted_row_bounds(payload)
        .into_iter()
        .map(|(row, _)| row)
        .collect();
    let rows = all_rows
        .iter()
        .filter(|row| {
            row.kind == crate::surface::SurfaceKind::Plane
                && row.boundary_type == crate::surface::BoundaryType::Code00
                && row.next_surface != 0
        })
        .cloned()
        .collect::<Vec<_>>();
    let parameters = crate::surface::parameter_records_for_rows(ctx, payload, &rows, losses)?;
    let systems = crate::surface::plane_local_systems_for_rows(ctx, payload, &rows, losses)?;
    let mut result = Vec::new();
    for row in &rows {
        if row.id == 0 || row.feature_id == 0 {
            losses.push(
                crate::loss::CreoLossCode::ActiveBoundaryPlaneUnresolved.note(format!(
                    "ActDatums plane {} at byte {} requires a nonzero identity and feature owner",
                    row.id, row.offset
                )),
            );
            continue;
        }
        if crate::surface::unique_surface_row(&all_rows, row.id).is_none() {
            losses.push(
                crate::loss::CreoLossCode::ActiveBoundaryPlaneUnresolved.note(format!(
                    "ActDatums plane {} at byte {} has an ambiguous source row",
                    row.id, row.offset
                )),
            );
            continue;
        }
        let Some(parameter) = crate::surface::unique_surface_parameter(&parameters, row.id) else {
            losses.push(
                crate::loss::CreoLossCode::ActiveBoundaryPlaneUnresolved.note(format!(
                    "ActDatums plane {} at byte {} has no unique parameter body",
                    row.id, row.offset
                )),
            );
            continue;
        };
        let mut frames = systems
            .iter()
            .filter(|frame| frame.surface_id == row.id && frame.row_offset == row.offset);
        let local_system = frames.next().filter(|_| frames.next().is_none()).cloned();
        result.push(BoundaryPlane {
            row: row.clone(),
            parameter: parameter.clone(),
            local_system,
        });
    }
    Ok(result)
}
