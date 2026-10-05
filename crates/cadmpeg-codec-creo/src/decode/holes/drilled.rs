// SPDX-License-Identifier: Apache-2.0
//! Simple drilled-hole recipes, envelopes, and dimension matching.

use crate::axis::Axis;
use crate::vecmath::unit_length;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::holes::HoleForm;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::scalar::PositiveLength;

use crate::container::ContainerScan;

use super::super::feature_history::dimensions::feature_dimension_table_complete;
use super::super::feature_history::round::unique_surface_parameter_record;
use super::super::sketch::equations_coordinate::approximately_equal;
use super::super::sweep::planes::unique_available_positional_cylinder_frame_records;
use super::super::uniqueness::exactly_one;

const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_COORDINATE_AGREEMENT: f64 = 1.0e-9;
const EPS_DIAMETER_NONZERO: f64 = 1.0e-12;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;

pub(in crate::decode) fn stepped_hole_form(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Result<Option<HoleForm>, CodecError> {
    let mut candidates = 0;
    for table in tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 29)
    {
        let paired = match paired_hole_replay_surfaces_by_source(ctx, feature_id, table, rows)? {
            Some(generated) => paired_hole_replay_is_counterbore(ctx, &generated)?,
            None => false,
        };
        if paired || split_patch_table_is_counterbore(ctx, feature_id, table, rows)? {
            candidates += 1;
        }
    }
    Ok((candidates == 1).then_some(HoleForm::Counterbore))
}

fn paired_hole_replay_is_counterbore(
    ctx: &DecodeContext<'_>,
    generated_by_source: &BTreeMap<u32, ReplaySurfacePair>,
) -> Result<bool, CodecError> {
    let mut cylinder_sources = 0usize;
    let mut planar_support_sources = 0usize;
    let mut has_cone = false;
    for (_, pair) in ctx.admit_iter(generated_by_source, "creo paired-hole source count")? {
        let entries = [pair.first, pair.second];
        if matches!(
            entries,
            [
                Some(crate::surface::SurfaceKind::Cylinder),
                Some(crate::surface::SurfaceKind::Cylinder)
            ]
        ) {
            cylinder_sources = cylinder_sources.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("creo paired-hole source count", u64::MAX, u64::MAX)
            })?;
        }
        let plane_count = ctx
            .admit_iter(&entries, "creo paired-hole plane count")?
            .filter(|kind| matches!(kind, Some(crate::surface::SurfaceKind::Plane)))
            .count();
        if plane_count == 1
            && ctx
                .admit_iter(&entries, "creo paired-hole absent count")?
                .filter(|kind| kind.is_none())
                .count()
                == 1
        {
            planar_support_sources = planar_support_sources.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("creo paired-hole source count", u64::MAX, u64::MAX)
            })?;
        }
        has_cone |= matches!(pair.first, Some(crate::surface::SurfaceKind::Cone))
            || matches!(pair.second, Some(crate::surface::SurfaceKind::Cone));
    }
    Ok(cylinder_sources == 2 && planar_support_sources == 1 && !has_cone)
}

