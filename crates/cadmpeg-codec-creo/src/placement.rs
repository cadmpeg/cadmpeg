// SPDX-License-Identifier: Apache-2.0
//! Model-space frames resolved from feature-section datum references.

use crate::datum::DatumPlaneRecord;
use crate::decode::uniqueness::{exactly_one, exactly_one_by};
use crate::feature::definitions::ReferencePlanes;
use crate::feature::definitions::{
    placement_instructions, BinaryFlag, FeatureDefinition, FeatureParameterFrameKind,
    FeatureSegmentKind,
};
use crate::feature::entity::FeatureEntityTable;
use crate::feature::rows::{AffectedIdKind, FeatureAffectedIds, FeatureGeometryTable};
use crate::surface::{
    unique_surface_row, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
    SurfaceKind, SurfaceParameterRecord,
};
use crate::vecmath::{add, cross, dot, local_system_lanes, normalize, scale, unit_length};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;

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

#[derive(Clone, Copy)]
pub(crate) struct PlacementSources<'a> {
    pub(crate) datums: &'a [DatumPlaneRecord],
    pub(crate) surface_rows: &'a crate::surface::SurfaceRows,
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

/// Unique carrier indexes shared by placement queries. None entries identify
/// repeated IDs. A namespace is built only when its first query needs it.
struct PlacementLookup<'ctx, 'source, 'input> {
    ctx: &'ctx DecodeContext<'input>,
    sources: PlacementSources<'source>,
    outlines: Option<std::collections::HashMap<u32, Option<&'source OutlinePlane>>>,
    envelopes: Option<std::collections::HashMap<u32, Option<&'source PlaneEnvelopeRecord>>>,
    parameters: Option<std::collections::HashMap<u32, Option<&'source SurfaceParameterRecord>>>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx, 'source, 'input> PlacementLookup<'ctx, 'source, 'input> {
    fn new(ctx: &'ctx DecodeContext<'input>, sources: &PlacementSources<'source>) -> Result<Self, CodecError> {
        Ok(Self { ctx, sources: *sources, outlines: None, envelopes: None, parameters: None,
            storage: ctx.reserve_scoped(0, "creo placement lookup storage")? })
    }

    fn generated_equation(&mut self, id: u32)
        -> Result<Option<([f64; 3], f64)>, CodecError>
    {
        let ctx = self.ctx;
        let sources = &self.sources;
        if self.outlines.is_none() {
            let mut index = std::collections::HashMap::new();
            for value in ctx.admit_iter(sources.outline_planes, "creo placement outline index traversal")? {
                match self.storage.with_storage(|| ctx.entry_hash_map(
                    &mut index, value.surface_id, "creo placement outline index nodes",
                ))? {
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(value)); }
                    std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
                }
            }
            self.outlines = Some(index);
        }
        if let Some(outline) = self.outlines.as_ref().and_then(|index| index.get(&id)) {
            return Ok(outline.map(|plane| (plane.normal(), dot(plane.normal(), plane.origin))));
        }
        if self.envelopes.is_none() {
            let mut index = std::collections::HashMap::new();
            for value in ctx.admit_iter(sources.plane_envelopes, "creo placement envelope index traversal")? {
                match self.storage.with_storage(|| ctx.entry_hash_map(
                    &mut index, value.surface_id, "creo placement envelope index nodes",
                ))? {
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(value)); }
                    std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
                }
            }
            self.envelopes = Some(index);
        }
        let Some(envelope) = self.envelopes.as_ref().and_then(|index| index.get(&id)).copied().flatten() else {
            return Ok(None);
        };
        let corners = match &envelope.envelope {
            PlaneEnvelope::Standard { corners_3d, .. } | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
        };
        let Some(axis) = (0..3).find(|axis| envelope.corner_coordinate_equal[*axis] == Some(true)) else { return Ok(None); };
        let Some(coordinate) = corners[0][axis] else { return Ok(None); };
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        Ok(Some((normal, coordinate)))
    }

    fn parameter(&mut self, id: u32)
        -> Result<Option<&'source SurfaceParameterRecord>, CodecError>
    {
        let ctx = self.ctx;
        let sources = &self.sources;
        if self.parameters.is_none() {
            let mut index = std::collections::HashMap::new();
            for value in ctx.admit_iter(sources.surface_parameters, "creo placement parameter index traversal")? {
                match self.storage.with_storage(|| ctx.entry_hash_map(
                    &mut index, value.surface_id, "creo placement parameter index nodes",
                ))? {
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(value)); }
                    std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
                }
            }
            self.parameters = Some(index);
        }
        Ok(self.parameters.as_ref().and_then(|index| index.get(&id)).copied().flatten())
    }
}

