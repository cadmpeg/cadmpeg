// SPDX-License-Identifier: Apache-2.0
//! Object-id topology in the CATIA `b5 03` short-frame family.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::le::{f32_at, f64_at};

/// Resolved `b5 03` object-stream topology graph: faces, loops, pcurves, and
/// surfaces bound through the in-stream `object_id` map ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)),
/// together with the `05 08 01` vertex points used to bind edge endpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct B5Graph {
    /// `true` when every serialized face and loop node belongs to the resolved
    /// reference-closed graph; `false` when the graph is its maximal closed
    /// subset.
    pub complete: bool,
    /// Topology-bearing `b5 03` records from length-closed A8/B5 frame runs, in
    /// stream order.
    pub records: Vec<B5Record>,
    /// `b5 03 5f` face nodes, in stream declaration order (equal to STEP
    /// `ADVANCED_FACE` order, [spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub faces: Vec<B5Face>,
    /// `b5 03 62` loop nodes, keyed by `object_id`.
    pub loops: BTreeMap<u32, B5Loop>,
    /// `b5 03 21` pcurve nodes, keyed by `object_id`.
    pub pcurves: BTreeMap<u32, B5Pcurve>,
    /// `b5 03 27/28/2d` analytic surface nodes and `a8 03 34` NURBS
    /// surfaces, keyed by `object_id`.
    pub surfaces: BTreeMap<u32, B5Surface>,
    /// World-frame `05 08 01` vertex coordinates, in stream order.
    pub vertex_points: Vec<[f64; 3]>,
    /// Per-edge pair of indices into `vertex_points`, resolved by lifting
    /// each edge's pcurve endpoints through its surface and matching them
    /// to a unique vertex point.
    pub edge_vertices: BTreeMap<u32, [usize; 2]>,
    /// `b5 03 0e`/`0f` line and arc profile curves, keyed by `object_id`;
    /// referenced by `B5Surface::Revolution::profile_curve`.
    pub profiles: BTreeMap<u32, B5Profile>,
}

/// A profile curve swept by a `b5 03 2d` surface of revolution.
#[derive(Debug, Clone, PartialEq)]
pub enum B5Profile {
    /// `b5 03 0e`: a line through `point` along `direction`.
    Line {
        /// A point on the line.
        point: [f64; 3],
        /// Unit direction of the line.
        direction: [f64; 3],
    },
    /// `b5 03 0f`: an arc with a positive radius.
    Arc {
        /// Arc center.
        center: [f64; 3],
        /// Unit vector from `center` toward the zero-angle point.
        direction_x: [f64; 3],
        /// Unit vector orthogonal to `direction_x` completing the arc
        /// plane's basis.
        direction_y: [f64; 3],
        /// Positive arc radius.
        radius: f64,
    },
}

/// A resolved `b5 03` surface node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub enum B5Surface {
    /// `b5 03 27`: a plane spanned by `origin`, `direction_u`, and
    /// `direction_v`.
    Plane {
        /// A point on the plane.
        origin: [f64; 3],
        /// First in-plane basis direction, as stored (not necessarily
        /// unit).
        direction_u: [f64; 3],
        /// Second in-plane basis direction, as stored (not necessarily
        /// unit).
        direction_v: [f64; 3],
    },
    /// `b5 03 28`: a cylinder with a positive radius.
    Cylinder {
        /// A point on the cylinder axis.
        origin: [f64; 3],
        /// Unit reference direction orthogonal to `axis`, the zero-angle
        /// ray.
        reference_x: [f64; 3],
        /// Unit cylinder axis, `reference_x × stored_v` normalized.
        axis: [f64; 3],
        /// Positive cylinder radius.
        radius: f64,
    },
    /// `b5 03 2d`: a surface of revolution sweeping `profile_curve` about
    /// `axis_origin`/`axis_direction`.
    Revolution {
        /// `object_id` of the swept [`B5Profile`].
        profile_curve: u32,
        /// A point on the revolution axis.
        axis_origin: [f64; 3],
        /// Unit revolution axis.
        axis_direction: [f64; 3],
        /// Nonzero scale mapping a pcurve's `v` parameter to a revolution
        /// angle in radians (`angle = v / gauge_radius`).
        gauge_radius: f64,
    },
    /// An `a8 03 34` inline-pole B-spline surface, resolved through
    /// [`crate::geometry::a8_surfaces`] and merged into the same
    /// `object_id` namespace.
    Nurbs(NurbsSurface),
}

