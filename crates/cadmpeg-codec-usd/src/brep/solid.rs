// SPDX-License-Identifier: Apache-2.0
//! The packed arrays of a `BrepArray` prim (spec §17): encoded from NURBS
//! B-rep records and decoded into them.

use std::collections::BTreeMap;

use super::{
    Brep, Catalog, EdgeRecord, FaceRecord, Form, LoopRecord, NurbsCurveRecord, NurbsSurfaceRecord,
    OrientedEdgeRecord, ShellRecord, SolidRecord, DEFAULT_TOLERANCE,
};
use crate::schema::{integer_tuples, integers, reals, tokens, vectors3, Issues};
use crate::stage::Prim;

/// The prim type of a B-rep array.
pub(crate) const PRIM_TYPE: &str = "BrepArray";
/// The vertex instance of `BrepPointAPI`.
pub(crate) const VERTEX_POINT_API: &str = "BrepPointAPI:vertexPoint";
/// The edge instance of `BrepCurve3dNurbAPI`.
pub(crate) const EDGE_CURVE_API: &str = "BrepCurve3dNurbAPI:edge3dNurb";
/// The surface API.
pub(crate) const SURFACE_API: &str = "BrepSurfaceNurbAPI";
/// The trim-curve API.
pub(crate) const TRIM_CURVE_API: &str = "BrepCurveUvNurbAPI";
/// The identity API (spec §18.1).
pub(crate) const IDENTITY_API: &str = "CadBrepIdentityAPI";
/// The type token of a NURBS edge curve.
const CURVE_TYPE: &str = "BrepCurve3dNurbAPI";
/// The type token of a point.
const POINT_TYPE: &str = "BrepPointAPI";
/// The `customData` key of a prim that is a fallback of a sibling
/// `BrepArray` prim (spec §17.5).
pub(crate) const FALLBACK_KEY: &str = "brepArray";
/// `loop:vertexIndex` of a loop that has edgeuses.
pub(crate) const NO_VERTEX: u32 = u32::MAX;

/// Why a prim's arrays do not describe a B-rep.
pub(crate) type Refusal = String;

/// The faceuses of one shell: each face with whether the use is its `same`
/// side.
type FaceUses = Vec<(usize, bool)>;

/// The packed NURBS curves of one curve API instance.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Curves<const N: usize> {
    pub(crate) control_vertices: Vec<[f64; N]>,
    pub(crate) vertex_count: Vec<u32>,
    pub(crate) order: Vec<u32>,
    pub(crate) knots: Vec<f64>,
    pub(crate) weights: Vec<f64>,
}

impl<const N: usize> Curves<N> {
    fn push(&mut self, curve: &NurbsCurveRecord<N>) -> Result<(), Refusal> {
        self.control_vertices.extend(curve.points.iter());
        self.vertex_count.push(count(curve.points.len())?);
        self.order.push(count(curve.order)?);
        self.knots.extend(curve.knots.iter());
        self.weights.extend(curve.weights.iter());
        Ok(())
    }

    fn len(&self) -> usize {
        self.vertex_count.len()
    }

    /// Curve `index` of the packed arrays, with its offsets precomputed.
    fn unpack(&self, name: &str) -> Result<Vec<NurbsCurveRecord<N>>, Refusal> {
        if self.order.len() != self.vertex_count.len() {
            return Err(format!("{name}: order and vertexCount differ in length"));
        }
        let total: usize = self.vertex_count.iter().map(|value| as_index(*value)).sum();
        if self.control_vertices.len() != total || self.weights.len() != total {
            return Err(format!(
                "{name}: control vertices or weights do not match vertexCount"
            ));
        }
        let mut out = Vec::with_capacity(self.vertex_count.len());
        let (mut point, mut knot) = (0, 0);
        for (vertices, order) in self.vertex_count.iter().zip(&self.order) {
            let (vertices, order) = (as_index(*vertices), as_index(*order));
            if order == 0 || order > vertices {
                return Err(format!("{name}: an order is 0 or above its vertex count"));
            }
            let knots = self
                .knots
                .get(knot..knot + vertices + order)
                .ok_or_else(|| format!("{name}: the knots end early"))?;
            out.push(NurbsCurveRecord {
                order,
                points: self.control_vertices[point..point + vertices].to_vec(),
                weights: self.weights[point..point + vertices].to_vec(),
                knots: knots.to_vec(),
                form: Form::Open,
            });
            point += vertices;
            knot += vertices + order;
        }
        if knot != self.knots.len() {
            return Err(format!("{name}: the knots do not match the curves"));
        }
        Ok(out)
    }
}

/// The packed NURBS surfaces of `BrepSurfaceNurbAPI`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Surfaces {
    pub(crate) control_vertices: Vec<[f64; 3]>,
    pub(crate) u_vertex_count: Vec<u32>,
    pub(crate) v_vertex_count: Vec<u32>,
    pub(crate) u_order: Vec<u32>,
    pub(crate) v_order: Vec<u32>,
    pub(crate) u_knots: Vec<f64>,
    pub(crate) v_knots: Vec<f64>,
    pub(crate) weights: Vec<f64>,
}

