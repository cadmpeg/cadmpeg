// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;

use crate::container::ContainerScan;

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

pub fn lanes(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureInputLane> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let section = block.section.as_deref()?;
            if !section.to_ascii_lowercase().contains("resolvedfeatures") {
                return None;
            }
            let parent = format!("sldprt:feature-input:resolved-features#{}", block.offset);
            let mut sketch_entities = block
                .payload
                .windows(SKETCH_MARKER.len())
                .enumerate()
                .filter_map(|(offset, bytes)| (bytes == SKETCH_MARKER).then_some(offset))
                .filter_map(|offset| {
                    let code = u32::from_le_bytes(
                        block
                            .payload
                            .get(offset + 17..offset + 21)?
                            .try_into()
                            .ok()?,
                    );
                    Some((offset, code))
                })
                .enumerate()
                .map(|(ordinal, (offset, code))| {
                    let id = format!(
                        "sldprt:feature-input:sketch-entity#{}:{offset}",
                        block.offset
                    );
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        section,
                        offset as u64,
                        "ff_ff_1f_00_03",
                        Exactness::ByteExact,
                    );
                    SketchInputEntity {
                        id,
                        parent: parent.clone(),
                        ordinal: ordinal as u32,
                        offset: offset as u64,
                        kind: SketchInputKind::from_native_code(code),
                    }
                })
                .collect::<Vec<_>>();
            sketch_entities.sort_by(|a, b| a.id.cmp(&b.id));
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                section,
                0,
                "ResolvedFeatures",
                Exactness::ByteExact,
            );
            Some(FeatureInputLane {
                id,
                configuration: configuration(section),
                native_payload: block.payload.clone(),
                sketch_entities,
            })
        })
        .collect()
}

fn configuration(section: &str) -> Option<String> {
    let start = section.find("Config-")? + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    (!tail[..end].is_empty()).then(|| tail[..end].to_string())
}

/// Decode nested feature-input Parasolid streams as placed planar sketches.
pub fn sketches(
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> (Vec<Sketch>, Vec<SketchEntity>) {
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    for block in &scan.blocks {
        let Some(section) = block.section.as_deref() else {
            continue;
        };
        if !section.to_ascii_lowercase().contains("resolvedfeatures") {
            continue;
        }
        let native_ref = format!("sldprt:feature-input:resolved-features#{}", block.offset);
        for (stream_ordinal, payload) in block.ps_streams.iter().enumerate() {
            let Some(header) = crate::parasolid::stream_header(payload) else {
                continue;
            };
            let brep = crate::brep::decode(payload, &header, section);
            project_brep(
                &brep,
                block.offset,
                stream_ordinal,
                section,
                &header.description,
                configuration(section).as_deref(),
                &native_ref,
                annotations,
                &mut sketches,
                &mut entities,
            );
        }
    }
    (sketches, entities)
}

#[allow(clippy::too_many_arguments)]
fn project_brep(
    brep: &crate::brep::Brep,
    block_offset: usize,
    stream_ordinal: usize,
    section: &str,
    sketch_name: &str,
    configuration: Option<&str>,
    native_ref: &str,
    annotations: &mut Annotations,
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
) {
    let surfaces = brep
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<HashMap<_, _>>();
    let loops = brep
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = brep
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = brep
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = brep
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, &vertex.point))
        .collect::<HashMap<_, _>>();
    let points = brep
        .points
        .iter()
        .map(|point| (&point.id, point.position))
        .collect::<HashMap<_, _>>();
    let curves = brep
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();

    for (face_ordinal, face) in brep.faces.iter().enumerate() {
        let Some(SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }) = surfaces.get(&face.surface).copied()
        else {
            continue;
        };
        let sketch_id = SketchId(format!(
            "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
        ));
        let v_axis = cross(*normal, *u_axis);
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, SketchEntityId>::new();
        let mut profiles = Vec::new();
        for loop_id in &face.loops {
            let Some(loop_) = loops.get(loop_id) else {
                continue;
            };
            let mut profile = Vec::new();
            for coedge_id in &loop_.coedges {
                let Some(coedge) = coedges.get(coedge_id) else {
                    continue;
                };
                let Some(edge) = edges.get(&coedge.edge) else {
                    continue;
                };
                let entity_id = if let Some(id) = edge_entities.get(&edge.id) {
                    id.clone()
                } else {
                    let id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                        edge_entities.len()
                    ));
                    let Some(geometry) =
                        project_edge(edge, &vertices, &points, &curves, *origin, *u_axis, v_axis)
                    else {
                        continue;
                    };
                    let Some(start_point) = vertices.get(&edge.start) else {
                        continue;
                    };
                    let Some(end_point) = vertices.get(&edge.end) else {
                        continue;
                    };
                    crate::annotations::note(
                        annotations,
                        id.0.clone(),
                        section,
                        0,
                        "feature_input_profile_edge",
                        Exactness::Derived,
                    );
                    entities.push(SketchEntity {
                        id: id.clone(),
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: Some(format!("{stream_ordinal}:{}", edge.id.0)),
                        geometry_ref: edge
                            .curve
                            .as_ref()
                            .map(|id| format!("{stream_ordinal}:{}", id.0)),
                        endpoint_refs: vec![
                            format!("{stream_ordinal}:{}", start_point.0),
                            format!("{stream_ordinal}:{}", end_point.0),
                        ],
                        geometry,
                    });
                    edge_entities.insert(&edge.id, id.clone());
                    id
                };
                profile.push(SketchEntityUse {
                    entity: entity_id,
                    reversed: coedge.sense == Sense::Reversed,
                });
            }
            if !profile.is_empty() {
                profiles.push(profile);
            }
        }
        if profiles.is_empty() {
            continue;
        }
        crate::annotations::note(
            annotations,
            sketch_id.0.clone(),
            section,
            0,
            "feature_input_profile",
            Exactness::Derived,
        );
        sketches.push(Sketch {
            id: sketch_id,
            name: (!sketch_name.is_empty()).then(|| sketch_name.to_string()),
            configuration: configuration.map(str::to_string),
            origin: *origin,
            normal: *normal,
            u_axis: *u_axis,
            profiles,
            native_ref: Some(native_ref.to_string()),
        });
    }
}