/// A resolved `b5 03 18` or `b5 03 21` pcurve node, represented as a 2D
/// B-spline curve in a surface's
/// parameter space ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq)]
pub struct B5Pcurve {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id` of the owning surface, taken directly from the pcurve's
    /// `catia_support_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
    pub surface: u32,
    /// B-spline degree.
    pub degree: u32,
    /// Distinct knot values, strictly increasing.
    pub distinct_knots: Vec<f64>,
    /// Per-knot multiplicities, index-aligned with `distinct_knots`.
    pub multiplicities: Vec<u32>,
    /// `(u, v)` control points in the surface's parameter space.
    pub control_points: Vec<[f64; 2]>,
    /// The curve's two clamped-end poles lifted through `surface` into
    /// world-frame 3D points, or `None` before [`parse`] resolves them or
    /// when the lift fails (unresolved surface, degenerate revolution
    /// scale, or NURBS evaluation failure).
    pub lifted_endpoints: Option<[[f64; 3]; 2]>,
}

/// One length-framed `b5 03` record as found by the stream walk ([spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#6-object-stream-record-framing-a5-03-a8-03-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Record {
    /// Byte offset of the `b5 03` marker in the source stream.
    pub offset: usize,
    /// Third header byte: the record's type/class code (`0x5f` face,
    /// `0x62` loop, `0x21` pcurve, `0x27`/`0x28`/`0x2d` surface, `0x5e`
    /// edge, `0x18` line pcurve, `0x0e`/`0x0f` profile, ...).
    pub class: u8,
    /// Dense creation-order `object_id` stored inline at `+4` ([spec §6.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#65-a8-03-common-object-stream-freeform-class)).
    pub object_id: u32,
    /// Raw record payload after the 8-byte header.
    pub payload: Vec<u8>,
}

/// A resolved `b5 03 5f` face node ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Face {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id` of the face's surface, taken from the first reference
    /// token.
    pub surface: u32,
    /// `object_id`s of the face's `b5 03 62` loop nodes, in reference
    /// order.
    pub loops: Vec<u32>,
}

/// A resolved `b5 03 62` loop node: payload `<0x80 + n_refs>
/// (pcurve_ref edge_ref)* surface_ref` ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#66-object-stream-topology-b5-03)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B5Loop {
    /// This record's stream `object_id`.
    pub object_id: u32,
    /// `object_id`s of the loop's member pcurves (or `0x18` lines), in
    /// serialized order.
    pub pcurves: Vec<u32>,
    /// `object_id`s of the loop's member `b5 03 5e` edges, index-aligned
    /// with `pcurves`.
    pub edges: Vec<u32>,
    /// `object_id` of the loop's surface (the trailing reference token).
    pub surface: u32,
}

