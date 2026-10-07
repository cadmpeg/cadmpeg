// SPDX-License-Identifier: Apache-2.0
//! Explicit IGES B-rep topology projection.

use super::geometry::{resolve_transform, ProjectionOutcome};
use super::pointer;
use super::trimming::pcurve_geometry;

use crate::directory::{DirectoryEntry, UseFlag};
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::eval::{finite_or_refusal, EvaluationFailure};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::PcurveMetadata;
use cadmpeg_ir::geometry::{
    pcurve::{Pcurve, PcurveGeometry},
    CurveGeometry,
};
use cadmpeg_ir::ids::{EdgeId, SurfaceId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundary, PcurveUse, Point,
    Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct EdgeDefinition {
    curve: u32,
    start_list: u32,
    start_index: usize,
    end_list: u32,
    end_index: usize,
}

#[derive(Clone)]
enum LoopUse {
    Edge {
        edge_list: u32,
        edge_index: usize,
        sense: Sense,
        pcurves: Vec<(bool, u32)>,
    },
    Vertex {
        vertex_list: u32,
        vertex_index: usize,
        pcurves: Vec<(bool, u32)>,
    },
}

#[derive(Clone)]
struct FaceDefinition {
    surface: u32,
    loops: FaceLoopPointers,
}

#[derive(Clone)]
enum FaceLoopPointers {
    OuterFirst { outer: u32, inner: Vec<u32> },
    Unclassified { first: u32, rest: Vec<u32> },
}

impl FaceLoopPointers {
    fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        let (first, rest) = match self {
            Self::OuterFirst { outer, inner } => (outer, inner),
            Self::Unclassified { first, rest } => (first, rest),
        };
        std::iter::once(*first).chain(rest.iter().copied())
    }
}

#[derive(Clone)]
struct ShellDefinition {
    form: i64,
    faces: Vec<(u32, Sense)>,
}

struct BodyDefinition<'a> {
    entry: &'a DirectoryEntry,
    kind: BodyKind,
    shells: Vec<(u32, Sense)>,
    closed: bool,
    transform: Option<cadmpeg_ir::transform::Transform>,
}

struct SurfaceSupport<'a> {
    id: &'a SurfaceId,
    geometry: &'a cadmpeg_ir::geometry::SurfaceGeometry,
    factor: f64,
}

#[derive(Debug)]
enum SourceEdgeSelectionError {
    NoMatch,
    Ambiguous,
    Codec(CodecError),
}

#[derive(Debug)]
enum PcurveProjectionError {
    Invalid(&'static str),
    Resource(CodecError),
}

impl From<CodecError> for PcurveProjectionError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

fn compose_sense(left: Sense, right: Sense) -> Sense {
    if left == right {
        Sense::Forward
    } else {
        Sense::Reversed
    }
}

fn list_index(record: &ParameterRecord, index: usize) -> Option<usize> {
    record
        .integer(index)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|value| value.checked_sub(1))
}

fn topology_vertex<'ids>(
    candidate: &mut ModelDraft,
    (vertex_ids, storage): (
        &'ids mut BTreeMap<(u32, usize), VertexId>,
        &mut ScopedReservation<'_>,
    ),
    vertex_lists: &BTreeMap<u32, Vec<Point3>>,
    stem: &crate::ids::Stem,
    vertex_key: (u32, usize),
    sequences: &mut super::geometry::SourceSequences,
    ctx: &DecodeContext<'_>,
) -> Result<Option<&'ids VertexId>, CodecError> {
    let (list, index) = vertex_key;
    if vertex_ids.contains_key(&(list, index)) {
        return Ok(vertex_ids.get(&(list, index)));
    }
    let Some(position) = FinitePoint3::new(vertex_lists[&list][index]) else {
        return Ok(None);
    };
    ctx.reserve_vec(
        &mut candidate.model_mut().points,
        1,
        "iges B-rep topology points",
    )?;
    ctx.reserve_vec(
        &mut candidate.model_mut().vertices,
        1,
        "iges B-rep topology vertices",
    )?;
    storage.with_storage(|| {
        ctx.admit_btree_entry(
            vertex_ids,
            &(list, index),
            "iges B-rep topology vertex index",
        )
    })?;
    let point_id = crate::ids::point_admitted(&stem.child(list).slot(index + 1), ctx)?;
    let point = Point::new(
        point_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
        position,
        None,
    );
    sequences.record_point(&point_id, stem, ctx)?;
    let vertex_id = crate::ids::vertex_admitted(&stem.child(list).slot(index + 1), ctx)?;
    ctx.charge_entities(1, "iges_geometry_brep")?;
    candidate.model_mut().points.push(point);
    ctx.charge_entities(1, "iges_geometry_brep")?;
    let stored_id =
        storage.with_storage(|| vertex_id.try_clone_for_decode(ctx, "iges B-rep identity copy"))?;
    candidate.model_mut().vertices.push(Vertex {
        id: vertex_id,
        point: point_id,
        tolerance: None,
    });
    Ok(Some(vertex_ids.entry((list, index)).or_insert(stored_id)))
}

fn source_edge_for_vertices<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &'a CadIr,
    candidates: &[usize],
    curve_geometry: &CurveGeometry,
    natural_start: Point3,
    natural_end: Point3,
    tolerance: f64,
) -> Result<&'a Edge, SourceEdgeSelectionError> {
    let mut matching = None;
    let mut positions = candidates.iter();
    while let Some(position) = ctx
        .next_charged(&mut positions, "iges B-rep source edge candidates")
        .map_err(SourceEdgeSelectionError::Codec)?
    {
        let Some(edge) = ir.model.edges.get(*position) else {
            continue;
        };
        let Some(range) = edge.param_range() else {
            continue;
        };
        let Some(start) = finite_or_refusal(
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx,
                curve_geometry,
                range[0],
            ))
            .map_err(|limit| SourceEdgeSelectionError::Codec(limit.into()))?,
        )
        .map_err(|limit| SourceEdgeSelectionError::Codec(limit.into()))?
        else {
            continue;
        };
        let start_agrees =
            cadmpeg_ir::math::Point3::distance(start.get(), natural_start) <= tolerance;
        if !start_agrees {
            continue;
        }
        let Some(end) = finite_or_refusal(
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx,
                curve_geometry,
                range[1],
            ))
            .map_err(|limit| SourceEdgeSelectionError::Codec(limit.into()))?,
        )
        .map_err(|limit| SourceEdgeSelectionError::Codec(limit.into()))?
        else {
            continue;
        };
        let endpoints_agree =
            cadmpeg_ir::math::Point3::distance(end.get(), natural_end) <= tolerance;
        if endpoints_agree {
            if matching.is_some() {
                return Err(SourceEdgeSelectionError::Ambiguous);
            }
            matching = Some(edge);
        }
    }
    matching.ok_or(SourceEdgeSelectionError::NoMatch)
}

