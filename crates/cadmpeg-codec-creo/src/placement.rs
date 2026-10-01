// SPDX-License-Identifier: Apache-2.0
//! Model-space frames resolved from feature-section datum references.

use crate::datum::DatumPlaneRecord;
use crate::decode::uniqueness::exactly_one;
use crate::feature::definitions::ReferencePlanes;
use crate::feature::definitions::{
    placement_instructions, BinaryFlag, FeatureDefinition, FeatureParameterFrameKind,
    FeatureSegmentKind,
};
use crate::feature::entity::FeatureEntityTable;
use crate::feature::rows::{AffectedIdKind, FeatureAffectedIds, FeatureGeometryTable};
use crate::surface::{
    unique_surface_row, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
    SurfaceKind, SurfaceParameterRecord, SurfaceRow,
};
use crate::vecmath::{add, cross, dot, local_system_lanes, normalize, scale, unit_length};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;
use std::collections::BTreeSet;

/// Tolerance of every placement quantity this module reconstructs by arithmetic.
const EPS_PLACEMENT_GEOMETRY: f64 = 1.0e-9;
/// Tolerance of every placement quantity this module reads or derives exactly.
const EPS_PLACEMENT_EXACT_GEOMETRY: f64 = 1.0e-12;

/// A feature's right-handed section-to-model rigid frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSectionTransform {
    /// Owning `feat_defs_<id>` record identifier.
    pub(crate) definition_id: u32,
    /// Unique modeling feature identifier inside the definition, when present.
    pub(crate) feature_id: Option<u32>,
    /// Model-space point corresponding to section coordinate `[0, 0, 0]`.
    origin: [f64; 3],
    /// Model-space direction of increasing section `u`.
    u_axis: [f64; 3],
    /// Model-space direction of increasing section `v`.
    v_axis: [f64; 3],
    /// Byte offset of the source `gsec3d_ptr` record.
    pub(crate) offset: usize,
}

impl FeatureSectionTransform {
    /// Constructs a finite right-handed orthonormal section frame.
    ///
    /// `unit` carries the finiteness admission of both axes: a non-finite component makes
    /// `dot(axis, axis)` `NaN` or `+inf`, and `squared.is_finite()` below refuses both. The
    /// origin reaches no such test and states its own.
    ///
    /// This is not the admission of [`cadmpeg_ir::units::OrthonormalFrame3::new`]. The
    /// orthogonality half bounds the dot product at the same value. The unit half bounds the
    /// squared norm relative to itself, where that admission bounds the norm absolutely, so a
    /// stored length of `1 + d` is admitted here for `|d|` up to about half the bound there.
    pub(crate) fn new(
        definition_id: u32,
        feature_id: Option<u32>,
        origin: [f64; 3],
        u_axis: [f64; 3],
        v_axis: [f64; 3],
        offset: usize,
    ) -> Option<Self> {
        let unit = |axis| {
            let squared = dot(axis, axis);
            squared.is_finite()
                && (squared - 1.0).abs() <= EPS_PLACEMENT_GEOMETRY * squared.abs().max(1.0)
        };
        (origin.iter().all(|value| value.is_finite())
            && unit(u_axis)
            && unit(v_axis)
            && dot(u_axis, v_axis).abs() <= EPS_PLACEMENT_GEOMETRY)
            .then_some(Self {
                definition_id,
                feature_id,
                origin,
                u_axis,
                v_axis,
                offset,
            })
    }

    /// Model-space section origin.
    pub(crate) fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// Model-space direction of increasing section u.
    pub(crate) fn u_axis(&self) -> [f64; 3] {
        self.u_axis
    }

    /// Model-space direction of increasing section v.
    pub(crate) fn v_axis(&self) -> [f64; 3] {
        self.v_axis
    }

    fn flipped_v(self) -> Self {
        Self {
            v_axis: scale(self.v_axis, -1.0),
            ..self
        }
    }

    fn flipped_u_and_v(self) -> Self {
        Self {
            u_axis: scale(self.u_axis, -1.0),
            v_axis: scale(self.v_axis, -1.0),
            ..self
        }
    }

    /// Right-handed normal derived from the section axes.
    pub(crate) fn normal(&self) -> [f64; 3] {
        cross(self.u_axis, self.v_axis)
    }

    /// Returns the normal as the geometry vector the carrier constructors take.
    pub(crate) fn normal_vector(&self) -> Vector3 {
        Vector3::from(self.normal())
    }

    /// Returns the section u direction as the geometry vector the carrier constructors take.
    pub(crate) fn u_axis_vector(&self) -> Vector3 {
        Vector3::from(self.u_axis)
    }
}

pub(crate) struct PlacementSources<'a> {
    pub(crate) datums: &'a [DatumPlaneRecord],
    pub(crate) surface_rows: &'a [SurfaceRow],
    pub(crate) model_planes: &'a [PlaneLocalSystem],
    pub(crate) outline_planes: &'a [OutlinePlane],
    pub(crate) plane_envelopes: &'a [PlaneEnvelopeRecord],
    pub(crate) surface_parameters: &'a [SurfaceParameterRecord],
    pub(crate) geometry_tables: &'a [FeatureGeometryTable],
    pub(crate) affected_ids: &'a [FeatureAffectedIds],
}

/// Plane in scalar form: `dot(normal, point) = offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SignedPlaneEquation {
    normal: [f64; 3],
    offset: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SectionFrameCandidate {
    reference_id: u32,
    sketch: SignedPlaneEquation,
    reference: SignedPlaneEquation,
}