/// Resolve the dominant object-stream topology graph through inline object ids.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<B5Graph> {
    let records = records(bytes);
    let by_id: HashMap<u32, &B5Record> = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    if records.is_empty() || by_id.len() != records.len() {
        return None;
    }
    let mut surfaces: BTreeMap<u32, B5Surface> = records
        .iter()
        .filter_map(|record| parse_surface(record).map(|surface| (record.object_id, surface)))
        .collect();
    for surface in crate::geometry::a8_surfaces(bytes) {
        if let SurfaceGeometry::Nurbs(nurbs) = surface.geometry {
            surfaces.insert(surface.object_id, B5Surface::Nurbs(nurbs));
        }
    }
    let profiles: BTreeMap<u32, B5Profile> = records
        .iter()
        .filter_map(|record| parse_profile(record).map(|profile| (record.object_id, profile)))
        .collect();
    let mut pcurves: BTreeMap<u32, B5Pcurve> = records
        .iter()
        .filter_map(|record| {
            let pcurve = match record.class {
                0x18 => parse_line_pcurve(record),
                0x21 => parse_pcurve(record),
                _ => None,
            }?;
            Some((record.object_id, pcurve))
        })
        .collect();
    for pcurve in pcurves.values_mut() {
        pcurve.lifted_endpoints = surfaces
            .get(&pcurve.surface)
            .and_then(|surface| lift_pcurve_endpoints(surface, &profiles, &pcurve.control_points));
    }
    let source_face_count = records.iter().filter(|record| record.class == 0x5f).count();
    let source_loop_count = records.iter().filter(|record| record.class == 0x62).count();
    let loops: BTreeMap<u32, B5Loop> = records
        .iter()
        .filter(|record| record.class == 0x62)
        .filter_map(|record| {
            parse_loop(record, &by_id, &pcurves, &surfaces).map(|loop_| (record.object_id, loop_))
        })
        .collect();
    let faces: Vec<B5Face> = records
        .iter()
        .filter(|record| record.class == 0x5f)
        .filter_map(|record| parse_face(record, &loops, &surfaces))
        .collect();
    if faces.is_empty() || loops.is_empty() {
        return None;
    }
    let vertex_points = vertex_points(bytes);
    let edge_vertices = bind_edge_vertices(&loops, &pcurves, &vertex_points)?;
    let referenced_loops: std::collections::HashSet<u32> = faces
        .iter()
        .flat_map(|face| face.loops.iter().copied())
        .collect();
    let complete = faces.len() == source_face_count
        && loops.len() == source_loop_count
        && loops.iter().all(|(loop_id, loop_)| {
            referenced_loops.contains(loop_id)
                && loop_
                    .pcurves
                    .iter()
                    .zip(&loop_.edges)
                    .all(|(pcurve, edge)| {
                        pcurves
                            .get(pcurve)
                            .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                            && edge_vertices.contains_key(edge)
                    })
                && unique_loop_chain(loop_, &edge_vertices)
        });
    Some(B5Graph {
        complete,
        records,
        faces,
        loops,
        pcurves,
        surfaces,
        vertex_points,
        edge_vertices,
        profiles,
    })
}

fn unique_loop_chain(loop_: &B5Loop, edge_vertices: &BTreeMap<u32, [usize; 2]>) -> bool {
    let Some(first) = loop_.edges.first().and_then(|edge| edge_vertices.get(edge)) else {
        return false;
    };
    let mut solution_count = 0;
    for first_reversed in [false, true] {
        let initial = first[usize::from(first_reversed)];
        let mut current = first[usize::from(!first_reversed)];
        let mut valid = true;
        for edge_id in &loop_.edges[1..] {
            let Some(endpoints) = edge_vertices.get(edge_id) else {
                valid = false;
                break;
            };
            match (endpoints[0] == current, endpoints[1] == current) {
                (true, false) => current = endpoints[1],
                (false, true) => current = endpoints[0],
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && current == initial {
            solution_count += 1;
        }
    }
    solution_count == 1
}

fn parse_profile(record: &B5Record) -> Option<B5Profile> {
    match record.class {
        0x0e => Some(B5Profile::Line {
            point: point(&record.payload, 1)?,
            direction: unit(point(&record.payload, 25)?)?,
        }),
        0x0f if record.payload.first() == Some(&0x80) => {
            let radius = scalar(&record.payload, 73)?;
            (radius > 0.0).then_some(B5Profile::Arc {
                center: point(&record.payload, 1)?,
                direction_x: unit(point(&record.payload, 25)?)?,
                direction_y: unit(point(&record.payload, 49)?)?,
                radius,
            })
        }
        _ => None,
    }
}

fn vertex_points(bytes: &[u8]) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    let mut position = 0;
    while position + 15 <= bytes.len() {
        if bytes.get(position..position + 3) != Some(&[0x05, 0x08, 0x01]) {
            position += 1;
            continue;
        }
        let Some(point) = f32_at(bytes, position + 3)
            .zip(f32_at(bytes, position + 7))
            .zip(f32_at(bytes, position + 11))
            .map(|((x, y), z)| [f64::from(x), f64::from(y), f64::from(z)])
        else {
            break;
        };
        if point
            .iter()
            .all(|value| value.is_finite() && value.abs() < 1e7)
        {
            points.push(point);
        }
        position += 15;
    }
    points
}

fn bind_edge_vertices(
    loops: &BTreeMap<u32, B5Loop>,
    pcurves: &BTreeMap<u32, B5Pcurve>,
    points: &[[f64; 3]],
) -> Option<BTreeMap<u32, [usize; 2]>> {
    let point_index = point_index(points);
    let mut edges: BTreeMap<u32, [usize; 2]> = BTreeMap::new();
    for loop_ in loops.values() {
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
            let Some(endpoints) = pcurves.get(&pcurve_id)?.lifted_endpoints else {
                continue;
            };
            let indices: Option<[usize; 2]> = endpoints
                .map(|endpoint| canonical_point(points, &point_index, endpoint))
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|indices| indices.try_into().ok());
            let Some(indices) = indices else {
                continue;
            };
            if let Some(previous) = edges.get(&edge_id) {
                let mut previous_sorted = *previous;
                let mut current_sorted = indices;
                previous_sorted.sort_unstable();
                current_sorted.sort_unstable();
                if previous_sorted != current_sorted {
                    return None;
                }
            } else {
                edges.insert(edge_id, indices);
            }
        }
    }
    Some(edges)
}