fn split_patch_table_is_counterbore(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    table: &crate::feature::entity::FeatureEntityTable,
    rows: &[crate::surface::SurfaceRow],
) -> Result<bool, CodecError> {
    let mut surface_count = 0;
    let mut cylinder_count = 0;
    let mut plane_count = 0;
    for surface_id in table.surface_ids_iter() {
        let Some(row) = crate::surface::unique_surface_row(rows, surface_id)
            .filter(|row| row.feature_id == feature_id)
        else {
            return Ok(false);
        };
        surface_count += 1;
        cylinder_count += usize::from(row.kind == crate::surface::SurfaceKind::Cylinder);
        plane_count += usize::from(row.kind == crate::surface::SurfaceKind::Plane);
    }
    let unique_surface_count = table.unique_surface_ids().len();
    if surface_count != 5 || unique_surface_count != 5 || cylinder_count != 4 || plane_count != 1 {
        return Ok(false);
    }
    let is_rowless = |entry: &crate::feature::entity::FeatureEntityTableEntry| {
        table.contains_non_surface_entity_id(entry.entity_id)
            && !table.contains_surface_id(entry.entity_id)
    };
    if !table.entries.windows(2).any(|entries| {
        entries[0].class_id() == 204
            && entries[1].class_id() == 203
            && is_rowless(&entries[0])
            && is_rowless(&entries[1])
    }) {
        return Ok(false);
    }

    let surface_ids = table.unique_surface_ids();
    let mut materialized_surface_ids = BTreeSet::new();
    let mut cylinder_ids_by_source = BTreeMap::<u32, Vec<u32>>::new();
    let mut plane_ids_by_source = BTreeMap::<u32, Vec<u32>>::new();
    let mut rowless_counts_by_source = BTreeMap::<u32, usize>::new();
    for entry in table.entries.iter().filter(|entry| entry.class_id() == 200) {
        let materialized = table.contains_surface_id(entry.entity_id);
        let rowless = is_rowless(entry);
        if !materialized && !rowless {
            return Ok(false);
        }
        let Some(source_id) = entry.source_entity_id() else {
            continue;
        };
        if source_id == 0 && materialized {
            return Ok(false);
        }
        if rowless {
            match ctx.entry_btree_map(
                &mut rowless_counts_by_source,
                source_id,
                "creo split-patch rowless source nodes",
            )? {
                std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(1);
                }
            }
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(rows, entry.entity_id)
            .filter(|row| row.feature_id == feature_id)
        else {
            return Ok(false);
        };
        if materialized_surface_ids.contains(&entry.entity_id) {
            return Ok(false);
        }
        ctx.insert_btree_set(
            &mut materialized_surface_ids,
            entry.entity_id,
            "creo split-patch materialized surface ID nodes",
        )?;
        if row.kind == crate::surface::SurfaceKind::Cylinder {
            let group = ctx
                .entry_btree_map(
                    &mut cylinder_ids_by_source,
                    source_id,
                    "creo split-patch cylinder source nodes",
                )?
                .or_default();
            ctx.reserve_vec(group, 1, "creo split-patch cylinder IDs")?;
            group.push(entry.entity_id);
        } else if row.kind == crate::surface::SurfaceKind::Plane {
            let group = ctx
                .entry_btree_map(
                    &mut plane_ids_by_source,
                    source_id,
                    "creo split-patch plane source nodes",
                )?
                .or_default();
            ctx.reserve_vec(group, 1, "creo split-patch plane IDs")?;
            group.push(entry.entity_id);
        }
    }
    if &materialized_surface_ids != surface_ids {
        return Ok(false);
    }
    let cylinder_id_count = cylinder_ids_by_source.values().map(Vec::len).sum::<usize>();
    let mut unique_cylinder_id_count = 0usize;
    let mut index = 0usize;
    for (_, ids) in ctx.admit_iter(&cylinder_ids_by_source, "creo split-patch cylinder groups")? {
        for id in ctx.admit_iter(ids, "creo split-patch cylinder IDs")? {
            let mut previous_index = 0usize;
            let mut seen = false;
            'previous: for (_, previous_ids) in ctx.admit_iter(
                &cylinder_ids_by_source,
                "creo split-patch prior cylinder groups",
            )? {
                for previous in
                    ctx.admit_iter(previous_ids, "creo split-patch prior cylinder IDs")?
                {
                    if previous_index == index {
                        break 'previous;
                    }
                    if previous == id {
                        seen = true;
                        break 'previous;
                    }
                    previous_index = previous_index.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "creo split-patch cylinder index",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                }
            }
            if !seen {
                unique_cylinder_id_count =
                    unique_cylinder_id_count.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "creo split-patch cylinder count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
            }
            index = index.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("creo split-patch cylinder index", u64::MAX, u64::MAX)
            })?;
        }
    }
    if plane_ids_by_source.len() != 1 {
        return Ok(false);
    }
    let Some(&plane_source) = plane_ids_by_source.keys().next() else {
        return Ok(false);
    };
    Ok(cylinder_ids_by_source.len() == 2
        && cylinder_id_count == 4
        && unique_cylinder_id_count == 4
        && plane_ids_by_source[&plane_source].len() == 1
        && !cylinder_ids_by_source.contains_key(&plane_source)
        && rowless_counts_by_source.get(&plane_source) == Some(&1)
        && cylinder_ids_by_source
            .values()
            .all(|surface_ids| surface_ids.len() == 2 && surface_ids[0] != surface_ids[1]))
}

struct ReplaySurfacePair {
    first: Option<crate::surface::SurfaceKind>,
    second: Option<crate::surface::SurfaceKind>,
}

