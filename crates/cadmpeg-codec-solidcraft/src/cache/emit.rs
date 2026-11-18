// SPDX-License-Identifier: Apache-2.0
//! Neutral topology of one native body.
//!
//! One neutral record is written per native record, keyed by the model
//! body's persistent key and the record's index. Native loops are stored
//! about their surface's natural normal; a face that its shell uses
//! reversed has each loop written backwards, so every neutral loop runs
//! counterclockwise about the face's oriented normal. Each coedge's radial
//! partner is the next use in its edge's radial order.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PcurveId, PointId, RegionId,
    ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary,
    LoopRing, ParameterInterval, PcurveUse, Point, Region, Sense, Shell, ShellMember, Vertex,
};
use cadmpeg_ir::Exactness;

use super::brep::{Brep, Loop as NativeLoop, Shell as NativeShell, Surface as NativeSurface};
use super::carrier::{self, Transfer};
use crate::loss::SolidcraftLossCode;

/// Counts that become one loss note per code.
#[derive(Debug, Default)]
pub(super) struct Tally {
    counts: BTreeMap<SolidcraftLossCode, usize>,
}

impl Tally {
    fn add(&mut self, code: SolidcraftLossCode, count: usize) {
        if count > 0 {
            *self.counts.entry(code).or_default() += count;
        }
    }

    /// Adds the counts of `other`.
    pub(super) fn merge(&mut self, other: Self) {
        for (code, count) in other.counts {
            self.add(code, count);
        }
    }

    /// The loss notes of the counts.
    pub(super) fn notes(self) -> Vec<LossNote> {
        self.counts
            .into_iter()
            .map(|(code, count)| {
                code.note(match code {
                    SolidcraftLossCode::CurveUntyped => format!(
                        "{count} cached edges have a curve with no neutral carrier and are \
                         written without one"
                    ),
                    SolidcraftLossCode::CurveApproximated => format!(
                        "{count} cached surface-intersection edges take their leader curve as \
                         their carrier"
                    ),
                    SolidcraftLossCode::SurfaceUntyped => {
                        format!("{count} cached faces have a surface with no neutral carrier")
                    }
                    SolidcraftLossCode::PcurveOmitted => format!(
                        "{count} cached pcurves are in the parameters of a NURBS form that the \
                         neutral carrier does not use, and are omitted"
                    ),
                    SolidcraftLossCode::BlendReduced => format!(
                        "{count} cached blend faces keep their net as their carrier; the blend \
                         definitions stay in the retained cache"
                    ),
                    SolidcraftLossCode::FaceShared => format!(
                        "{count} cached faces are used by two shells and are written in the \
                         first one only"
                    ),
                    _ => format!("{count} cached records"),
                })
            })
            .collect()
    }
}

/// Destination of the neutral records of one body.
pub(super) struct Sink<'s> {
    /// Neutral topology the records are appended to.
    pub(super) geometry: &'s mut Topology,
    /// Exactness of the records.
    pub(super) annotations: &'s mut AnnotationBuilder,
    /// Losses of the body.
    pub(super) tally: &'s mut Tally,
}

/// Identities of the records of one body, keyed under `scope`.
struct Ids<'k> {
    scope: &'k IdentityKey,
}

macro_rules! minter {
    ($name:ident, $ty:ty, $kind:literal) => {
        fn $name(&self, key: impl Into<IdentityKey>) -> $ty {
            <$ty>::compose(
                &cadmpeg_ir::identity_namespace!("solidcraft", "cache", $kind),
                self.scope.clone().colon(key),
            )
        }
    };
}

impl Ids<'_> {
    minter!(surface, SurfaceId, "surface");
    minter!(face, FaceId, "face");
    minter!(loop_, LoopId, "loop");
    minter!(coedge_key, CoedgeId, "coedge");
    minter!(edge, EdgeId, "edge");
    minter!(curve, CurveId, "curve");
    minter!(pcurve_key, PcurveId, "pcurve");
    minter!(vertex, VertexId, "vertex");
    minter!(point, PointId, "point");
    minter!(shell, ShellId, "shell");
    minter!(region, RegionId, "region");

    fn body(&self) -> BodyId {
        BodyId::compose(
            &cadmpeg_ir::identity_namespace!("solidcraft", "cache", "body"),
            self.scope.clone(),
        )
    }

    fn use_key(face: usize, loop_: usize, position: usize) -> IdentityKey {
        IdentityKey::from(face).colon(loop_).colon(position)
    }

    fn loop_id(&self, face: usize, loop_: usize) -> LoopId {
        self.loop_(IdentityKey::from(face).colon(loop_))
    }

    fn coedge(&self, face: usize, loop_: usize, position: usize) -> CoedgeId {
        self.coedge_key(Self::use_key(face, loop_, position))
    }

    fn pcurve(&self, face: usize, loop_: usize, position: usize) -> PcurveId {
        self.pcurve_key(Self::use_key(face, loop_, position))
    }
}