const POINT_TOLERANCE: f64 = 1.5e-3;

fn point_cell(point: [f64; 3]) -> [i64; 3] {
    point.map(|coordinate| (coordinate / POINT_TOLERANCE).floor() as i64)
}

fn point_index(points: &[[f64; 3]]) -> HashMap<[i64; 3], Vec<usize>> {
    let mut index = HashMap::<[i64; 3], Vec<usize>>::new();
    for (point_index, point) in points.iter().enumerate() {
        index
            .entry(point_cell(*point))
            .or_default()
            .push(point_index);
    }
    index
}

fn canonical_point(
    points: &[[f64; 3]],
    index: &HashMap<[i64; 3], Vec<usize>>,
    endpoint: [f64; 3],
) -> Option<usize> {
    let cell = point_cell(endpoint);
    let mut matches = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let neighbor = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                matches.extend(index.get(&neighbor).into_iter().flatten().filter_map(
                    |&point_index| {
                        (distance_squared(points[point_index], endpoint)
                            <= POINT_TOLERANCE * POINT_TOLERANCE)
                            .then_some(point_index)
                    },
                ));
            }
        }
    }
    matches.into_iter().min()
}

fn distance_squared(left: [f64; 3], right: [f64; 3]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

fn parse_surface(record: &B5Record) -> Option<B5Surface> {
    match record.class {
        0x27 => Some(B5Surface::Plane {
            origin: point(&record.payload, 1)?,
            direction_u: point(&record.payload, 25)?,
            direction_v: point(&record.payload, 49)?,
        }),
        0x28 => {
            let direction_u = unit(point(&record.payload, 25)?)?;
            let axis = unit(cross(direction_u, point(&record.payload, 49)?))?;
            let radius = scalar(&record.payload, 73)?;
            (radius > 0.0).then_some(B5Surface::Cylinder {
                origin: point(&record.payload, 1)?,
                reference_x: direction_u,
                axis,
                radius,
            })
        }
        0x2d => {
            let mut position = 1;
            let profile_curve = reference(&record.payload, &mut position, record.object_id)?;
            let gauge_radius = scalar(&record.payload, position.checked_add(130)?)?;
            (gauge_radius.abs() > f64::EPSILON).then_some(B5Surface::Revolution {
                profile_curve,
                axis_origin: point(&record.payload, position)?,
                axis_direction: unit(point(&record.payload, position.checked_add(72)?)?)?,
                gauge_radius,
            })
        }
        _ => None,
    }
}

fn lift_pcurve_endpoints(
    surface: &B5Surface,
    profiles: &BTreeMap<u32, B5Profile>,
    control_points: &[[f64; 2]],
) -> Option<[[f64; 3]; 2]> {
    let endpoints = [*control_points.first()?, *control_points.last()?];
    match surface {
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => Some(
            endpoints
                .map(|[u, v]| add(*origin, add(scale(*direction_u, u), scale(*direction_v, v)))),
        ),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => {
            let reference_y = cross(*axis, *reference_x);
            Some(endpoints.map(|[u, v]| {
                let angle = u / radius;
                add(
                    *origin,
                    add(
                        scale(
                            add(
                                scale(*reference_x, angle.cos()),
                                scale(reference_y, angle.sin()),
                            ),
                            *radius,
                        ),
                        scale(*axis, v),
                    ),
                )
            }))
        }
        B5Surface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            gauge_radius,
        } => {
            let profile = profiles.get(profile_curve)?;
            Some(endpoints.map(|[u, v]| {
                let point = match profile {
                    B5Profile::Line { point, direction } => add(*point, scale(*direction, u)),
                    B5Profile::Arc {
                        center,
                        direction_x,
                        direction_y,
                        radius,
                    } => {
                        let angle = u / radius;
                        add(
                            *center,
                            scale(
                                add(
                                    scale(*direction_x, angle.cos()),
                                    scale(*direction_y, angle.sin()),
                                ),
                                *radius,
                            ),
                        )
                    }
                };
                rotate_about_axis(point, *axis_origin, *axis_direction, v / gauge_radius)
            }))
        }
        B5Surface::Nurbs(surface) => Some([
            evaluate_nurbs(surface, endpoints[0][0], endpoints[0][1])?,
            evaluate_nurbs(surface, endpoints[1][0], endpoints[1][1])?,
        ]),
    }
}