fn project_edge(
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> Option<SketchGeometry> {
    let start = project_point(
        *points.get(vertices.get(&edge.start)?)?,
        origin,
        u_axis,
        v_axis,
    );
    let end = project_point(
        *points.get(vertices.get(&edge.end)?)?,
        origin,
        u_axis,
        v_axis,
    );
    match edge.curve.as_ref().and_then(|id| curves.get(id).copied()) {
        Some(CurveGeometry::Circle { center, radius, .. }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            if (start.u - end.u).hypot(start.v - end.v) <= 1.0e-9 {
                Some(SketchGeometry::Circle {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                })
            } else {
                Some(SketchGeometry::Arc {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                    start_angle: cadmpeg_ir::features::Angle(
                        (start.v - center.v).atan2(start.u - center.u),
                    ),
                    end_angle: cadmpeg_ir::features::Angle(
                        (end.v - center.v).atan2(end.u - center.u),
                    ),
                })
            }
        }
        Some(CurveGeometry::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            let major_u = dot(*major_direction, u_axis);
            let major_v = dot(*major_direction, v_axis);
            let major_angle = major_v.atan2(major_u);
            let full = (start.u - end.u).hypot(start.v - end.v) <= 1.0e-9;
            let parameter = |point: Point2| {
                let du = point.u - center.u;
                let dv = point.v - center.v;
                let major_component = du * major_angle.cos() + dv * major_angle.sin();
                let minor_component = -du * major_angle.sin() + dv * major_angle.cos();
                (minor_component / *minor_radius).atan2(major_component / *major_radius)
            };
            Some(SketchGeometry::Ellipse {
                center,
                major_angle: cadmpeg_ir::features::Angle(major_angle),
                major_radius: cadmpeg_ir::features::Length(*major_radius),
                minor_radius: cadmpeg_ir::features::Length(*minor_radius),
                start_angle: (!full).then(|| cadmpeg_ir::features::Angle(parameter(start))),
                end_angle: (!full).then(|| cadmpeg_ir::features::Angle(parameter(end))),
            })
        }
        Some(CurveGeometry::Nurbs(nurbs)) => Some(SketchGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots.clone(),
            control_points: nurbs
                .control_points
                .iter()
                .map(|point| project_point(*point, origin, u_axis, v_axis))
                .collect(),
            weights: nurbs.weights.clone(),
            periodic: nurbs.periodic,
        }),
        Some(CurveGeometry::Line { .. }) | None => Some(SketchGeometry::Line { start, end }),
        Some(other) => Some(SketchGeometry::Native {
            native_kind: format!("{other:?}"),
        }),
    }
}