fn paired_hole_replay_surfaces_by_source(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    table: &crate::feature::entity::FeatureEntityTable,
    rows: &[crate::surface::SurfaceRow],
) -> Result<Option<BTreeMap<u32, ReplaySurfacePair>>, CodecError> {
    let entry_kind = |entry: &crate::feature::entity::FeatureEntityTableEntry| {
        if table.contains_surface_id(entry.entity_id) {
            Some(Some(
                crate::surface::unique_surface_row(rows, entry.entity_id)
                    .filter(|row| row.feature_id == feature_id)?
                    .kind,
            ))
        } else {
            (table.contains_non_surface_entity_id(entry.entity_id)
                && !table.contains_surface_id(entry.entity_id))
            .then_some(None)
        }
    };
    let mut runs = Vec::<BTreeMap<u32, Option<crate::surface::SurfaceKind>>>::new();
    let mut framed_class_200_count = 0;
    let mut source_zero_count = 0;
    let mut index = 0;
    while index < table.entries.len() {
        let Some(class_203) = table.entries.get(index + 1) else {
            break;
        };
        let class_204 = &table.entries[index];
        if class_204.class_id() != 204 || class_203.class_id() != 203 {
            index += 1;
            continue;
        }
        if !matches!(entry_kind(class_204), Some(None))
            || !matches!(entry_kind(class_203), Some(None))
        {
            return Ok(None);
        }
        index += 2;
        let mut run = BTreeMap::new();
        while let Some(entry) = table
            .entries
            .get(index)
            .filter(|entry| entry.class_id() == 200)
        {
            framed_class_200_count += 1;
            let Some(kind) = entry_kind(entry) else {
                return Ok(None);
            };
            match entry.source_entity_id() {
                Some(0) => {
                    if kind.is_some() {
                        return Ok(None);
                    }
                    source_zero_count += 1;
                }
                Some(source_id) => {
                    if run.contains_key(&source_id) {
                        return Ok(None);
                    }
                    ctx.insert_btree_map(
                        &mut run,
                        source_id,
                        kind,
                        "creo paired-hole run source nodes",
                    )?;
                }
                None if kind.is_some() => return Ok(None),
                None => {}
            }
            index += 1;
        }
        ctx.reserve_vec(&mut runs, 1, "creo paired-hole runs")?;
        runs.push(run);
    }
    if !(source_zero_count <= 1
        && framed_class_200_count
            == ctx
                .admit_iter(&table.entries, "creo paired-hole class count")?
                .filter(|entry| entry.class_id() == 200)
                .count())
    {
        return Ok(None);
    }
    let mut materialized = runs.iter().filter(|run| run.values().any(Option::is_some));
    let Some(first) = materialized.next() else {
        return Ok(None);
    };
    let Some(second) = materialized.next() else {
        return Ok(None);
    };
    if materialized.next().is_some() || !first.keys().eq(second.keys()) {
        return Ok(None);
    }
    let mut paired_by_source = BTreeMap::new();
    for (source_id, first_kind) in first {
        let Some(second_kind) = second.get(source_id) else {
            return Ok(None);
        };
        ctx.insert_btree_map(
            &mut paired_by_source,
            *source_id,
            ReplaySurfacePair {
                first: *first_kind,
                second: *second_kind,
            },
            "creo paired-hole result source nodes",
        )?;
    }
    Ok(Some(paired_by_source))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::decode) enum SimpleDrilledDimensionFamily {
    ExternalId2Depth,
    ExternalId4Depth,
}

impl SimpleDrilledDimensionFamily {
    fn depth_external_id(self) -> u32 {
        match self {
            Self::ExternalId2Depth => 2,
            Self::ExternalId4Depth => 4,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::decode) struct SimpleDrilledHoleRecipe<'a> {
    pub(in crate::decode) table: &'a crate::feature::entity::FeatureEntityTable,
    pub(in crate::decode) dimension_family: SimpleDrilledDimensionFamily,
}

pub(in crate::decode) fn simple_drilled_hole_recipe<'a>(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &'a [crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Result<Option<SimpleDrilledHoleRecipe<'a>>, CodecError> {
    let mut candidate = None;
    for table in tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 29)
    {
        let Some(generated_by_source) =
            paired_hole_replay_surfaces_by_source(ctx, feature_id, table, rows)?
        else {
            continue;
        };
        let paired = |kind| -> Result<usize, CodecError> {
            Ok(ctx
                .admit_iter(&generated_by_source, "creo drilled paired source count")?
                .map(|(_, pair)| [pair.first, pair.second])
                .filter(|entries| entries.as_slice() == [Some(kind), Some(kind)])
                .count())
        };
        let rowless = ctx
            .admit_iter(&generated_by_source, "creo drilled rowless source count")?
            .map(|(_, pair)| [pair.first, pair.second])
            .filter(|entries| entries.as_slice() == [None, None])
            .count();
        let dimension_family = match rowless {
            2 => SimpleDrilledDimensionFamily::ExternalId2Depth,
            3 => SimpleDrilledDimensionFamily::ExternalId4Depth,
            _ => continue,
        };
        if paired(crate::surface::SurfaceKind::Cone)? == 1
            && paired(crate::surface::SurfaceKind::Cylinder)? == 1
            && generated_by_source.len() == rowless + 2
        {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(SimpleDrilledHoleRecipe {
                table,
                dimension_family,
            });
        }
    }
    Ok(candidate)
}

pub(in crate::decode) fn simple_drilled_hole_envelope_spans(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: &crate::feature::entity::FeatureEntityTable,
) -> Result<Option<[[Option<PositiveLength>; 2]; 3]>, CodecError> {
    let Some([first, second]) = simple_drilled_hole_corner_envelopes(ctx, scan, table)? else {
        return Ok(None);
    };
    Ok(paired_corner_envelope_axis_spans(first, second))
}