/// The arrays of one `BrepArray` prim.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Packed {
    pub(crate) intersect_tol: Vec<f64>,
    pub(crate) extent: Vec<[f64; 3]>,
    pub(crate) region_count: Vec<u32>,
    pub(crate) region_shell_count: Vec<u32>,
    pub(crate) region_solid: Vec<bool>,
    pub(crate) shell_faceuse_count: Vec<u32>,
    pub(crate) shell_wire_edge_count: Vec<u32>,
    pub(crate) shell_point: Vec<bool>,
    pub(crate) faceuse_face: Vec<u32>,
    pub(crate) faceuse_same: Vec<bool>,
    pub(crate) face_loop_count: Vec<u32>,
    pub(crate) face_range: Vec<[f64; 2]>,
    pub(crate) loop_edgeuse_count: Vec<u32>,
    pub(crate) loop_vertex: Vec<u32>,
    pub(crate) edgeuse_edge: Vec<u32>,
    pub(crate) edgeuse_same: Vec<bool>,
    pub(crate) edgeuse_next_radial: Vec<u32>,
    pub(crate) edgeuse_top_entry: Vec<bool>,
    pub(crate) edge_range: Vec<f64>,
    pub(crate) edge_vertices: Vec<[u32; 2]>,
    pub(crate) wire_edge_count: usize,
    pub(crate) vertices: Vec<[f64; 3]>,
    pub(crate) shell_points: Vec<[f64; 3]>,
    pub(crate) edge_curves: Curves<3>,
    pub(crate) surfaces: Surfaces,
    pub(crate) trim_curves: Option<Curves<2>>,
    /// The CADIR identity of each B-rep, face, edge, and vertex; an empty
    /// string is no identity.
    pub(crate) identities: PackedIdentities,
}

/// The `cad:` identity arrays of `CadBrepIdentityAPI` (spec §18.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PackedIdentities {
    pub(crate) breps: Vec<String>,
    pub(crate) faces: Vec<String>,
    pub(crate) edges: Vec<String>,
    pub(crate) vertices: Vec<String>,
}

impl PackedIdentities {
    /// Whether any entity has an identity.
    pub(crate) fn any(&self) -> bool {
        [&self.breps, &self.faces, &self.edges, &self.vertices]
            .iter()
            .any(|values| values.iter().any(|value| !value.is_empty()))
    }
}

/// `values`, padded with `None` to `count`, as packed strings.
fn packed_ids(values: &[Option<String>], count: usize) -> impl Iterator<Item = String> + '_ {
    (0..count).map(move |index| values.get(index).cloned().flatten().unwrap_or_default())
}

fn count(value: usize) -> Result<u32, Refusal> {
    u32::try_from(value).map_err(|_| "a B-rep count exceeds 32 bits".to_owned())
}

fn as_index(value: u32) -> usize {
    cadmpeg_core::decode::index_from_u32(value)
}

/// The bounds of `points`, as `[min, max]`.
fn bounds(points: impl Iterator<Item = [f64; 3]>) -> Option<[[f64; 3]; 2]> {
    let mut out: Option<[[f64; 3]; 2]> = None;
    for point in points {
        let [min, max] = out.get_or_insert([point, point]);
        for axis in 0..3 {
            if point[axis] < min[axis] {
                min[axis] = point[axis];
            }
            if point[axis] > max[axis] {
                max[axis] = point[axis];
            }
        }
    }
    out
}

/// The parameter domain `[knots[p], knots[n]]` of one surface direction.
fn domain(knots: &[f64], order: usize, count: usize) -> Option<[f64; 2]> {
    Some([*knots.get(order.checked_sub(1)?)?, *knots.get(count)?])
}

/// The loop of `face` written with the face to the left seen from the
/// surface normal: the record loop when the face normal agrees with the
/// surface normal, and the reversed loop otherwise.
fn surface_loop(brep: &Brep, loop_: &LoopRecord, same_sense: bool) -> Vec<(usize, bool)> {
    let edges = loop_.oriented_edges.iter().filter_map(|index| {
        brep.oriented_edges
            .get(*index)
            .map(|record| (*index, record.orientation))
    });
    if same_sense {
        edges.collect()
    } else {
        let mut reversed: Vec<(usize, bool)> = edges
            .map(|(index, orientation)| (index, !orientation))
            .collect();
        reversed.reverse();
        reversed
    }
}