fn project_point(point: Point3, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point2 {
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    Point2::new(dot(delta, u_axis), dot(delta, v_axis))
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

/// Stable hash of neutral sketch records.
pub fn sketch_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&(
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.sketch_constraints,
    ))
}

/// Stable hash of retained native feature-input lanes.
pub fn lane_hash(native: &crate::native::SldprtNative) -> String {
    hash_debug(&native.feature_input_lanes)
}

fn hash_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    let bytes = format!("{value:?}");
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes.as_bytes()) {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

/// Reject unsupported neutral sketch edits before native lane replay.
pub fn prepare_sketches_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_sketch_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_sketch_sha256"));
    let current_neutral = sketch_hash(ir);
    let current_native = native.as_ref().map(lane_hash);
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.sketches.is_empty()
            && ir.model.sketch_entities.is_empty()
            && ir.model.sketch_constraints.is_empty()
        {
            return Ok(());
        }
        let generated = source_less_lanes(ir)?;
        native
            .get_or_insert_with(crate::native::SldprtNative::default)
            .feature_input_lanes
            .extend(generated);
        return Ok(());
    }
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &current_neutral);
    if !neutral_changed {
        return Ok(());
    }
    let native_changed = match (&current_native, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if native_changed {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "conflicting neutral and native SLDPRT sketch edits".into(),
        ));
    }
    patch_line_profiles(
        ir,
        native.as_mut().ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back requires retained feature-input lanes".into(),
            )
        })?,
    )
}

fn source_less_lanes(
    ir: &cadmpeg_ir::CadIr,
) -> Result<Vec<FeatureInputLane>, cadmpeg_ir::codec::CodecError> {
    let mut lanes = Vec::<FeatureInputLane>::new();
    for sketch in &ir.model.sketches {
        let configuration = sketch.configuration.clone().unwrap_or_else(|| "0".into());
        let section = format!("Contents/Config-{configuration}-ResolvedFeatures");
        let lane = if let Some(lane) = lanes
            .iter_mut()
            .find(|lane| lane.configuration.as_deref() == Some(configuration.as_str()))
        {
            lane
        } else {
            lanes.push(FeatureInputLane {
                id: section,
                configuration: Some(configuration.clone()),
                native_payload: Vec::new(),
                sketch_entities: Vec::new(),
            });
            lanes.last_mut().expect("lane was inserted")
        };
        let sketch_ir = sketch_brep(ir, sketch)?;
        let body = crate::writer::brep_body(&sketch_ir, 0.001, false)?;
        lane.native_payload
            .extend(crate::writer::parasolid_stream(&body, "SCH_SW_33103_11000"));
    }
    Ok(lanes)
}