fn simple_drilled_hole_corner_envelopes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: &crate::feature::entity::FeatureEntityTable,
) -> Result<Option<[[[f64; 3]; 2]; 2]>, CodecError> {
    let feature_id = table.feature_id;
    let mut envelopes = table
        .surface_ids_iter()
        .filter_map(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)
                .filter(|row| row.feature_id == feature_id)
                .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        })
        .map(|row| -> Result<Option<[[f64; 3]; 2]>, CodecError> {
            Ok(unique_surface_parameter_record(ctx, scan, row)?
                .and_then(crate::surface::SurfaceParameterRecord::type24_terminal_corner_envelope))
        });
    let Some(first) = envelopes.next() else {
        return Ok(None);
    };
    let Some(first) = first? else {
        return Ok(None);
    };
    let Some(second) = envelopes.next() else {
        return Ok(None);
    };
    let Some(second) = second? else {
        return Ok(None);
    };
    let Some(third) = envelopes.next() else {
        return Ok(Some([first, second]));
    };
    // discarded-value: A third matching row makes the two-corner form invalid.
    let _ = third?;
    Ok(None)
}

fn simple_drilled_hole_cone_terminal_points(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: &crate::feature::entity::FeatureEntityTable,
) -> Result<Option<[[f64; 3]; 2]>, CodecError> {
    let feature_id = table.feature_id;
    let mut points = table
        .surface_ids_iter()
        .filter_map(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)
                .filter(|row| row.feature_id == feature_id)
                .filter(|row| row.kind == crate::surface::SurfaceKind::Cone)
        })
        .map(|row| -> Result<Option<[f64; 3]>, CodecError> {
            let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
                return Ok(None);
            };
            if record.boundary != crate::surface::SurfaceBodyBoundary::CompoundClose
                || record.scalar_tokens.len() != 7
            {
                return Ok(None);
            }
            let Some(x) = record.scalar_tokens[4]
                .value
                .filter(|value| value.is_finite())
            else {
                return Ok(None);
            };
            let Some(y) = record.scalar_tokens[5]
                .value
                .filter(|value| value.is_finite())
            else {
                return Ok(None);
            };
            let Some(z) = record.scalar_tokens[6]
                .value
                .filter(|value| value.is_finite())
            else {
                return Ok(None);
            };
            Ok(Some([x, y, z]))
        });
    let Some(first) = points.next() else {
        return Ok(None);
    };
    let Some(first) = first? else {
        return Ok(None);
    };
    let Some(second) = points.next() else {
        return Ok(None);
    };
    let Some(second) = second? else {
        return Ok(None);
    };
    let Some(third) = points.next() else {
        return Ok(Some([first, second]));
    };
    // discarded-value: A third matching row makes the two-point form invalid.
    let _ = third?;
    Ok(None)
}

pub(in crate::decode) fn simple_drilled_hole_placement(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: &crate::feature::entity::FeatureEntityTable,
    diameter: f64,
    depth: f64,
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    let Some(corners) = simple_drilled_hole_corner_envelopes(ctx, scan, table)? else {
        return Ok(None);
    };
    if let Some(placement) = drilled_hole_placement_from_corner_envelopes(corners, diameter, depth)
    {
        return Ok(Some(placement));
    }
    let Some(points) = simple_drilled_hole_cone_terminal_points(ctx, scan, table)? else {
        return Ok(None);
    };
    Ok(clipped_drilled_hole_placement_from_cone_points(
        corners, points, diameter, depth,
    ))
}