fn generated_cylinder_section_transform(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Option<FeatureSectionTransform>, CodecError> {
    let Some(feature_id) = definition.identity.owner_feature_id() else {
        return Ok(None);
    };
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    if !segments.is_complete() {
        return Ok(None);
    }
    let Some(variables) = definition.variables.as_ref() else {
        return Ok(None);
    };
    let points = variables.reconciled_points(ctx)?;
    if !points.ambiguous.is_empty() {
        return Ok(None);
    }
    let mut correspondences = Vec::<([f64; 2], [f64; 3], UnitVector3, usize)>::new();
    for (_, entry) in entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| table.entries.iter().map(move |entry| (table, entry)))
        .filter(|(table, entry)| table.contains_surface_id(entry.entity_id))
    {
        let Some(external_id) = entry.source_entity_id() else {
            continue;
        };
        let Some(segment) = segments.segment(external_id) else {
            continue;
        };
        if !matches!(segment.kind, FeatureSegmentKind::Arc(_)) {
            continue;
        }
        let Some(center_id) = segment.center_id else {
            continue;
        };
        let Some([Some(u), Some(v)]) = points.points.get(&center_id).copied() else {
            continue;
        };
        let Some(row) = unique_surface_row(sources.surface_rows, entry.entity_id)
            .filter(|row| row.feature_id == feature_id && row.kind == SurfaceKind::Cylinder)
        else {
            continue;
        };
        let parameters = exactly_one(
            sources
                .surface_parameters
                .iter()
                .filter(|record| record.surface_id == row.id),
        );
        let Some(parameters) = parameters else {
            continue;
        };
        let Some(frame) = parameters.positional_cylinder_frame() else {
            continue;
        };
        ctx.reserve_vec(
            &mut correspondences,
            1,
            "creo cylinder placement correspondences",
        )?;
        correspondences.push((
            [u, v],
            frame.frame().origin(),
            *frame.frame().orthonormal_frame().axis(),
            parameters.offset,
        ));
    }
    let result = (|| {
        let first = correspondences.first()?;
        let normal = unit_length(first.2);
        let scale = correspondences
            .iter()
            .flat_map(|(local, model, _, _)| local.iter().chain(model))
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let close = |left: f64, right: f64| {
            (left - right).abs() <= EPS_PLACEMENT_GEOMETRY * left.abs().max(right.abs()).max(1.0)
        };
        correspondences
            .iter()
            .all(|(_, _, axis, _)| {
                unit_length(*axis)
                    .iter()
                    .zip(normal)
                    .all(|(left, right)| close(*left, right))
            })
            .then_some(())?;

        let mut frame = None::<([f64; 3], [f64; 3], [f64; 3])>;
        for second in correspondences.iter().skip(1) {
            let local = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
            let model = std::array::from_fn::<_, 3, _>(|index| second.1[index] - first.1[index]);
            let local_squared = dot([local[0], local[1], 0.0], [local[0], local[1], 0.0]);
            if local_squared <= 1e-24 * scale * scale
                || !close(dot(model, model), local_squared)
                || !close(dot(model, normal), 0.0)
            {
                continue;
            }
            let normal_cross_model = cross(normal, model);
            let u_axis = std::array::from_fn(|index| {
                (local[0] * model[index] - local[1] * normal_cross_model[index]) / local_squared
            });
            let Some(u_axis) = normalize(u_axis) else {
                continue;
            };
            let v_axis = cross(normal, u_axis);
            let origin = std::array::from_fn(|index| {
                first.1[index] - first.0[0] * u_axis[index] - first.0[1] * v_axis[index]
            });
            let candidate = (origin, u_axis, v_axis);
            if !correspondences.iter().all(|(local, model, _, _)| {
                (0..3).all(|index| {
                    close(
                        candidate.0[index]
                            + local[0] * candidate.1[index]
                            + local[1] * candidate.2[index],
                        model[index],
                    )
                })
            }) {
                continue;
            }
            if let Some(previous) = frame {
                if !candidate
                    .0
                    .iter()
                    .chain(candidate.1.iter())
                    .chain(candidate.2.iter())
                    .zip(
                        previous
                            .0
                            .iter()
                            .chain(previous.1.iter())
                            .chain(previous.2.iter()),
                    )
                    .all(|(left, right)| close(*left, *right))
                {
                    return None;
                }
            } else {
                frame = Some(candidate);
            }
        }
        let frame = frame?;
        let offset = definition.section_3d.as_ref().map_or_else(
            || correspondences.iter().map(|item| item.3).min(),
            |section| Some(section.offset),
        )?;
        FeatureSectionTransform::new(
            definition.identity.id(),
            Some(feature_id),
            frame.0,
            frame.1,
            frame.2,
            offset,
        )
    })();
    Ok(result)
}