fn sketch_brep(
    source: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
) -> Result<cadmpeg_ir::CadIr, cadmpeg_ir::codec::CodecError> {
    let mut ir = cadmpeg_ir::CadIr::empty(source.units.clone());
    let prefix = format!("generated:sldprt:sketch:{}", sketch.id.0);
    let body_id = BodyId(format!("{prefix}:body"));
    let region_id = RegionId(format!("{prefix}:region"));
    let shell_id = ShellId(format!("{prefix}:shell"));
    let face_id = FaceId(format!("{prefix}:face"));
    let surface_id = SurfaceId(format!("{prefix}:surface"));
    let v_axis = cross(sketch.normal, sketch.u_axis);
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: sketch.origin,
            normal: sketch.normal,
            u_axis: sketch.u_axis,
        },
        source_object: None,
    });
    let entities = source
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let mut face_loops = Vec::new();
    let mut vertex_by_position = HashMap::<(u64, u64), VertexId>::new();
    for (profile_index, profile) in sketch.profiles.iter().enumerate() {
        if profile.is_empty() {
            continue;
        }
        let loop_id = LoopId(format!("{prefix}:loop:{profile_index}"));
        face_loops.push(loop_id.clone());
        let mut coedge_ids = Vec::new();
        for (use_index, entity_use) in profile.iter().enumerate() {
            let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch {} references missing entity {}",
                    sketch.id.0, entity_use.entity.0
                ))
            })?;
            let SketchGeometry::Line { start, end } = entity.geometry else {
                return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT sketch {} contains non-line entity {}",
                    sketch.id.0, entity.id.0
                )));
            };
            let start_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                start,
                sketch,
                v_axis,
            );
            let end_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                end,
                sketch,
                v_axis,
            );
            let start_3d = lift_point(start, sketch.origin, sketch.u_axis, v_axis);
            let end_3d = lift_point(end, sketch.origin, sketch.u_axis, v_axis);
            let delta = Vector3::new(
                end_3d.x - start_3d.x,
                end_3d.y - start_3d.y,
                end_3d.z - start_3d.z,
            );
            let length = (dot(delta, delta)).sqrt();
            if length == 0.0 {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch entity {} has zero length",
                    entity.id.0
                )));
            }
            let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{use_index}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{use_index}"));
            let coedge_id = CoedgeId(format!("{prefix}:coedge:{profile_index}:{use_index}"));
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Line {
                    origin: start_3d,
                    direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
                },
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: start_vertex,
                end: end_vertex,
                param_range: Some([0.0, length]),
                tolerance: None,
            });
            coedge_ids.push(coedge_id.clone());
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: if entity_use.reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurve: None,
            });
        }
        let count = coedge_ids.len();
        for (index, coedge) in ir
            .model
            .coedges
            .iter_mut()
            .rev()
            .take(count)
            .rev()
            .enumerate()
        {
            coedge.next = coedge_ids[(index + 1) % count].clone();
            coedge.previous = coedge_ids[(index + count - 1) % count].clone();
        }
        ir.model.loops.push(Loop {
            id: loop_id,
            face: face_id.clone(),
            coedges: coedge_ids,
        });
    }
    if face_loops.is_empty() {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} has no profiles",
            sketch.id.0
        )));
    }
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: surface_id,
        sense: Sense::Forward,
        loops: face_loops,
        name: sketch.name.clone(),
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: vec![face_id],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: vec![region_id],
        transform: None,
        name: sketch.name.clone(),
        color: None,
        visible: None,
    });
    ir.model.finalize();
    Ok(ir)
}

fn sketch_vertex(
    ir: &mut cadmpeg_ir::CadIr,
    vertices: &mut HashMap<(u64, u64), VertexId>,
    prefix: &str,
    position: Point2,
    sketch: &Sketch,
    v_axis: Vector3,
) -> VertexId {
    let key = (position.u.to_bits(), position.v.to_bits());
    if let Some(id) = vertices.get(&key) {
        return id.clone();
    }
    let ordinal = vertices.len();
    let point_id = PointId(format!("{prefix}:point:{ordinal}"));
    let vertex_id = VertexId(format!("{prefix}:vertex:{ordinal}"));
    ir.model.points.push(Point {
        id: point_id.clone(),
        position: lift_point(position, sketch.origin, sketch.u_axis, v_axis),
    });
    ir.model.vertices.push(Vertex {
        id: vertex_id.clone(),
        point: point_id,
        tolerance: None,
    });
    vertices.insert(key, vertex_id.clone());
    vertex_id
}

