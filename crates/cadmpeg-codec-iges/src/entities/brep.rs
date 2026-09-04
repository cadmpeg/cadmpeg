// SPDX-License-Identifier: Apache-2.0
//! Explicit IGES B-rep topology projection.

use super::evaluation;
use super::geometry::{entity_loss, resolve_transform, ProjectionOutcome};
use super::trimming::pcurve_geometry;
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::geometry::{CurveGeometry, Pcurve, PcurveGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
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
    loops: Vec<u32>,
    has_outer_loop: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceEdgeSelectionError {
    NoMatch,
    Ambiguous,
}

fn compose_sense(left: Sense, right: Sense) -> Sense {
    if left == right {
        Sense::Forward
    } else {
        Sense::Reversed
    }
}

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn list_index(record: &ParameterRecord, index: usize) -> Option<usize> {
    record
        .integer(index)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|value| value.checked_sub(1))
}

fn topology_vertex(
    candidate: &mut ModelDraft,
    vertex_ids: &mut BTreeMap<(u32, usize), VertexId>,
    vertex_lists: &BTreeMap<u32, Vec<Point3>>,
    stem: &str,
    list: u32,
    index: usize,
) -> VertexId {
    vertex_ids
        .entry((list, index))
        .or_insert_with(|| {
            let point_id = PointId::mint(format!("iges:model:point#{stem}:D{list}:{}", index + 1))
                .expect("identity grammar");
            let vertex_id =
                VertexId::mint(format!("iges:model:vertex#{stem}:D{list}:{}", index + 1))
                    .expect("identity grammar");
            candidate.model_mut().points.push(Point {
                source_object: None,
                id: point_id.clone(),
                position: vertex_lists[&list][index],
            });
            candidate.model_mut().vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            vertex_id
        })
        .clone()
}

fn source_edge_for_vertices<'a>(
    ir: &'a CadIr,
    candidates: &[usize],
    curve_geometry: &CurveGeometry,
    natural_start: Point3,
    natural_end: Point3,
    tolerance: f64,
) -> Result<&'a Edge, SourceEdgeSelectionError> {
    let mut matching = None;
    for edge in candidates
        .iter()
        .filter_map(|position| ir.model.edges.get(*position))
    {
        let endpoints_agree = edge.param_range.is_some_and(|range| {
            evaluation::curve(curve_geometry, range[0])
                .is_some_and(|point| evaluation::distance(point, natural_start) <= tolerance)
                && evaluation::curve(curve_geometry, range[1])
                    .is_some_and(|point| evaluation::distance(point, natural_end) <= tolerance)
        });
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
    id_stem: &str,
) -> Vec<PcurveUse> {
    uses.iter()
        .zip(resolved)
        .enumerate()
        .map(|(index, ((isoparametric, _), (geometry, range)))| {
            let id = PcurveId::mint(format!("{id_stem}:{index}")).expect("identity grammar");
            candidate.model_mut().pcurves.push(Pcurve {
                id: id.clone(),
                geometry,
                metadata: cadmpeg_ir::geometry::PcurveMetadata::general(
                    None,
                    Some(range),
                    fit_tolerance,
                ),
            });
            PcurveUse {
                pcurve: id,
                isoparametric: Some(*isoparametric),
                parameter_range: None,
            }
        })
        .collect()
}