fn generated_planar_section_transform(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Option<FeatureSectionTransform>, CodecError> {
    let Some(feature_id) = definition.identity.owner_feature_id() else {
        return Ok(None);
    };
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    if !segments.is_complete() {
        return Ok(None);
    }
    let Some(variables) = definition.variables.as_ref() else {
        return Ok(None);
    };
    let crate::feature::definitions::ReconciledPoints {
        points,
        ambiguous: conflicting_points,
    } = variables.reconciled_points(ctx)?;
    if !conflicting_points.is_empty() {
        return Ok(None);
    }
    let mut table = None;
    for candidate in entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
    {
        if generated_planar_table_shape(ctx, candidate)? {
            if table.is_some() {
                return Ok(None);
            }
            table = Some(candidate);
        }
    }
    let Some(table) = table else {
        return Ok(None);
    };
    let offset = definition
        .section_3d
        .as_ref()
        .map_or(table.offset, |section| section.offset);
    let generated_plane_equation = |entry: &crate::feature::entity::FeatureEntityTableEntry| {
        let mut matches = sources
            .outline_planes
            .iter()
            .filter(|plane| plane.surface_id == entry.entity_id);
        if let Some(plane) = matches.next() {
            return matches
                .next()
                .is_none()
                .then_some((plane.normal(), dot(plane.normal(), plane.origin)));
        }
        let mut envelopes = sources
            .plane_envelopes
            .iter()
            .filter(|record| record.surface_id == entry.entity_id);
        let envelope = envelopes.next()?;
        envelopes.next().is_none().then_some(())?;
        let corners = match &envelope.envelope {
            PlaneEnvelope::Standard { corners_3d, .. }
            | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
        };
        let axis = (0..3).find(|axis| envelope.corner_coordinate_equal[*axis] == Some(true))?;
        let coordinate = corners[0][axis]?;
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        Some((normal, coordinate))
    };
    let (Some(first_cap), Some(second_cap)) = (
        generated_plane_equation(&table.entries[0]),
        generated_plane_equation(&table.entries[1]),
    ) else {
        return Ok(None);
    };
    let caps = [first_cap, second_cap];
    let mut sides = Vec::new();
    for entry in table.entries[2..]
        .iter()
        .filter(|entry| table.contains_surface_id(entry.entity_id))
    {
        let Some(model_plane) = generated_plane_equation(entry) else {
            continue;
        };
        let Some(segment) = entry.source_entity_id().and_then(|id| segments.segment(id)) else {
            continue;
        };
        if !matches!(segment.kind, FeatureSegmentKind::Line(_)) {
            return Ok(None);
        }
        let point = |point_id| {
            let point = points.get(&point_id)?;
            Some([point[0]?, point[1]?])
        };
        let Some(start) = point(segment.point_ids()[0]) else {
            return Ok(None);
        };
        let Some(end) = point(segment.point_ids()[1]) else {
            return Ok(None);
        };
        let direction = [end[0] - start[0], end[1] - start[1]];
        let length = direction[0].hypot(direction[1]);
        if !length.is_finite() || length <= EPS_PLACEMENT_EXACT_GEOMETRY {
            return Ok(None);
        }
        let local_normal = [direction[1] / length, -direction[0] / length];
        let local_offset = local_normal[0].mul_add(start[0], local_normal[1] * start[1]);
        let (model_normal, model_offset) = model_plane;
        let magnitude = dot(model_normal, model_normal).sqrt();
        if !magnitude.is_finite() || magnitude <= EPS_PLACEMENT_EXACT_GEOMETRY {
            return Ok(None);
        }
        ctx.reserve_vec(&mut sides, 1, "creo planar placement sides")?;
        sides.push((
            local_normal,
            local_offset,
            scale(model_normal, magnitude.recip()),
            model_offset / magnitude,
        ));
    }

    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_PLACEMENT_GEOMETRY * left.abs().max(right.abs()).max(1.0)
    };
    let vectors_close = |left: [f64; 3], right: [f64; 3]| {
        left.into_iter()
            .zip(right)
            .all(|(left, right)| close(left, right))
    };
    let mut candidates = Vec::new();
    for first_index in 0..sides.len() {
        for second_index in first_index + 1..sides.len() {
            let first = sides[first_index];
            let second = sides[second_index];
            let determinant = first.0[0].mul_add(second.0[1], -(first.0[1] * second.0[0]));
            if determinant.abs() <= EPS_PLACEMENT_GEOMETRY {
                continue;
            }
            for first_sign in [-1.0, 1.0] {
                for second_sign in [-1.0, 1.0] {
                    let first_normal = scale(first.2, first_sign);
                    let second_normal = scale(second.2, second_sign);
                    let u_axis = std::array::from_fn(|axis| {
                        (second.0[1] * first_normal[axis] - first.0[1] * second_normal[axis])
                            / determinant
                    });
                    let v_axis = std::array::from_fn(|axis| {
                        (-second.0[0] * first_normal[axis] + first.0[0] * second_normal[axis])
                            / determinant
                    });
                    let normal = cross(u_axis, v_axis);
                    let cap_alignment = dot(normal, caps[0].0);
                    if !close(cap_alignment.abs(), 1.0) {
                        continue;
                    }
                    let cap_offset = if cap_alignment.is_sign_negative() {
                        -caps[0].1
                    } else {
                        caps[0].1
                    };
                    let side_coordinate = |side: &([f64; 2], f64, [f64; 3], f64)| {
                        let predicted = add(scale(u_axis, side.0[0]), scale(v_axis, side.0[1]));
                        let alignment = dot(predicted, side.2);
                        close(alignment.abs(), 1.0).then(|| {
                            let offset = if alignment.is_sign_negative() {
                                -side.3
                            } else {
                                side.3
                            };
                            (predicted, offset - side.1)
                        })
                    };
                    let Some((_, first_coordinate)) = side_coordinate(&first) else {
                        continue;
                    };
                    let Some((_, second_coordinate)) = side_coordinate(&second) else {
                        continue;
                    };
                    let origin_u = (second.0[1] * first_coordinate
                        - first.0[1] * second_coordinate)
                        / determinant;
                    let origin_v = (-second.0[0] * first_coordinate
                        + first.0[0] * second_coordinate)
                        / determinant;
                    let origin = add(
                        add(scale(u_axis, origin_u), scale(v_axis, origin_v)),
                        scale(normal, cap_offset),
                    );
                    if sides.iter().any(|side| {
                        side_coordinate(side).is_none_or(|(predicted, coordinate)| {
                            !close(dot(predicted, origin), coordinate)
                        })
                    }) {
                        continue;
                    }
                    let second_cap_alignment = dot(normal, caps[1].0);
                    if !close(second_cap_alignment.abs(), 1.0) {
                        continue;
                    }
                    let second_cap_offset = if second_cap_alignment.is_sign_negative() {
                        -caps[1].1
                    } else {
                        caps[1].1
                    };
                    if close(second_cap_offset, cap_offset) {
                        continue;
                    }
                    let Some(candidate) = FeatureSectionTransform::new(
                        definition.identity.id(),
                        Some(feature_id),
                        origin,
                        u_axis,
                        v_axis,
                        offset,
                    ) else {
                        continue;
                    };
                    if !candidates.iter().any(|existing: &FeatureSectionTransform| {
                        vectors_close(existing.origin(), candidate.origin())
                            && vectors_close(existing.u_axis(), candidate.u_axis())
                            && vectors_close(existing.v_axis(), candidate.v_axis())
                            && vectors_close(existing.normal(), candidate.normal())
                    }) {
                        ctx.reserve_vec(&mut candidates, 1, "creo planar placement candidates")?;
                        candidates.push(candidate);
                    }
                }
            }
        }
    }
    let [transform] = candidates.as_slice() else {
        return Ok(None);
    };
    Ok(Some(transform.clone()))
}