pub(in crate::decode) fn simple_drilled_hole_axis_placement(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: &crate::feature::entity::FeatureEntityTable,
    diameter: f64,
) -> Result<Option<cadmpeg_ir::features::holes::HolePlacement>, CodecError> {
    let feature_id = table.feature_id;
    let mut cylinder_ids = BTreeSet::new();
    for surface_id in table.surface_ids_iter().filter(|surface_id| {
        crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        ctx.insert_btree_set(
            &mut cylinder_ids,
            surface_id,
            "creo drilled cylinder ID nodes",
        )?;
    }
    let Some(frame_records) = unique_available_positional_cylinder_frame_records(
        ctx,
        &cylinder_ids,
        &scan.surfaces.parameters,
    )?
    else {
        return Ok(None);
    };
    let mut frames = Vec::new();
    ctx.reserve_vec(
        &mut frames,
        frame_records.len(),
        "creo drilled cylinder frame copies",
    )?;
    frames.extend(frame_records.into_iter().map(|(_, frame)| frame));
    Ok(simple_drilled_axis_placement_from_frames(&frames, diameter))
}

pub(in crate::decode) fn simple_drilled_axis_placement_from_frames(
    frames: &[crate::surface::PositionalCylinderFrame],
    diameter: f64,
) -> Option<cadmpeg_ir::features::holes::HolePlacement> {
    let first = *frames.first()?;
    let axis = unit_length(*first.frame().orthonormal_frame().axis());
    let coordinate_scale = frames
        .iter()
        .flat_map(|frame| frame.frame().origin())
        .map(f64::abs)
        .fold(1.0, f64::max);
    (diameter.is_finite() && diameter > 0.0).then_some(())?;
    let radius = 0.5 * diameter;
    frames
        .iter()
        .all(|frame| {
            let candidate_axis = unit_length(*frame.frame().orthonormal_frame().axis());
            let radius_scale = frame.radius().get().max(radius.abs()).max(1.0);
            if (frame.radius().get() - radius).abs() > EPS_RADIUS_AGREEMENT * radius_scale {
                return false;
            }
            let alignment = axis
                .into_iter()
                .zip(candidate_axis)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if alignment.abs() < 1.0 - EPS_AXIS_ALIGNMENT {
                return false;
            }
            let delta = std::array::from_fn::<_, 3, _>(|index| {
                frame.frame().origin()[index] - first.frame().origin()[index]
            });
            let axial_delta = delta
                .into_iter()
                .zip(axis)
                .map(|(component, axis)| component * axis)
                .sum::<f64>();
            delta
                .into_iter()
                .zip(axis)
                .map(|(component, axis)| component - axial_delta * axis)
                .map(|component| component * component)
                .sum::<f64>()
                .sqrt()
                <= EPS_COORDINATE_AGREEMENT * coordinate_scale
        })
        .then_some(cadmpeg_ir::features::holes::HolePlacement::Axis {
            origin: first.frame().finite_origin(),
            axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(axis))?,
        })
}

#[derive(Debug, Clone, Copy)]
struct DrilledHoleEnvelopeLayout {
    corners: [[[f64; 3]; 2]; 2],
    axis: Axis,
    diameter: f64,
    depth: f64,
}

fn envelope_intervals(corners: [[[f64; 3]; 2]; 2]) -> [[[f64; 2]; 3]; 2] {
    corners.map(|patch| {
        std::array::from_fn::<_, 3, _>(|axis| {
            [
                patch[0][axis].min(patch[1][axis]),
                patch[0][axis].max(patch[1][axis]),
            ]
        })
    })
}

fn envelope_scale(corners: &[[[f64; 3]; 2]; 2], diameter: f64, depth: f64) -> f64 {
    corners
        .iter()
        .flatten()
        .flatten()
        .chain([&diameter, &depth])
        .map(|value| value.abs())
        .fold(1.0, f64::max)
}

impl DrilledHoleEnvelopeLayout {
    fn intervals(&self) -> [[[f64; 2]; 3]; 2] {
        envelope_intervals(self.corners)
    }

    fn scale(&self) -> f64 {
        envelope_scale(&self.corners, self.diameter, self.depth)
    }

    fn axial_delta(&self) -> f64 {
        self.corners[0][1][self.axis.index()] - self.corners[0][0][self.axis.index()]
    }

    fn new(corners: [[[f64; 3]; 2]; 2], diameter: f64, depth: f64) -> Option<Self> {
        corners
            .iter()
            .flatten()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(())?;
        let scale = envelope_scale(&corners, diameter, depth);
        (diameter > EPS_DIAMETER_NONZERO * scale && depth > EPS_DIAMETER_NONZERO * scale)
            .then_some(())?;
        let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
        let intervals = envelope_intervals(corners);
        let shared = |axis: Axis| {
            close(intervals[0][axis.index()][0], intervals[1][axis.index()][0])
                && close(intervals[0][axis.index()][1], intervals[1][axis.index()][1])
        };
        let span = |axis: Axis| {
            intervals[0][axis.index()][1].max(intervals[1][axis.index()][1])
                - intervals[0][axis.index()][0].min(intervals[1][axis.index()][0])
        };
        let axis = exactly_one(
            Axis::ALL
                .into_iter()
                .filter(|axis| shared(*axis) && close(span(*axis), depth)),
        )?;
        let axial_deltas = corners.map(|patch| patch[1][axis.index()] - patch[0][axis.index()]);
        (close(axial_deltas[0], axial_deltas[1]) && close(axial_deltas[0].abs(), depth))
            .then_some(())?;
        Some(Self {
            corners,
            axis,
            diameter,
            depth,
        })
    }