fn evaluate_nurbs(surface: &NurbsSurface, u: f64, v: f64) -> Option<[f64; 3]> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_basis = basis_values(
        &surface.u_knots,
        usize::try_from(surface.u_degree).ok()?,
        u,
        u_count,
    )?;
    let v_basis = basis_values(
        &surface.v_knots,
        usize::try_from(surface.v_degree).ok()?,
        v,
        v_count,
    )?;
    let mut numerator = [0.0; 3];
    let mut denominator = 0.0;
    for (u_index, u_value) in u_basis.iter().enumerate() {
        for (v_index, v_value) in v_basis.iter().enumerate() {
            let index = u_index.checked_mul(v_count)?.checked_add(v_index)?;
            let point = surface.control_points.get(index)?;
            let weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index))
                .copied()
                .unwrap_or(1.0);
            let factor = u_value * v_value * weight;
            numerator[0] += factor * point.x;
            numerator[1] += factor * point.y;
            numerator[2] += factor * point.z;
            denominator += factor;
        }
    }
    (denominator.abs() > f64::EPSILON).then(|| scale(numerator, 1.0 / denominator))
}

fn basis_values(knots: &[f64], degree: usize, parameter: f64, count: usize) -> Option<Vec<f64>> {
    if knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let mut basis = vec![0.0; count + degree];
    for index in 0..basis.len() {
        if (knots[index] <= parameter && parameter < knots[index + 1])
            || (parameter == *knots.last()? && index + 1 == count)
        {
            basis[index] = 1.0;
        }
    }
    for order in 1..=degree {
        for index in 0..count + degree - order {
            let left_denominator = knots[index + order] - knots[index];
            let right_denominator = knots[index + order + 1] - knots[index + 1];
            let left = if left_denominator.abs() > f64::EPSILON {
                (parameter - knots[index]) / left_denominator * basis[index]
            } else {
                0.0
            };
            let right = if right_denominator.abs() > f64::EPSILON {
                (knots[index + order + 1] - parameter) / right_denominator * basis[index + 1]
            } else {
                0.0
            };
            basis[index] = left + right;
        }
    }
    basis.truncate(count);
    Some(basis)
}

fn rotate_about_axis(point: [f64; 3], origin: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let relative = [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ];
    let cross_term = cross(axis, relative);
    let dot = axis[0] * relative[0] + axis[1] * relative[1] + axis[2] * relative[2];
    add(
        origin,
        add(
            scale(relative, angle.cos()),
            add(
                scale(cross_term, angle.sin()),
                scale(axis, dot * (1.0 - angle.cos())),
            ),
        ),
    )
}