fn generated_planar_table_shape(
    ctx: &DecodeContext<'_>,
    table: &FeatureEntityTable,
) -> Result<bool, CodecError> {
    let [first, second, rest @ ..] = table.entries.as_slice() else {
        return Ok(false);
    };
    if first.class_id() != 204
        || second.class_id() != 203
        || rest.is_empty()
        || !rest.iter().all(|entry| entry.source_entity_id().is_some())
    {
        return Ok(false);
    }
    let mut entry_ids = BTreeSet::new();
    for entry in &table.entries {
        if entry_ids.contains(&entry.entity_id) {
            return Ok(false);
        }
        ctx.charge_collection_items(1, "creo generated planar table entry nodes")?;
        entry_ids.insert(entry.entity_id);
    }
    Ok(true)
}

fn plane_equation(
    id: u32,
    datums: &[DatumPlaneRecord],
    model_planes: &[PlaneLocalSystem],
    outline_planes: &[OutlinePlane],
) -> Option<SignedPlaneEquation> {
    let mut matching_datums = datums.iter().filter(|datum| datum.id == id);
    let datum = matching_datums.next();
    let duplicate_datums = matching_datums.next().is_some();
    let mut matching_model_planes = model_planes.iter().filter(|plane| plane.surface_id == id);
    let model_plane = matching_model_planes.next();
    let duplicate_model_planes = matching_model_planes.next().is_some();
    let model_equation = match (model_plane, duplicate_model_planes) {
        (Some(plane), false) => {
            let frame = plane.frame();
            frame
                .normal()
                .zip(frame.origin)
                .map(|(normal, origin)| SignedPlaneEquation {
                    normal,
                    offset: dot(normal, origin),
                })
        }
        _ => None,
    };
    let outline_equation =
        exactly_one(outline_planes.iter().filter(|plane| plane.surface_id == id)).map(|plane| {
            SignedPlaneEquation {
                normal: plane.normal(),
                offset: dot(plane.normal(), plane.origin),
            }
        });
    // The datum-geometry and model-surface identifiers are separate namespaces;
    // a numeric collision supplies no rule for choosing between their equations.
    if datum.is_some()
        && !duplicate_datums
        && (model_equation.is_some() || outline_equation.is_some())
    {
        return None;
    }
    if duplicate_datums {
        return None;
    }
    if let Some(datum) = datum {
        return Some(SignedPlaneEquation {
            normal: datum.plane().normal(),
            offset: datum.plane().offset(),
        });
    }
    if let Some(equation) = model_equation {
        return Some(equation);
    }
    if duplicate_model_planes {
        return None;
    }
    outline_equation
}

fn definition_local_plane_equation(definition: &FeatureDefinition) -> Option<SignedPlaneEquation> {
    let [.., raw_normal, origin] =
        local_system_lanes(unique_complete_local_system(definition)?.get());
    let normal = normalize(raw_normal)?;
    Some(SignedPlaneEquation {
        normal,
        offset: dot(normal, origin),
    })
}

pub(crate) fn unique_complete_local_system(
    definition: &FeatureDefinition,
) -> Option<cadmpeg_ir::units::FiniteVector<12>> {
    let mut frames = definition
        .parameter_frames
        .iter()
        .filter(|frame| frame.kind == FeatureParameterFrameKind::LocalSystem)
        .filter_map(|frame| frame.decoded_values.as_ref());
    let values = *frames.next()?;
    frames.next().is_none().then_some(values)
}

fn reference_flip_for_reference(
    section: &crate::feature::definitions::FeatureSection3d,
    reference_id: Option<u32>,
) -> Option<BinaryFlag> {
    match &section.reference_planes {
        ReferencePlanes::Named(_) => section.orientation.reference_flip,
        ReferencePlanes::Positional(rows) => {
            let reference_id = reference_id?;
            exactly_one(
                rows.iter()
                    .filter(|row| row.plane_entity_id == reference_id),
            )
            .and_then(|row| row.reference_flip)
        }
    }
}

fn unique_carrier_reference_id(
    section: &crate::feature::definitions::FeatureSection3d,
) -> Option<u32> {
    if let Some(id) = section.reference_plane_datum_geometry_id {
        return Some(id);
    }
    let mut ids = section.reference_planes.entity_ids();
    let id = ids.next()?;
    ids.all(|candidate| candidate == id).then_some(id)
}

fn apply_section_orientation(
    mut transform: FeatureSectionTransform,
    section: &crate::feature::definitions::FeatureSection3d,
) -> FeatureSectionTransform {
    if section.sketch_plane_flip == Some(BinaryFlag::Set) {
        transform = transform.flipped_v();
    }
    if section.orientation.section_flip == Some(BinaryFlag::Set) {
        transform = transform.flipped_v();
    }
    if reference_flip_for_reference(section, unique_carrier_reference_id(section))
        == Some(BinaryFlag::Set)
    {
        transform = transform.flipped_u_and_v();
    }
    transform
}