fn project_pcurve_uses(
    candidate: &mut ModelDraft,
    uses: &[(bool, u32)],
    resolved: Vec<(PcurveGeometry, [f64; 2])>,
    fit_tolerance: Option<f64>,
    id_stem: &crate::ids::Stem,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<PcurveUse>, PcurveProjectionError> {
    let mut projected = ctx.collection_vec(resolved.len(), "iges B-rep projected pcurve uses")?;
    for (index, ((isoparametric, _), (geometry, range))) in ctx
        .admit_iter(uses, "iges B-rep pcurve projection traversal")
        .map_err(CodecError::from)?
        .zip(resolved)
        .enumerate()
    {
        let parameter_range = cadmpeg_ir::units::FiniteVector::new(range).ok_or(
            PcurveProjectionError::Invalid(PcurveMetadata::NON_FINITE_PARAMETER_RANGE),
        )?;
        let checked_tolerance = fit_tolerance
            .map(|value| {
                cadmpeg_ir::geometry::FitTolerance::try_new(value).map_err(|_| {
                    PcurveProjectionError::Invalid(PcurveMetadata::INVALID_FIT_TOLERANCE)
                })
            })
            .transpose()?;
        ctx.reserve_vec(
            &mut candidate.model_mut().pcurves,
            1,
            "iges B-rep pcurve slots",
        )?;
        let id = crate::ids::pcurve_admitted(&id_stem.slot(index), ctx)?;
        ctx.charge_entities(1, "iges_geometry_brep")?;
        candidate.model_mut().pcurves.push(Pcurve {
            id: id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
            geometry,
            metadata: PcurveMetadata::general(None, Some(parameter_range), checked_tolerance),
        });
        projected.push(PcurveUse {
            pcurve: id,
            isoparametric: Some(*isoparametric),
            parameter_range: None,
        });
    }
    Ok(projected)
}

fn surface_point_or_refusal(
    evaluation: Result<FinitePoint3, EvaluationFailure<Point3>>,
) -> Result<Option<FinitePoint3>, super::composite::CompositeCurveError> {
    finite_or_refusal(evaluation).map_err(|limit| CodecError::from(limit).into())
}

// The resolved vector carries each pcurve and range into projection.
// Resolve and evaluate each use before advancing to the next: either step
// can refuse or invalidate the use. Both read the same immutable source.
/// One resolved pcurve use: its geometry and the parameter range it covers.
type ResolvedPcurveUses = Vec<(PcurveGeometry, [f64; 2])>;

#[derive(Clone, Copy)]
struct PcurveEndpointCheck {
    start: Point3,
    end: Point3,
    tolerance: f64,
}

fn resolve_pcurve_uses<'a>(
    source: &'a CadIr,
    uses: &[(bool, u32)],
    support: &SurfaceSupport<'_>,
    endpoints: PcurveEndpointCheck,
    ctx: &'a DecodeContext<'_>,
    model_index: &mut Option<cadmpeg_ir::index::DecodeModelIndex<'a, 'a>>,
) -> Result<Option<ResolvedPcurveUses>, super::composite::CompositeCurveError> {
    let PcurveEndpointCheck {
        start: expected_start,
        end: expected_end,
        tolerance,
    } = endpoints;
    if uses.is_empty() {
        return Ok(Some(Vec::new()));
    }
    // A model without procedural surface records evaluates the support directly.
    if !source.model.procedural_surfaces.is_empty() && model_index.is_none() {
        *model_index = Some(cadmpeg_ir::index::ModelIndex::new_model_only(source, ctx)?);
    }
    let index = model_index.as_ref();
    let point_on_surface = |u, v| {
        let evaluation = if let Some(index) = index {
            cadmpeg_ir::eval::model_surface_point_by_id(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
                index,
                support.id,
                u,
                v,
            )
        } else {
            cadmpeg_ir::eval::decode::surface_point(ctx, support.geometry, u, v)
        };
        surface_point_or_refusal(evaluation)
    };
    let mut resolved = ctx.collection_vec(uses.len(), "iges B-rep resolved pcurves")?;
    let (mut mapped, _mapped_storage) =
        ctx.temporary_vec(uses.len(), "iges B-rep mapped pcurves")?;
    let mut use_sequences = uses.iter();
    while let Some((_, sequence)) =
        ctx.next_charged(&mut use_sequences, "iges B-rep pcurve resolution traversal")?
    {
        let Some((geometry, range)) = pcurve_geometry(
            source,
            *sequence,
            &super::trimming::PcurveSupport {
                surface_id: support.id,
                geometry: support.geometry,
                factor: support.factor,
            },
            Some(tolerance),
            ctx,
            None,
        )?
        else {
            return Ok(None);
        };
        let (Some(start), Some(end)) = (
            finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, range[0]),
            )?)
            .map_err(CodecError::from)?
            .map(|uv| point_on_surface(uv.u, uv.v))
            .transpose()?
            .flatten(),
            finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, range[1]),
            )?)
            .map_err(CodecError::from)?
            .map(|uv| point_on_surface(uv.u, uv.v))
            .transpose()?
            .flatten(),
        ) else {
            return Ok(None);
        };
        resolved.push((geometry, range));
        mapped.push((start.get(), end.get()));
    }
    Ok(
        (cadmpeg_ir::math::Point3::distance(mapped[0].0, expected_start) <= tolerance
            && cadmpeg_ir::math::Point3::distance(mapped[mapped.len() - 1].1, expected_end)
                <= tolerance
            && ctx.all_by(
                mapped.windows(2),
                |pair| Ok(cadmpeg_ir::math::Point3::distance(pair[0].1, pair[1].0) <= tolerance),
                "iges B-rep pcurve continuity",
            )?)
        .then_some(resolved),
    )
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    (entries, records): (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<ProjectionOutcome, CodecError> {
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let factor = global.length_factor_mm();
    let tolerance = global.minimum_resolution_mm();
    let mut definition_storage = ctx.reserve_scoped(0, "iges B-rep definitions")?;
    // Successful record reservations live as long as the definition tables.
    let mut definition_reservations = Vec::new();
    let mut vertex_lists = BTreeMap::<u32, Vec<Point3>>::new();
    let mut edge_lists = BTreeMap::<u32, Vec<EdgeDefinition>>::new();
    let mut loops = BTreeMap::<u32, Vec<LoopUse>>::new();
    let mut faces = BTreeMap::<u32, FaceDefinition>::new();

    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 502 && entry.form == 1)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        if entry.transform != 0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "vertex lists cannot carry a transformation"),
            )?;
            continue;
        }
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "vertex-list count is not positive"),
            )?;
            continue;
        };
        let mut record_storage = ctx.reserve_scoped(0, "iges B-rep definition record scratch")?;
        let mut points = record_storage
            .with_storage(|| ctx.collection_vec(count, "iges B-rep vertex-list points"))?;
        let mut tuples = 0..count;
        while let Some(index) = ctx.next_charged(&mut tuples, "iges B-rep definition tuples")? {
            let start = 2 + index * 3;
            let values = [
                record.number(start),
                record.number(start + 1),
                record.number(start + 2),
            ];
            let [Some(x), Some(y), Some(z)] = values else {
                points.clear();
                break;
            };
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                points.clear();
                break;
            }
            points.push(Point3::new(x * factor, y * factor, z * factor));
        }
        if points.len() != count {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "vertex-list coordinates are truncated or non-finite"),
            )?;
            continue;
        }
        definition_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut vertex_lists,
                entry.sequence,
                points,
                "iges B-rep vertex-list nodes",
            )
        })?;
        definition_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut definition_reservations,
                1,
                "iges B-rep definition reservations",
            )
        })?;
        definition_reservations.push(record_storage);
    }

    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 504 && entry.form == 1)
    {
        if entry.transform != 0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "edge lists cannot carry a transformation"),
            )?;
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "edge-list count is not positive"),
            )?;
            continue;
        };
        let mut record_storage = ctx.reserve_scoped(0, "iges B-rep definition record scratch")?;
        let mut edges = record_storage
            .with_storage(|| ctx.collection_vec(count, "iges B-rep edge-list edges"))?;
        let mut tuples = 0..count;
        while let Some(item) = ctx.next_charged(&mut tuples, "iges B-rep definition tuples")? {
            let start = 2 + item * 5;
            let Some(edge) = pointer(record, start)
                .zip(pointer(record, start + 1))
                .zip(list_index(record, start + 2))
                .zip(pointer(record, start + 3))
                .zip(list_index(record, start + 4))
                .map(
                    |((((curve, start_list), start_index), end_list), end_index)| EdgeDefinition {
                        curve,
                        start_list,
                        start_index,
                        end_list,
                        end_index,
                    },
                )
            else {
                edges.clear();
                break;
            };
            if vertex_lists
                .get(&edge.start_list)
                .is_none_or(|list| edge.start_index >= list.len())
                || vertex_lists
                    .get(&edge.end_list)
                    .is_none_or(|list| edge.end_index >= list.len())
            {
                edges.clear();
                break;
            }
            edges.push(edge);
        }
        if edges.len() != count {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "edge-list tuple is invalid or names a missing vertex"),
            )?;
            continue;
        }
        definition_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut edge_lists,
                entry.sequence,
                edges,
                "iges B-rep edge-list nodes",
            )
        })?;
        definition_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut definition_reservations,
                1,
                "iges B-rep definition reservations",
            )
        })?;
        definition_reservations.push(record_storage);
    }

    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 508 && entry.form == 1)
    {
        if entry.transform != 0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "loops cannot carry a transformation"),
            )?;
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "loop edge-use count is not positive"),
            )?;
            continue;
        };
        let mut index = 2;
        let mut record_storage = ctx.reserve_scoped(0, "iges B-rep definition record scratch")?;
        let mut uses =
            record_storage.with_storage(|| ctx.collection_vec(count, "iges B-rep loop uses"))?;
        let mut tuples = 0..count;
        while ctx
            .next_charged(&mut tuples, "iges B-rep definition tuples")?
            .is_some()
        {
            let Some(use_type) = record.integer(index) else {
                uses.clear();
                break;
            };
            let Some(list) = pointer(record, index + 1) else {
                uses.clear();
                break;
            };
            let Some(item_index) = list_index(record, index + 2) else {
                uses.clear();
                break;
            };
            let Some(pcurve_count) = record.count(index + 4) else {
                uses.clear();
                break;
            };
            let mut pcurves = record_storage
                .with_storage(|| ctx.collection_vec(pcurve_count, "iges B-rep use pcurves"))?;
            let mut pcurve_tuples = 0..pcurve_count;
            while let Some(pcurve_index) =
                ctx.next_charged(&mut pcurve_tuples, "iges B-rep definition tuples")?
            {
                let isoparametric = match record.integer(index + 5 + pcurve_index * 2) {
                    Some(1) => true,
                    Some(0) => false,
                    _ => {
                        pcurves.clear();
                        break;
                    }
                };
                let Some(sequence) = pointer(record, index + 6 + pcurve_index * 2) else {
                    pcurves.clear();
                    break;
                };
                if entries.get(&sequence).is_none_or(|entry| {
                    entry.status.use_flag(global.global_table()) != Some(UseFlag::Parametric)
                }) {
                    pcurves.clear();
                    break;
                }
                pcurves.push((isoparametric, sequence));
            }
            if pcurves.len() != pcurve_count {
                uses.clear();
                break;
            }
            let use_ = match use_type {
                0 => {
                    let sense = match record.integer(index + 3) {
                        Some(1) => Sense::Forward,
                        Some(0) => Sense::Reversed,
                        _ => {
                            uses.clear();
                            break;
                        }
                    };
                    if edge_lists
                        .get(&list)
                        .is_none_or(|items| item_index >= items.len())
                    {
                        uses.clear();
                        break;
                    }
                    LoopUse::Edge {
                        edge_list: list,
                        edge_index: item_index,
                        sense,
                        pcurves,
                    }
                }
                1 => {
                    if vertex_lists
                        .get(&list)
                        .is_none_or(|items| item_index >= items.len())
                    {
                        uses.clear();
                        break;
                    }
                    LoopUse::Vertex {
                        vertex_list: list,
                        vertex_index: item_index,
                        pcurves,
                    }
                }
                _ => {
                    uses.clear();
                    break;
                }
            };
            uses.push(use_);
            index += 5 + pcurve_count * 2;
        }
        if uses.len() != count {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "loop edge-use tuple is invalid"),
            )?;
            continue;
        }
        definition_storage.with_storage(|| {
            ctx.insert_btree_map(&mut loops, entry.sequence, uses, "iges B-rep loop nodes")
        })?;
        definition_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut definition_reservations,
                1,
                "iges B-rep definition reservations",
            )
        })?;
        definition_reservations.push(record_storage);
    }

    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 510 && entry.form == 1)
    {
        if entry.transform != 0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "faces cannot carry a transformation"),
            )?;
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(surface) = pointer(record, 1) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "face surface pointer is invalid"),
            )?;
            continue;
        };
        let Some(count) = record.count(2).filter(|count| *count > 0) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "face loop count is not positive"),
            )?;
            continue;
        };
        let has_outer_loop = match record.integer(3) {
            Some(1) => true,
            Some(0) => false,
            _ => {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "face outer-loop flag is not logical"),
                )?;
                continue;
            }
        };
        let Some(first) = pointer(record, 4) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "face loop pointer is invalid"),
            )?;
            continue;
        };
        let mut record_storage = ctx.reserve_scoped(0, "iges B-rep definition record scratch")?;
        let mut rest = record_storage
            .with_storage(|| ctx.collection_vec(count - 1, "iges B-rep face loop pointers"))?;
        let mut valid_pointers = true;
        let mut tuples = 1..count;
        while let Some(index) = ctx.next_charged(&mut tuples, "iges B-rep definition tuples")? {
            let Some(sequence) = pointer(record, 4 + index) else {
                valid_pointers = false;
                break;
            };
            rest.push(sequence);
        }
        if !valid_pointers {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "face loop pointer is invalid"),
            )?;
            continue;
        }
        let face_loops = if has_outer_loop {
            FaceLoopPointers::OuterFirst {
                outer: first,
                inner: rest,
            }
        } else {
            FaceLoopPointers::Unclassified { first, rest }
        };
        if ctx.any_by(
            face_loops.iter(),
            |sequence| Ok(!loops.contains_key(&sequence)),
            "iges B-rep face loop references",
        )? {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "face loop is missing"),
            )?;
            continue;
        }
        definition_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut faces,
                entry.sequence,
                FaceDefinition {
                    surface,
                    loops: face_loops,
                },
                "iges B-rep face nodes",
            )
        })?;
        if count > 1 {
            definition_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut definition_reservations,
                    1,
                    "iges B-rep definition reservations",
                )
            })?;
            definition_reservations.push(record_storage);
        }
    }

    let mut shell_definitions = BTreeMap::new();
    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 514 && matches!(entry.form, 1 | 2))
    {
        if entry.transform != 0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "shells cannot carry a transformation"),
            )?;
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "shell face count is not positive"),
            )?;
            continue;
        };
        let mut record_storage = ctx.reserve_scoped(0, "iges B-rep definition record scratch")?;
        let mut face_uses = record_storage
            .with_storage(|| ctx.collection_vec(count, "iges B-rep shell face uses"))?;
        let mut tuples = 0..count;
        while let Some(index) = ctx.next_charged(&mut tuples, "iges B-rep definition tuples")? {
            let Some(face) = pointer(record, 2 + index * 2) else {
                face_uses.clear();
                break;
            };
            let sense = match record.integer(3 + index * 2) {
                Some(1) => Sense::Forward,
                Some(0) => Sense::Reversed,
                _ => {
                    face_uses.clear();
                    break;
                }
            };
            if !faces.contains_key(&face) {
                face_uses.clear();
                break;
            }
            face_uses.push((face, sense));
        }
        if face_uses.len() != count {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "shell face-use tuple is invalid"),
            )?;
            continue;
        }
        definition_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut shell_definitions,
                entry.sequence,
                ShellDefinition {
                    form: entry.form,
                    faces: face_uses,
                },
                "iges B-rep shell nodes",
            )
        })?;
        definition_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut definition_reservations,
                1,
                "iges B-rep definition reservations",
            )
        })?;
        definition_reservations.push(record_storage);
    }

    let mut body_definitions = Vec::new();
    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 514 && entry.form == 2)
    {
        if shell_definitions.contains_key(&entry.sequence) {
            let mut shells = definition_storage
                .with_storage(|| ctx.collection_vec(1, "iges B-rep sheet shell uses"))?;
            shells.push((entry.sequence, Sense::Forward));
            definition_storage.with_storage(|| {
                ctx.reserve_vec(&mut body_definitions, 1, "iges B-rep body definitions")
            })?;
            body_definitions.push(BodyDefinition {
                entry,
                kind: BodyKind::Sheet,
                shells,
                closed: false,
                transform: None,
            });
        }
    }
    let mut referenced_closed_shells = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 186 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(outer) = pointer(record, 1) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid outer-shell pointer is invalid"),
            )?;
            continue;
        };
        let outer_sense = match record.integer(2) {
            Some(1) => Sense::Forward,
            Some(0) => Sense::Reversed,
            _ => {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "solid outer-shell orientation is not logical"),
                )?;
                continue;
            }
        };
        let Some(void_count) = record.count(3) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid void-shell count is invalid"),
            )?;
            continue;
        };
        let shell_count = void_count.checked_add(1).ok_or_else(|| {
            cadmpeg_core::decode::refuse_local_limit("iges B-rep solid shell uses", u64::MAX, 1)
        })?;
        let mut record_storage = ctx.reserve_scoped(0, "iges B-rep definition record scratch")?;
        let mut shell_uses = record_storage
            .with_storage(|| ctx.collection_vec(shell_count, "iges B-rep solid shell uses"))?;
        shell_uses.push((outer, outer_sense));
        let mut valid = true;
        let mut tuples = 0..void_count;
        while let Some(index) = ctx.next_charged(&mut tuples, "iges B-rep definition tuples")? {
            let Some(shell) = pointer(record, 4 + index * 2) else {
                valid = false;
                break;
            };
            let sense = match record.integer(5 + index * 2) {
                Some(1) => Sense::Forward,
                Some(0) => Sense::Reversed,
                _ => {
                    valid = false;
                    break;
                }
            };
            shell_uses.push((shell, sense));
        }
        if !valid
            || ctx.any_by(
                &shell_uses,
                |(sequence, _)| {
                    Ok(shell_definitions
                        .get(sequence)
                        .is_none_or(|shell| shell.form != 1))
                },
                "iges B-rep solid shell references",
            )?
        {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid shell-use tuple is invalid or not closed"),
            )?;
            continue;
        }
        for (sequence, _) in ctx.admit_iter(&shell_uses, "iges B-rep shell reference traversal")? {
            definition_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut referenced_closed_shells,
                    *sequence,
                    "iges B-rep referenced closed shells",
                )
            })?;
        }
        let mut transform_storage = ctx.reserve_scoped(0, "iges B-rep transform scratch")?;
        let transform = match transform_storage.with_storage(|| {
            resolve_transform(
                entry.transform,
                entries,
                records,
                factor,
                global.real_precision(),
                &mut BTreeSet::new(),
                ctx,
            )
        }) {
            Ok(transform) => (entry.transform != 0).then_some(transform),
            Err(error) => {
                let message = error.non_resource()?;
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        definition_storage.with_storage(|| {
            ctx.reserve_vec(&mut body_definitions, 1, "iges B-rep body definitions")
        })?;
        body_definitions.push(BodyDefinition {
            entry,
            kind: BodyKind::Solid,
            shells: shell_uses,
            closed: true,
            transform,
        });
        definition_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut definition_reservations,
                1,
                "iges B-rep definition reservations",
            )
        })?;
        definition_reservations.push(record_storage);
    }
    for entry in ctx
        .admit_iter(directory, "iges B-rep directory traversal")?
        .filter(|entry| entry.entity_type == 514 && entry.form == 1)
    {
        if shell_definitions.contains_key(&entry.sequence)
            && !referenced_closed_shells.contains(&entry.sequence)
        {
            let mut shells = definition_storage
                .with_storage(|| ctx.collection_vec(1, "iges B-rep sheet shell uses"))?;
            shells.push((entry.sequence, Sense::Forward));
            definition_storage.with_storage(|| {
                ctx.reserve_vec(&mut body_definitions, 1, "iges B-rep body definitions")
            })?;
            body_definitions.push(BodyDefinition {
                entry,
                kind: BodyKind::Sheet,
                shells,
                closed: true,
                transform: None,
            });
        }
    }

    // The surface and curve arenas never change while bodies project: the
    // only writer is the per-body draft commit, which appends, and a
    // topology draft carries no surface or curve. One first-occurrence
    // position map per arena therefore serves the whole call. The guard
    // keeps files without explicit B-rep from paying for either map.
    let mut index_storage = ctx.reserve_scoped(0, "iges B-rep geometry indexes")?;
    let mut surface_positions = BTreeMap::<String, usize>::new();
    let mut curve_positions = BTreeMap::<String, usize>::new();
    if !body_definitions.is_empty() {
        for (position, surface) in ctx
            .admit_iter(&ir.model.surfaces, "iges B-rep surface index traversal")?
            .enumerate()
        {
            if !ctx.contains_key_btree_map(
                &surface_positions,
                surface.id.as_str(),
                "iges B-rep surface index lookup",
            )? {
                let key = ctx.copy_scoped_text(
                    surface.id.as_str(),
                    &mut index_storage,
                    "iges B-rep surface index keys",
                )?;
                index_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut surface_positions,
                        key,
                        position,
                        "iges B-rep surface index nodes",
                    )
                })?;
            }
        }
        for (position, curve) in ctx
            .admit_iter(&ir.model.curves, "iges B-rep curve index traversal")?
            .enumerate()
        {
            if !ctx.contains_key_btree_map(
                &curve_positions,
                curve.id.as_str(),
                "iges B-rep curve index lookup",
            )? {
                let key = ctx.copy_scoped_text(
                    curve.id.as_str(),
                    &mut index_storage,
                    "iges B-rep curve index keys",
                )?;
                index_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut curve_positions,
                        key,
                        position,
                        "iges B-rep curve index nodes",
                    )
                })?;
            }
        }
    }

    // The session holds the document's exclusive borrow. Its identity index
    // remains unbuilt until the first body reaches commit admission.
    let mut commit_session = CommitSession::new(ir, ctx, None)?;
    let mut edges_by_curve = BTreeMap::<String, Vec<usize>>::new();
    let mut indexed_edge_count = 0;
    for definition in ctx.admit_iter(body_definitions, "iges B-rep body traversal")? {
        let ir = commit_session.document();
        let entry = definition.entry;
        let mut model_index = None;
        // Extend the source index only over edges appended since the last body.
        // Keys own their text because a successful body commit changes the arena.
        let mut body_storage = ctx.reserve_scoped(0, "iges B-rep body indexes")?;
        let mut radial_identity_storage = ctx.reserve_scoped(0, "iges B-rep radial identities")?;
        let mut candidate = ModelDraft::new();
        let stem = crate::ids::Stem::directory(entry.sequence);
        let body_id = crate::ids::body_admitted(&stem, ctx)?;
        sequences.record_body(&body_id, entry.sequence, &stem, ctx)?;
        let region_id = crate::ids::region_admitted(&stem, ctx)?;
        let mut vertex_ids = BTreeMap::<(u32, usize), VertexId>::new();
        let mut edge_ids = BTreeMap::<(u32, usize), EdgeId>::new();
        let mut radial = BTreeMap::<(u32, u32, usize), Vec<usize>>::new();
        let mut region_shells =
            ctx.collection_vec(definition.shells.len(), "iges B-rep region shell ids")?;
        let mut consumed = BTreeSet::new();
        let mut valid = true;
        let mut shell_uses = definition.shells.iter();
        while let Some(&(shell_sequence, shell_sense)) =
            ctx.next_charged(&mut shell_uses, "iges B-rep body shell traversal")?
        {
            let shell_definition = &shell_definitions[&shell_sequence];
            let shell_stem = if shell_sequence == entry.sequence && definition.shells.len() == 1 {
                std::borrow::Cow::Borrowed(&stem)
            } else {
                std::borrow::Cow::Owned(stem.child(shell_sequence))
            };
            let shell_id = crate::ids::shell_admitted(&shell_stem, ctx)?;
            let mut shell_faces =
                ctx.collection_vec(shell_definition.faces.len(), "iges B-rep shell face ids")?;
            let mut face_uses = shell_definition.faces.iter();
            while let Some(&(face_sequence, native_face_sense)) =
                ctx.next_charged(&mut face_uses, "iges B-rep shell face traversal")?
            {
                let face_sense = compose_sense(native_face_sense, shell_sense);
                let face_definition = &faces[&face_sequence];
                let surface_id = crate::ids::surface_admitted(
                    &crate::ids::Stem::directory(face_definition.surface),
                    ctx,
                )?;
                let Some(support_geometry) = ctx
                    .get_btree_map(
                        &surface_positions,
                        surface_id.as_str(),
                        "iges B-rep surface lookup",
                    )?
                    .and_then(|position| ir.model.surfaces.get(*position))
                    .map(|surface| &surface.geometry)
                else {
                    valid = false;
                    break;
                };
                let face_id = crate::ids::face_admitted(&shell_stem.child(face_sequence), ctx)?;
                sequences.record_face(&face_id, face_sequence, ctx)?;
                let loop_id_for =
                    |sequence| crate::ids::loop_admitted(&shell_stem.child(sequence), ctx);
                let mut loop_sequences = face_definition.loops.iter();
                while let Some(loop_sequence) =
                    ctx.next_charged(&mut loop_sequences, "iges B-rep face loop traversal")?
                {
                    let uses = &loops[&loop_sequence];
                    let loop_id = loop_id_for(loop_sequence)?;
                    let edge_use_count = ctx
                        .admit_iter(uses, "iges B-rep edge use count")?
                        .filter(|use_| matches!(use_, LoopUse::Edge { .. }))
                        .count();
                    let mut coedge_ids =
                        ctx.collection_vec(edge_use_count, "iges B-rep coedge ids")?;
                    for (index, use_) in ctx
                        .admit_iter(uses, "iges B-rep coedge identity traversal")?
                        .enumerate()
                    {
                        if matches!(use_, LoopUse::Edge { .. }) {
                            coedge_ids.push(crate::ids::coedge_admitted(
                                &shell_stem.child(loop_sequence).slot(index),
                                ctx,
                            )?);
                        }
                    }
                    let mut coedge_position = 0;
                    let mut predecessor = coedge_ids.last();
                    let vertex_use_count = uses.len() - edge_use_count;
                    let (mut loop_vertex_uses, vertex_use_storage) =
                        ctx.temporary_vec(vertex_use_count, "iges B-rep loop vertex uses")?;
                    let mut loop_uses = uses.iter().enumerate();
                    while let Some((use_index, use_)) =
                        ctx.next_charged(&mut loop_uses, "iges B-rep loop use traversal")?
                    {
                        let LoopUse::Edge {
                            edge_list,
                            edge_index,
                            sense,
                            pcurves,
                        } = use_
                        else {
                            let LoopUse::Vertex {
                                vertex_list,
                                vertex_index,
                                pcurves,
                            } = use_
                            else {
                                continue;
                            };
                            let Some(vertex) = topology_vertex(
                                &mut candidate,
                                (&mut vertex_ids, &mut body_storage),
                                &vertex_lists,
                                &stem,
                                (*vertex_list, *vertex_index),
                                sequences,
                                ctx,
                            )?
                            else {
                                continue;
                            };
                            let after = predecessor
                                .map(|id| {
                                    id.try_clone_for_decode(ctx, "iges loop predecessor identity")
                                })
                                .transpose()?;
                            let expected = vertex_lists[vertex_list][*vertex_index];
                            let Some(resolved) = (match resolve_pcurve_uses(
                                ir,
                                pcurves,
                                &SurfaceSupport {
                                    id: &surface_id,
                                    geometry: support_geometry,
                                    factor,
                                },
                                PcurveEndpointCheck {
                                    start: expected,
                                    end: expected,
                                    tolerance,
                                },
                                ctx,
                                &mut model_index,
                            ) {
                                Ok(resolved) => resolved,
                                Err(error) => {
                                    let error = error.non_resource()?;
                                    super::push_entity_loss(
                                        ctx,
                                        &mut losses,
                                        entry,
                                        format_args!(
                                            "a loop vertex-use pcurve states no carrier: {error}"
                                        ),
                                    )?;
                                    valid = false;
                                    break;
                                }
                            }) else {
                                super::push_entity_loss(
                                    ctx,
                                    &mut losses,
                                    entry,
                                    format_args!(
                                        "{}",
                                        "loop vertex-use pcurves disagree with the pole vertex"
                                    ),
                                )?;
                                valid = false;
                                break;
                            };
                            let projected = match project_pcurve_uses(
                                &mut candidate,
                                pcurves,
                                resolved,
                                Some(tolerance),
                                &shell_stem.child(loop_sequence).slot(use_index),
                                ctx,
                            ) {
                                Ok(projected) => projected,
                                Err(PcurveProjectionError::Invalid(error)) => {
                                    super::push_entity_loss(
                                        ctx,
                                        &mut losses,
                                        entry,
                                        format_args!("{error}"),
                                    )?;
                                    valid = false;
                                    break;
                                }
                                Err(PcurveProjectionError::Resource(error)) => return Err(error),
                            };
                            loop_vertex_uses.push((
                                vertex.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                                after,
                                projected,
                            ));
                            continue;
                        };
                        let edge_definition = edge_lists[edge_list][*edge_index];
                        let mut placed = true;
                        for (list, index) in [
                            (edge_definition.start_list, edge_definition.start_index),
                            (edge_definition.end_list, edge_definition.end_index),
                        ] {
                            placed = topology_vertex(
                                &mut candidate,
                                (&mut vertex_ids, &mut body_storage),
                                &vertex_lists,
                                &stem,
                                (list, index),
                                sequences,
                                ctx,
                            )?
                            .is_some()
                                && placed;
                        }
                        if !placed {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!(
                                    "{}",
                                    "an edge vertex position states a non-finite coordinate"
                                ),
                            )?;
                            valid = false;
                            break;
                        }
                        let edge_key = (*edge_list, *edge_index);
                        let natural_start =
                            vertex_lists[&edge_definition.start_list][edge_definition.start_index];
                        let natural_end =
                            vertex_lists[&edge_definition.end_list][edge_definition.end_index];
                        let (expected_start, expected_end) = if *sense == Sense::Forward {
                            (natural_start, natural_end)
                        } else {
                            (natural_end, natural_start)
                        };
                        let Some(resolved) = (match resolve_pcurve_uses(
                            ir,
                            pcurves,
                            &SurfaceSupport {
                                id: &surface_id,
                                geometry: support_geometry,
                                factor,
                            },
                            PcurveEndpointCheck {
                                start: expected_start,
                                end: expected_end,
                                tolerance,
                            },
                            ctx,
                            &mut model_index,
                        ) {
                            Ok(resolved) => resolved,
                            Err(error) => {
                                let error = error.non_resource()?;
                                super::push_entity_loss(
                                    ctx,
                                    &mut losses,
                                    entry,
                                    format_args!(
                                        "a loop edge-use pcurve states no carrier: {error}"
                                    ),
                                )?;
                                valid = false;
                                break;
                            }
                        }) else {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!(
                                    "{}",
                                    "loop edge-use pcurves disagree with the edge vertices"
                                ),
                            )?;
                            valid = false;
                            break;
                        };
                        let edge_id = if let Some(id) = edge_ids.get(&edge_key) {
                            id.try_clone_for_decode(ctx, "iges B-rep identity copy")?
                        } else {
                            let curve_id = crate::ids::curve_admitted(
                                &crate::ids::Stem::directory(edge_definition.curve),
                                ctx,
                            )?;
                            let positions = &mut edges_by_curve;
                            for (offset, edge) in ctx
                                .admit_iter(
                                    &ir.model.edges[indexed_edge_count..],
                                    "iges B-rep source edge index traversal",
                                )?
                                .enumerate()
                            {
                                if let Some(curve) = edge.curve() {
                                    if !ctx.contains_key_btree_map(
                                        positions,
                                        curve.as_str(),
                                        "iges B-rep source edge lookup",
                                    )? {
                                        let key = ctx.copy_scoped_text(
                                            curve.as_str(),
                                            &mut index_storage,
                                            "iges B-rep source edge index keys",
                                        )?;
                                        index_storage.with_storage(|| {
                                            ctx.insert_btree_map(
                                                positions,
                                                key,
                                                Vec::new(),
                                                "iges B-rep source edge index nodes",
                                            )
                                        })?;
                                    }
                                    let indexed = ctx
                                        .get_mut_btree_map(
                                            positions,
                                            curve.as_str(),
                                            "iges B-rep source edge lookup",
                                        )?
                                        .ok_or_else(|| {
                                            CodecError::malformed(
                                                "IGES B-rep source edge index is absent",
                                            )
                                        })?;
                                    index_storage.with_storage(|| {
                                        ctx.reserve_vec(
                                            indexed,
                                            1,
                                            "iges B-rep source edge positions",
                                        )
                                    })?;
                                    indexed.push(indexed_edge_count + offset);
                                }
                            }
                            indexed_edge_count = ir.model.edges.len();
                            let Some(candidates) = ctx.get_btree_map(
                                positions,
                                curve_id.as_str(),
                                "iges B-rep source edge lookup",
                            )?
                            else {
                                valid = false;
                                break;
                            };
                            let Some(curve) = ctx
                                .get_btree_map(
                                    &curve_positions,
                                    curve_id.as_str(),
                                    "iges B-rep curve lookup",
                                )?
                                .and_then(|position| ir.model.curves.get(*position))
                            else {
                                super::push_entity_loss(
                                    ctx,
                                    &mut losses,
                                    entry,
                                    format_args!(
                                        "{}",
                                        "edge curve endpoints disagree with the vertex-list points"
                                    ),
                                )?;
                                valid = false;
                                break;
                            };
                            let source_edge = match source_edge_for_vertices(
                                ctx,
                                ir,
                                candidates,
                                &curve.geometry,
                                natural_start,
                                natural_end,
                                tolerance,
                            ) {
                                Ok(source_edge) => source_edge,
                                Err(SourceEdgeSelectionError::NoMatch) => {
                                    super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "edge curve endpoints disagree with the vertex-list points"))?;
                                    valid = false;
                                    break;
                                }
                                Err(SourceEdgeSelectionError::Ambiguous) => {
                                    super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "edge curve maps to multiple ambiguous edge occurrences"))?;
                                    valid = false;
                                    break;
                                }
                                Err(SourceEdgeSelectionError::Codec(error)) => {
                                    return Err(error);
                                }
                            };
                            let id = crate::ids::edge_admitted(
                                &stem.child(edge_key.0).slot(edge_key.1 + 1),
                                ctx,
                            )?;
                            let carrier = match cadmpeg_ir::topology::EdgeCarrier::new(
                                Some(curve_id),
                                source_edge
                                    .param_range()
                                    .map(cadmpeg_ir::units::FiniteVector::get),
                            ) {
                                Ok(carrier) => carrier,
                                Err(error) => {
                                    super::push_entity_loss(
                                        ctx,
                                        &mut losses,
                                        entry,
                                        format_args!("{error}"),
                                    )?;
                                    valid = false;
                                    break;
                                }
                            };
                            ctx.reserve_vec(
                                &mut candidate.model_mut().edges,
                                1,
                                "iges B-rep topology edges",
                            )?;
                            ctx.charge_entities(1, "iges_geometry_brep")?;
                            candidate.model_mut().edges.push(Edge {
                                id: id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                                carrier,
                                start: vertex_ids
                                    [&(edge_definition.start_list, edge_definition.start_index)]
                                    .try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                                end: vertex_ids
                                    [&(edge_definition.end_list, edge_definition.end_index)]
                                    .try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                                tolerance: None,
                            });
                            body_storage.with_storage(|| {
                                ctx.insert_btree_map(
                                    &mut edge_ids,
                                    edge_key,
                                    id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                                    "iges B-rep topology edge index",
                                )
                            })?;
                            id
                        };
                        let projected = match project_pcurve_uses(
                            &mut candidate,
                            pcurves,
                            resolved,
                            Some(tolerance),
                            &shell_stem.child(loop_sequence).slot(use_index),
                            ctx,
                        ) {
                            Ok(projected) => projected,
                            Err(PcurveProjectionError::Invalid(error)) => {
                                super::push_entity_loss(
                                    ctx,
                                    &mut losses,
                                    entry,
                                    format_args!("{error}"),
                                )?;
                                valid = false;
                                break;
                            }
                            Err(PcurveProjectionError::Resource(error)) => return Err(error),
                        };
                        let coedge_id = coedge_ids[coedge_position]
                            .try_clone_for_decode(ctx, "iges B-rep identity copy")?;
                        predecessor = Some(&coedge_ids[coedge_position]);
                        coedge_position += 1;
                        let radial_key = (shell_sequence, edge_key.0, edge_key.1);
                        body_storage.with_storage(|| {
                            ctx.admit_btree_entry(
                                &radial,
                                &radial_key,
                                "iges B-rep radial index nodes",
                            )
                        })?;
                        let ring = radial.entry(radial_key).or_default();
                        body_storage.with_storage(|| {
                            ctx.reserve_vec(ring, 1, "iges B-rep radial coedge ids")
                        })?;
                        ring.push(candidate.model().coedges.len());
                        ctx.reserve_vec(
                            &mut candidate.model_mut().coedges,
                            1,
                            "iges B-rep topology coedges",
                        )?;
                        ctx.charge_entities(1, "iges_geometry_brep")?;
                        let initial_radial = radial_identity_storage.with_storage(|| {
                            coedge_id.try_clone_for_decode(ctx, "iges B-rep identity copy")
                        })?;
                        candidate.model_mut().coedges.push(Coedge {
                            id: coedge_id,
                            owner_loop: loop_id
                                .try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                            edge: edge_id,
                            radial_next: initial_radial,
                            sense: *sense,
                            pcurves: projected,
                            use_curve: None,
                        });
                    }
                    if !valid {
                        break;
                    }
                    let boundary = if coedge_ids.is_empty() {
                        let mut uses = loop_vertex_uses.into_iter();
                        let Some((vertex, None, pcurves)) = uses.next() else {
                            super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "vertex-only loop does not contain exactly one unanchored vertex"))?;
                            valid = false;
                            break;
                        };
                        if uses.next().is_some() {
                            super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "vertex-only loop does not contain exactly one unanchored vertex"))?;
                            valid = false;
                            break;
                        }
                        drop(uses);
                        drop(vertex_use_storage);
                        LoopBoundary::Vertex { vertex, pcurves }
                    } else {
                        let Some(vertex_uses) = ctx.collect_options(
                            loop_vertex_uses
                                .into_iter()
                                .map(|(vertex, after, pcurves)| {
                                    let after = after?;
                                    Some(AnchoredVertexUse {
                                        vertex,
                                        after,
                                        pcurves,
                                    })
                                }),
                            "iges B-rep anchored vertex uses",
                        )?
                        else {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!("{}", "edge loop contains an unanchored vertex use"),
                            )?;
                            valid = false;
                            break;
                        };
                        drop(vertex_use_storage);
                        let Ok(ring) =
                            cadmpeg_ir::topology::LoopRing::new(ctx, coedge_ids, vertex_uses)
                                .map_err(cadmpeg_core::CodecError::from)?
                        else {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!("{}", "edge loop has no coedges"),
                            )?;
                            valid = false;
                            break;
                        };
                        LoopBoundary::Ring(ring)
                    };
                    ctx.reserve_vec(
                        &mut candidate.model_mut().loops,
                        1,
                        "iges B-rep topology loops",
                    )?;
                    ctx.charge_entities(1, "iges_geometry_brep")?;
                    candidate.model_mut().loops.push(Loop {
                        id: loop_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                        face: face_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                        boundary,
                    });
                    body_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut consumed,
                            loop_sequence,
                            "iges B-rep consumed loop nodes",
                        )
                    })?;
                }
                if !valid {
                    break;
                }
                let face_loops = match &face_definition.loops {
                    FaceLoopPointers::OuterFirst { outer, inner } => {
                        let mut inner_ids =
                            ctx.collection_vec(inner.len(), "iges B-rep face inner loop ids")?;
                        for sequence in ctx
                            .admit_iter(inner, "iges B-rep inner loop traversal")?
                            .copied()
                        {
                            inner_ids.push(loop_id_for(sequence)?);
                        }
                        cadmpeg_ir::topology::FaceLoops::classified(loop_id_for(*outer)?, inner_ids)
                    }
                    FaceLoopPointers::Unclassified { first, rest } => {
                        let count = rest.len().checked_add(1).ok_or_else(|| {
                            cadmpeg_core::decode::refuse_local_limit(
                                "iges B-rep face unspecified loop ids",
                                u64::MAX,
                                1,
                            )
                        })?;
                        let mut loop_ids =
                            ctx.collection_vec(count, "iges B-rep face unspecified loop ids")?;
                        loop_ids.push(loop_id_for(*first)?);
                        for sequence in ctx
                            .admit_iter(rest, "iges B-rep unspecified loop traversal")?
                            .copied()
                        {
                            loop_ids.push(loop_id_for(sequence)?);
                        }
                        cadmpeg_ir::topology::FaceLoops::unspecified(loop_ids)
                    }
                };
                ctx.reserve_vec(
                    &mut candidate.model_mut().faces,
                    1,
                    "iges B-rep topology faces",
                )?;
                ctx.charge_entities(1, "iges_geometry_brep")?;
                candidate.model_mut().faces.push(Face {
                    id: face_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                    shell: shell_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                    surface: surface_id,
                    sense: face_sense,
                    loops: face_loops,
                    name: None,
                    color: None,
                    tolerance: None,
                });
                shell_faces.push(face_id);
                body_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut consumed,
                        face_sequence,
                        "iges B-rep consumed face nodes",
                    )
                })?;
            }
            if !valid {
                break;
            }
            ctx.reserve_vec(
                &mut candidate.model_mut().shells,
                1,
                "iges B-rep topology shells",
            )?;
            ctx.charge_entities(1, "iges_geometry_brep")?;
            candidate.model_mut().shells.push(
                match Shell::new(
                    shell_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                    region_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
                    shell_faces,
                    Vec::new(),
                    Vec::new(),
                ) {
                    Ok(shell) => shell,
                    Err(_) => {
                        valid = false;
                        break;
                    }
                },
            );
            region_shells.push(shell_id);
            body_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut consumed,
                    shell_sequence,
                    "iges B-rep consumed shell nodes",
                )
            })?;
        }
        if !valid {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "shell topology references missing geometry"),
            )?;
            continue;
        }
        if definition.closed
            && ctx.any_by(
                &radial,
                |(_, ring)| {
                    Ok(ring.len() != 2
                        || candidate.model().coedges[ring[0]].sense
                            == candidate.model().coedges[ring[1]].sense)
                },
                "iges B-rep radial closure",
            )?
        {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "closed shell does not use every edge exactly twice with opposite senses"
                ),
            )?;
            continue;
        }
        for (_, ring) in ctx.admit_iter(&radial, "iges B-rep radial ring traversal")? {
            for (index, position) in ctx
                .admit_iter(ring, "iges B-rep radial member traversal")?
                .enumerate()
            {
                let next = candidate.model().coedges[ring[(index + 1) % ring.len()]]
                    .id
                    .try_clone_for_decode(ctx, "iges B-rep identity copy")?;
                candidate.model_mut().coedges[*position].radial_next = next;
            }
        }
        drop(radial_identity_storage);
        ctx.reserve_vec(
            &mut candidate.model_mut().regions,
            1,
            "iges B-rep topology regions",
        )?;
        ctx.charge_entities(1, "iges_geometry_brep")?;
        candidate.model_mut().regions.push(Region {
            id: region_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
            body: body_id.try_clone_for_decode(ctx, "iges B-rep identity copy")?,
            shells: region_shells,
        });
        let mut body_regions = ctx.collection_vec(1, "iges B-rep body region ids")?;
        body_regions.push(region_id);
        ctx.reserve_vec(
            &mut candidate.model_mut().bodies,
            1,
            "iges B-rep topology bodies",
        )?;
        ctx.charge_entities(1, "iges_geometry_brep")?;
        candidate.model_mut().bodies.push(Body {
            id: body_id,
            kind: definition.kind,
            regions: body_regions,
            transform: definition.transform,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model_mut().finalize(ctx)?;
        drop(model_index);
        if commit_session.commit_model(candidate)?.is_err() {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "shell candidate failed neutral validation"),
            )?;
            continue;
        }
        ctx.insert_btree_set(&mut decoded, entry.sequence, "iges brep decoded sequences")?;
        for sequence in ctx
            .admit_iter(consumed, "iges B-rep consumed traversal")?
            .chain(
                ctx.admit_iter(&edge_ids, "iges B-rep edge list traversal")?
                    .map(|(key, _)| key.0),
            )
            .chain(
                ctx.admit_iter(&vertex_ids, "iges B-rep vertex list traversal")?
                    .map(|(key, _)| key.0),
            )
        {
            ctx.insert_btree_set(
                &mut decoded,
                sequence,
                "iges B-rep decoded topology sequences",
            )?;
        }
    }

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