impl Packed {
    /// Appends one NURBS B-rep whose surface `i` is the surface of face `i`,
    /// curve `i` the curve of edge `i`, and pcurve `i`, when present, the
    /// pcurve of oriented edge `i`.
    pub(crate) fn push(&mut self, brep: &Brep, trimmed: bool) -> Result<(), Refusal> {
        let face_base = self.face_loop_count.len();
        let vertex_base = self.vertices.len();
        let edge_base = self.edge_vertices.len();
        if trimmed != self.trim_curves.is_some() && face_base > 0 {
            return Err("trim curves are written for every B-rep of a prim or for none".to_owned());
        }
        if trimmed {
            self.trim_curves.get_or_insert_with(Curves::default);
        }
        let offset = |base: usize, index: usize| count(base + index);

        // Regions, shells, and faceuses.
        let mut in_solid = vec![false; brep.shells.len()];
        for solid in &brep.solids {
            for shell in std::iter::once(&solid.outer).chain(&solid.voids) {
                if let Some(flag) = in_solid.get_mut(*shell) {
                    *flag = true;
                }
            }
        }
        let void_side = |face: usize| brep.faces.get(face).is_some_and(|face| face.same_sense);
        let mut regions: Vec<(bool, Vec<FaceUses>)> = Vec::new();
        let mut outside = Vec::new();
        for solid in &brep.solids {
            let shell = brep
                .shells
                .get(solid.outer)
                .ok_or("a solid names no shell")?;
            outside.push(
                shell
                    .faces
                    .iter()
                    .map(|face| (*face, void_side(*face)))
                    .collect(),
            );
        }
        for (index, shell) in brep.shells.iter().enumerate() {
            if !in_solid[index] {
                outside.push(
                    shell
                        .faces
                        .iter()
                        .flat_map(|face| [(*face, void_side(*face)), (*face, !void_side(*face))])
                        .collect(),
                );
            }
        }
        regions.push((false, outside));
        for solid in &brep.solids {
            let mut shells = Vec::new();
            for shell in std::iter::once(&solid.outer).chain(&solid.voids) {
                let shell = brep.shells.get(*shell).ok_or("a solid names no shell")?;
                shells.push(
                    shell
                        .faces
                        .iter()
                        .map(|face| (*face, !void_side(*face)))
                        .collect(),
                );
            }
            regions.push((true, shells));
            for void in &solid.voids {
                let shell = brep.shells.get(*void).ok_or("a solid names no shell")?;
                regions.push((
                    false,
                    vec![shell
                        .faces
                        .iter()
                        .map(|face| (*face, void_side(*face)))
                        .collect()],
                ));
            }
        }
        self.region_count.push(count(regions.len())?);
        for (solid, shells) in &regions {
            self.region_solid.push(*solid);
            self.region_shell_count.push(count(shells.len())?);
            for faceuses in shells {
                self.shell_faceuse_count.push(count(faceuses.len())?);
                self.shell_wire_edge_count.push(0);
                self.shell_point.push(false);
                for (face, same) in faceuses {
                    self.faceuse_face.push(offset(face_base, *face)?);
                    self.faceuse_same.push(*same);
                }
            }
        }

        // Faces, loops, and edgeuses.
        let mut edge_uses: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for face in &brep.faces {
            let (Catalog::Nurbs, surface) = face.surface else {
                return Err("a face has no NURBS surface".to_owned());
            };
            let record = brep
                .nurbs_surfaces
                .get(surface)
                .ok_or("a face names no surface")?;
            self.push_surface(record)?;
            let u = domain(&record.u_knots, record.u_order, record.u_count)
                .ok_or("a surface has no domain")?;
            let v = domain(&record.v_knots, record.v_order, record.v_count)
                .ok_or("a surface has no domain")?;
            self.face_range.push([u[0], v[0]]);
            self.face_range.push([u[1], v[1]]);
            self.face_loop_count.push(count(face.loops.len())?);
            for loop_index in &face.loops {
                let loop_ = brep.loops.get(*loop_index).ok_or("a face names no loop")?;
                let uses = surface_loop(brep, loop_, face.same_sense);
                self.loop_edgeuse_count.push(count(uses.len())?);
                self.loop_vertex.push(match loop_.vertex {
                    Some(vertex) if uses.is_empty() => offset(vertex_base, vertex)?,
                    _ => NO_VERTEX,
                });
                for (oriented, same) in uses {
                    let record = brep
                        .oriented_edges
                        .get(oriented)
                        .ok_or("a loop names no oriented edge")?;
                    edge_uses
                        .entry(record.edge)
                        .or_default()
                        .push(self.edgeuse_edge.len());
                    self.edgeuse_edge.push(offset(edge_base, record.edge)?);
                    self.edgeuse_same.push(same);
                    self.edgeuse_top_entry.push(!same);
                    if let Some(curves) = &mut self.trim_curves {
                        if trimmed {
                            let (Catalog::Nurbs, pcurve) =
                                record.pcurve.ok_or("an oriented edge has no pcurve")?
                            else {
                                return Err("a pcurve is not NURBS".to_owned());
                            };
                            let curve = brep
                                .nurbs_pcurves
                                .get(pcurve)
                                .ok_or("an oriented edge names no pcurve")?;
                            let written = if same == record.orientation {
                                curve.clone()
                            } else {
                                super::convert::reversed(curve)
                            };
                            curves.push(&written)?;
                        }
                    }
                }
            }
        }
        self.edgeuse_next_radial.resize(self.edgeuse_edge.len(), 0);
        for uses in edge_uses.values() {
            for (position, edgeuse) in uses.iter().enumerate() {
                let next = uses[(position + 1) % uses.len()];
                self.edgeuse_next_radial[*edgeuse] = count(next)?;
            }
        }

        // Edges and vertices.
        for (index, edge) in brep.edges.iter().enumerate() {
            let EdgeRecord {
                vertices,
                curve,
                range,
                same_sense,
                ..
            } = edge;
            if *curve != Some((Catalog::Nurbs, index)) || !same_sense {
                return Err("an edge curve is not its own NURBS curve in its direction".to_owned());
            }
            let record = brep
                .nurbs_curves
                .get(index)
                .ok_or("an edge names no curve")?;
            self.edge_curves.push(record)?;
            self.edge_range.extend(range.iter());
            self.edge_vertices.push([
                offset(vertex_base, vertices[0])?,
                offset(vertex_base, vertices[1])?,
            ]);
        }
        self.vertices.extend(brep.vertices.iter());
        let ids = &brep.identities;
        self.identities
            .breps
            .push(ids.body.clone().unwrap_or_default());
        self.identities
            .faces
            .extend(packed_ids(&ids.faces, brep.faces.len()));
        self.identities
            .edges
            .extend(packed_ids(&ids.edges, brep.edges.len()));
        self.identities
            .vertices
            .extend(packed_ids(&ids.vertices, brep.vertices.len()));

        self.intersect_tol.push(brep.tolerance);
        let box_ = bounds(
            brep.vertices
                .iter()
                .chain(
                    brep.nurbs_curves
                        .iter()
                        .flat_map(|curve| curve.points.iter()),
                )
                .chain(
                    brep.nurbs_surfaces
                        .iter()
                        .flat_map(|surface| surface.points.iter()),
                )
                .copied(),
        )
        .ok_or("a B-rep has no point")?;
        self.extent.extend(box_);
        Ok(())
    }