fn definition_local_frame_transform(
    definition: &FeatureDefinition,
    section: &crate::feature::definitions::FeatureSection3d,
) -> Option<FeatureSectionTransform> {
    let feature_id = definition.identity.owner_feature_id()?;
    let [stored_u_axis, _, stored_axis, origin] =
        local_system_lanes(unique_complete_local_system(definition)?.get());
    let mut u_axis = normalize(stored_u_axis)?;
    let raw_normal = normalize(stored_axis)?;
    (dot(u_axis, raw_normal).abs() <= EPS_PLACEMENT_EXACT_GEOMETRY).then_some(())?;
    let mut normal = raw_normal;
    if section.sketch_plane_flip == Some(BinaryFlag::Set) {
        normal = scale(normal, -1.0);
    }
    if section.orientation.section_flip == Some(BinaryFlag::Set) {
        normal = scale(normal, -1.0);
    }
    if reference_flip_for_reference(section, None) == Some(BinaryFlag::Set) {
        u_axis = scale(u_axis, -1.0);
    }
    let v_axis = cross(normal, u_axis);
    ((dot(v_axis, v_axis) - 1.0).abs() <= EPS_PLACEMENT_EXACT_GEOMETRY).then_some(())?;
    FeatureSectionTransform::new(
        definition.identity.id(),
        Some(feature_id),
        origin,
        u_axis,
        v_axis,
        section.offset,
    )
}

fn generated_datum_plane_equation(
    sketch_id: u32,
    reference_id: u32,
    reference_normal: [f64; 3],
    sources: &PlacementSources<'_>,
) -> Option<SignedPlaneEquation> {
    let datum_ids = sources
        .geometry_tables
        .iter()
        .filter_map(|table| table.kind.datum_ids())
        .flatten()
        .filter(|id| **id == sketch_id)
        .count();
    (datum_ids == 1).then_some(())?;
    let mut datums = sources
        .datums
        .iter()
        .filter(|datum| datum.id == reference_id);
    let reference_feature = match (datums.next(), datums.next()) {
        (Some(datum), None) => Some(datum.feature_id),
        (None, None) => unique_surface_row(sources.surface_rows, reference_id)
            .filter(|row| row.kind == SurfaceKind::Plane)
            .map(|row| row.feature_id),
        _ => None,
    }?;
    exactly_one(
        sources
            .affected_ids
            .iter()
            .filter(|record| {
                record.kind == AffectedIdKind::Parents && record.ids.contains(&reference_feature)
            })
            .filter_map(|parents| {
                let other = exactly_one(
                    parents
                        .ids
                        .iter()
                        .filter(|parent| **parent != reference_feature),
                )?;
                let (equation, ambiguous) = sources
                    .datums
                    .iter()
                    .filter(|datum| datum.feature_id == *other)
                    .map(|datum| SignedPlaneEquation {
                        normal: datum.plane().normal(),
                        offset: datum.plane().offset(),
                    })
                    .chain(
                        sources
                            .surface_rows
                            .iter()
                            .filter(|row| {
                                row.feature_id == *other && row.kind == SurfaceKind::Plane
                            })
                            .filter_map(|row| {
                                plane_equation(
                                    row.id,
                                    sources.datums,
                                    sources.model_planes,
                                    sources.outline_planes,
                                )
                            }),
                    )
                    .chain(
                        sources
                            .surface_rows
                            .iter()
                            .filter(|row| {
                                row.feature_id == *other && row.kind == SurfaceKind::Plane
                            })
                            .flat_map(|row| {
                                sources
                                    .plane_envelopes
                                    .iter()
                                    .filter(move |record| record.surface_id == row.id)
                            })
                            .flat_map(|record| {
                                let corners = match &record.envelope {
                                    PlaneEnvelope::Standard { corners_3d, .. }
                                    | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
                                };
                                (0..3).filter_map(move |axis| {
                                    if record.corner_coordinate_equal[axis] != Some(true) {
                                        return None;
                                    }
                                    let coordinate = corners[0][axis]?;
                                    let mut normal = [0.0; 3];
                                    normal[axis] = 1.0;
                                    Some(SignedPlaneEquation {
                                        normal,
                                        offset: coordinate,
                                    })
                                })
                            }),
                    )
                    .filter(|equation| {
                        dot(equation.normal, reference_normal).abs() <= EPS_PLACEMENT_EXACT_GEOMETRY
                    })
                    .fold((None, false), |(first, ambiguous), candidate| match first {
                        None => (Some(candidate), ambiguous),
                        Some(first) if first == candidate => (Some(first), ambiguous),
                        Some(first) => (Some(first), true),
                    });
                (!ambiguous).then_some(equation?)
            }),
    )
}

