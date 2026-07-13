// SPDX-License-Identifier: Apache-2.0
//! Analytic and free-form surface projection.

use super::geometry::{entity_loss, resolve_transform, source_object};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_SURFACE_POLES: usize = 1_000_000;

pub(super) struct SurfaceProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> SurfaceProjection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 128 && (0..=9).contains(&entry.form))
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let indices = [record.integer(1), record.integer(2)];
        let degrees = [record.integer(3), record.integer(4)];
        let [Some(k1), Some(k2)] = indices.map(|value| value.and_then(|v| usize::try_from(v).ok()))
        else {
            losses.push(entity_loss(
                entry,
                "surface upper indices K1 or K2 are invalid",
            ));
            continue;
        };
        let [Some(u_degree), Some(v_degree)] =
            degrees.map(|value| value.and_then(|v| u32::try_from(v).ok()))
        else {
            losses.push(entity_loss(entry, "surface degrees M1 or M2 are invalid"));
            continue;
        };
        let [u_degree_usize, v_degree_usize] = [u_degree, v_degree].map(|degree| degree as usize);
        if u_degree == 0 || v_degree == 0 || k1 < u_degree_usize || k2 < v_degree_usize {
            losses.push(entity_loss(
                entry,
                "surface pole counts are smaller than their degrees plus one",
            ));
            continue;
        }
        let flags = (5..=9)
            .map(|index| record.integer(index))
            .collect::<Vec<_>>();
        if flags.iter().any(|flag| !matches!(flag, Some(0 | 1))) {
            losses.push(entity_loss(
                entry,
                "one or more surface flags are not 0 or 1",
            ));
            continue;
        }
        let (Some(u_count), Some(v_count)) = (k1.checked_add(1), k2.checked_add(1)) else {
            losses.push(entity_loss(entry, "surface pole count overflows"));
            continue;
        };
        let (Ok(u_count_u32), Ok(v_count_u32)) = (u32::try_from(u_count), u32::try_from(v_count))
        else {
            losses.push(entity_loss(entry, "surface pole dimensions exceed u32"));
            continue;
        };
        let Some(pole_count) = u_count.checked_mul(v_count) else {
            losses.push(entity_loss(entry, "surface pole grid size overflows"));
            continue;
        };
        if pole_count > MAX_SURFACE_POLES {
            losses.push(entity_loss(
                entry,
                format!("surface exceeds the {MAX_SURFACE_POLES}-pole limit"),
            ));
            continue;
        }
        let Some(u_knot_count) = u_count
            .checked_add(u_degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "u-knot count overflows"));
            continue;
        };
        let Some(v_knot_count) = v_count
            .checked_add(v_degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "v-knot count overflows"));
            continue;
        };
        let u_knot_start = 10_usize;
        let Some(v_knot_start) = u_knot_start.checked_add(u_knot_count) else {
            losses.push(entity_loss(entry, "v-knot offset overflows"));
            continue;
        };
        let Some(weight_start) = v_knot_start.checked_add(v_knot_count) else {
            losses.push(entity_loss(entry, "surface weight offset overflows"));
            continue;
        };
        let Some(pole_start) = weight_start.checked_add(pole_count) else {
            losses.push(entity_loss(entry, "surface pole offset overflows"));
            continue;
        };
        let Some(pole_value_count) = pole_count.checked_mul(3) else {
            losses.push(entity_loss(entry, "surface pole value count overflows"));
            continue;
        };
        let Some(range_start) = pole_start.checked_add(pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "surface parameter-range offset overflows",
            ));
            continue;
        };
        let collect_numbers = |start: usize, count: usize| -> Option<Vec<f64>> {
            (start..start.checked_add(count)?)
                .map(|index| record.number(index).filter(|value| value.is_finite()))
                .collect()
        };
        let Some(u_knots) = collect_numbers(u_knot_start, u_knot_count) else {
            losses.push(entity_loss(
                entry,
                "u-knot vector is truncated or non-finite",
            ));
            continue;
        };
        let Some(v_knots) = collect_numbers(v_knot_start, v_knot_count) else {
            losses.push(entity_loss(
                entry,
                "v-knot vector is truncated or non-finite",
            ));
            continue;
        };
        if u_knots.windows(2).any(|pair| pair[0] > pair[1])
            || v_knots.windows(2).any(|pair| pair[0] > pair[1])
        {
            losses.push(entity_loss(entry, "surface knot vector is decreasing"));
            continue;
        }
        let Some(native_weights) = collect_numbers(weight_start, pole_count) else {
            losses.push(entity_loss(
                entry,
                "surface weight vector is truncated or non-finite",
            ));
            continue;
        };
        if native_weights.iter().any(|weight| *weight <= 0.0) {
            losses.push(entity_loss(
                entry,
                "surface weights are not strictly positive",
            ));
            continue;
        }
        let polynomial = native_weights
            .first()
            .is_some_and(|first| native_weights.iter().all(|weight| weight == first));
        if (flags[2] == Some(1)) != polynomial {
            losses.push(entity_loss(
                entry,
                "surface polynomial flag does not agree with its weights",
            ));
            continue;
        }
        let Some(native_poles) = collect_numbers(pole_start, pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "surface poles are truncated or non-finite",
            ));
            continue;
        };
        let Some(ranges) = collect_numbers(range_start, 4) else {
            losses.push(entity_loss(entry, "surface parameter ranges are missing"));
            continue;
        };
        if ranges[0] > ranges[1]
            || ranges[2] > ranges[3]
            || ranges[0] < u_knots[u_degree_usize]
            || ranges[1] > u_knots[u_count]
            || ranges[2] < v_knots[v_degree_usize]
            || ranges[3] > v_knots[v_count]
        {
            losses.push(entity_loss(
                entry,
                "surface parameter ranges lie outside their knot domains",
            ));
            continue;
        }
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let native_points = native_poles
            .chunks_exact(3)
            .map(|point| Point3::new(point[0] * factor, point[1] * factor, point[2] * factor))
            .collect::<Vec<_>>();
        let mut control_points = Vec::with_capacity(pole_count);
        let mut weights = (!polynomial).then(|| Vec::with_capacity(pole_count));
        for u in 0..u_count {
            for v in 0..v_count {
                let native_index = v * u_count + u;
                control_points.push(transform.point(native_points[native_index]));
                if let Some(weights) = &mut weights {
                    weights.push(native_weights[native_index]);
                }
            }
        }
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("iges:model:surface#D{}", entry.sequence)),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                u_count: u_count_u32,
                v_count: v_count_u32,
                control_points,
                weights,
                u_periodic: flags[3] == Some(1),
                v_periodic: flags[4] == Some(1),
            }),
            source_object: Some(source_object(entry)),
        });
        decoded.insert(entry.sequence);
    }

    SurfaceProjection {
        handled,
        decoded,
        losses,
    }
}