fn patch_line_profiles(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let mut requested = HashMap::<(String, usize, u16), Point3>::new();
    let mut curves = Vec::new();
    for sketch in &ir.model.sketches {
        let lane_id = sketch.native_ref.as_ref().ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back requires native sketch provenance".into(),
            )
        })?;
        let v_axis = cross(sketch.normal, sketch.u_axis);
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            if entity.endpoint_refs.len() != 2 {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch entity {} lacks two endpoint references",
                    entity.id.0
                )));
            }
            match &entity.geometry {
                SketchGeometry::Line { start, end } => {
                    for (reference, point) in entity.endpoint_refs.iter().zip([start, end]) {
                        let (stream, attr) = parse_point_ref(reference)?;
                        let point = lift_point(*point, sketch.origin, sketch.u_axis, v_axis);
                        let key = (lane_id.clone(), stream, attr);
                        if let Some(previous) = requested.insert(key, point) {
                            if distance(previous, point) > 1.0e-9 {
                                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                    "SLDPRT shared sketch point {reference} has conflicting positions"
                                )));
                            }
                        }
                    }
                }
                geometry @ (SketchGeometry::Circle { .. }
                | SketchGeometry::Arc { .. }
                | SketchGeometry::Ellipse { .. }
                | SketchGeometry::Nurbs { .. }) => {
                    let geometry_ref = entity.geometry_ref.as_deref().ok_or_else(|| {
                        cadmpeg_ir::codec::CodecError::Malformed(
                            "SLDPRT sketch curve lacks native carrier provenance".into(),
                        )
                    })?;
                    let (stream, carrier_attr) = parse_point_ref(geometry_ref)?;
                    let (_, start_attr) = parse_point_ref(&entity.endpoint_refs[0])?;
                    let (_, end_attr) = parse_point_ref(&entity.endpoint_refs[1])?;
                    if let Some(endpoints) = bounded_endpoints(geometry) {
                        for (reference, point) in entity.endpoint_refs.iter().zip(endpoints) {
                            let (point_stream, attr) = parse_point_ref(reference)?;
                            let point = lift_point(point, sketch.origin, sketch.u_axis, v_axis);
                            let key = (lane_id.clone(), point_stream, attr);
                            if let Some(previous) = requested.insert(key, point) {
                                if distance(previous, point) > 1.0e-9 {
                                    return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                        "SLDPRT shared sketch point {reference} has conflicting positions"
                                    )));
                                }
                            }
                        }
                    }
                    curves.push(CurvePatch {
                        lane_id: lane_id.clone(),
                        stream,
                        carrier_attr,
                        start_attr,
                        end_attr,
                        geometry: geometry.clone(),
                        origin: sketch.origin,
                        u_axis: sketch.u_axis,
                        v_axis,
                    });
                }
                _ => {
                    return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                        "SLDPRT sketch write-back does not support this curve family".into(),
                    ))
                }
            }
        }
    }
    for ((lane_id, stream_ordinal, attr), point) in requested {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == lane_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch lane {lane_id} is missing"
                ))
            })?;
        patch_direct_stream_point(&mut lane.native_payload, stream_ordinal, attr, point)?;
    }
    for request in curves {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == request.lane_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch lane {} is missing",
                    request.lane_id
                ))
            })?;
        patch_direct_curve(&mut lane.native_payload, &request)?;
    }
    Ok(())
}

fn bounded_endpoints(geometry: &SketchGeometry) -> Option<[Point2; 2]> {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            offset_point(*center, polar(radius.0, start_angle.0)),
            offset_point(*center, polar(radius.0, end_angle.0)),
        ]),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start),
            end_angle: Some(end),
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            Some([point(start.0), point(end.0)])
        }
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } if control_points.len() >= 2 => {
            Some([control_points[0], control_points[control_points.len() - 1]])
        }
        _ => None,
    }
}

struct CurvePatch {
    lane_id: String,
    stream: usize,
    carrier_attr: u16,
    start_attr: u16,
    end_attr: u16,
    geometry: SketchGeometry,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
}

fn parse_point_ref(reference: &str) -> Result<(usize, u16), cadmpeg_ir::codec::CodecError> {
    let (stream, id) = reference.split_once(':').ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(format!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))
    })?;
    let attr = id.rsplit('#').next().and_then(|value| value.parse().ok());
    match (stream.parse().ok(), attr) {
        (Some(stream), Some(attr)) => Ok((stream, attr)),
        _ => Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))),
    }
}

fn lift_point(point: Point2, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point3 {
    Point3::new(
        origin.x + point.u * u_axis.x + point.v * v_axis.x,
        origin.y + point.u * u_axis.y + point.v * v_axis.y,
        origin.z + point.u * u_axis.z + point.v * v_axis.z,
    )
}

fn distance(left: Point3, right: Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

fn patch_direct_stream_point(
    payload: &mut [u8],
    stream_ordinal: usize,
    attr: u16,
    point_mm: Point3,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let streams = crate::parasolid::extract_streams(payload);
    let stream = streams.get(stream_ordinal).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed("SLDPRT sketch stream is missing".into())
    })?;
    let start = payload
        .windows(stream.len())
        .position(|candidate| candidate == stream.as_slice())
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back for compressed transmit streams is not implemented"
                    .into(),
            )
        })?;
    let header = crate::parasolid::stream_header(stream).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed("invalid retained SLDPRT sketch stream".into())
    })?;
    let body = payload
        .get_mut(start + header.body_offset..start + stream.len())
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed("SLDPRT sketch stream bounds changed".into())
        })?;
    let xyz_m = [point_mm.x * 0.001, point_mm.y * 0.001, point_mm.z * 0.001];
    if !crate::brep::patch_point(body, attr, xyz_m) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "SLDPRT sketch point {attr} is missing"
        )));
    }
    Ok(())
}