    fn push_surface(&mut self, record: &NurbsSurfaceRecord) -> Result<(), Refusal> {
        let surfaces = &mut self.surfaces;
        surfaces.control_vertices.extend(record.points.iter());
        surfaces.u_vertex_count.push(count(record.u_count)?);
        surfaces.v_vertex_count.push(count(record.v_count)?);
        surfaces.u_order.push(count(record.u_order)?);
        surfaces.v_order.push(count(record.v_order)?);
        surfaces.u_knots.extend(record.u_knots.iter());
        surfaces.v_knots.extend(record.v_knots.iter());
        surfaces.weights.extend(record.weights.iter());
        Ok(())
    }

    /// The bounds of every B-rep.
    pub(crate) fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        bounds(self.extent.iter().copied())
    }
}

/// The NURBS surfaces of the packed arrays, one for each face.
fn unpack_surfaces(surfaces: &Surfaces) -> Result<Vec<NurbsSurfaceRecord>, Refusal> {
    let n = surfaces.u_vertex_count.len();
    if [
        &surfaces.v_vertex_count,
        &surfaces.u_order,
        &surfaces.v_order,
    ]
    .iter()
    .any(|values| values.len() != n)
    {
        return Err("the surface count arrays differ in length".to_owned());
    }
    let mut out = Vec::with_capacity(n);
    let (mut point, mut u_knot, mut v_knot) = (0, 0, 0);
    for index in 0..n {
        let (u_count, v_count) = (
            as_index(surfaces.u_vertex_count[index]),
            as_index(surfaces.v_vertex_count[index]),
        );
        let (u_order, v_order) = (
            as_index(surfaces.u_order[index]),
            as_index(surfaces.v_order[index]),
        );
        if u_order == 0 || v_order == 0 || u_order > u_count || v_order > v_count {
            return Err("a surface order is 0 or above its vertex count".to_owned());
        }
        let points = u_count
            .checked_mul(v_count)
            .ok_or("a surface vertex count overflows")?;
        let range = point..point + points;
        out.push(NurbsSurfaceRecord {
            u_order,
            v_order,
            u_count,
            v_count,
            u_form: Form::Open,
            v_form: Form::Open,
            points: surfaces
                .control_vertices
                .get(range.clone())
                .ok_or("the surface control vertices end early")?
                .to_vec(),
            weights: surfaces
                .weights
                .get(range)
                .ok_or("the surface weights end early")?
                .to_vec(),
            u_knots: surfaces
                .u_knots
                .get(u_knot..u_knot + u_count + u_order)
                .ok_or("the surface U knots end early")?
                .to_vec(),
            v_knots: surfaces
                .v_knots
                .get(v_knot..v_knot + v_count + v_order)
                .ok_or("the surface V knots end early")?
                .to_vec(),
        });
        point += points;
        u_knot += u_count + u_order;
        v_knot += v_count + v_order;
    }
    if point != surfaces.control_vertices.len()
        || point != surfaces.weights.len()
        || u_knot != surfaces.u_knots.len()
        || v_knot != surfaces.v_knots.len()
    {
        return Err("the surface arrays do not match the surface counts".to_owned());
    }
    Ok(out)
}