fn feature_generated_plane_equation(
    ctx: &DecodeContext<'_>,
    id: u32,
    definitions: &[FeatureDefinition],
    transforms: &[FeatureSectionTransform],
    sources: &PlacementSources<'_>,
) -> Result<Option<SignedPlaneEquation>, CodecError> {
    let Some(surface_row) = exactly_one(
        sources
            .surface_rows
            .iter()
            .filter(|row| row.id == id && row.kind == SurfaceKind::Plane),
    ) else {
        return Ok(None);
    };
    let feature_id = surface_row.feature_id;
    let Some(transform) = exactly_one(
        transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id)),
    ) else {
        return Ok(None);
    };
    let Some(definition) = exactly_one(
        definitions
            .iter()
            .filter(|definition| definition.identity.id() == transform.definition_id),
    ) else {
        return Ok(None);
    };
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    let Some(segment) = segments.segment(id) else {
        return Ok(None);
    };
    if !matches!(segment.kind, FeatureSegmentKind::Line(_)) {
        return Ok(None);
    }
    let Some(variables) = definition.variables.as_ref() else {
        return Ok(None);
    };
    let crate::feature::definitions::ReconciledPoints { points, .. } =
        variables.reconciled_points(ctx)?;
    let result = (|| {
        let point = |point_id| {
            let point = points.get(&point_id)?;
            Some([point[0]?, point[1]?])
        };
        let start = point(segment.point_ids()[0])?;
        let end = point(segment.point_ids()[1])?;
        let place = |point: [f64; 2]| {
            std::array::from_fn(|axis| {
                transform.origin[axis]
                    + point[0] * transform.u_axis[axis]
                    + point[1] * transform.v_axis[axis]
            })
        };
        let start = place(start);
        let end = place(end);
        let direction = std::array::from_fn(|axis| end[axis] - start[axis]);
        let magnitude = dot(direction, direction).sqrt();
        (magnitude > EPS_PLACEMENT_EXACT_GEOMETRY).then_some(())?;
        let direction = scale(direction, magnitude.recip());
        let normal = cross(direction, transform.normal());
        let magnitude = dot(normal, normal).sqrt();
        (magnitude > EPS_PLACEMENT_EXACT_GEOMETRY).then_some(())?;
        let normal = scale(normal, magnitude.recip());
        Some(SignedPlaneEquation {
            normal,
            offset: dot(normal, start),
        })
    })();
    Ok(result)
}

fn generated_cap_pair_plane_equation(
    table: &FeatureEntityTable,
    sources: &PlacementSources<'_>,
) -> Option<SignedPlaneEquation> {
    let [first, second, ..] = table.entries.as_slice() else {
        return None;
    };
    if [first.class_id(), second.class_id()] != [204, 203] {
        return None;
    }
    let first = plane_equation(
        first.entity_id,
        sources.datums,
        sources.model_planes,
        sources.outline_planes,
    )?;
    let second = plane_equation(
        second.entity_id,
        sources.datums,
        sources.model_planes,
        sources.outline_planes,
    )?;
    let oriented_cosine = dot(first.normal, second.normal);
    let cosine = oriented_cosine.abs();
    let second_offset = if oriented_cosine.is_sign_negative() {
        -second.offset
    } else {
        second.offset
    };
    let scale = first.offset.abs().max(second.offset.abs()).max(1.0);
    ((cosine - 1.0).abs() <= EPS_PLACEMENT_EXACT_GEOMETRY
        && (first.offset - second_offset).abs() > EPS_PLACEMENT_EXACT_GEOMETRY * scale)
        .then_some(first)
}