    fn close(&self, left: f64, right: f64) -> bool {
        (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * self.scale()
    }

    fn shared(&self, axis: Axis) -> bool {
        self.close(
            self.intervals()[0][axis.index()][0],
            self.intervals()[1][axis.index()][0],
        ) && self.close(
            self.intervals()[0][axis.index()][1],
            self.intervals()[1][axis.index()][1],
        )
    }

    fn adjacent(&self, axis: Axis) -> bool {
        self.close(
            self.intervals()[0][axis.index()][1],
            self.intervals()[1][axis.index()][0],
        ) || self.close(
            self.intervals()[1][axis.index()][1],
            self.intervals()[0][axis.index()][0],
        )
    }

    fn span(&self, axis: Axis) -> f64 {
        self.intervals()[0][axis.index()][1].max(self.intervals()[1][axis.index()][1])
            - self.intervals()[0][axis.index()][0].min(self.intervals()[1][axis.index()][0])
    }

    fn placement(&self, radial_coordinates: [f64; 2]) -> (Point3, Vector3) {
        let mut position = [0.0; 3];
        position[self.axis.index()] = f64::midpoint(
            self.corners[0][0][self.axis.index()],
            self.corners[1][0][self.axis.index()],
        );
        for (radial_axis, coordinate) in self.axis.complement().into_iter().zip(radial_coordinates)
        {
            position[radial_axis.index()] = coordinate;
        }
        let mut direction = [0.0; 3];
        direction[self.axis.index()] = self.axial_delta().signum();
        (Point3::from(position), Vector3::from(direction))
    }
}

pub(in crate::decode) fn drilled_hole_placement_from_corner_envelopes(
    corners: [[[f64; 3]; 2]; 2],
    diameter: f64,
    depth: f64,
) -> Option<(Point3, Vector3)> {
    let layout = DrilledHoleEnvelopeLayout::new(corners, diameter, depth)?;
    let radial_forms = layout.axis.complement().map(|radial_axis| {
        (
            layout.shared(radial_axis),
            layout.adjacent(radial_axis),
            layout.span(radial_axis),
        )
    });
    let complementary =
        (radial_forms[0].0 && radial_forms[1].1) || (radial_forms[0].1 && radial_forms[1].0);
    if complementary
        && radial_forms
            .iter()
            .all(|(_, _, span)| layout.close(*span, diameter))
    {
        let radial_coordinates = layout.axis.complement().map(|radial_axis| {
            f64::midpoint(
                layout.intervals()[0][radial_axis.index()][0]
                    .min(layout.intervals()[1][radial_axis.index()][0]),
                layout.intervals()[0][radial_axis.index()][1]
                    .max(layout.intervals()[1][radial_axis.index()][1]),
            )
        });
        return Some(layout.placement(radial_coordinates));
    }

    let nonshared_bounds = layout.axis.complement().map(|radial_axis| {
        let intervals = layout.intervals().map(|patch| patch[radial_axis.index()]);
        match (
            layout.close(intervals[0][0], intervals[1][0]),
            layout.close(intervals[0][1], intervals[1][1]),
        ) {
            (true, false) => Some([intervals[0][1], intervals[1][1]]),
            (false, true) => Some([intervals[0][0], intervals[1][0]]),
            _ => None,
        }
    });
    let common_diameter = layout.axis.complement().map(|radial_axis| {
        layout.shared(radial_axis) && layout.close(layout.span(radial_axis), diameter)
    });
    let (clipped_index, [first, second]) = match (common_diameter, nonshared_bounds) {
        ([true, false], [None, Some(bounds)]) => (1, bounds),
        ([false, true], [Some(bounds), None]) => (0, bounds),
        _ => return None,
    };
    layout
        .close((first - second).abs(), diameter)
        .then_some(())?;
    let radial_coordinates = std::array::from_fn(|index| {
        let radial_axis = layout.axis.complement()[index];
        if index == clipped_index {
            f64::midpoint(first, second)
        } else {
            f64::midpoint(
                layout.intervals()[0][radial_axis.index()][0],
                layout.intervals()[0][radial_axis.index()][1],
            )
        }
    });
    Some(layout.placement(radial_coordinates))
}

pub(in crate::decode) fn clipped_drilled_hole_placement_from_cone_points(
    corners: [[[f64; 3]; 2]; 2],
    cone_points: [[f64; 3]; 2],
    diameter: f64,
    depth: f64,
) -> Option<(Point3, Vector3)> {
    let layout = DrilledHoleEnvelopeLayout::new(corners, diameter, depth)?;
    cone_points
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    let diameter_axis =
        exactly_one(
            layout.axis.complement().iter().copied().filter(|axis| {
                layout.adjacent(*axis) && layout.close(layout.span(*axis), diameter)
            }),
        )?;
    let clipped_axis = layout
        .axis
        .complement()
        .iter()
        .copied()
        .find(|axis| *axis != diameter_axis)?;
    (layout.shared(clipped_axis)
        && layout.span(clipped_axis) > 0.0
        && !layout.close(layout.span(clipped_axis), diameter))
    .then_some(())?;
    (0..2)
        .all(|patch| {
            layout.close(
                cone_points[patch][layout.axis.index()],
                corners[patch][0][layout.axis.index()],
            ) && layout.axis.complement().iter().all(|axis| {
                layout.close(
                    cone_points[patch][axis.index()],
                    corners[patch][1][axis.index()],
                )
            })
        })
        .then_some(())?;
    layout
        .close(
            cone_points[0][clipped_axis.index()],
            cone_points[1][clipped_axis.index()],
        )
        .then_some(())?;
    let radial_coordinates = layout.axis.complement().map(|axis| {
        if axis == clipped_axis {
            f64::midpoint(cone_points[0][axis.index()], cone_points[1][axis.index()])
        } else {
            f64::midpoint(
                layout.intervals()[0][axis.index()][0].min(layout.intervals()[1][axis.index()][0]),
                layout.intervals()[0][axis.index()][1].max(layout.intervals()[1][axis.index()][1]),
            )
        }
    });
    Some(layout.placement(radial_coordinates))
}

pub(in crate::decode) fn paired_corner_envelope_axis_spans(
    first: [[f64; 3]; 2],
    second: [[f64; 3]; 2],
) -> Option<[[Option<PositiveLength>; 2]; 3]> {
    first
        .iter()
        .chain(&second)
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    let intervals = |corners: [[f64; 3]; 2]| {
        std::array::from_fn::<_, 3, _>(|axis| {
            let values = [corners[0][axis], corners[1][axis]];
            [values[0].min(values[1]), values[0].max(values[1])]
        })
    };
    let first = intervals(first);
    let second = intervals(second);
    let spans = std::array::from_fn::<_, 3, _>(|axis| {
        let common_lower = (FiniteReal::new(first[axis][0]))
            .zip(FiniteReal::new(second[axis][0]))
            .is_some_and(|(first, second)| approximately_equal(first, second));
        let common_upper = (FiniteReal::new(first[axis][1]))
            .zip(FiniteReal::new(second[axis][1]))
            .is_some_and(|(first, second)| approximately_equal(first, second));
        let shared = common_lower && common_upper;
        let shared_span = shared.then(|| {
            f64::midpoint(
                first[axis][1] - first[axis][0],
                second[axis][1] - second[axis][0],
            )
        });
        let shared_span = shared_span.and_then(PositiveLength::new);
        let adjacent = (FiniteReal::new(first[axis][1]))
            .zip(FiniteReal::new(second[axis][0]))
            .is_some_and(|(first, second)| approximately_equal(first, second))
            || (FiniteReal::new(second[axis][1]))
                .zip(FiniteReal::new(first[axis][0]))
                .is_some_and(|(first, second)| approximately_equal(first, second));
        let adjacent_span = adjacent
            .then(|| first[axis][1].max(second[axis][1]) - first[axis][0].min(second[axis][0]));
        let one_sided_span = (common_lower != common_upper).then(|| {
            if common_lower {
                (first[axis][1] - second[axis][1]).abs()
            } else {
                (first[axis][0] - second[axis][0]).abs()
            }
        });
        let paired_span = adjacent_span
            .or(one_sided_span)
            .and_then(PositiveLength::new);
        [shared_span, paired_span]
    });
    Some(spans)
}

pub(in crate::decode) fn simple_drilled_hole_dimensions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    observed_envelope_spans: Option<[[Option<PositiveLength>; 2]; 3]>,
    family: SimpleDrilledDimensionFamily,
) -> Result<Option<(f64, f64, f64)>, CodecError> {
    simple_drilled_hole_dimension_values(
        ctx,
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.identity.id() == 911)
            .filter_map(|definition| definition.dimensions.as_ref()),
        observed_envelope_spans,
        family,
    )
}