fn generated_cylinder_section_transform(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    entity_tables: &[FeatureEntityTable],
    lookup: &mut PlacementLookup<'_, '_, '_>,
) -> Result<Option<FeatureSectionTransform>, CodecError> {
    let sources = lookup.sources;
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
    let (mut correspondences, mut correspondence_storage) = ctx.temporary_vec::<([f64; 2], [f64; 3], UnitVector3)>(0, "creo cylinder placement correspondence storage")?;
    let mut coordinate_scale = 1.0f64;
    let mut first_offset = None::<usize>;
    for table in ctx.admit_iter(entity_tables, "creo cylinder placement table traversal")? {
        if table.feature_id != feature_id { continue; }
        for entry in ctx.admit_iter(&table.entries, "creo cylinder placement entry traversal")? {
        if !ctx.contains_btree_set(table.unique_surface_ids(), &entry.entity_id, "creo cylinder placement surface membership")? { continue; }
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
        let Some([Some(u), Some(v)]) = ctx.get_btree_map(&points.points, &center_id, "creo cylinder placement point lookup")?.copied() else {
            continue;
        };
        let Some(row) = sources.surface_rows.unique(entry.entity_id)
            .filter(|row| row.feature_id == feature_id && row.kind == SurfaceKind::Cylinder)
        else {
            continue;
        };
        let Some(parameters) = lookup.parameter(row.id)? else {
            continue;
        };
        let Some(frame) = parameters.positional_cylinder_frame() else {
            continue;
        };
        correspondence_storage.with_storage(|| ctx.reserve_vec(&mut correspondences, 1, "creo cylinder placement correspondences"))?;
        for value in [u, v].iter().chain(frame.frame().origin().iter()) { coordinate_scale = coordinate_scale.max(value.abs()); }
        first_offset = Some(first_offset.map_or(parameters.offset, |known| known.min(parameters.offset)));
        correspondences.push((
            [u, v],
            frame.frame().origin(),
            *frame.frame().orthonormal_frame().axis(),
        ));
    }
        }
    let Some(first) = correspondences.first() else { return Ok(None); };
    let normal = unit_length(first.2);
    let scale = coordinate_scale;
        let close = |left: f64, right: f64| {
            (left - right).abs() <= EPS_PLACEMENT_GEOMETRY * left.abs().max(right.abs()).max(1.0)
        };
        if !ctx.all_by(&correspondences, |(_, _, axis)| Ok(unit_length(*axis).iter().zip(normal)
            .all(|(left, right)| close(*left, right))), "creo cylinder placement axis agreement")? { return Ok(None); }


        let mut frame = None::<([f64; 3], [f64; 3], [f64; 3])>;
        let mut seconds = correspondences[1..].iter();
        while let Some(second) = ctx.next_charged(&mut seconds, "creo cylinder placement frame candidates")? {
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
            if !ctx.all_by(&correspondences, |(local, model, _)| {
                Ok(
                (0..3).all(|index| {
                    close(
                        candidate.0[index]
                            + local[0] * candidate.1[index]
                            + local[1] * candidate.2[index],
                        model[index],
                    )
                }))
            }, "creo cylinder placement candidate agreement")? {
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
                    return Ok(None);
                }
            } else {
                frame = Some(candidate);
            }
        }
        let Some(frame) = frame else { return Ok(None); };
        let Some(offset) = definition.section_3d.as_ref().map(|section| section.offset).or(first_offset) else { return Ok(None); };
        Ok(FeatureSectionTransform::new(
            definition.identity.id(),
            Some(feature_id),
            frame.0,
            frame.1,
            frame.2,
            offset,
        ))
}

