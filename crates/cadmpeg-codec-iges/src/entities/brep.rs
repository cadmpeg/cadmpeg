// SPDX-License-Identifier: Apache-2.0
//! Explicit IGES B-rep topology projection.

use super::evaluation;
use super::geometry::{entity_loss, resolve_transform};
use super::trimming::pcurve_geometry;
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::Pcurve;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Point, Region, Sense, Shell, Vertex,
    VertexUse,
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
    candidate: &mut CadIr,
    vertex_ids: &mut BTreeMap<(u32, usize), VertexId>,
    vertex_lists: &BTreeMap<u32, Vec<Point3>>,
    stem: &str,
    list: u32,
    index: usize,
) -> VertexId {
    vertex_ids
        .entry((list, index))
        .or_insert_with(|| {
            let point_id = PointId(format!("iges:model:point#{stem}:D{list}:{}", index + 1));
            let vertex_id = VertexId(format!("iges:model:vertex#{stem}:D{list}:{}", index + 1));
            candidate.model.points.push(Point {
                source_object: None,
                id: point_id.clone(),
                position: vertex_lists[&list][index],
            });
            candidate.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            vertex_id
        })
        .clone()
}

fn project_pcurve_uses(
    candidate: &mut CadIr,
    source: &CadIr,
    uses: &[(bool, u32)],
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    factor: f64,
    id_stem: &str,
) -> Option<Vec<PcurveUse>> {
    uses.iter()
        .enumerate()
        .map(|(index, (isoparametric, sequence))| {
            let (geometry, range) = pcurve_geometry(source, *sequence, surface, factor)?;
            let id = PcurveId(format!("{id_stem}:{index}"));
            candidate.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some(range),
                fit_tolerance: None,
            });
            Some(PcurveUse {
                pcurve: id,
                isoparametric: Some(*isoparametric),
                parameter_range: None,
            })
        })
        .collect()
}

fn pcurves_agree(
    source: &CadIr,
    uses: &[(bool, u32)],
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    factor: f64,
    expected_start: Point3,
    expected_end: Point3,
    tolerance: f64,
) -> bool {
    if uses.is_empty() {
        return true;
    }
    let mapped = uses
        .iter()
        .map(|(_, sequence)| {
            let (geometry, range) = pcurve_geometry(source, *sequence, surface, factor)?;
            let start = evaluation::pcurve(&geometry, range[0])
                .and_then(|uv| evaluation::surface(surface, uv))?;
            let end = evaluation::pcurve(&geometry, range[1])
                .and_then(|uv| evaluation::surface(surface, uv))?;
            Some((start, end))
        })
        .collect::<Option<Vec<_>>>();
    let Some(mapped) = mapped else {
        return false;
    };
    evaluation::distance(mapped[0].0, expected_start) <= tolerance
        && evaluation::distance(mapped[mapped.len() - 1].1, expected_end) <= tolerance
        && mapped
            .windows(2)
            .all(|pair| evaluation::distance(pair[0].1, pair[1].0) <= tolerance)
}