/// Reads the arrays of a `BrepArray` prim.
pub(crate) fn read(prim: &Prim<'_>, issues: &mut Issues) -> Result<Packed, Refusal> {
    let indices = |issues: &mut Issues, name: &str| -> Result<Vec<u32>, Refusal> {
        integers(prim, name, issues)
            .unwrap_or_default()
            .into_iter()
            .map(|value| {
                u32::try_from(value).map_err(|_| format!("{name}: an index is not a uint"))
            })
            .collect()
    };
    let flags =
        |issues: &mut Issues, name: &str, yes: &str, no: &str| -> Result<Vec<bool>, Refusal> {
            tokens(prim, name, issues)
                .unwrap_or_default()
                .into_iter()
                .map(|token| match token.as_str() {
                    value if value == yes => Ok(true),
                    value if value == no => Ok(false),
                    _ => Err(format!("{name}: unknown token `{token}`")),
                })
                .collect()
        };
    let pairs = |issues: &mut Issues, name: &str| -> Vec<[f64; 2]> {
        reals(prim, name, 2, issues)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| [pair[0], pair[1]])
            .collect()
    };
    let only = |issues: &mut Issues, name: &str, token: &str| -> Result<usize, Refusal> {
        let values = tokens(prim, name, issues).unwrap_or_default();
        if let Some(other) = values.iter().find(|value| *value != token) {
            return Err(format!("{name}: unsupported type `{other}`"));
        }
        Ok(values.len())
    };
    let curves3 = |issues: &mut Issues, instance: &str| -> Result<Curves<3>, Refusal> {
        let name = |field: &str| format!("brep:{instance}:curve3d:nurb:{field}");
        Ok(Curves {
            control_vertices: vectors3(prim, &name("controlVertices"), issues).unwrap_or_default(),
            vertex_count: indices(issues, &name("vertexCount"))?,
            order: indices(issues, &name("order"))?,
            knots: reals(prim, &name("knots"), 1, issues).unwrap_or_default(),
            weights: reals(prim, &name("weights"), 1, issues).unwrap_or_default(),
        })
    };
    let index_pairs = |issues: &mut Issues, name: &str| -> Result<Vec<[u32; 2]>, Refusal> {
        integer_tuples(prim, name, 2, issues)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| {
                Ok([
                    u32::try_from(pair[0]).map_err(|_| format!("{name}: an index is negative"))?,
                    u32::try_from(pair[1]).map_err(|_| format!("{name}: an index is negative"))?,
                ])
            })
            .collect()
    };
    let schemas = prim.api_schemas();
    let edge_curve_count = only(issues, "edge:curveType", CURVE_TYPE)?;
    let face_count = only(issues, "face:surfaceType", SURFACE_API)?;
    let wire_edge_count = only(issues, "wireEdge:curveType", CURVE_TYPE)?;
    let vertex_count = only(issues, "vertex:pointType", POINT_TYPE)?;
    let shell_point = tokens(prim, "shell:pointType", issues)
        .unwrap_or_default()
        .into_iter()
        .map(|token| match token.as_str() {
            POINT_TYPE => Ok(true),
            "none" => Ok(false),
            _ => Err(format!("shell:pointType: unsupported type `{token}`")),
        })
        .collect::<Result<Vec<bool>, Refusal>>()?;
    let trim_curves = if schemas.contains(TRIM_CURVE_API) {
        let name = |field: &str| format!("brep:curveUv:nurb:{field}");
        Some(Curves {
            control_vertices: pairs(issues, &name("controlVertices")),
            vertex_count: indices(issues, &name("vertexCount"))?,
            order: indices(issues, &name("order"))?,
            knots: reals(prim, &name("knots"), 1, issues).unwrap_or_default(),
            weights: reals(prim, &name("weights"), 1, issues).unwrap_or_default(),
        })
    } else {
        None
    };
    let packed = Packed {
        intersect_tol: reals(prim, "brep:intersectTol3d", 1, issues).unwrap_or_default(),
        extent: vectors3(prim, "brep:extent", issues).unwrap_or_default(),
        region_count: indices(issues, "brep:regionCount")?,
        region_shell_count: indices(issues, "region:shellCount")?,
        region_solid: flags(issues, "region:type", "solidRegion", "voidRegion")?,
        shell_faceuse_count: indices(issues, "shell:faceuseCount")?,
        shell_wire_edge_count: indices(issues, "shell:wireEdgeCount")?,
        shell_point,
        faceuse_face: indices(issues, "faceuse:faceIndex")?,
        faceuse_same: flags(issues, "faceuse:orientationType", "same", "opposite")?,
        face_loop_count: indices(issues, "face:loopCount")?,
        face_range: pairs(issues, "face:range"),
        loop_edgeuse_count: indices(issues, "loop:edgeuseCount")?,
        loop_vertex: indices(issues, "loop:vertexIndex")?,
        edgeuse_edge: indices(issues, "edgeuse:edgeIndex")?,
        edgeuse_same: flags(issues, "edgeuse:orientationType", "same", "opposite")?,
        edgeuse_next_radial: indices(issues, "edgeuse:nextRadialEUIndex")?,
        edgeuse_top_entry: flags(
            issues,
            "edgeuse:thisRadialEntryType",
            "topEntry",
            "bottomEntry",
        )?,
        edge_range: reals(prim, "edge:range", 1, issues).unwrap_or_default(),
        edge_vertices: index_pairs(issues, "edge:vertexIndices")?,
        wire_edge_count,
        vertices: vectors3(prim, "brep:vertexPoint:point:position", issues).unwrap_or_default(),
        shell_points: vectors3(prim, "brep:shellPoint:point:position", issues).unwrap_or_default(),
        edge_curves: curves3(issues, "edge3dNurb")?,
        surfaces: Surfaces {
            control_vertices: vectors3(prim, "brep:surface:nurb:controlVertices", issues)
                .unwrap_or_default(),
            u_vertex_count: indices(issues, "brep:surface:nurb:uVertexCount")?,
            v_vertex_count: indices(issues, "brep:surface:nurb:vVertexCount")?,
            u_order: indices(issues, "brep:surface:nurb:uOrder")?,
            v_order: indices(issues, "brep:surface:nurb:vOrder")?,
            u_knots: reals(prim, "brep:surface:nurb:uKnots", 1, issues).unwrap_or_default(),
            v_knots: reals(prim, "brep:surface:nurb:vKnots", 1, issues).unwrap_or_default(),
            weights: reals(prim, "brep:surface:nurb:weights", 1, issues).unwrap_or_default(),
        },
        trim_curves,
        identities: if schemas.contains(IDENTITY_API) {
            PackedIdentities {
                breps: tokens(prim, "cad:brep:id", issues).unwrap_or_default(),
                faces: tokens(prim, "cad:face:id", issues).unwrap_or_default(),
                edges: tokens(prim, "cad:edge:id", issues).unwrap_or_default(),
                vertices: tokens(prim, "cad:vertex:id", issues).unwrap_or_default(),
            }
        } else {
            PackedIdentities::default()
        },
    };
    if packed.edge_curves.len() != edge_curve_count
        || packed.edge_vertices.len() != edge_curve_count
    {
        return Err("the edge arrays differ in length".to_owned());
    }
    if packed.face_loop_count.len() != face_count {
        return Err("the face arrays differ in length".to_owned());
    }
    if packed.vertices.len() != vertex_count {
        return Err("the vertex arrays differ in length".to_owned());
    }
    Ok(packed)
}