fn scalar(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f64_at(bytes, offset)?;
    value.is_finite().then_some(value)
}

fn point(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        scalar(bytes, offset)?,
        scalar(bytes, offset + 8)?,
        scalar(bytes, offset + 16)?,
    ])
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale(value: [f64; 3], scalar: f64) -> [f64; 3] {
    [value[0] * scalar, value[1] * scalar, value[2] * scalar]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    (length > f64::EPSILON).then(|| scale(value, 1.0 / length))
}

fn parse_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = reference(&record.payload, &mut position, record.object_id)?;
    if record.payload.get(position) != Some(&0x01) {
        return None;
    }
    position += 1;
    let degree = compact(&record.payload, &mut position)?;
    position = position.checked_add(2)?;
    record.payload.get(..position)?;
    let knot_count = usize::try_from(compact(&record.payload, &mut position)?).ok()?;
    if !(2..=4096).contains(&knot_count) {
        return None;
    }
    position += if record.payload.get(position) == Some(&0x08) {
        2
    } else {
        1
    };
    record.payload.get(..position)?;
    let mut distinct_knots = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        let value = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        if !value.is_finite() {
            return None;
        }
        distinct_knots.push(value);
        position += 8;
    }
    if distinct_knots.windows(2).any(|pair| pair[0] >= pair[1]) {
        return None;
    }
    let mut multiplicities = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        multiplicities.push(compact(&record.payload, &mut position)?);
    }
    let pole_count = multiplicities
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(degree + 1)?;
    if !(2..=8192).contains(&pole_count) {
        return None;
    }
    let mut control_points = Vec::with_capacity(usize::try_from(pole_count).ok()?);
    for _ in 0..pole_count {
        let u = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        let v = f64::from_le_bytes(
            record
                .payload
                .get(position + 8..position + 16)?
                .try_into()
                .ok()?,
        );
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        control_points.push([u, v]);
        position += 16;
    }
    if record.payload.get(position..position + 2) != Some(&[0x05, 0x05])
        || record.payload.last() != Some(&0x07)
    {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree,
        distinct_knots,
        multiplicities,
        control_points,
        lifted_endpoints: None,
    })
}

fn parse_line_pcurve(record: &B5Record) -> Option<B5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = reference(&record.payload, &mut position, record.object_id)?;
    let mode = *record.payload.get(position)?;
    position += 1;
    let (start, end, control_points) = match mode {
        0x01 if record.payload.len() == position.checked_add(48)? => {
            let [u, v, du, dv, start, end] = line_values::<6>(&record.payload, position)?;
            if du.abs().max(dv.abs()) <= f64::EPSILON {
                return None;
            }
            (
                start,
                end,
                vec![
                    [u + start * du, v + start * dv],
                    [u + end * du, v + end * dv],
                ],
            )
        }
        0x05 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = line_values::<3>(&record.payload, position)?;
            (start, end, vec![[constant, start], [constant, end]])
        }
        0x09 if record.payload.len() == position.checked_add(24)? => {
            let [constant, start, end] = line_values::<3>(&record.payload, position)?;
            (start, end, vec![[start, constant], [end, constant]])
        }
        _ => return None,
    };
    if start >= end {
        return None;
    }
    Some(B5Pcurve {
        object_id: record.object_id,
        surface,
        degree: 1,
        distinct_knots: vec![start, end],
        multiplicities: vec![2, 2],
        control_points,
        lifted_endpoints: None,
    })
}

fn line_values<const N: usize>(payload: &[u8], mut position: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = f64::from_le_bytes(payload.get(position..position + 8)?.try_into().ok()?);
        if !value.is_finite() {
            return None;
        }
        position += 8;
    }
    Some(values)
}