pub(in crate::decode) fn simple_drilled_hole_dimension_values<'a>(
    ctx: &DecodeContext<'_>,
    tables: impl Iterator<Item = &'a crate::feature::definitions::FeatureDimensionTable>,
    observed_envelope_spans: Option<[[Option<PositiveLength>; 2]; 3]>,
    family: SimpleDrilledDimensionFamily,
) -> Result<Option<(f64, f64, f64)>, CodecError> {
    let depth_external_id = family.depth_external_id();
    let mut first: Option<(f64, f64, f64)> = None;
    for table in
        tables.filter(|table| feature_dimension_table_complete(table) && table.rows.len() == 3)
    {
        let mut signature_matches = true;
        for (external_id, dimension_type) in [(0, 2), (1, 10), (depth_external_id, 2)] {
            if ctx
                .admit_iter(&table.rows, "creo drilled dimension signature count")?
                .filter(|row| {
                    row.external_id == external_id && row.dimension_type == dimension_type
                })
                .count()
                != 1
            {
                signature_matches = false;
                break;
            }
        }
        if !signature_matches {
            continue;
        }
        let Some(candidate) =
            (|| {
                let value = |external_id, dimension_type| {
                    let row = exactly_one(table.rows.iter().filter(|row| {
                        row.external_id == external_id && row.dimension_type == dimension_type
                    }))?;
                    row.value.resolved().filter(|value| value.is_finite())
                };
                let bore_radius = value(0, 2)?;
                let signed_depth = value(depth_external_id, 2)?;
                let bore_diameter = 2.0 * bore_radius;
                (bore_diameter.is_finite() && bore_diameter > 0.0 && signed_depth != 0.0)
                    .then_some(())?;
                let blind_depth = signed_depth.abs();
                if observed_envelope_spans.is_some_and(|spans| {
                    !dimension_pair_matches_envelope_spans(bore_diameter, blind_depth, spans)
                }) {
                    return Some(None);
                }
                let drill_point_angle = value(1, 10)?;
                (drill_point_angle > 0.0 && drill_point_angle < std::f64::consts::PI)
                    .then_some(Some((bore_diameter, drill_point_angle, blind_depth)))
            })()
        else {
            return Ok(None);
        };
        let Some(candidate) = candidate else {
            continue;
        };
        if let Some(first_value) = first {
            let agrees = [candidate.0, candidate.1, candidate.2]
                .into_iter()
                .zip([first_value.0, first_value.1, first_value.2])
                .all(|(candidate, first)| {
                    (FiniteReal::new(candidate))
                        .zip(FiniteReal::new(first))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                });
            if !agrees {
                return Ok(None);
            }
        } else {
            first = Some(candidate);
        }
    }
    Ok(first)
}