fn generated_planar_section_transform(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    entity_tables: &[FeatureEntityTable],
    lookup: &mut PlacementLookup<'_, '_, '_>,
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
    let Some(table) = exactly_one_by(ctx, entity_tables,
        |table| Ok(table.feature_id == feature_id && generated_planar_table_shape(ctx, table)?),
        "creo planar placement table selection")? else { return Ok(None); };
    let offset = definition
        .section_3d
        .as_ref()
        .map_or(table.offset, |section| section.offset);
    let Some(first_cap) = lookup.generated_equation(table.entries[0].entity_id)? else { return Ok(None); };
    let Some(second_cap) = lookup.generated_equation(table.entries[1].entity_id)? else { return Ok(None); };
    let caps = [first_cap, second_cap];
    let (mut sides, mut side_storage) = ctx.temporary_vec(0, "creo planar placement side storage")?;
    let mut entries = table.entries[2..].iter();
    while let Some(entry) = ctx.next_charged(&mut entries, "creo planar placement side traversal")? {
        if !ctx.contains_btree_set(table.unique_surface_ids(), &entry.entity_id, "creo planar placement surface membership")? { continue; }
        let Some(model_plane) = lookup.generated_equation(entry.entity_id)? else {
            continue;
        };
        let Some(segment) = entry.source_entity_id().and_then(|id| segments.segment(id)) else {
            continue;
        };
        if !matches!(segment.kind, FeatureSegmentKind::Line(_)) {
            return Ok(None);
        }
        let Some(start) = ctx.get_btree_map(&points, &segment.point_ids()[0], "creo planar placement start point lookup")?.and_then(|point| Some([point[0]?, point[1]?])) else { return Ok(None); };
        let Some(end) = ctx.get_btree_map(&points, &segment.point_ids()[1], "creo planar placement end point lookup")?.and_then(|point| Some([point[0]?, point[1]?])) else { return Ok(None); };
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
        side_storage.with_storage(|| ctx.reserve_vec(&mut sides, 1, "creo planar placement sides"))?;
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
    let (mut candidates, mut candidate_storage) = ctx.temporary_vec(0, "creo planar placement candidate storage")?;
    for first_index in ctx.admit_iter(0..sides.len(), "creo planar placement first side traversal")? {
        for second_index in ctx.admit_iter(first_index + 1..sides.len(), "creo planar placement side pair traversal")? {
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
                    if ctx.any_by(&sides, |side| Ok(
                        side_coordinate(side).is_none_or(|(predicted, coordinate)| {
                            !close(dot(predicted, origin), coordinate)
                        })
                    ), "creo planar placement side agreement")? {
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
                    if !ctx.any_by(&candidates, |existing: &FeatureSectionTransform| {
                        Ok(
                        vectors_close(existing.origin(), candidate.origin())
                            && vectors_close(existing.u_axis(), candidate.u_axis())
                            && vectors_close(existing.v_axis(), candidate.v_axis())
                            && vectors_close(existing.normal(), candidate.normal()))
                    }, "creo planar placement candidate deduplication")? {
                        candidate_storage.with_storage(|| ctx.reserve_vec(&mut candidates, 1, "creo planar placement candidates"))?;
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
    if first.class_id() != 204 || second.class_id() != 203 || rest.is_empty()
        || !ctx.all_by(rest, |entry| Ok(entry.source_entity_id().is_some()), "creo generated planar table source validation")? {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, "creo generated planar table identity storage")?;
    let mut entry_ids = std::collections::HashSet::new();
    let mut entries = table.entries.iter();
    while let Some(entry) = ctx.next_charged(&mut entries, "creo generated planar table identity traversal")? {
        if entry_ids.contains(&entry.entity_id) { return Ok(false); }
        storage.with_storage(|| ctx.insert_hash_set(&mut entry_ids, entry.entity_id, "creo generated planar table entry nodes"))?;
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

fn definition_local_plane_equation(ctx: &DecodeContext<'_>, definition: &FeatureDefinition) -> Result<Option<SignedPlaneEquation>, CodecError> {
    let Some(values) = unique_complete_local_system(ctx, definition)? else { return Ok(None); };
    let [.., raw_normal, origin] = local_system_lanes(values.get());
    Ok(normalize(raw_normal).map(|normal| SignedPlaneEquation { normal, offset: dot(normal, origin) }))
}

pub(crate) fn unique_complete_local_system(
    ctx: &DecodeContext<'_>, definition: &FeatureDefinition,
) -> Result<Option<cadmpeg_ir::units::FiniteVector<12>>, CodecError> {
    Ok(exactly_one_by(ctx, &definition.parameter_frames,
        |frame| Ok(frame.kind == FeatureParameterFrameKind::LocalSystem && frame.decoded_values.is_some()),
        "creo complete local frame selection")?.and_then(|frame| frame.decoded_values))
}

fn reference_flip_for_reference(
    ctx: &DecodeContext<'_>, section: &crate::feature::definitions::FeatureSection3d,
    reference_id: Option<u32>,
) -> Result<Option<BinaryFlag>, CodecError> {
    Ok(match &section.reference_planes {
        ReferencePlanes::Named(_) => section.orientation.reference_flip,
        ReferencePlanes::Positional(rows) => match reference_id {
            Some(reference_id) => exactly_one_by(ctx, rows,
                |row| Ok(row.plane_entity_id == reference_id),
                "creo positional reference plane selection")?.and_then(|row| row.reference_flip),
            None => None,
        },
    })
}

fn unique_carrier_reference_id(
    ctx: &DecodeContext<'_>, section: &crate::feature::definitions::FeatureSection3d,
) -> Result<Option<u32>, CodecError> {
    if let Some(id) = section.reference_plane_datum_geometry_id { return Ok(Some(id)); }
    let mut ids = section.reference_planes.entity_ids();
    let Some(id) = ctx.next_charged(&mut ids, "creo carrier reference ID selection")? else { return Ok(None); };
    Ok(ctx.all_by(ids, |candidate| Ok(candidate == id), "creo carrier reference ID selection")?.then_some(id))
}

fn apply_section_orientation(
    ctx: &DecodeContext<'_>, mut transform: FeatureSectionTransform,
    section: &crate::feature::definitions::FeatureSection3d,
) -> Result<FeatureSectionTransform, CodecError> {
    if section.sketch_plane_flip == Some(BinaryFlag::Set) { transform = transform.flipped_v(); }
    if section.orientation.section_flip == Some(BinaryFlag::Set) { transform = transform.flipped_v(); }
    let reference_flip = match &section.reference_planes {
        ReferencePlanes::Named(_) => section.orientation.reference_flip,
        ReferencePlanes::Positional(_) => {
            let reference_id = unique_carrier_reference_id(ctx, section)?;
            reference_flip_for_reference(ctx, section, reference_id)?
        }
    };
    if reference_flip == Some(BinaryFlag::Set) {
        transform = transform.flipped_u_and_v();
    }
    Ok(transform)
}

fn definition_local_frame_transform(
    ctx: &DecodeContext<'_>, definition: &FeatureDefinition,
    section: &crate::feature::definitions::FeatureSection3d,
) -> Result<Option<FeatureSectionTransform>, CodecError> {
    let Some(feature_id) = definition.identity.owner_feature_id() else { return Ok(None); };
    let Some(values) = unique_complete_local_system(ctx, definition)? else { return Ok(None); };
    let [stored_u_axis, _, stored_axis, origin] = local_system_lanes(values.get());
    let Some(mut u_axis) = normalize(stored_u_axis) else { return Ok(None); };
    let Some(raw_normal) = normalize(stored_axis) else { return Ok(None); };
    if dot(u_axis, raw_normal).abs() > EPS_PLACEMENT_EXACT_GEOMETRY { return Ok(None); }
    let mut normal = raw_normal;
    if section.sketch_plane_flip == Some(BinaryFlag::Set) { normal = scale(normal, -1.0); }
    if section.orientation.section_flip == Some(BinaryFlag::Set) { normal = scale(normal, -1.0); }
    if reference_flip_for_reference(ctx, section, None)? == Some(BinaryFlag::Set) { u_axis = scale(u_axis, -1.0); }
    let v_axis = cross(normal, u_axis);
    if (dot(v_axis, v_axis) - 1.0).abs() > EPS_PLACEMENT_EXACT_GEOMETRY { return Ok(None); }
    Ok(FeatureSectionTransform::new(definition.identity.id(), Some(feature_id), origin, u_axis, v_axis, section.offset))
}

fn generated_datum_plane_equation(
    ctx: &DecodeContext<'_>,
    sketch_id: u32,
    reference_id: u32,
    reference_normal: [f64; 3],
    sources: &PlacementSources<'_>,
) -> Result<Option<SignedPlaneEquation>, CodecError> {
    let mut datum_ids = 0usize;
    for table in ctx.admit_iter(sources.geometry_tables, "creo generated datum table scan")? {
        if let Some(ids) = table.kind.datum_ids() {
            let matches = ctx
                .admit_iter(ids, "creo generated datum ID count")?
                .filter(|id| **id == sketch_id)
                .count();
            datum_ids = datum_ids.checked_add(matches).ok_or_else(|| {
                ctx.refuse_codec_limit("creo generated datum ID count", u64::MAX, u64::MAX)
            })?;
        }
    }
    if datum_ids != 1 {
        return Ok(None);
    }
    Ok((|| {
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
                    record.kind == AffectedIdKind::Parents
                        && record.ids.contains(&reference_feature)
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
                            dot(equation.normal, reference_normal).abs()
                                <= EPS_PLACEMENT_EXACT_GEOMETRY
                        })
                        .fold((None, false), |(first, ambiguous), candidate| match first {
                            None => (Some(candidate), ambiguous),
                            Some(first) if first == candidate => (Some(first), ambiguous),
                            Some(first) => (Some(first), true),
                        });
                    (!ambiguous).then_some(equation?)
                }),
        )
    })())
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
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    section: &crate::feature::definitions::FeatureSection3d,
    reference_id: u32,
    reference: SignedPlaneEquation,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Option<SignedPlaneEquation>, CodecError> {
    let Some(feature_id) = definition.identity.owner_feature_id() else {
        return Ok(None);
    };
    let Some(sketch_id) = section.sketch_plane_entity_id else {
        return Ok(None);
    };
    let mut instructions = placement_instructions(ctx, definition)?;
    let Some(()) = (|| {
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
        Some(())
    })() else {
        return Ok(None);
    };
    let datum_tables = ctx
        .admit_iter(
            sources.geometry_tables,
            "creo standard section datum table count",
        )?
        .filter(|table| {
            table.feature_id == feature_id && table.kind.datum_ids() == Some(&[sketch_id])
        })
        .count();
    if datum_tables != 1 {
        return Ok(None);
    }
    Ok((|| {
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
    })())
}

struct SectionPlaneAxes {
    plane: SignedPlaneEquation,
    u_axis: [f64; 3],
    v_axis: [f64; 3],
}

fn circular_profile_aligned_origin(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    feature_id: u32,
    axes: SectionPlaneAxes,
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(table) = crate::decode::uniqueness::exactly_one_by(
        ctx, entity_tables,
        |table| Ok(table.feature_id == feature_id && table.entries.iter()
            .map(crate::feature::entity::FeatureEntityTableEntry::class_id)
            .eq([204, 203, 200, 200])),
        "creo circular profile entity table selection",
    )? else { return Ok(None); };
    let Some(profile_external_id) = table.entries[2].source_entity_id() else { return Ok(None); };
    let Some(order) = definition.order_table.as_ref() else { return Ok(None); };
    let Some(profile_internal_id) = order.internal_id(profile_external_id) else {
        return Ok(None);
    };
    let Some(section) = definition.saved_section.as_ref() else { return Ok(None); };
    let Some(entity) = crate::decode::uniqueness::exactly_one_by(
        ctx, &section.entities,
        |entity| Ok(matches!(entity, crate::feature::definitions::FeatureSavedEntity::Circle(circle) if circle.entity_id == profile_internal_id)),
        "creo circular profile saved entity selection",
    )? else { return Ok(None); };
    let crate::feature::definitions::FeatureSavedEntity::Circle(circle) = entity else { return Ok(None); };
    let [Some(center_u), Some(center_v), _] = circle.center else { return Ok(None); };
    let Some(radius) = circle.radius.filter(|radius| *radius > EPS_PLACEMENT_EXACT_GEOMETRY) else { return Ok(None); };
    let cap_id = table.entries[1].entity_id;
    let Some(envelope) = crate::decode::uniqueness::exactly_one_by(
        ctx, sources.plane_envelopes,
        |record| Ok(record.surface_id == cap_id),
        "creo circular profile envelope selection",
    )? else { return Ok(None); };
    Ok((|| {
        let corners = match &envelope.envelope {
            PlaneEnvelope::Standard { corners_3d, .. }
            | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
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
        let cap_center: [f64; 3] =
            std::array::from_fn(|index| 0.5 * (first[index] + second[index]));
        let signed_distance = dot(axes.plane.normal, cap_center) - axes.plane.offset;
        let profile_center = add(cap_center, scale(axes.plane.normal, -signed_distance));
        Some(add(
            add(profile_center, scale(axes.u_axis, -center_u)),
            scale(axes.v_axis, -center_v),
        ))
    })())
}

/// Resolve feature frames whose sketch and orientation references reduce to
/// two perpendicular model-space datum planes.
pub(crate) fn resolve(
    ctx: &DecodeContext<'_>,
    definitions: &[FeatureDefinition],
    sources: &PlacementSources<'_>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Vec<FeatureSectionTransform>, CodecError> {
    let mut lookup = PlacementLookup::new(ctx, sources)?;
    let mut result = Vec::new();
    for definition in definitions {
        let Some(section) = &definition.section_3d else {
            continue;
        };
        let Some(sketch_id) = section.sketch_plane_entity_id else {
            continue;
        };
        let carrier_transform =
            match generated_cylinder_section_transform(ctx, definition, entity_tables, &mut lookup)? {
                Some(transform) => Some(transform),
                None => {
                    generated_planar_section_transform(ctx, definition, entity_tables, &mut lookup)?
                }
            }
            .map(|transform| apply_section_orientation(ctx, transform, section)).transpose()?;
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
        ctx.sort_unstable_by(
            &mut reference_ids,
            |value| value,
            Ord::cmp,
            "creo placement reference ID sort",
        )?;
        reference_ids.dedup();
        let direct_sketch = match plane_equation(
            sketch_id, sources.datums, sources.model_planes, sources.outline_planes,
        ) {
            Some(plane) => Some(plane),
            None => match definition_local_plane_equation(ctx, definition)? {
                Some(plane) => Some(plane),
                None => match definition.identity.owner_feature_id() {
                    Some(feature_id) => generated_section_cap_plane_equation(
                        sketch_id, feature_id, sources, entity_tables,
                    ),
                    None => None,
                },
            },
        };
        let mut candidates = Vec::<SectionFrameCandidate>::new();
        for reference_id in reference_ids {
            let direct_reference = plane_equation(
                reference_id,
                sources.datums,
                sources.model_planes,
                sources.outline_planes,
            );
            if let Some(sketch) = direct_sketch {
                let mut reference = match direct_reference {
                    Some(reference) => Some(reference),
                    None => generated_datum_plane_equation(
                        ctx,
                        reference_id,
                        sketch_id,
                        sketch.normal,
                        sources,
                    )?,
                };
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
                let sketch = match generated_datum_plane_equation(
                    ctx,
                    sketch_id,
                    reference_id,
                    reference.normal,
                    sources,
                )? {
                    Some(sketch) => Some(sketch),
                    None => zero_offset_standard_section_plane_equation(
                        ctx,
                        definition,
                        section,
                        reference_id,
                        reference,
                        sources,
                        entity_tables,
                    )?,
                };
                if let Some(sketch) = sketch {
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
            } else if let Some(transform) = definition_local_frame_transform(ctx, definition, section)? {
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
        let reference_flip = match &section.reference_planes {
            ReferencePlanes::Named(_) => section.orientation.reference_flip,
            ReferencePlanes::Positional(rows) => {
                let Some(row) = exactly_one_by(ctx, rows,
                    |row| Ok(row.plane_entity_id == candidate.reference_id),
                    "creo placement positional row selection")? else {
                    continue;
                };
                row.reference_flip
            }
        };
        if reference_flip == Some(BinaryFlag::Set) {
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
        let origin = match definition.identity.owner_feature_id() {
            Some(feature_id) => circular_profile_aligned_origin(
                ctx,
                definition,
                feature_id,
                SectionPlaneAxes {
                    plane: SignedPlaneEquation { normal: sketch_normal, offset: sketch_offset },
                    u_axis, v_axis: reference_axis,
                },
                sources,
                entity_tables,
            )?,
            None => None,
        }
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
            match generated_cylinder_section_transform(ctx, definition, entity_tables, &mut lookup)? {
                Some(transform) => Some(transform),
                None => {
                    generated_planar_section_transform(ctx, definition, entity_tables, &mut lookup)?
                }
            };
        if let Some(transform) = transform {
            ctx.reserve_vec(&mut result, 1, "creo placement transforms")?;
            result.push(transform);
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo resolve result ordering",
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests;