fn compact(bytes: &[u8], position: &mut usize) -> Option<u32> {
    let lead = *bytes.get(*position)?;
    if lead % 4 == 1 {
        *position += 1;
        Some(u32::from((lead - 1) / 4))
    } else if lead != 0 && lead % 4 == 0 {
        let width = usize::from(lead / 4);
        if width > 4 {
            return None;
        }
        let mut value = 0u32;
        for (shift, byte) in bytes
            .get(*position + 1..*position + 1 + width)?
            .iter()
            .enumerate()
        {
            value |= u32::from(*byte) << (8 * shift);
        }
        *position += width + 1;
        Some(value)
    } else {
        None
    }
}

fn records(bytes: &[u8]) -> Vec<B5Record> {
    let mut records = Vec::new();
    let mut seen = HashMap::<u32, (u8, Vec<u8>)>::new();
    let mut position = 0;
    while position + 8 <= bytes.len() {
        let Some((end, _, _, _)) = object_frame(bytes, position) else {
            position += 1;
            continue;
        };
        let start = position;
        let mut at = position;
        let mut run = Vec::new();
        while let Some((next, family, class, object_id)) = object_frame(bytes, at) {
            run.push((at, next, family, class, object_id));
            at = next;
        }
        if run.len() < 2 {
            position = start + 1;
            continue;
        }
        for (record_start, record_end, family, class, object_id) in run {
            if family != 0xb5 || !is_topology_class(class) {
                continue;
            }
            let payload = bytes[record_start + 8..record_end].to_vec();
            if seen
                .get(&object_id)
                .is_some_and(|(seen_class, seen_payload)| {
                    *seen_class == class && *seen_payload == payload
                })
            {
                continue;
            }
            seen.insert(object_id, (class, payload.clone()));
            records.push(B5Record {
                offset: record_start,
                class,
                object_id,
                payload,
            });
        }
        position = at.max(end);
    }
    records
}

fn object_frame(bytes: &[u8], start: usize) -> Option<(usize, u8, u8, u32)> {
    if bytes.get(start + 1) != Some(&0x03) {
        return None;
    }
    let family = *bytes.get(start)?;
    let class = *bytes.get(start + 2)?;
    let (header, length, object_id) = match family {
        0xb5 => (
            8usize,
            usize::from(*bytes.get(start + 3)?),
            u32::from_le_bytes(bytes.get(start + 4..start + 8)?.try_into().ok()?),
        ),
        0xa8 => (
            11usize,
            usize::try_from(u32::from_le_bytes(
                bytes.get(start + 3..start + 7)?.try_into().ok()?,
            ))
            .ok()?,
            u32::from_le_bytes(bytes.get(start + 7..start + 11)?.try_into().ok()?),
        ),
        _ => return None,
    };
    let end = start.checked_add(header)?.checked_add(length)?;
    (end <= bytes.len()).then_some((end, family, class, object_id))
}

fn is_topology_class(class: u8) -> bool {
    matches!(
        class,
        0x0e | 0x0f | 0x18 | 0x21 | 0x27 | 0x28 | 0x2d | 0x5e | 0x5f | 0x62
    )
}

