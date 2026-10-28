// SPDX-License-Identifier: Apache-2.0
//! Preserve compact cone origins with unresolved coordinate ownership.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::container::ContainerScan;
use crate::surface::SurfaceKind;

use super::super::uniqueness::UniqueById;

pub(in crate::decode) fn withhold_unresolved_cone_origins(
    ctx: &DecodeContext<'_>,
    scan: &mut ContainerScan<'_>,
) -> Result<(), CodecError> {
    let mut visible = BTreeSet::new();
    let mut nonvisible = BTreeSet::new();
    for (parameters, rows, withheld) in [
        (
            &mut scan.surfaces.parameters,
            &scan.surfaces.rows,
            &mut visible,
        ),
        (
            &mut scan.surfaces.nonvisible_parameters,
            &scan.surfaces.nonvisible_rows,
            &mut nonvisible,
        ),
    ] {
        let mut candidates = BTreeSet::new();
        for record in parameters.iter() {
            if record.unresolved_cone_origin_suffix().is_some() {
                candidates.insert(record.surface_id);
            }
        }
        if candidates.is_empty() {
            continue;
        }
        let rows = UniqueById::new(rows, |row| row.id);
        let unique_parameters = UniqueById::new(parameters.iter(), |record| record.surface_id);
        let ids = parameters
            .iter()
            .filter(|record| {
                candidates.contains(&record.surface_id)
                    && unique_parameters.get(record.surface_id).is_some()
                    && rows
                        .get(record.surface_id)
                        .is_some_and(|row| row.kind == SurfaceKind::Cone)
            })
            .map(|record| record.surface_id)
            .collect::<BTreeSet<_>>();
        for record in parameters {
            if ids.contains(&record.surface_id) && record.withhold_unresolved_cone_origin(ctx)? {
                withheld.insert(record.surface_id);
            }
        }
    }
    if visible.is_empty() {
        scan.surfaces.withheld_cone_origin_surface_ids.clear();
        scan.surfaces.withheld_cone_origin_curve_ids.clear();
        return Ok(());
    }
    scan.surfaces.withheld_cone_origin_curve_ids = scan
        .curves
        .topology_rows
        .iter()
        .filter(|row| {
            row.faces
                .iter()
                .flatten()
                .any(|face| visible.contains(&face.get()))
        })
        .map(|row| row.id)
        .collect();
    scan.surfaces.withheld_cone_origin_surface_ids = visible;
    Ok(())
}

#[cfg(test)]
mod tests;