fn generated_section_cap_plane_equation(
    sketch_id: u32,
    feature_id: u32,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<SignedPlaneEquation> {
    exactly_one(sources.geometry_tables.iter().filter(|table| {
        table.feature_id == feature_id && table.kind.datum_ids() == Some(&[sketch_id])
    }))?;
    exactly_one(
        entity_tables
            .iter()
            .filter(|table| table.feature_id == feature_id)
            .filter_map(|table| generated_cap_pair_plane_equation(table, sources)),
    )
}

fn zero_offset_standard_section_plane_equation(
    definition: &FeatureDefinition,
    section: &crate::feature::definitions::FeatureSection3d,
    reference_id: u32,
    reference: SignedPlaneEquation,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<SignedPlaneEquation> {
    let feature_id = definition.identity.owner_feature_id()?;
    let sketch_id = section.sketch_plane_entity_id?;
    let mut instructions = placement_instructions(definition);
    let instruction = instructions.next()?;
    instructions
        .all(|candidate| {
            candidate.kind == instruction.kind
                && candidate.zero_offset == instruction.zero_offset
                && candidate.dimension_id == instruction.dimension_id
                && candidate.reference_id == instruction.reference_id
                && candidate.geometry1_id == instruction.geometry1_id
                && candidate.geometry2_id == instruction.geometry2_id
                && candidate.member1 == instruction.member1
                && candidate.member2 == instruction.member2
        })
        .then_some(())?;
    (instruction.kind == 20_127
        && instruction.zero_offset
        && instruction.dimension_id.is_none()
        && instruction.reference_id.is_none()
        && instruction.geometry1_id == Some(reference_id)
        && instruction.geometry2_id.is_none()
        && instruction.member1 == 0
        && instruction.member2 == 0)
        .then_some(())?;
    let datum_tables = sources
        .geometry_tables
        .iter()
        .filter(|table| {
            table.feature_id == feature_id && table.kind.datum_ids() == Some(&[sketch_id])
        })
        .count();
    (datum_tables == 1).then_some(())?;
    let mut tables = entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .filter(|table| {
            table
                .entries
                .iter()
                .map(crate::feature::entity::FeatureEntityTableEntry::class_id)
                .eq([204, 203, 200, 200])
        });
    let table = tables.next()?;
    tables.next().is_none().then_some(())?;
    let cap_id = table.entries[1].entity_id;
    let cap = plane_equation(
        cap_id,
        sources.datums,
        sources.model_planes,
        sources.outline_planes,
    )?;
    let mut candidates = sources.datums.iter().filter_map(|datum| {
        let equation = SignedPlaneEquation {
            normal: datum.plane().normal(),
            offset: datum.plane().offset(),
        };
        let cap_alignment = dot(equation.normal, cap.normal).abs();
        let reference_alignment = dot(equation.normal, reference.normal).abs();
        ((cap_alignment - 1.0).abs() <= EPS_PLACEMENT_EXACT_GEOMETRY
            && reference_alignment <= EPS_PLACEMENT_EXACT_GEOMETRY)
            .then_some(equation)
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(())?;
    let aligned_cap_offset = if dot(candidate.normal, cap.normal).is_sign_negative() {
        -cap.offset
    } else {
        cap.offset
    };
    let separation = (candidate.offset - aligned_cap_offset).abs();
    let scale = candidate.offset.abs().max(cap.offset.abs()).max(1.0);
    (separation > EPS_PLACEMENT_EXACT_GEOMETRY * scale).then_some(candidate)
}

fn circular_profile_aligned_origin(
    definition: &FeatureDefinition,
    feature_id: u32,
    sketch_plane: SignedPlaneEquation,
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Option<[f64; 3]> {
    let table = exactly_one(
        entity_tables
            .iter()
            .filter(|table| table.feature_id == feature_id)
            .filter(|table| {
                table
                    .entries
                    .iter()
                    .map(crate::feature::entity::FeatureEntityTableEntry::class_id)
                    .eq([204, 203, 200, 200])
            }),
    )?;
    let profile_external_id = table.entries[2].source_entity_id()?;
    let profile_internal_id = definition
        .order_table
        .as_ref()?
        .internal_id(profile_external_id)?;
    let circle = exactly_one(
        definition
            .saved_section
            .iter()
            .flat_map(|section| &section.entities)
            .filter_map(|entity| match entity {
                crate::feature::definitions::FeatureSavedEntity::Circle(circle)
                    if circle.entity_id == profile_internal_id =>
                {
                    Some(circle)
                }
                _ => None,
            }),
    )?;
    let [Some(center_u), Some(center_v), _] = circle.center else {
        return None;
    };
    let radius = circle
        .radius
        .filter(|radius| *radius > EPS_PLACEMENT_EXACT_GEOMETRY)?;
    let cap_id = table.entries[1].entity_id;
    let envelope = exactly_one(
        sources
            .plane_envelopes
            .iter()
            .filter(|record| record.surface_id == cap_id),
    )?;
    let corners = match &envelope.envelope {
        PlaneEnvelope::Standard { corners_3d, .. } | PlaneEnvelope::Compact { corners_3d, .. } => {
            corners_3d
        }
    };
    let decode_corner = |corner: &[Option<f64>; 3]| Some([corner[0]?, corner[1]?, corner[2]?]);
    let first = decode_corner(&corners[0])?;
    let second = decode_corner(&corners[1])?;
    let axis = (0..3).find(|axis| envelope.corner_coordinate_equal[*axis] == Some(true))?;
    let radial = match axis {
        0 => [1, 2],
        1 => [0, 2],
        2 => [0, 1],
        _ => return None,
    };
    let spans = radial.map(|index| (second[index] - first[index]).abs());
    let tolerance_scale = spans
        .iter()
        .chain(std::iter::once(&radius))
        .copied()
        .fold(1.0, f64::max);
    (spans[0] > EPS_PLACEMENT_EXACT_GEOMETRY
        && (spans[0] - spans[1]).abs() <= EPS_PLACEMENT_GEOMETRY * tolerance_scale
        && (0.5 * spans[0] - radius).abs() <= EPS_PLACEMENT_GEOMETRY * tolerance_scale)
        .then_some(())?;
    let cap_center: [f64; 3] = std::array::from_fn(|index| 0.5 * (first[index] + second[index]));
    let signed_distance = dot(sketch_plane.normal, cap_center) - sketch_plane.offset;
    let profile_center = add(cap_center, scale(sketch_plane.normal, -signed_distance));
    Some(add(
        add(profile_center, scale(u_axis, -center_u)),
        scale(v_axis, -center_v),
    ))
}

/// Resolve feature frames whose sketch and orientation references reduce to
/// two perpendicular model-space datum planes.
pub(crate) fn resolve(
    ctx: &DecodeContext<'_>,
    definitions: &[FeatureDefinition],
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Vec<FeatureSectionTransform>, CodecError> {
    let mut result = Vec::new();
    for definition in definitions {
        let Some(section) = &definition.section_3d else {
            continue;
        };
        let Some(sketch_id) = section.sketch_plane_entity_id else {
            continue;
        };
        let carrier_transform =
            match generated_cylinder_section_transform(ctx, definition, sources, entity_tables)? {
                Some(transform) => Some(transform),
                None => {
                    generated_planar_section_transform(ctx, definition, sources, entity_tables)?
                }
            }
            .map(|transform| apply_section_orientation(transform, section));
        let mut reference_ids = Vec::new();
        if let Some(id) = section.reference_plane_datum_geometry_id {
            ctx.reserve_vec(&mut reference_ids, 1, "creo placement reference IDs")?;
            reference_ids.push(id);
        } else {
            for id in section.reference_planes.entity_ids() {
                ctx.reserve_vec(&mut reference_ids, 1, "creo placement reference IDs")?;
                reference_ids.push(id);
            }
        }
        reference_ids.sort_unstable();
        reference_ids.dedup();
        let direct_sketch = plane_equation(
            sketch_id,
            sources.datums,
            sources.model_planes,
            sources.outline_planes,
        )
        .or_else(|| definition_local_plane_equation(definition))
        .or_else(|| {
            generated_section_cap_plane_equation(
                sketch_id,
                definition.identity.owner_feature_id()?,
                sources,
                entity_tables,
            )
        });
        let mut candidates = Vec::<SectionFrameCandidate>::new();
        for reference_id in reference_ids {
            let direct_reference = plane_equation(
                reference_id,
                sources.datums,
                sources.model_planes,
                sources.outline_planes,
            );
            if let Some(sketch) = direct_sketch {
                let mut reference = direct_reference.or_else(|| {
                    generated_datum_plane_equation(reference_id, sketch_id, sketch.normal, sources)
                });
                if reference.is_none() {
                    reference = feature_generated_plane_equation(
                        ctx,
                        reference_id,
                        definitions,
                        &result,
                        sources,
                    )?;
                }
                if let Some(reference) = reference {
                    if dot(sketch.normal, reference.normal).abs()
                        < 1.0 - EPS_PLACEMENT_EXACT_GEOMETRY
                        && !candidates.iter().any(|candidate| {
                            candidate.sketch == sketch && candidate.reference == reference
                        })
                    {
                        ctx.reserve_vec(&mut candidates, 1, "creo placement candidates")?;
                        candidates.push(SectionFrameCandidate {
                            reference_id,
                            sketch,
                            reference,
                        });
                    }
                }
            } else if let Some(reference) = direct_reference {
                if let Some(sketch) = generated_datum_plane_equation(
                    sketch_id,
                    reference_id,
                    reference.normal,
                    sources,
                )
                .or_else(|| {
                    zero_offset_standard_section_plane_equation(
                        definition,
                        section,
                        reference_id,
                        reference,
                        sources,
                        entity_tables,
                    )
                }) {
                    if dot(sketch.normal, reference.normal).abs()
                        < 1.0 - EPS_PLACEMENT_EXACT_GEOMETRY
                        && !candidates.iter().any(|candidate| {
                            candidate.sketch == sketch && candidate.reference == reference
                        })
                    {
                        ctx.reserve_vec(&mut candidates, 1, "creo placement candidates")?;
                        candidates.push(SectionFrameCandidate {
                            reference_id,
                            sketch,
                            reference,
                        });
                    }
                }
            }
        }
        if candidates.len() != 1 {
            if let Some(transform) = carrier_transform {
                ctx.reserve_vec(&mut result, 1, "creo placement transforms")?;
                result.push(transform);
            } else if let Some(transform) = definition_local_frame_transform(definition, section) {
                ctx.reserve_vec(&mut result, 1, "creo placement transforms")?;
                result.push(transform);
            }
            continue;
        }
        let [candidate] = candidates.as_slice() else {
            continue;
        };
        let SignedPlaneEquation {
            normal: mut sketch_normal,
            offset: mut sketch_offset,
        } = candidate.sketch;
        let SignedPlaneEquation {
            normal: mut reference_normal,
            offset: mut reference_offset,
        } = candidate.reference;
        if section.sketch_plane_flip == Some(BinaryFlag::Set) {
            sketch_normal = scale(sketch_normal, -1.0);
            sketch_offset = -sketch_offset;
        }
        if section.orientation.section_flip == Some(BinaryFlag::Set) {
            sketch_normal = scale(sketch_normal, -1.0);
            sketch_offset = -sketch_offset;
        }
        if let ReferencePlanes::Positional(rows) = &section.reference_planes {
            let reference_rows = rows
                .iter()
                .filter(|row| row.plane_entity_id == candidate.reference_id);
            if exactly_one(reference_rows).is_none() {
                continue;
            }
        }
        if reference_flip_for_reference(section, Some(candidate.reference_id))
            == Some(BinaryFlag::Set)
        {
            reference_normal = scale(reference_normal, -1.0);
            reference_offset = -reference_offset;
        }
        let normal = sketch_normal;
        let cosine = dot(normal, reference_normal);
        let denominator = 1.0 - cosine * cosine;
        if denominator <= EPS_PLACEMENT_EXACT_GEOMETRY {
            continue;
        }
        let reference_axis = scale(
            add(reference_normal, scale(normal, -cosine)),
            denominator.sqrt().recip(),
        );
        let u_axis = cross(reference_axis, normal);
        if (dot(u_axis, u_axis) - 1.0).abs() > EPS_PLACEMENT_EXACT_GEOMETRY {
            continue;
        }
        let sketch_factor = (sketch_offset - cosine * reference_offset) / denominator;
        let reference_factor = (reference_offset - cosine * sketch_offset) / denominator;
        let intersection_origin = add(
            scale(sketch_normal, sketch_factor),
            scale(reference_normal, reference_factor),
        );
        let origin = definition
            .identity
            .owner_feature_id()
            .and_then(|feature_id| {
                circular_profile_aligned_origin(
                    definition,
                    feature_id,
                    SignedPlaneEquation {
                        normal: sketch_normal,
                        offset: sketch_offset,
                    },
                    u_axis,
                    reference_axis,
                    sources,
                    entity_tables,
                )
            })
            .unwrap_or(intersection_origin);
        let direct_transform = FeatureSectionTransform::new(
            definition.identity.id(),
            definition.identity.owner_feature_id(),
            origin,
            u_axis,
            reference_axis,
            section.offset,
        );
        if let Some(transform) = carrier_transform.or(direct_transform) {
            ctx.reserve_vec(&mut result, 1, "creo placement transforms")?;
            result.push(transform);
        }
    }
    for definition in definitions {
        if result
            .iter()
            .any(|transform| transform.definition_id == definition.identity.id())
        {
            continue;
        }
        let transform =
            match generated_cylinder_section_transform(ctx, definition, sources, entity_tables)? {
                Some(transform) => Some(transform),
                None => {
                    generated_planar_section_transform(ctx, definition, sources, entity_tables)?
                }
            };
        if let Some(transform) = transform {
            ctx.reserve_vec(&mut result, 1, "creo placement transforms")?;
            result.push(transform);
        }
    }
    crate::sort::stable_sort_by_key(
        ctx,
        result.as_mut_slice(),
        |transform| transform.offset,
        "creo resolve result ordering",
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests;