fn parse_face(
    record: &B5Record,
    loops: &BTreeMap<u32, B5Loop>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Face> {
    let references = if let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
    {
        let mut position = 1;
        let references = (0..count)
            .map(|_| reference(&record.payload, &mut position, record.object_id))
            .collect::<Option<Vec<_>>>()?;
        if position < record.payload.len() && record.payload[position] != 0x05 {
            return None;
        }
        references
    } else {
        uncounted_references(&record.payload, record.object_id)?
    };
    let surface = *references.first()?;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    let loop_ids: Vec<u32> = references[1..]
        .iter()
        .copied()
        .filter(|reference| loops.contains_key(reference))
        .collect();
    if loop_ids.is_empty() || loop_ids.len() + 1 != references.len() {
        return None;
    }
    Some(B5Face {
        object_id: record.object_id,
        surface,
        loops: loop_ids,
    })
}

fn parse_loop(
    record: &B5Record,
    by_id: &HashMap<u32, &B5Record>,
    parsed_pcurves: &BTreeMap<u32, B5Pcurve>,
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Option<B5Loop> {
    let count = usize::from(record.payload.first()?.checked_sub(0x80)?);
    if count < 3 || count % 2 == 0 {
        return None;
    }
    let mut position = 1;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        references.push(reference(&record.payload, &mut position, record.object_id)?);
    }
    let edge_count = (count - 1) / 2;
    if position < record.payload.len()
        && record.payload.get(position).copied() != u8::try_from(0x80 + edge_count).ok()
    {
        return None;
    }
    let surface = *references.last()?;
    if !surfaces.contains_key(&surface) {
        return None;
    }
    let mut pcurves = Vec::with_capacity((count - 1) / 2);
    let mut edges = Vec::with_capacity((count - 1) / 2);
    for pair in references[..count - 1].chunks_exact(2) {
        if !matches!(by_id.get(&pair[0])?.class, 0x18 | 0x21)
            || by_id.get(&pair[1])?.class != 0x5e
            || !parsed_pcurves.contains_key(&pair[0])
        {
            return None;
        }
        pcurves.push(pair[0]);
        edges.push(pair[1]);
    }
    Some(B5Loop {
        object_id: record.object_id,
        pcurves,
        edges,
        surface,
    })
}

fn uncounted_references(bytes: &[u8], anchor: u32) -> Option<Vec<u32>> {
    let mut position = 0;
    let mut references = Vec::new();
    while position < bytes.len() {
        references.push(reference(bytes, &mut position, anchor)?);
    }
    Some(references)
}

fn reference(bytes: &[u8], position: &mut usize, _anchor: u32) -> Option<u32> {
    let lead = *bytes.get(*position)?;
    let (value, width) = match lead {
        0x38 => (
            u32::from_le_bytes([
                *bytes.get(*position + 1)?,
                *bytes.get(*position + 2)?,
                *bytes.get(*position + 3)?,
                0,
            ]),
            4,
        ),
        0x30 => (
            u32::from(u16::from_le_bytes([
                *bytes.get(*position + 1)?,
                *bytes.get(*position + 2)?,
            ])) << 8,
            3,
        ),
        0x28 => (
            u32::from(*bytes.get(*position + 1)?) | (u32::from(*bytes.get(*position + 2)?) << 16),
            3,
        ),
        0x20 => (u32::from(*bytes.get(*position + 1)?) << 16, 2),
        0x18 => (
            u32::from(u16::from_le_bytes([
                *bytes.get(*position + 1)?,
                *bytes.get(*position + 2)?,
            ])),
            3,
        ),
        0x10 => (u32::from(*bytes.get(*position + 1)?) << 8, 2),
        0x08 => (u32::from(*bytes.get(*position + 1)?), 2),
        0x80..=0xff => (u32::from(lead - 0x80), 1),
        _ => return None,
    };
    *position += width;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_reference_tokens_fill_selected_id_bytes() {
        let mut position = 0;
        assert_eq!(
            reference(&[0x28, 0x34, 0x02], &mut position, 0x02_0033),
            Some(0x02_0034)
        );
        assert_eq!(position, 3);
        position = 0;
        assert_eq!(reference(&[0x20, 0x07], &mut position, 0), Some(0x07_0000));
        assert_eq!(position, 2);
        position = 0;
        assert_eq!(reference(&[0x8b], &mut position, 0), Some(11));
        assert_eq!(position, 1);
    }

    #[test]
    fn revolution_surface_accepts_sparse_profile_reference() {
        let mut payload = vec![0; 175];
        payload[1..4].copy_from_slice(&[0x30, 0x86, 0x16]);
        payload[4..12].copy_from_slice(&1.0f64.to_le_bytes());
        payload[76..84].copy_from_slice(&1.0f64.to_le_bytes());
        payload[134..142].copy_from_slice(&2.0f64.to_le_bytes());
        let record = B5Record {
            offset: 0,
            class: 0x2d,
            object_id: 0x16_8601,
            payload,
        };
        assert_eq!(
            parse_surface(&record),
            Some(B5Surface::Revolution {
                profile_curve: 0x16_8600,
                axis_origin: [1.0, 0.0, 0.0],
                axis_direction: [1.0, 0.0, 0.0],
                gauge_radius: 2.0,
            })
        );
    }
}