fn patch_direct_curve(
    payload: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let streams = crate::parasolid::extract_streams(payload);
    let stream = streams.get(request.stream).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed("SLDPRT sketch stream is missing".into())
    })?;
    let start = payload
        .windows(stream.len())
        .position(|candidate| candidate == stream.as_slice())
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back for compressed transmit streams is not implemented"
                    .into(),
            )
        })?;
    let header = crate::parasolid::stream_header(stream).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed("invalid retained SLDPRT sketch stream".into())
    })?;
    let body = payload
        .get_mut(start + header.body_offset..start + stream.len())
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed("SLDPRT sketch stream bounds changed".into())
        })?;
    if matches!(request.geometry, SketchGeometry::Nurbs { .. }) {
        return patch_direct_nurbs(body, request);
    }
    let Some(CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    }) = crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return patch_direct_ellipse(body, request);
    };
    let (center_2d, radius, angles) = match request.geometry {
        SketchGeometry::Circle { center, radius } => (center, radius.0, None),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (center, radius.0, Some((start_angle.0, end_angle.0))),
        _ => {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch carrier family changed".into(),
            ))
        }
    };
    let center = lift_point(center_2d, request.origin, request.u_axis, request.v_axis);
    let curve = CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch circle carrier cannot be patched".into(),
        ));
    }
    let endpoints = angles.map_or(
        [offset_point(center_2d, polar(radius, 0.0)); 2],
        |(start, end)| {
            [
                offset_point(center_2d, polar(radius, start)),
                offset_point(center_2d, polar(radius, end)),
            ]
        },
    );
    for (attr, endpoint) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(endpoints)
    {
        let point = lift_point(endpoint, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch curve endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn patch_direct_nurbs(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let SketchGeometry::Nurbs {
        degree,
        ref knots,
        ref control_points,
        ref weights,
        periodic,
    } = request.geometry
    else {
        unreachable!();
    };
    let curve = cadmpeg_ir::geometry::NurbsCurve {
        degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| lift_point(*point, request.origin, request.u_axis, request.v_axis))
            .collect(),
        weights: weights.clone(),
        periodic,
    };
    if !crate::brep::patch_nurbs_by_attr(body, request.carrier_attr, &curve) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT sketch NURBS edit changes native storage shape".into(),
        ));
    }
    Ok(())
}

fn patch_direct_ellipse(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let Some(CurveGeometry::Ellipse { axis, .. }) =
        crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch analytic carrier is missing".into(),
        ));
    };
    let SketchGeometry::Ellipse {
        center,
        major_angle,
        major_radius,
        minor_radius,
        start_angle,
        end_angle,
    } = request.geometry
    else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch carrier family changed".into(),
        ));
    };
    let center_3d = lift_point(center, request.origin, request.u_axis, request.v_axis);
    let major_direction = Vector3::new(
        request.u_axis.x * major_angle.0.cos() + request.v_axis.x * major_angle.0.sin(),
        request.u_axis.y * major_angle.0.cos() + request.v_axis.y * major_angle.0.sin(),
        request.u_axis.z * major_angle.0.cos() + request.v_axis.z * major_angle.0.sin(),
    );
    let curve = CurveGeometry::Ellipse {
        center: center_3d,
        axis,
        major_direction,
        major_radius: major_radius.0,
        minor_radius: minor_radius.0,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch ellipse carrier cannot be patched".into(),
        ));
    }
    let parameters = match (start_angle, end_angle) {
        (Some(start), Some(end)) => [start.0, end.0],
        (None, None) => [0.0, 0.0],
        _ => unreachable!(),
    };
    for (attr, parameter) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(parameters)
    {
        let local = Point2::new(
            center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
            center.v
                + major_angle.0.sin() * major_radius.0 * parameter.cos()
                + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
        );
        let point = lift_point(local, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch ellipse endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn polar(radius: f64, angle: f64) -> Point2 {
    Point2::new(radius * angle.cos(), radius * angle.sin())
}

fn offset_point(origin: Point2, delta: Point2) -> Point2 {
    Point2::new(origin.u + delta.u, origin.v + delta.v)
}