/// The B-reps of one prim and what they hold that records do not.
#[derive(Debug, Default)]
pub(crate) struct Decoded {
    pub(crate) breps: Vec<Brep>,
    /// The B-rep array index of each record face, for each B-rep.
    pub(crate) faces: Vec<Vec<usize>>,
    /// Wire edges and point shells, which records do not hold.
    pub(crate) untransferred: usize,
}

/// Consecutive ranges of `counts`.
fn ranges(counts: &[u32]) -> Vec<std::ops::Range<usize>> {
    let mut start = 0;
    counts
        .iter()
        .map(|value| {
            let end = start + as_index(*value);
            let range = start..end;
            start = end;
            range
        })
        .collect()
}

fn check_total(counts: &[u32], len: usize, name: &str) -> Result<(), Refusal> {
    let total: usize = counts.iter().map(|value| as_index(*value)).sum();
    if total == len {
        Ok(())
    } else {
        Err(format!(
            "{name} does not match the length of its partner arrays"
        ))
    }
}

/// Decodes the B-reps of `packed`.
pub(crate) fn decode(packed: &Packed) -> Result<Decoded, Refusal> {
    let breps = packed.region_count.len();
    if packed.intersect_tol.len() != breps && !packed.intersect_tol.is_empty() {
        return Err("brep:intersectTol3d does not have one value for each B-rep".to_owned());
    }
    check_total(
        &packed.region_count,
        packed.region_shell_count.len(),
        "brep:regionCount",
    )?;
    if packed.region_solid.len() != packed.region_shell_count.len() {
        return Err("region:type does not have one token for each region".to_owned());
    }
    check_total(
        &packed.region_shell_count,
        packed.shell_faceuse_count.len(),
        "region:shellCount",
    )?;
    check_total(
        &packed.shell_faceuse_count,
        packed.faceuse_face.len(),
        "shell:faceuseCount",
    )?;
    if packed.faceuse_same.len() != packed.faceuse_face.len() {
        return Err("the faceuse arrays differ in length".to_owned());
    }
    check_total(
        &packed.face_loop_count,
        packed.loop_edgeuse_count.len(),
        "face:loopCount",
    )?;
    check_total(
        &packed.loop_edgeuse_count,
        packed.edgeuse_edge.len(),
        "loop:edgeuseCount",
    )?;
    if packed.loop_vertex.len() != packed.loop_edgeuse_count.len() {
        return Err("loop:vertexIndex does not have one index for each loop".to_owned());
    }
    if packed.edgeuse_same.len() != packed.edgeuse_edge.len() {
        return Err("the edgeuse arrays differ in length".to_owned());
    }
    if packed.edge_range.len() != 2 * packed.edge_vertices.len() {
        return Err("edge:range does not have two values for each edge".to_owned());
    }
    let surfaces = unpack_surfaces(&packed.surfaces)?;
    if surfaces.len() != packed.face_loop_count.len() {
        return Err("BrepSurfaceNurbAPI does not have one surface for each face".to_owned());
    }
    let curves = packed.edge_curves.unpack("BrepCurve3dNurbAPI:edge3dNurb")?;
    let trims = match &packed.trim_curves {
        Some(trims) => {
            let unpacked = trims.unpack("BrepCurveUvNurbAPI")?;
            (unpacked.len() == packed.edgeuse_edge.len()).then_some(unpacked)
        }
        None => None,
    };
    let loops = ranges(&packed.face_loop_count);
    let uses = ranges(&packed.loop_edgeuse_count);
    let faceuses = ranges(&packed.shell_faceuse_count);
    let shells_of = ranges(&packed.region_shell_count);
    let regions_of = ranges(&packed.region_count);
    let mut out = Decoded {
        untransferred: packed.wire_edge_count
            + packed.shell_point.iter().filter(|point| **point).count(),
        ..Decoded::default()
    };

    for (brep_index, regions) in regions_of.into_iter().enumerate() {
        // The regions on each side of each face.
        let mut sides: BTreeMap<usize, Vec<(usize, bool)>> = BTreeMap::new();
        for region in regions.clone() {
            for shell in shells_of[region].clone() {
                for faceuse in faceuses[shell].clone() {
                    let face = as_index(packed.faceuse_face[faceuse]);
                    if face >= surfaces.len() {
                        return Err("faceuse:faceIndex names no face".to_owned());
                    }
                    sides
                        .entry(face)
                        .or_default()
                        .push((region, packed.faceuse_same[faceuse]));
                }
            }
        }
        let mut brep = Brep {
            tolerance: packed
                .intersect_tol
                .get(brep_index)
                .copied()
                .unwrap_or(DEFAULT_TOLERANCE),
            angle_scale: 1.0,
            valid: true,
            ..Brep::default()
        };
        let mut face_map: BTreeMap<usize, usize> = BTreeMap::new();
        let mut same_sense: BTreeMap<usize, bool> = BTreeMap::new();
        for (face, uses) in &sides {
            let [(first, first_same), (second, second_same)] = uses.as_slice() else {
                return Err(format!("face {face} does not have two faceuses"));
            };
            if first_same == second_same {
                return Err(format!("both faceuses of face {face} have one orientation"));
            }
            let (first_solid, second_solid) =
                (packed.region_solid[*first], packed.region_solid[*second]);
            let sense = match (first_solid, second_solid) {
                (true, true) => return Err(format!("face {face} bounds two solid regions")),
                (false, true) => *first_same,
                (true, false) => *second_same,
                (false, false) => *first_same,
            };
            same_sense.insert(*face, sense);
            face_map.insert(*face, face_map.len());
        }
        let mut vertex_map: BTreeMap<usize, usize> = BTreeMap::new();
        let mut edge_map: BTreeMap<usize, usize> = BTreeMap::new();
        let mut vertex = |brep: &mut Brep, index: u32| -> Result<usize, Refusal> {
            let index = as_index(index);
            let position = *packed
                .vertices
                .get(index)
                .ok_or("an index names no vertex")?;
            Ok(*vertex_map.entry(index).or_insert_with(|| {
                brep.vertices.push(position);
                brep.vertices.len() - 1
            }))
        };
        // Faces in face order, with their loops and edgeuses.
        let mut faces_in_order: Vec<usize> = face_map.keys().copied().collect();
        faces_in_order.sort_unstable();
        for (written, face) in faces_in_order.iter().enumerate() {
            face_map.insert(*face, written);
            let sense = same_sense[face];
            let mut loop_indices = Vec::new();
            for loop_ in loops[*face].clone() {
                let mut oriented: Vec<usize> = Vec::new();
                for edgeuse in uses[loop_].clone() {
                    let edge = as_index(packed.edgeuse_edge[edgeuse]);
                    let [start, end] = *packed
                        .edge_vertices
                        .get(edge)
                        .ok_or("edgeuse:edgeIndex names no edge")?;
                    let record_edge = match edge_map.get(&edge) {
                        Some(index) => *index,
                        None => {
                            let (start, end) = (vertex(&mut brep, start)?, vertex(&mut brep, end)?);
                            let curve = curves.get(edge).ok_or("an edge has no curve")?.clone();
                            brep.nurbs_curves.push(curve);
                            brep.edges.push(EdgeRecord {
                                vertices: [start, end],
                                curve: Some((Catalog::Nurbs, brep.nurbs_curves.len() - 1)),
                                range: [
                                    packed.edge_range[2 * edge],
                                    packed.edge_range[2 * edge + 1],
                                ],
                                same_sense: true,
                                tolerance: None,
                            });
                            edge_map.insert(edge, brep.edges.len() - 1);
                            brep.edges.len() - 1
                        }
                    };
                    let same = packed.edgeuse_same[edgeuse];
                    let pcurve = match &trims {
                        Some(trims) => {
                            let curve = &trims[edgeuse];
                            brep.nurbs_pcurves.push(if sense {
                                curve.clone()
                            } else {
                                super::convert::reversed(curve)
                            });
                            Some((Catalog::Nurbs, brep.nurbs_pcurves.len() - 1))
                        }
                        None => None,
                    };
                    brep.oriented_edges.push(OrientedEdgeRecord {
                        edge: record_edge,
                        orientation: if sense { same } else { !same },
                        pcurve,
                    });
                    oriented.push(brep.oriented_edges.len() - 1);
                }
                if !sense {
                    oriented.reverse();
                }
                let loop_vertex = if oriented.is_empty() {
                    Some(vertex(&mut brep, packed.loop_vertex[loop_])?)
                } else {
                    None
                };
                brep.loops.push(LoopRecord {
                    oriented_edges: oriented,
                    vertex: loop_vertex,
                });
                loop_indices.push(brep.loops.len() - 1);
            }
            brep.nurbs_surfaces.push(surfaces[*face].clone());
            brep.faces.push(FaceRecord {
                loops: loop_indices,
                surface: (Catalog::Nurbs, brep.nurbs_surfaces.len() - 1),
                same_sense: sense,
                id: i64::try_from(*face).unwrap_or(-1),
            });
        }
        // Shells: the shells of each solid region, then the sheet faces of
        // each void-region shell.
        for region in regions.clone() {
            if !packed.region_solid[region] {
                continue;
            }
            let mut shells = Vec::new();
            for shell in shells_of[region].clone() {
                let mut faces: Vec<usize> = faceuses[shell]
                    .clone()
                    .map(|faceuse| face_map[&as_index(packed.faceuse_face[faceuse])])
                    .collect();
                faces.dedup();
                if faces.is_empty() {
                    continue;
                }
                brep.shells.push(ShellRecord {
                    faces,
                    closed: true,
                });
                shells.push(brep.shells.len() - 1);
            }
            if let Some((outer, voids)) = shells.split_first() {
                brep.solids.push(SolidRecord {
                    outer: *outer,
                    voids: voids.to_vec(),
                });
            }
        }
        for region in regions {
            if packed.region_solid[region] {
                continue;
            }
            for shell in shells_of[region].clone() {
                let mut faces: Vec<usize> = Vec::new();
                for faceuse in faceuses[shell].clone() {
                    let face = as_index(packed.faceuse_face[faceuse]);
                    let sheet = sides[&face]
                        .iter()
                        .all(|(region, _)| !packed.region_solid[*region]);
                    let written = face_map[&face];
                    if sheet && !faces.contains(&written) {
                        faces.push(written);
                    }
                }
                if !faces.is_empty() {
                    brep.shells.push(ShellRecord {
                        faces,
                        closed: false,
                    });
                }
            }
        }
        if trims.is_none() {
            super::convert::derive_pcurves(&mut brep);
        }
        let named = |values: &[String], index: usize| {
            values.get(index).filter(|value| !value.is_empty()).cloned()
        };
        let ids = &packed.identities;
        brep.identities.body = named(&ids.breps, brep_index);
        brep.identities.faces = faces_in_order
            .iter()
            .map(|face| named(&ids.faces, *face))
            .collect();
        brep.identities.edges = vec![None; brep.edges.len()];
        for (global, record) in &edge_map {
            brep.identities.edges[*record] = named(&ids.edges, *global);
        }
        brep.identities.vertices = vec![None; brep.vertices.len()];
        for (global, record) in &vertex_map {
            brep.identities.vertices[*record] = named(&ids.vertices, *global);
        }
        out.faces.push(faces_in_order);
        out.breps.push(brep);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