pub(in crate::decode) fn dimension_pair_matches_envelope_spans(
    bore_diameter: f64,
    blind_depth: f64,
    spans: [[Option<PositiveLength>; 2]; 3],
) -> bool {
    for diameter_axis in 0..3 {
        for depth_axis in 0..3 {
            if diameter_axis != depth_axis
                && spans[diameter_axis].into_iter().flatten().any(|span| {
                    (FiniteReal::new(span.get()))
                        .zip(FiniteReal::new(bore_diameter))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                })
                && spans[depth_axis].into_iter().flatten().any(|span| {
                    (FiniteReal::new(span.get()))
                        .zip(FiniteReal::new(blind_depth))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                })
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod resource_tests {
    use super::*;

    fn axis_placement_with_limit(
        limit: u64,
    ) -> Result<Option<cadmpeg_ir::features::holes::HolePlacement>, CodecError> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: 1,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 7,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        });
        let frame = crate::surface::PositionalCylinderFrame::new(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            0.75,
            Some(2.0),
        )
        .expect("cylinder frame");
        scan.surfaces
            .parameters
            .push(crate::surface::SurfaceParameterRecord {
                surface_id: 1,
                body: Vec::new(),
                scalar_tokens: Vec::new(),
                opaque_spans: Vec::new(),
                scalar_frames: Vec::new(),
                carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                    crate::surface::InlineSurfaceCarrier::Cylinder {
                        frame,
                        split_bounds: None,
                    },
                ),
                boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
                offset: 0,
                body_offset: 0,
            });
        let table = crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![crate::feature::entity::dummy_table_entry(1)],
            &BTreeSet::new(),
            0,
        )
        .with_surface_ids([1]);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        simple_drilled_hole_axis_placement(&ctx, &scan, &table, 1.5)
    }

    fn axis_placement_limit_error(limit: u64) -> CodecError {
        axis_placement_with_limit(limit).expect_err("next collection exceeds limit")
    }

    #[test]
    fn drilled_axis_placement_preserves_service_carrier() {
        assert!(matches!(
            axis_placement_with_limit(3).expect("service resources"),
            Some(cadmpeg_ir::features::holes::HolePlacement::Axis { .. })
        ));
    }

    #[test]
    fn drilled_cylinder_id_nodes_refuse_collection_limit() {
        assert!(
            matches!(axis_placement_limit_error(0), CodecError::ResourceLimit(resource)
            if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && resource.operation == "creo drilled cylinder ID nodes")
        );
    }

    #[test]
    fn drilled_available_frames_refuse_collection_limit() {
        assert!(
            matches!(axis_placement_limit_error(1), CodecError::ResourceLimit(resource)
            if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && resource.operation == "creo available positional cylinder frames")
        );
    }

    #[test]
    fn drilled_frame_copies_refuse_collection_limit() {
        assert!(
            matches!(axis_placement_limit_error(2), CodecError::ResourceLimit(resource)
            if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && resource.operation == "creo drilled cylinder frame copies")
        );
    }
}