fn index(value: u32, length: usize, what: &str) -> Result<usize, CodecError> {
    usize::try_from(value)
        .ok()
        .filter(|index| *index < length)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "cached body {what} index {value} is out of range"
            ))
        })
}

fn tolerance(value: f64) -> Option<PositiveReal> {
    PositiveReal::new(value)
}

/// Writes the neutral B-rep of `brep` and returns its body, or `None` for a
/// body with no shell.
pub(super) fn emit(
    ctx: &DecodeContext<'_>,
    brep: &Brep,
    scope: &IdentityKey,
    name: Option<String>,
    sink: &mut Sink<'_>,
) -> Result<Option<BodyId>, CodecError> {
    let ids = Ids { scope };
    let records = brep.vertices.len()
        + brep.edges.len()
        + brep.faces.len()
        + brep.shells.len()
        + brep.regions.len();
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(records),
        "solidcraft cached body",
    )?;
    if brep.shells.is_empty() {
        return Ok(None);
    }

    // Shell ownership: the first bounded material region that lists a shell
    // owns it, and the infinite region owns the rest.
    let mut owner: Vec<Option<usize>> = vec![None; brep.shells.len()];
    for (region, record) in brep.regions.iter().enumerate().skip(1) {
        if record.void {
            continue;
        }
        for shell in &record.shells {
            let shell = index(*shell, brep.shells.len(), "shell")?;
            owner[shell].get_or_insert(region);
        }
    }
    let owner: Vec<usize> = owner
        .into_iter()
        .map(|region| region.unwrap_or(0))
        .collect();

    // Face uses: the first shell that uses a face holds it.
    let mut face_use: Vec<Option<(usize, bool)>> = vec![None; brep.faces.len()];
    let mut shared = 0;
    for (shell, record) in brep.shells.iter().enumerate() {
        if let NativeShell::Faces { faces, .. } = record {
            for oriented in faces {
                let face = index(oriented.face, brep.faces.len(), "face")?;
                match face_use[face] {
                    None => face_use[face] = Some((shell, oriented.forward)),
                    Some(_) => shared += 1,
                }
            }
        }
    }
    sink.tally.add(SolidcraftLossCode::FaceShared, shared);

    // Radial order: each use's partner is the next use of its edge.
    let mut radial: HashMap<(usize, usize, usize), CoedgeId> = HashMap::new();
    for record in &brep.edges {
        let uses: Vec<(usize, usize, usize)> = record
            .uses
            .iter()
            .map(|edge_use| {
                Ok((
                    index(edge_use.face, brep.faces.len(), "face")?,
                    usize::try_from(edge_use.loop_)
                        .map_err(|_| CodecError::malformed("cached loop index"))?,
                    usize::try_from(edge_use.position)
                        .map_err(|_| CodecError::malformed("cached use index"))?,
                ))
            })
            .collect::<Result<_, CodecError>>()?;
        for (at, key) in uses.iter().enumerate() {
            let next = uses[(at + 1) % uses.len()];
            radial.insert(*key, ids.coedge(next.0, next.1, next.2));
        }
    }

    // Vertices.
    let mut positions = Vec::with_capacity(brep.vertices.len());
    for (vertex, record) in brep.vertices.iter().enumerate() {
        let position = FinitePoint3::new(Point3::new(
            record.point[0],
            record.point[1],
            record.point[2],
        ))
        .ok_or_else(|| CodecError::malformed("cached vertex is not finite"))?;
        sink.geometry
            .points
            .push(Point::new(ids.point(vertex), position, None));
        sink.geometry.vertices.push(Vertex {
            id: ids.vertex(vertex),
            point: ids.point(vertex),
            tolerance: tolerance(record.tolerance),
            native_ref: None,
        });
        positions.push(record.point);
    }

    // Edges and their curves.
    let mut untyped_curves = 0;
    let mut approximated = 0;
    for (edge, record) in brep.edges.iter().enumerate() {
        let start = index(record.vertices[0], positions.len(), "vertex")?;
        let end = index(record.vertices[1], positions.len(), "vertex")?;
        let carrier =
            match carrier::edge_carrier(record, [positions[start], positions[end]], start == end) {
                Ok(found) => {
                    let curve = ids.curve(edge);
                    sink.geometry.curves.push(Curve {
                        id: curve.clone(),
                        geometry: CurveGeometry::Solved(found.geometry),
                        parameter_range: None,
                        source_object: None,
                        native_ref: None,
                    });
                    sink.annotations.exactness(&curve, Exactness::Derived);
                    if found.approximated {
                        approximated += 1;
                    }
                    match ParameterInterval::new(found.interval) {
                        Ok(interval) => EdgeCarrier::Bounded(curve, interval),
                        Err(_) => EdgeCarrier::Curve(curve),
                    }
                }
                Err(_) => {
                    untyped_curves += 1;
                    EdgeCarrier::Free
                }
            };
        sink.geometry.edges.push(Edge {
            id: ids.edge(edge),
            carrier,
            ends: Some(EdgeEnds::new(ids.vertex(start), ids.vertex(end))),
            tolerance: tolerance(record.tolerance),
            native_ref: None,
        });
    }
    sink.tally
        .add(SolidcraftLossCode::CurveUntyped, untyped_curves);
    sink.tally
        .add(SolidcraftLossCode::CurveApproximated, approximated);

    // Faces, their surfaces, loops, coedges, and pcurves.
    let mut untyped_surfaces = 0;
    let mut omitted = 0;
    let mut blends = 0;
    for (face, record) in brep.faces.iter().enumerate() {
        let Some((shell, forward)) = face_use[face] else {
            return Err(CodecError::malformed(format_args!(
                "cached face {face} is in no shell"
            )));
        };
        let (geometry, transfer, agrees) = match carrier::face_carrier(record) {
            Ok(found) => {
                if found.blend {
                    blends += 1;
                }
                (found.geometry, found.transfer, found.agrees)
            }
            Err(_) => {
                untyped_surfaces += 1;
                (
                    SolvedSurfaceGeometry::Unknown { record: None },
                    Transfer::Omit,
                    true,
                )
            }
        };
        let uses: usize = record
            .loops
            .iter()
            .map(|lp| match lp {
                NativeLoop::Edges { edges } => edges.len(),
                NativeLoop::Vertex { .. } => 0,
            })
            .sum();
        if matches!(transfer, Transfer::Omit) && record.nurbs.is_some() {
            omitted += uses;
        }
        let surface = ids.surface(face);
        sink.geometry.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        sink.annotations.exactness(&surface, Exactness::Derived);
        let face_id = ids.face(face);
        let reversed = !forward;
        let mut loops = Vec::with_capacity(record.loops.len());
        for (loop_index, native) in record.loops.iter().enumerate() {
            let loop_id = ids.loop_id(face, loop_index);
            let boundary = match native {
                NativeLoop::Vertex { vertex } => LoopBoundary::Vertex {
                    vertex: ids.vertex(index(*vertex, positions.len(), "vertex")?),
                    pcurves: Vec::new(),
                },
                NativeLoop::Edges { edges } => {
                    let order: Vec<usize> = if reversed {
                        (0..edges.len()).rev().collect()
                    } else {
                        (0..edges.len()).collect()
                    };
                    let mut ring = Vec::with_capacity(edges.len());
                    for position in order {
                        let oriented = &edges[position];
                        let edge = index(oriented.edge, brep.edges.len(), "edge")?;
                        let runs = oriented.forward != reversed;
                        let coedge = ids.coedge(face, loop_index, position);
                        let mut pcurves = Vec::new();
                        if let Transfer::Map(map, offset) = transfer {
                            match carrier::pcurve(&oriented.pcurve, map, offset) {
                                Ok(pcurve_geometry) => {
                                    let pcurve = ids.pcurve(face, loop_index, position);
                                    sink.geometry.pcurves.push(Pcurve {
                                        id: pcurve.clone(),
                                        geometry: pcurve_geometry,
                                        metadata: PcurveMetadata::general(None, None, None),
                                        native_ref: None,
                                    });
                                    let [low, high] = brep.edges[edge].interval;
                                    let range = if runs { [low, high] } else { [high, low] };
                                    pcurves.push(PcurveUse {
                                        pcurve,
                                        isoparametric: None,
                                        parameter_range: DirectedParameterRange::new(range).ok(),
                                    });
                                }
                                Err(_) => omitted += 1,
                            }
                        }
                        sink.geometry.coedges.push(Coedge {
                            id: coedge.clone(),
                            owner_loop: loop_id.clone(),
                            edge: ids.edge(edge),
                            radial_next: radial
                                .get(&(face, loop_index, position))
                                .cloned()
                                .unwrap_or_else(|| coedge.clone()),
                            sense: if runs {
                                Sense::Forward
                            } else {
                                Sense::Reversed
                            },
                            pcurves,
                            use_curve: None,
                            native_ref: None,
                        });
                        ring.push(coedge);
                    }
                    LoopBoundary::Ring(
                        LoopRing::new(ring, Vec::new()).map_err(CodecError::malformed)?,
                    )
                }
            };
            sink.geometry.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary,
                native_ref: None,
            });
            loops.push(loop_id);
        }
        if record.nurbs.is_some()
            && !matches!(
                record.surface,
                NativeSurface::Spline { .. } | NativeSurface::RationalSpline { .. }
            )
        {
            sink.annotations.exactness(&face_id, Exactness::Derived);
        }
        sink.geometry.faces.push(Face {
            id: face_id,
            shell: ids.shell(shell),
            surface,
            sense: if forward == agrees {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: FaceLoops::unspecified(loops),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
    }
    sink.tally
        .add(SolidcraftLossCode::SurfaceUntyped, untyped_surfaces);
    sink.tally.add(SolidcraftLossCode::PcurveOmitted, omitted);
    sink.tally.add(SolidcraftLossCode::BlendReduced, blends);

    // Shells, regions, and the body.
    let body = ids.body();
    let mut region_shells: BTreeMap<usize, Vec<ShellId>> = BTreeMap::new();
    let mut solid = false;
    let mut open = false;
    for (shell, record) in brep.shells.iter().enumerate() {
        let members = match record {
            NativeShell::Faces { wires, .. } => wires
                .iter()
                .map(|edge| {
                    Ok(ShellMember::WireEdge {
                        id: ids.edge(index(*edge, brep.edges.len(), "edge")?),
                    })
                })
                .collect::<Result<Vec<_>, CodecError>>()?,
            NativeShell::Wire { edges } => edges
                .iter()
                .map(|edge| {
                    Ok(ShellMember::WireEdge {
                        id: ids.edge(index(*edge, brep.edges.len(), "edge")?),
                    })
                })
                .collect::<Result<Vec<_>, CodecError>>()?,
            NativeShell::Vertex { vertex } => vec![ShellMember::FreeVertex {
                id: ids.vertex(index(*vertex, positions.len(), "vertex")?),
            }],
        };
        if owner[shell] == 0 {
            open = true;
        } else {
            solid = true;
        }
        sink.geometry.shells.push(Shell {
            id: ids.shell(shell),
            region: ids.region(owner[shell]),
            members,
            native_ref: None,
        });
        region_shells
            .entry(owner[shell])
            .or_default()
            .push(ids.shell(shell));
    }
    for (region, shells) in region_shells {
        // A bounded region lists its outer shell first.
        let shells = match brep.regions.get(region) {
            Some(record) if region > 0 => record
                .shells
                .iter()
                .filter_map(|shell| usize::try_from(*shell).ok())
                .filter(|shell| owner.get(*shell) == Some(&region))
                .map(|shell| ids.shell(shell))
                .collect(),
            _ => shells,
        };
        sink.geometry.regions.push(Region {
            id: ids.region(region),
            body: body.clone(),
            shells,
            native_ref: None,
        });
    }
    let only_wires = brep
        .shells
        .iter()
        .all(|shell| !matches!(shell, NativeShell::Faces { .. }));
    sink.geometry.bodies.push(Body {
        id: body.clone(),
        kind: match (solid, open) {
            (true, false) => BodyKind::Solid,
            (false, true) if only_wires => BodyKind::Wire,
            (false, true) => BodyKind::Sheet,
            _ => BodyKind::General,
        },
        transform: None,
        name,
        color: None,
        visible: None,
        native_ref: None,
    });
    Ok(Some(body))
}