// Validation hands back the resolved pcurve geometry instead of a verdict:
// projection needs exactly what was just computed, from the same unmutated
// `ir` with the same arguments, so returning it keeps each pcurve resolved
// once instead of twice. The per-use interleave in the loop body is
// load-bearing — `pcurve_geometry` can fuse the decode budget, so a later
// use's geometry must not be resolved once an earlier use's evaluation has
// already failed.
#[allow(clippy::too_many_arguments)] // the lazily built model index rides along as the eighth argument
fn resolve_pcurve_uses<'a>(
    source: &'a CadIr,
    uses: &[(bool, u32)],
    support: &SurfaceSupport<'_>,
    expected_start: Point3,
    expected_end: Point3,
    tolerance: f64,
    ctx: Option<&DecodeContext<'_>>,
    model_index: &mut Option<cadmpeg_ir::index::ModelIndex<'a>>,
) -> Option<Vec<(PcurveGeometry, [f64; 2])>> {
    if uses.is_empty() {
        return Some(Vec::new());
    }
    let index = model_index.get_or_insert_with(|| cadmpeg_ir::index::ModelIndex::new(source));
    let mut resolved = Vec::with_capacity(uses.len());
    let mut mapped = Vec::with_capacity(uses.len());
    for (_, sequence) in uses {
        let (geometry, range) = pcurve_geometry(
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
        )?;
        let start = evaluation::pcurve(&geometry, range[0]).and_then(|uv| {
            cadmpeg_ir::eval::model_surface_point_by_id(index, support.id, uv.u, uv.v)
        })?;
        let end = evaluation::pcurve(&geometry, range[1]).and_then(|uv| {
            cadmpeg_ir::eval::model_surface_point_by_id(index, support.id, uv.u, uv.v)
        })?;
        resolved.push((geometry, range));
        mapped.push((start, end));
    }
    (evaluation::distance(mapped[0].0, expected_start) <= tolerance
        && evaluation::distance(mapped[mapped.len() - 1].1, expected_end) <= tolerance
        && mapped
            .windows(2)
            .all(|pair| evaluation::distance(pair[0].1, pair[1].0) <= tolerance))
    .then_some(resolved)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> ProjectionOutcome {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let factor = global.length_factor_mm();
    let tolerance = global.minimum_resolution_mm();
    let mut vertex_lists = BTreeMap::<u32, Vec<Point3>>::new();
    let mut edge_lists = BTreeMap::<u32, Vec<EdgeDefinition>>::new();
    let mut loops = BTreeMap::<u32, Vec<LoopUse>>::new();
    let mut faces = BTreeMap::<u32, FaceDefinition>::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 502 && entry.form == 1)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "vertex lists cannot carry a transformation",
            ));
            continue;
        }
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "vertex-list count is not positive"));
            continue;
        };
        let mut points = Vec::with_capacity(count);
        for index in 0..count {
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
            losses.push(entity_loss(
                entry,
                "vertex-list coordinates are truncated or non-finite",
            ));
            continue;
        }
        vertex_lists.insert(entry.sequence, points);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 504 && entry.form == 1)
    {
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "edge lists cannot carry a transformation",
            ));
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "edge-list count is not positive"));
            continue;
        };
        let mut edges = Vec::with_capacity(count);
        for item in 0..count {
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
            losses.push(entity_loss(
                entry,
                "edge-list tuple is invalid or names a missing vertex",
            ));
            continue;
        }
        edge_lists.insert(entry.sequence, edges);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 508 && entry.form == 1)
    {
        if entry.transform != 0 {
            losses.push(entity_loss(entry, "loops cannot carry a transformation"));
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "loop edge-use count is not positive"));
            continue;
        };
        let mut index = 2;
        let mut uses = Vec::with_capacity(count);
        for _ in 0..count {
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
            let mut pcurves = Vec::with_capacity(pcurve_count);
            for pcurve_index in 0..pcurve_count {
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
                if entries
                    .get(&sequence)
                    .is_none_or(|entry| entry.status.use_flag != 5)
                {
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
            losses.push(entity_loss(entry, "loop edge-use tuple is invalid"));
            continue;
        }
        loops.insert(entry.sequence, uses);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 510 && entry.form == 1)
    {
        if entry.transform != 0 {
            losses.push(entity_loss(entry, "faces cannot carry a transformation"));
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(surface) = pointer(record, 1) else {
            losses.push(entity_loss(entry, "face surface pointer is invalid"));
            continue;
        };
        let Some(count) = record.count(2).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "face loop count is not positive"));
            continue;
        };
        let has_outer_loop = match record.integer(3) {
            Some(1) => true,
            Some(0) => false,
            _ => {
                losses.push(entity_loss(entry, "face outer-loop flag is not logical"));
                continue;
            }
        };
        let Some(face_loops) = (0..count)
            .map(|index| pointer(record, 4 + index))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(entry, "face loop pointer is invalid"));
            continue;
        };
        if face_loops
            .iter()
            .any(|sequence| !loops.contains_key(sequence))
        {
            losses.push(entity_loss(entry, "face loop is missing"));
            continue;
        }
        faces.insert(
            entry.sequence,
            FaceDefinition {
                surface,
                loops: face_loops,
                has_outer_loop,
            },
        );
    }

    let mut shell_definitions = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 514 && matches!(entry.form, 1 | 2))
    {
        if entry.transform != 0 {
            losses.push(entity_loss(entry, "shells cannot carry a transformation"));
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "shell face count is not positive"));
            continue;
        };
        let mut face_uses = Vec::with_capacity(count);
        for index in 0..count {
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
            losses.push(entity_loss(entry, "shell face-use tuple is invalid"));
            continue;
        }
        shell_definitions.insert(
            entry.sequence,
            ShellDefinition {
                form: entry.form,
                faces: face_uses,
            },
        );
    }

    let mut body_definitions = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 514 && entry.form == 2)
    {
        if shell_definitions.contains_key(&entry.sequence) {
            body_definitions.push(BodyDefinition {
                entry,
                kind: BodyKind::Sheet,
                shells: vec![(entry.sequence, Sense::Forward)],
                closed: false,
                transform: None,
            });
        }
    }
    let mut referenced_closed_shells = BTreeSet::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 186 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(outer) = pointer(record, 1) else {
            losses.push(entity_loss(entry, "solid outer-shell pointer is invalid"));
            continue;
        };
        let outer_sense = match record.integer(2) {
            Some(1) => Sense::Forward,
            Some(0) => Sense::Reversed,
            _ => {
                losses.push(entity_loss(
                    entry,
                    "solid outer-shell orientation is not logical",
                ));
                continue;
            }
        };
        let Some(void_count) = record.count(3) else {
            losses.push(entity_loss(entry, "solid void-shell count is invalid"));
            continue;
        };
        let mut shell_uses = vec![(outer, outer_sense)];
        let mut valid = true;
        for index in 0..void_count {
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
            || shell_uses.iter().any(|(sequence, _)| {
                shell_definitions
                    .get(sequence)
                    .is_none_or(|shell| shell.form != 1)
            })
        {
            losses.push(entity_loss(
                entry,
                "solid shell-use tuple is invalid or not closed",
            ));
            continue;
        }
        referenced_closed_shells.extend(shell_uses.iter().map(|(sequence, _)| *sequence));
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            Ok(transform) => (entry.transform != 0).then(|| transform.body_transform()),
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        body_definitions.push(BodyDefinition {
            entry,
            kind: BodyKind::Solid,
            shells: shell_uses,
            closed: true,
            transform,
        });
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 514 && entry.form == 1)
    {
        if shell_definitions.contains_key(&entry.sequence)
            && !referenced_closed_shells.contains(&entry.sequence)
        {
            body_definitions.push(BodyDefinition {
                entry,
                kind: BodyKind::Sheet,
                shells: vec![(entry.sequence, Sense::Forward)],
                closed: true,
                transform: None,
            });
        }
    }

    // The surface and curve arenas never change while bodies project: the
    // only writer is the per-body draft commit, which appends, and a
    // topology draft carries no surface or curve. One first-occurrence
    // position map per arena therefore serves the whole call, replacing a
    // linear scan per face. The emptiness guard keeps files without
    // explicit B-rep from paying for either map.
    let mut surface_positions = BTreeMap::<String, usize>::new();
    let mut curve_positions = BTreeMap::<String, usize>::new();
    if !body_definitions.is_empty() {
        for (position, surface) in ir.model.surfaces.iter().enumerate() {
            surface_positions
                .entry(surface.id.0.clone())
                .or_insert(position);
        }
        for (position, curve) in ir.model.curves.iter().enumerate() {
            curve_positions
                .entry(curve.id.0.clone())
                .or_insert(position);
        }
    }

    // One session serves every body commit in this call. That is sound for
    // the same reason the position maps above are: the per-body commit is
    // the only writer of `ir` here, and it only appends. Each successful
    // commit moves its identities into the session, so a later body still
    // collides with an earlier body's ids and can resolve references into
    // them — exactly what a per-commit index rebuild provided. The session
    // is built at the first commit rather than up front so that files with
    // no explicit B-rep, and bodies rejected before reaching commit, never
    // pay for the identity index.
    let mut commit_session: Option<CommitSession> = None;
    for definition in body_definitions {
        let entry = definition.entry;
        let mut model_index = None;
        // The edge arena does grow — every committed body appends to it —
        // so this index lives for one body and is built on first use, where
        // it replaces a full-arena scan per distinct edge. Its `&str` keys
        // borrow from `ir`; the borrow must stay dead by the time the body
        // commits below, or the commit's `&mut ir` will not compile.
        let mut edges_by_curve: Option<BTreeMap<&str, Vec<usize>>> = None;
        let mut candidate = ModelDraft::new();
        let stem = format!("D{}", entry.sequence);
        let body_id = BodyId::mint(format!("iges:model:body#{stem}")).expect("identity grammar");
        let region_id =
            RegionId::mint(format!("iges:model:region#{stem}")).expect("identity grammar");
        let mut vertex_ids = BTreeMap::<(u32, usize), VertexId>::new();
        let mut edge_ids = BTreeMap::<(u32, usize), EdgeId>::new();
        let mut radial = BTreeMap::<(u32, u32, usize), Vec<CoedgeId>>::new();
        let mut region_shells = Vec::new();
        let mut consumed = BTreeSet::new();
        let mut valid = true;
        for (shell_sequence, shell_sense) in definition.shells.iter().copied() {
            let shell_definition = shell_definitions[&shell_sequence].clone();
            let shell_stem = if shell_sequence == entry.sequence && definition.shells.len() == 1 {
                stem.clone()
            } else {
                format!("{stem}:D{shell_sequence}")
            };
            let shell_id =
                ShellId::mint(format!("iges:model:shell#{shell_stem}")).expect("identity grammar");
            let mut shell_faces = Vec::new();
            for (face_sequence, native_face_sense) in shell_definition.faces {
                let face_sense = compose_sense(native_face_sense, shell_sense);
                let face_definition = faces[&face_sequence].clone();
                let surface_id =
                    SurfaceId::mint(format!("iges:model:surface#D{}", face_definition.surface))
                        .expect("identity grammar");
                let Some(support_geometry) = surface_positions
                    .get(surface_id.0.as_str())
                    .and_then(|position| ir.model.surfaces.get(*position))
                    .map(|surface| surface.geometry.clone())
                else {
                    valid = false;
                    break;
                };
                let face_id =
                    FaceId::mint(format!("iges:model:face#{shell_stem}:D{face_sequence}"))
                        .expect("identity grammar");
                let mut face_loops = Vec::new();
                for (face_loop_index, loop_sequence) in
                    face_definition.loops.into_iter().enumerate()
                {
                    let uses = loops[&loop_sequence].clone();
                    let loop_id =
                        LoopId::mint(format!("iges:model:loop#{shell_stem}:D{loop_sequence}"))
                            .expect("identity grammar");
                    let edge_use_indices = uses
                        .iter()
                        .enumerate()
                        .filter_map(|(index, use_)| {
                            matches!(use_, LoopUse::Edge { .. }).then_some(index)
                        })
                        .collect::<Vec<_>>();
                    let coedge_ids = edge_use_indices
                        .iter()
                        .map(|index| {
                            CoedgeId::mint(format!(
                                "iges:model:coedge#{shell_stem}:D{loop_sequence}:{index}"
                            ))
                            .expect("identity grammar")
                        })
                        .collect::<Vec<_>>();
                    let coedge_by_use = edge_use_indices
                        .iter()
                        .copied()
                        .zip(coedge_ids.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    let mut loop_vertex_uses = Vec::new();
                    for (use_index, use_) in uses.iter().enumerate() {
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
                            let vertex = topology_vertex(
                                &mut candidate,
                                &mut vertex_ids,
                                &vertex_lists,
                                &stem,
                                *vertex_list,
                                *vertex_index,
                            );
                            let after = if coedge_ids.is_empty() {
                                None
                            } else {
                                (1..=uses.len()).find_map(|distance| {
                                    let prior = (use_index + uses.len() - distance) % uses.len();
                                    coedge_by_use.get(&prior).cloned()
                                })
                            };
                            let expected = vertex_lists[vertex_list][*vertex_index];
                            let Some(resolved) = resolve_pcurve_uses(
                                ir,
                                pcurves,
                                &SurfaceSupport {
                                    id: &surface_id,
                                    geometry: &support_geometry,
                                    factor,
                                },
                                expected,
                                expected,
                                tolerance,
                                ctx,
                                &mut model_index,
                            ) else {
                                losses.push(entity_loss(
                                    entry,
                                    "loop vertex-use pcurves disagree with the pole vertex",
                                ));
                                valid = false;
                                break;
                            };
                            let projected = project_pcurve_uses(
                                &mut candidate,
                                pcurves,
                                resolved,
                                Some(tolerance),
                                &format!(
                                    "iges:model:pcurve#{shell_stem}:D{loop_sequence}:{use_index}"
                                ),
                            );
                            loop_vertex_uses.push((vertex, after, projected));
                            continue;
                        };
                        let edge_definition = edge_lists[edge_list][*edge_index];
                        for (list, index) in [
                            (edge_definition.start_list, edge_definition.start_index),
                            (edge_definition.end_list, edge_definition.end_index),
                        ] {
                            topology_vertex(
                                &mut candidate,
                                &mut vertex_ids,
                                &vertex_lists,
                                &stem,
                                list,
                                index,
                            );
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
                        let Some(resolved) = resolve_pcurve_uses(
                            ir,
                            pcurves,
                            &SurfaceSupport {
                                id: &surface_id,
                                geometry: &support_geometry,
                                factor,
                            },
                            expected_start,
                            expected_end,
                            tolerance,
                            ctx,
                            &mut model_index,
                        ) else {
                            losses.push(entity_loss(
                                entry,
                                "loop edge-use pcurves disagree with the edge vertices",
                            ));
                            valid = false;
                            break;
                        };
                        let edge_id = if let Some(id) = edge_ids.get(&edge_key) {
                            id.clone()
                        } else {
                            let curve_id = CurveId::mint(format!(
                                "iges:model:curve#D{}",
                                edge_definition.curve
                            ))
                            .expect("identity grammar");
                            let curve_edges = edges_by_curve.get_or_insert_with(|| {
                                let mut positions = BTreeMap::<&str, Vec<usize>>::new();
                                for (position, edge) in ir.model.edges.iter().enumerate() {
                                    if let Some(curve) = &edge.curve {
                                        positions
                                            .entry(curve.0.as_str())
                                            .or_default()
                                            .push(position);
                                    }
                                }
                                positions
                            });
                            let Some(candidates) = curve_edges.get(curve_id.0.as_str()) else {
                                valid = false;
                                break;
                            };
                            let Some(curve) = curve_positions
                                .get(curve_id.0.as_str())
                                .and_then(|position| ir.model.curves.get(*position))
                            else {
                                losses.push(entity_loss(
                                    entry,
                                    "edge curve endpoints disagree with the vertex-list points",
                                ));
                                valid = false;
                                break;
                            };
                            let source_edge = match source_edge_for_vertices(
                                ir,
                                candidates,
                                &curve.geometry,
                                natural_start,
                                natural_end,
                                tolerance,
                            ) {
                                Ok(source_edge) => source_edge,
                                Err(SourceEdgeSelectionError::NoMatch) => {
                                    losses.push(entity_loss(
                                        entry,
                                        "edge curve endpoints disagree with the vertex-list points",
                                    ));
                                    valid = false;
                                    break;
                                }
                                Err(SourceEdgeSelectionError::Ambiguous) => {
                                    losses.push(entity_loss(
                                        entry,
                                        "edge curve maps to multiple ambiguous edge occurrences",
                                    ));
                                    valid = false;
                                    break;
                                }
                            };
                            let id = EdgeId::mint(format!(
                                "iges:model:edge#{stem}:D{}:{}",
                                edge_key.0,
                                edge_key.1 + 1
                            ))
                            .expect("identity grammar");
                            candidate.model_mut().edges.push(Edge {
                                id: id.clone(),
                                curve: Some(curve_id),
                                start: vertex_ids
                                    [&(edge_definition.start_list, edge_definition.start_index)]
                                    .clone(),
                                end: vertex_ids
                                    [&(edge_definition.end_list, edge_definition.end_index)]
                                    .clone(),
                                param_range: source_edge.param_range,
                                tolerance: None,
                            });
                            edge_ids.insert(edge_key, id.clone());
                            id
                        };
                        let projected = project_pcurve_uses(
                            &mut candidate,
                            pcurves,
                            resolved,
                            Some(tolerance),
                            &format!("iges:model:pcurve#{shell_stem}:D{loop_sequence}:{use_index}"),
                        );
                        let Some(coedge_position) = edge_use_indices
                            .iter()
                            .position(|index| *index == use_index)
                        else {
                            valid = false;
                            break;
                        };
                        let coedge_id = coedge_ids[coedge_position].clone();
                        radial
                            .entry((shell_sequence, edge_key.0, edge_key.1))
                            .or_default()
                            .push(coedge_id.clone());
                        candidate.model_mut().coedges.push(Coedge {
                            id: coedge_id.clone(),
                            owner_loop: loop_id.clone(),
                            edge: edge_id,
                            radial_next: coedge_id.clone(),
                            sense: *sense,
                            pcurves: projected,
                            use_curve: None,
                        });
                    }
                    if !valid {
                        break;
                    }
                    let boundary = if coedge_ids.is_empty() {
                        let [(vertex, None, pcurves)] = loop_vertex_uses.as_slice() else {
                            losses.push(entity_loss(
                                entry,
                                "vertex-only loop does not contain exactly one unanchored vertex",
                            ));
                            valid = false;
                            break;
                        };
                        LoopBoundary::Vertex {
                            vertex: vertex.clone(),
                            pcurves: pcurves.clone(),
                        }
                    } else {
                        let Some(vertex_uses) = loop_vertex_uses
                            .into_iter()
                            .map(|(vertex, after, pcurves)| {
                                let after = after?;
                                coedge_ids.contains(&after).then_some(AnchoredVertexUse {
                                    vertex,
                                    after,
                                    pcurves,
                                })
                            })
                            .collect::<Option<Vec<_>>>()
                        else {
                            losses.push(entity_loss(
                                entry,
                                "edge loop contains an unanchored vertex use",
                            ));
                            valid = false;
                            break;
                        };
                        LoopBoundary::Ring {
                            coedges: coedge_ids,
                            vertex_uses,
                        }
                    };
                    candidate.model_mut().loops.push(Loop {
                        id: loop_id.clone(),
                        face: face_id.clone(),
                        boundary_role: if face_definition.has_outer_loop && face_loop_index == 0 {
                            cadmpeg_ir::topology::LoopBoundaryRole::Outer
                        } else if face_definition.has_outer_loop {
                            cadmpeg_ir::topology::LoopBoundaryRole::Inner
                        } else {
                            cadmpeg_ir::topology::LoopBoundaryRole::Unspecified
                        },
                        boundary,
                    });
                    face_loops.push(loop_id);
                    consumed.insert(loop_sequence);
                }
                if !valid {
                    break;
                }
                candidate.model_mut().faces.push(Face {
                    id: face_id.clone(),
                    shell: shell_id.clone(),
                    surface: surface_id,
                    sense: face_sense,
                    loops: face_loops,
                    name: None,
                    color: None,
                    tolerance: None,
                });
                shell_faces.push(face_id);
                consumed.insert(face_sequence);
            }
            if !valid {
                break;
            }
            candidate.model_mut().shells.push(Shell {
                id: shell_id.clone(),
                region: region_id.clone(),
                faces: shell_faces,
                wire_edges: Vec::new(),
                free_vertices: Vec::new(),
            });
            region_shells.push(shell_id);
            consumed.insert(shell_sequence);
        }
        if !valid {
            losses.push(entity_loss(
                entry,
                "shell topology references missing geometry",
            ));
            continue;
        }
        if definition.closed
            && radial.values().any(|ring| {
                if ring.len() != 2 {
                    return true;
                }
                let senses = ring
                    .iter()
                    .filter_map(|id| {
                        candidate
                            .model()
                            .coedges
                            .iter()
                            .find(|coedge| coedge.id == *id)
                            .map(|coedge| coedge.sense)
                    })
                    .collect::<Vec<_>>();
                senses.len() != 2 || senses[0] == senses[1]
            })
        {
            losses.push(entity_loss(
                entry,
                "closed shell does not use every edge exactly twice with opposite senses",
            ));
            continue;
        }
        for ring in radial.values() {
            for (index, id) in ring.iter().enumerate() {
                if let Some(coedge) = candidate
                    .model_mut()
                    .coedges
                    .iter_mut()
                    .find(|coedge| coedge.id == *id)
                {
                    coedge.radial_next = ring[(index + 1) % ring.len()].clone();
                }
            }
        }
        candidate.model_mut().regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: region_shells,
        });
        candidate.model_mut().bodies.push(Body {
            id: body_id,
            kind: definition.kind,
            regions: vec![region_id],
            transform: definition.transform,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model_mut().finalize();
        let session = commit_session.get_or_insert_with(|| CommitSession::new(ir));
        if session.commit_model(candidate, ir).is_err() {
            losses.push(entity_loss(
                entry,
                "shell candidate failed neutral validation",
            ));
            continue;
        }
        decoded.insert(entry.sequence);
        decoded.extend(consumed);
        decoded.extend(edge_ids.keys().map(|key| key.0));
        decoded.extend(vertex_ids.keys().map(|key| key.0));
    }

    ProjectionOutcome { decoded, losses }
}

#[cfg(test)]
mod tests;
