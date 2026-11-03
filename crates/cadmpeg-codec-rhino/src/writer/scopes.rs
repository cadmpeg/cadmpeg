// SPDX-License-Identifier: Apache-2.0
//! Complete body closures before Rhino capability-specific projection.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::RhinoLossCode;

pub(super) struct BodyScope {
    pub(super) ir: CadIr,
    pub(super) points: std::collections::BTreeSet<String>,
    pub(super) surfaces: std::collections::BTreeSet<String>,
    pub(super) curves: std::collections::BTreeSet<String>,
    pub(super) pcurves: std::collections::BTreeSet<String>,
}

pub(super) fn collect(ir: &CadIr) -> Result<(Vec<BodyScope>, Vec<LossNote>), CodecError> {
    use std::collections::BTreeSet;

    let model = &ir.model;
    let mut scopes = Vec::new();
    let mut losses = Vec::new();

    for body in &model.bodies {
        let regions = body
            .regions
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let shells = model
            .regions
            .iter()
            .filter(|region| regions.contains(region.id.as_str()))
            .flat_map(|region| region.shells.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let faces = model
            .shells
            .iter()
            .filter(|shell| shells.contains(shell.id.as_str()))
            .flat_map(|shell| shell.faces().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let surfaces = model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .map(|face| face.surface.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let loops = model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .flat_map(|face| face.loops.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let coedges = model
            .loops
            .iter()
            .filter(|loop_| loops.contains(loop_.id.as_str()))
            .flat_map(|loop_| loop_.coedges().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let edges = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .map(|coedge| coedge.edge.as_str().to_owned())
            .chain(
                model
                    .shells
                    .iter()
                    .filter(|shell| shells.contains(shell.id.as_str()))
                    .flat_map(|shell| shell.wire_edges().iter().map(ToString::to_string)),
            )
            .collect::<BTreeSet<_>>();
        let pcurves = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .flat_map(|coedge| coedge.pcurves.iter())
            .chain(
                model
                    .loops
                    .iter()
                    .filter(|row| loops.contains(row.id.as_str()))
                    .flat_map(cadmpeg_ir::topology::Loop::vertex_pcurves),
            )
            .map(|use_| use_.pcurve.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let vertices = model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .flat_map(|edge| [edge.start.as_str().to_owned(), edge.end.as_str().to_owned()])
            .chain(
                model
                    .loops
                    .iter()
                    .filter(|row| loops.contains(row.id.as_str()))
                    .flat_map(cadmpeg_ir::topology::Loop::vertices)
                    .map(ToString::to_string),
            )
            .chain(
                model
                    .shells
                    .iter()
                    .filter(|shell| shells.contains(shell.id.as_str()))
                    .flat_map(|shell| shell.free_vertices().iter().map(ToString::to_string)),
            )
            .collect::<BTreeSet<_>>();
        let mut curves = model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .filter_map(|edge| edge.curve().map(|id| id.as_str().to_owned()))
            .chain(
                model
                    .coedges
                    .iter()
                    .filter(|row| coedges.contains(row.id.as_str()))
                    .filter_map(|row| row.use_curve.as_ref().map(|use_| use_.curve.to_string())),
            )
            .collect::<BTreeSet<_>>();
        // A kept composite carrier needs the chain it resolves through: the
        // writer admits edges on the resolved leaf and names the skipped
        // wrappers, so every wrapper and the leaf stay in the closure.
        let mut frontier: Vec<_> = curves.iter().cloned().collect();
        while let Some(id) = frontier.pop() {
            let Some(curve) = model.curves.iter().find(|curve| curve.id.as_str() == id) else {
                continue;
            };
            if let Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Composite { segments, .. }) =
                curve.geometry.solved()
            {
                for segment in segments.iter() {
                    if curves.insert(segment.curve.to_string()) {
                        frontier.push(segment.curve.to_string());
                    }
                }
            }
        }
        let points = model
            .vertices
            .iter()
            .filter(|vertex| vertices.contains(vertex.id.as_str()))
            .map(|vertex| vertex.point.as_str().to_owned())
            .collect::<BTreeSet<_>>();

        let mut scoped = CadIr::empty();
        scoped.tolerances = ir.tolerances;
        scoped.model.bodies.push(body.clone());
        scoped.model.regions = model
            .regions
            .iter()
            .filter(|entity| regions.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.shells = model
            .shells
            .iter()
            .filter(|entity| shells.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.faces = model
            .faces
            .iter()
            .filter(|entity| faces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.loops = model
            .loops
            .iter()
            .filter(|entity| loops.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.coedges = model
            .coedges
            .iter()
            .filter(|entity| coedges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.edges = model
            .edges
            .iter()
            .filter(|entity| edges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.vertices = model
            .vertices
            .iter()
            .filter(|entity| vertices.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.points = model
            .points
            .iter()
            .filter(|entity| points.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.surfaces = model
            .surfaces
            .iter()
            .filter(|entity| surfaces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.curves = model
            .curves
            .iter()
            .filter(|entity| curves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        for curve in &mut scoped.model.curves {
            if let Some(construction) = curve.geometry.procedural_construction() {
                let construction = construction.as_str().to_owned();
                losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                    "curve {} procedural construction {construction} was reduced to a held solved carrier",
                    curve.id
                )));
            }
            if let Some(cache) = curve.geometry.solved().cloned() {
                curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(cache);
            }
        }
        for surface in &mut scoped.model.surfaces {
            if let Some(construction) = surface.geometry.procedural_construction() {
                let construction = construction.as_str().to_owned();
                losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                    "surface {} procedural construction {construction} was reduced to a held solved carrier",
                    surface.id
                )));
            }
            if let Some(cache) = surface.geometry.solved().cloned() {
                surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(cache);
            }
        }
        scoped.model.pcurves = model
            .pcurves
            .iter()
            .filter(|entity| pcurves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        resolve_wrappers(&mut scoped, &mut losses);
        prune_dangling_topology(&mut scoped, &mut losses);
        super::reduce::validate(&scoped)?;
        let scope = BodyScope::pruned(scoped);
        scopes.push(scope);
    }
    Ok((scopes, losses))
}

/// Bake `Transformed` surfaces with rigid placements and `Transformed` /
/// `Trimmed` pcurves to the explicit carriers the Brep writer admits.
///
/// A `Transformed` surface under a proper-rigid placement is the same
/// surface in world coordinates; a `Transformed` line pcurve under an affine
/// parameter map is the same trim line; a `Transformed` circle pcurve under
/// a rigid parameter map is the same trim circle; a `Trimmed` line over its
/// range is the same segment. The writer keeps the baked carrier, so each
/// baked record charges `WriterCarrierResolved` naming it: a trim and a
/// Brep face each hold one carrier per record, not the wrapper.
pub(super) fn resolve_wrappers(scoped: &mut CadIr, losses: &mut Vec<LossNote>) {
    for surface in &mut scoped.model.surfaces {
        resolve_surface(surface, losses);
    }
    for pcurve in &mut scoped.model.pcurves {
        resolve_pcurve(pcurve, losses);
    }
}

pub(super) fn resolve_surface(
    surface: &mut cadmpeg_ir::geometry::Surface,
    losses: &mut Vec<LossNote>,
) {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    let (basis, transform) = match &surface.geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(placed)) => {
            (placed.basis().clone(), *placed.transform())
        }
        _ => return,
    };
    if !transform.is_proper_rigid() {
        return;
    }
    let mut baked = SurfaceGeometry::Solved(basis);
    if super::placement::transform_surface(&mut baked, transform).is_err() {
        return;
    }
    surface.geometry = baked;
    losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
        "surface {} transformed carrier was resolved to an explicit carrier",
        surface.id
    )));
}

pub(super) fn resolve_pcurve(
    pcurve: &mut cadmpeg_ir::geometry::pcurve::Pcurve,
    losses: &mut Vec<LossNote>,
) {
    use cadmpeg_ir::geometry::pcurve::{CirclePcurve, LinePcurve, PcurveGeometry};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::units::{FinitePoint2, NonzeroPoint2};
    let (baked, what) = match &pcurve.geometry {
        PcurveGeometry::Transformed(placed) => {
            let transform = *placed.transform();
            match placed.basis() {
                PcurveGeometry::Line(line) => {
                    let origin = transform.apply_point(line.origin().get());
                    let direction = transform.apply_vector(*line.direction().as_raw());
                    let (Some(origin), Some(direction)) =
                        (FinitePoint2::new(origin), NonzeroPoint2::new(direction))
                    else {
                        return;
                    };
                    let Some(baked) =
                        LinePcurve::try_new(origin.get(), Point2::from(direction)).ok()
                    else {
                        return;
                    };
                    (PcurveGeometry::Line(baked), "transformed")
                }
                PcurveGeometry::Circle(circle) => {
                    if !is_rigid_2(transform) {
                        return;
                    }
                    let center = transform.apply_point(circle.center().get());
                    let x = transform.apply_vector(*circle.x_axis().as_raw());
                    let y = transform.apply_vector(*circle.y_axis().as_raw());
                    let (Some(center), Some(x), Some(y)) = (
                        FinitePoint2::new(center),
                        FinitePoint2::new(x),
                        FinitePoint2::new(y),
                    ) else {
                        return;
                    };
                    let Some(baked) = CirclePcurve::from_parts(center, x, y, circle.radius())
                    else {
                        return;
                    };
                    (PcurveGeometry::Circle(baked), "transformed")
                }
                _ => return,
            }
        }
        PcurveGeometry::Trimmed(trimmed) => {
            let PcurveGeometry::Line(line) = trimmed.basis() else {
                return;
            };
            // A trim shares its basis parametrization (the evaluator reads
            // the basis at the trim parameter), so the baked carrier is the
            // basis itself; the edge domain selects the used interval.
            (PcurveGeometry::Line(*line), "trimmed")
        }
        _ => return,
    };
    pcurve.geometry = baked;
    losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
        "pcurve {} {what} carrier was resolved to an explicit carrier",
        pcurve.id
    )));
}

/// Whether a parameter-space affine map is a proper-rigid motion, mirroring
/// `Transform::is_proper_rigid` at its 1e-9 bound.
pub(super) fn is_rigid_2(transform: cadmpeg_ir::transform::Transform2) -> bool {
    const EPSILON: f64 = 1.0e-9;
    let rows = transform.affine_rows();
    let x = [rows[0][0], rows[1][0]];
    let y = [rows[0][1], rows[1][1]];
    (x[0] * x[0] + x[1] * x[1] - 1.0).abs() <= EPSILON
        && (y[0] * y[0] + y[1] * y[1] - 1.0).abs() <= EPSILON
        && (x[0] * y[0] + x[1] * y[1]).abs() <= EPSILON
        && (x[0] * y[1] - x[1] * y[0] - 1.0).abs() <= EPSILON
}

impl BodyScope {
    /// Recompute the closure sets from the pruned scope records, so dangling
    /// identities pruned from the records cannot linger in the sets.
    fn pruned(scoped: CadIr) -> Self {
        use std::collections::BTreeSet;
        let model = &scoped.model;
        let coedges = model
            .coedges
            .iter()
            .map(|row| row.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let loops = model
            .loops
            .iter()
            .map(|row| row.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let points = model
            .vertices
            .iter()
            .map(|vertex| vertex.point.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let surfaces = model
            .faces
            .iter()
            .map(|face| face.surface.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let mut curves = model
            .edges
            .iter()
            .filter_map(|edge| edge.curve().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        curves.extend(
            model
                .coedges
                .iter()
                .filter(|row| coedges.contains(row.id.as_str()))
                .filter_map(|row| {
                    row.use_curve
                        .as_ref()
                        .map(|use_| use_.curve.as_str().to_owned())
                }),
        );
        let mut pcurves = model
            .coedges
            .iter()
            .filter(|row| coedges.contains(row.id.as_str()))
            .flat_map(|row| row.pcurves.iter())
            .map(|use_| use_.pcurve.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        pcurves.extend(
            model
                .loops
                .iter()
                .filter(|row| loops.contains(row.id.as_str()))
                .flat_map(cadmpeg_ir::topology::Loop::vertex_pcurves)
                .map(|use_| use_.pcurve.as_str().to_owned()),
        );
        BodyScope {
            ir: scoped,
            points,
            surfaces,
            curves,
            pcurves,
        }
    }
}

/// Strip member references with no scoped record, charging one loss per
/// pruned record or entry (W1 cascade, #217 §5.0). Records that need a
/// removed record are removed; records that list one lose only that entry.
fn prune_dangling_topology(scoped: &mut CadIr, losses: &mut Vec<LossNote>) {
    macro_rules! existing {
        ($arena:ident) => {
            scoped
                .model
                .$arena
                .iter()
                .map(|row| row.id.as_str().to_owned())
                .collect::<std::collections::BTreeSet<_>>()
        };
    }

    let mut pruned = |arena: &str, id: &str, reason: String| {
        losses.push(
            RhinoLossCode::WriterEntityOmitted
                .note(format!("topology {arena} {id} omitted: {reason}")),
        );
    };
    let points = existing!(points);
    let surfaces = existing!(surfaces);
    let curves = existing!(curves);
    let pcurves = existing!(pcurves);

    // Repeat until no record drops: coedges reference their owning loops
    // while loops reference their coedges, so one bottom-up pass cannot
    // settle both directions. Each pass drops at least one record or stops.
    loop {
        let before = (
            scoped.model.vertices.len(),
            scoped.model.edges.len(),
            scoped.model.coedges.len(),
            scoped.model.loops.len(),
            scoped.model.faces.len(),
            scoped.model.shells.len(),
            scoped.model.regions.len(),
        );
        prune_pass(scoped, &mut pruned, &points, &surfaces, &curves, &pcurves);
        let after = (
            scoped.model.vertices.len(),
            scoped.model.edges.len(),
            scoped.model.coedges.len(),
            scoped.model.loops.len(),
            scoped.model.faces.len(),
            scoped.model.shells.len(),
            scoped.model.regions.len(),
        );
        if before == after {
            break;
        }
    }
}

fn prune_pass(
    scoped: &mut CadIr,
    pruned: &mut impl FnMut(&str, &str, String),
    points: &std::collections::BTreeSet<String>,
    surfaces: &std::collections::BTreeSet<String>,
    curves: &std::collections::BTreeSet<String>,
    pcurves: &std::collections::BTreeSet<String>,
) {
    use cadmpeg_ir::topology::{FaceLoops, LoopBoundary};

    macro_rules! existing {
        ($arena:ident) => {
            scoped
                .model
                .$arena
                .iter()
                .map(|row| row.id.as_str().to_owned())
                .collect::<std::collections::BTreeSet<_>>()
        };
    }

    scoped.model.vertices.retain(|vertex| {
        let keep = points.contains(vertex.point.as_str());
        if !keep {
            pruned(
                "vertex",
                vertex.id.as_str(),
                format!("point {} is missing", vertex.point),
            );
        }
        keep
    });
    let vertices = existing!(vertices);
    scoped.model.edges.retain(|edge| {
        let missing = (!vertices.contains(edge.start.as_str()))
            .then(|| format!("vertex {} is missing", edge.start))
            .or_else(|| {
                (!vertices.contains(edge.end.as_str()))
                    .then(|| format!("vertex {} is missing", edge.end))
            })
            .or_else(|| {
                edge.curve()
                    .filter(|id| !curves.contains(id.as_str()))
                    .map(|id| format!("curve {id} is missing"))
            });
        if let Some(missing) = missing {
            pruned("edge", edge.id.as_str(), missing);
            return false;
        }
        true
    });
    let edges = existing!(edges);
    let loops = existing!(loops);
    let coedges = existing!(coedges);
    for coedge in &mut scoped.model.coedges {
        coedge.pcurves.retain(|use_| {
            let keep = pcurves.contains(use_.pcurve.as_str());
            if !keep {
                pruned(
                    "coedge",
                    coedge.id.as_str(),
                    format!("pcurve {} is missing", use_.pcurve),
                );
            }
            keep
        });
        if coedge
            .use_curve
            .as_ref()
            .is_some_and(|use_| !curves.contains(use_.curve.as_str()))
        {
            pruned(
                "coedge",
                coedge.id.as_str(),
                format!(
                    "curve {} is missing",
                    coedge.use_curve.as_ref().expect("checked use").curve
                ),
            );
            coedge.use_curve = None;
        }
    }
    scoped.model.coedges.retain(|coedge| {
        let missing = (!edges.contains(coedge.edge.as_str()))
            .then(|| format!("edge {} is missing", coedge.edge))
            .or_else(|| {
                (!loops.contains(coedge.owner_loop.as_str()))
                    .then(|| format!("loop {} is missing", coedge.owner_loop))
            })
            .or_else(|| {
                (!coedges.contains(coedge.radial_next.as_str()))
                    .then(|| format!("coedge {} is missing", coedge.radial_next))
            });
        if let Some(missing) = missing {
            pruned("coedge", coedge.id.as_str(), missing);
            return false;
        }
        true
    });
    let coedges = existing!(coedges);
    let faces = existing!(faces);
    scoped.model.loops.retain_mut(|row| {
        if !faces.contains(row.face.as_str()) {
            pruned(
                "loop",
                row.id.as_str(),
                format!("face {} is missing", row.face),
            );
            return false;
        }
        match &mut row.boundary {
            LoopBoundary::Vertex {
                vertex,
                pcurves: uses,
            } => {
                if !vertices.contains(vertex.as_str()) {
                    pruned(
                        "loop",
                        row.id.as_str(),
                        format!("vertex {vertex} is missing"),
                    );
                    return false;
                }
                uses.retain(|use_| {
                    let keep = pcurves.contains(use_.pcurve.as_str());
                    if !keep {
                        pruned(
                            "loop",
                            row.id.as_str(),
                            format!("pcurve {} is missing", use_.pcurve),
                        );
                    }
                    keep
                });
                true
            }
            LoopBoundary::Ring(_) => {
                let kept = row
                    .coedges()
                    .iter()
                    .filter(|id| coedges.contains(id.as_str()))
                    .cloned()
                    .collect::<Vec<_>>();
                for id in row.coedges() {
                    if !coedges.contains(id.as_str()) {
                        pruned("loop", row.id.as_str(), format!("coedge {id} is missing"));
                    }
                }
                let mut kept_uses = Vec::new();
                for use_ in row.anchored_vertex_uses() {
                    if !kept.iter().any(|id| id == &use_.after) {
                        pruned(
                            "loop",
                            row.id.as_str(),
                            format!("vertex use after coedge {} is missing", use_.after),
                        );
                        continue;
                    }
                    if !vertices.contains(use_.vertex.as_str()) {
                        pruned(
                            "loop",
                            row.id.as_str(),
                            format!("vertex {} is missing", use_.vertex),
                        );
                        continue;
                    }
                    let mut use_ = use_.clone();
                    use_.pcurves.retain(|p| {
                        let keep = pcurves.contains(p.pcurve.as_str());
                        if !keep {
                            pruned(
                                "loop",
                                row.id.as_str(),
                                format!("pcurve {} is missing", p.pcurve),
                            );
                        }
                        keep
                    });
                    kept_uses.push(use_);
                }
                if row.replace_ring(kept, kept_uses).is_err() {
                    pruned("loop", row.id.as_str(), "no coedge ring remains".to_owned());
                    return false;
                }
                true
            }
        }
    });
    let loops = existing!(loops);
    scoped.model.faces.retain_mut(|row| {
        if !surfaces.contains(row.surface.as_str()) {
            pruned(
                "face",
                row.id.as_str(),
                format!("surface {} is missing", row.surface),
            );
            return false;
        }
        let kept = row
            .loops
            .to_vec()
            .into_iter()
            .filter(|id| loops.contains(id.as_str()));
        let kept = kept.collect::<Vec<_>>();
        for id in row.loops.iter() {
            if !loops.contains(id.as_str()) {
                pruned("face", row.id.as_str(), format!("loop {id} is missing"));
            }
        }
        if kept.is_empty() {
            pruned("face", row.id.as_str(), "no loops remain".to_owned());
            return false;
        }
        row.loops = match &row.loops {
            FaceLoops::Unspecified { .. } => FaceLoops::unspecified(kept),
            FaceLoops::Classified { outer, .. } if kept.contains(outer) => {
                let mut inner = kept;
                inner.retain(|id| id != outer);
                FaceLoops::classified(outer.clone(), inner)
            }
            FaceLoops::Classified { .. } => FaceLoops::unspecified(kept),
        };
        true
    });
    scoped.model.shells.retain_mut(|row| {
        let faces = row
            .faces()
            .iter()
            .filter(|id| existing!(faces).contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let wires = row
            .wire_edges()
            .iter()
            .filter(|id| existing!(edges).contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let free = row
            .free_vertices()
            .iter()
            .filter(|id| existing!(vertices).contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for id in row.faces() {
            if !existing!(faces).contains(id.as_str()) {
                pruned("shell", row.id.as_str(), format!("face {id} is missing"));
            }
        }
        for id in row.wire_edges() {
            if !existing!(edges).contains(id.as_str()) {
                pruned(
                    "shell",
                    row.id.as_str(),
                    format!("wire edge {id} is missing"),
                );
            }
        }
        for id in row.free_vertices() {
            if !existing!(vertices).contains(id.as_str()) {
                pruned(
                    "shell",
                    row.id.as_str(),
                    format!("free vertex {id} is missing"),
                );
            }
        }
        let rebuilt = cadmpeg_ir::topology::Shell::new(
            row.id.clone(),
            row.region.clone(),
            faces,
            wires,
            free,
        );
        match rebuilt {
            Ok(shell) => {
                *row = shell;
                true
            }
            Err(_) => {
                pruned("shell", row.id.as_str(), "no members remain".to_owned());
                false
            }
        }
    });
    let shells = existing!(shells);
    scoped.model.regions.retain_mut(|row| {
        for id in row.shells.clone() {
            if !shells.contains(id.as_str()) {
                pruned("region", row.id.as_str(), format!("shell {id} is missing"));
            }
        }
        row.shells.retain(|id| shells.contains(id.as_str()));
        if row.shells.is_empty() {
            pruned("region", row.id.as_str(), "no shells remain".to_owned());
            return false;
        }
        true
    });
    let regions = existing!(regions);
    for body in &mut scoped.model.bodies {
        for id in body.regions.clone() {
            if !regions.contains(id.as_str()) {
                pruned("body", body.id.as_str(), format!("region {id} is missing"));
            }
        }
        body.regions.retain(|id| regions.contains(id.as_str()));
    }
}