pub(super) struct BrepProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> BrepProjection {
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
    let Some(factor) = global.length_factor_mm() else {
        return BrepProjection {
            handled,
            decoded,
            losses,
        };
    };
    let mut vertex_lists = BTreeMap::<u32, Vec<Point3>>::new();
    let mut edge_lists = BTreeMap::<u32, Vec<EdgeDefinition>>::new();
    let mut loops = BTreeMap::<u32, Vec<LoopUse>>::new();
    let mut faces = BTreeMap::<u32, FaceDefinition>::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 502 && entry.form == 1)
    {
        handled.insert(entry.sequence);
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
        handled.insert(entry.sequence);
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
        handled.insert(entry.sequence);
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
        handled.insert(entry.sequence);
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
        handled.insert(entry.sequence);
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
        handled.insert(entry.sequence);
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
            &mut BTreeSet::new(),
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

    for definition in body_definitions {
        let entry = definition.entry;
        let mut candidate = ir.clone();
        let stem = format!("D{}", entry.sequence);
        let body_id = BodyId(format!("iges:model:body#{stem}"));
        let region_id = RegionId(format!("iges:model:region#{stem}"));
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
            let shell_id = ShellId(format!("iges:model:shell#{shell_stem}"));
            let mut shell_faces = Vec::new();
            for (face_sequence, native_face_sense) in shell_definition.faces {
                let face_sense = compose_sense(native_face_sense, shell_sense);
                let face_definition = faces[&face_sequence].clone();
                let surface_id =
                    SurfaceId(format!("iges:model:surface#D{}", face_definition.surface));
                let Some(support) = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == surface_id)
                else {
                    valid = false;
                    break;
                };
                let face_id = FaceId(format!("iges:model:face#{shell_stem}:D{face_sequence}"));
                let mut face_loops = Vec::new();
                for (face_loop_index, loop_sequence) in
                    face_definition.loops.into_iter().enumerate()
                {
                    let uses = loops[&loop_sequence].clone();
                    let loop_id = LoopId(format!("iges:model:loop#{shell_stem}:D{loop_sequence}"));
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
                            CoedgeId(format!(
                                "iges:model:coedge#{shell_stem}:D{loop_sequence}:{index}"
                            ))
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
                            if !global.minimum_resolution_mm().is_some_and(|tolerance| {
                                pcurves_agree(
                                    ir,
                                    pcurves,
                                    &support.geometry,
                                    factor,
                                    expected,
                                    expected,
                                    tolerance,
                                )
                            }) {
                                losses.push(entity_loss(
                                    entry,
                                    "loop vertex-use pcurves disagree with the pole vertex",
                                ));
                                valid = false;
                                break;
                            }
                            let Some(projected) = project_pcurve_uses(
                                &mut candidate,
                                ir,
                                pcurves,
                                &support.geometry,
                                factor,
                                &format!(
                                    "iges:model:pcurve#{shell_stem}:D{loop_sequence}:{use_index}"
                                ),
                            ) else {
                                valid = false;
                                break;
                            };
                            loop_vertex_uses.push(VertexUse {
                                vertex,
                                after,
                                pcurves: projected,
                            });
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
                        if !global.minimum_resolution_mm().is_some_and(|tolerance| {
                            pcurves_agree(
                                ir,
                                pcurves,
                                &support.geometry,
                                factor,
                                expected_start,
                                expected_end,
                                tolerance,
                            )
                        }) {
                            losses.push(entity_loss(
                                entry,
                                "loop edge-use pcurves disagree with the edge vertices",
                            ));
                            valid = false;
                            break;
                        }
                        let edge_id = if let Some(id) = edge_ids.get(&edge_key) {
                            id.clone()
                        } else {
                            let curve_id =
                                CurveId(format!("iges:model:curve#D{}", edge_definition.curve));
                            let Some(source_edge) = ir
                                .model
                                .edges
                                .iter()
                                .find(|edge| edge.curve.as_ref() == Some(&curve_id))
                            else {
                                valid = false;
                                break;
                            };
                            let curve_agrees = source_edge.param_range.is_some_and(|range| {
                                ir.model
                                    .curves
                                    .iter()
                                    .find(|curve| curve.id == curve_id)
                                    .is_some_and(|curve| {
                                        let evaluated_start =
                                            evaluation::curve(&curve.geometry, range[0]);
                                        let evaluated_end =
                                            evaluation::curve(&curve.geometry, range[1]);
                                        global.minimum_resolution_mm().is_some_and(|tolerance| {
                                            evaluated_start.is_some_and(|point| {
                                                evaluation::distance(point, natural_start)
                                                    <= tolerance
                                            }) && evaluated_end.is_some_and(|point| {
                                                evaluation::distance(point, natural_end)
                                                    <= tolerance
                                            })
                                        })
                                    })
                            });
                            if !curve_agrees {
                                losses.push(entity_loss(
                                    entry,
                                    "edge curve endpoints disagree with the vertex-list points",
                                ));
                                valid = false;
                                break;
                            }
                            let id = EdgeId(format!(
                                "iges:model:edge#{stem}:D{}:{}",
                                edge_key.0,
                                edge_key.1 + 1
                            ));
                            candidate.model.edges.push(Edge {
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
                        let Some(projected) = project_pcurve_uses(
                            &mut candidate,
                            ir,
                            pcurves,
                            &support.geometry,
                            factor,
                            &format!("iges:model:pcurve#{shell_stem}:D{loop_sequence}:{use_index}"),
                        ) else {
                            valid = false;
                            break;
                        };
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
                        candidate.model.coedges.push(Coedge {
                            id: coedge_id.clone(),
                            owner_loop: loop_id.clone(),
                            edge: edge_id,
                            next: coedge_ids[(coedge_position + 1) % coedge_ids.len()].clone(),
                            previous: coedge_ids
                                [(coedge_position + coedge_ids.len() - 1) % coedge_ids.len()]
                            .clone(),
                            radial_next: coedge_id.clone(),
                            sense: *sense,
                            pcurves: projected,
                            use_curve: None,
                            use_curve_parameter_range: None,
                        });
                    }
                    if !valid {
                        break;
                    }
                    candidate.model.loops.push(Loop {
                        id: loop_id.clone(),
                        face: face_id.clone(),
                        boundary_role: if face_definition.has_outer_loop && face_loop_index == 0 {
                            cadmpeg_ir::topology::LoopBoundaryRole::Outer
                        } else {
                            cadmpeg_ir::topology::LoopBoundaryRole::Inner
                        },
                        coedges: coedge_ids,
                        vertex_uses: loop_vertex_uses,
                    });
                    face_loops.push(loop_id);
                    consumed.insert(loop_sequence);
                }
                if !valid {
                    break;
                }
                candidate.model.faces.push(Face {
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
            candidate.model.shells.push(Shell {
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
                            .model
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
                    .model
                    .coedges
                    .iter_mut()
                    .find(|coedge| coedge.id == *id)
                {
                    coedge.radial_next = ring[(index + 1) % ring.len()].clone();
                }
            }
        }
        candidate.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: region_shells,
        });
        candidate.model.bodies.push(Body {
            id: body_id,
            kind: definition.kind,
            regions: vec![region_id],
            transform: definition.transform,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model.finalize();
        if !cadmpeg_ir::validate(&candidate, Vec::new()).is_ok() {
            losses.push(entity_loss(
                entry,
                "shell candidate failed neutral validation",
            ));
            continue;
        }
        *ir = candidate;
        decoded.insert(entry.sequence);
        decoded.extend(consumed);
        decoded.extend(edge_ids.keys().map(|key| key.0));
        decoded.extend(vertex_ids.keys().map(|key| key.0));
    }

    BrepProjection {
        handled,
        decoded,
        losses,
    }
}
