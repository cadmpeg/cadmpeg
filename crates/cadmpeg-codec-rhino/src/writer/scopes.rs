// SPDX-License-Identifier: Apache-2.0
//! Complete body closures before Rhino capability-specific projection.

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

/// Admit an invalidated document by degrading the invalidated records, so
/// the plan names the record instead of refusing the document (#217 §5.0):
/// drop duplicate identities (the first record kept) and drop records whose
/// ownership back-reference names a missing record, each with a per-record
/// loss. The forward owner lists stay authoritative for the closure, and
/// the existing prune cascade removes the members a dropped record owned.
/// A consistent document passes through borrowed and unchanged.
pub(super) fn admit(ir: &CadIr) -> (std::borrow::Cow<'_, CadIr>, Vec<LossNote>) {
    if !needs_admission(ir) {
        return (std::borrow::Cow::Borrowed(ir), Vec::new());
    }
    let mut admitted = ir.clone();
    let mut losses = Vec::new();
    let model = &mut admitted.model;
    dedupe(&mut model.bodies, "body", &mut losses);
    dedupe(&mut model.regions, "region", &mut losses);
    dedupe(&mut model.shells, "shell", &mut losses);
    dedupe(&mut model.faces, "face", &mut losses);
    dedupe(&mut model.loops, "loop", &mut losses);
    dedupe(&mut model.coedges, "coedge", &mut losses);
    dedupe(&mut model.edges, "edge", &mut losses);
    dedupe(&mut model.vertices, "vertex", &mut losses);
    dedupe(&mut model.points, "point", &mut losses);
    dedupe(&mut model.surfaces, "surface", &mut losses);
    dedupe(&mut model.curves, "curve", &mut losses);
    dedupe(&mut model.pcurves, "pcurve", &mut losses);
    let bodies: std::collections::BTreeSet<String> = model
        .bodies
        .iter()
        .map(|row| row.id.as_str().to_owned())
        .collect();
    let regions: std::collections::BTreeSet<String> = model
        .regions
        .iter()
        .map(|row| row.id.as_str().to_owned())
        .collect();
    let shells: std::collections::BTreeSet<String> = model
        .shells
        .iter()
        .map(|row| row.id.as_str().to_owned())
        .collect();
    drop_dangling_owner(&mut model.regions, "region", &bodies, &mut losses);
    drop_dangling_owner(&mut model.shells, "shell", &regions, &mut losses);
    drop_dangling_owner(&mut model.faces, "face", &shells, &mut losses);
    drop_dangling_bindings(model, &mut losses);
    (std::borrow::Cow::Owned(admitted), losses)
}

/// Whether the document holds a duplicate identity or a dangling ownership
/// back-reference that refusal would otherwise catch.
fn needs_admission(ir: &CadIr) -> bool {
    use std::collections::BTreeSet;
    fn duplicated<'a>(mut ids: impl Iterator<Item = &'a str>) -> bool {
        let mut seen = BTreeSet::new();
        ids.any(|id| !seen.insert(id))
    }
    let model = &ir.model;
    duplicated(model.bodies.iter().map(|row| row.id.as_str()))
        || duplicated(model.regions.iter().map(|row| row.id.as_str()))
        || duplicated(model.shells.iter().map(|row| row.id.as_str()))
        || duplicated(model.faces.iter().map(|row| row.id.as_str()))
        || duplicated(model.loops.iter().map(|row| row.id.as_str()))
        || duplicated(model.coedges.iter().map(|row| row.id.as_str()))
        || duplicated(model.edges.iter().map(|row| row.id.as_str()))
        || duplicated(model.vertices.iter().map(|row| row.id.as_str()))
        || duplicated(model.points.iter().map(|row| row.id.as_str()))
        || duplicated(model.surfaces.iter().map(|row| row.id.as_str()))
        || duplicated(model.curves.iter().map(|row| row.id.as_str()))
        || duplicated(model.pcurves.iter().map(|row| row.id.as_str()))
        || {
            let bodies: BTreeSet<&str> = model.bodies.iter().map(|row| row.id.as_str()).collect();
            let regions: BTreeSet<&str> = model.regions.iter().map(|row| row.id.as_str()).collect();
            let shells: BTreeSet<&str> = model.shells.iter().map(|row| row.id.as_str()).collect();
            model
                .regions
                .iter()
                .any(|row| !bodies.contains(row.body.as_str()))
                || model
                    .shells
                    .iter()
                    .any(|row| !regions.contains(row.region.as_str()))
                || model
                    .faces
                    .iter()
                    .any(|row| !shells.contains(row.shell.as_str()))
                || model.appearance_bindings.iter().any(|row| {
                    binding_target_missing(model, row)
                        || !model
                            .appearances
                            .iter()
                            .any(|appearance| appearance.id == row.appearance)
                })
        }
}

/// Drop later records that repeat an identity, keeping the first.
fn dedupe<T>(rows: &mut Vec<T>, arena: &str, losses: &mut Vec<LossNote>)
where
    T: DedupeIdentity,
{
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    let mut dropped = Vec::new();
    rows.retain(|row| {
        let keep = seen.insert(row.identity().to_owned());
        if !keep {
            dropped.push(row.identity().to_owned());
        }
        keep
    });
    for id in dropped {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "topology {arena} {id} omitted: duplicate identity, first record kept"
        )));
    }
}

/// The identity a topology or geometry record deduplicates on.
trait DedupeIdentity {
    fn identity(&self) -> &str;
}

macro_rules! dedupe_identity {
    ($($ty:ty),*) => {
        $(
            impl DedupeIdentity for $ty {
                fn identity(&self) -> &str {
                    self.id.as_str()
                }
            }
        )*
    };
}

dedupe_identity!(
    cadmpeg_ir::topology::Body,
    cadmpeg_ir::topology::Region,
    cadmpeg_ir::topology::Shell,
    cadmpeg_ir::topology::Face,
    cadmpeg_ir::topology::Loop,
    cadmpeg_ir::topology::Coedge,
    cadmpeg_ir::topology::Edge,
    cadmpeg_ir::topology::Vertex,
    cadmpeg_ir::topology::Point,
    cadmpeg_ir::geometry::Surface,
    cadmpeg_ir::geometry::Curve,
    cadmpeg_ir::geometry::pcurve::Pcurve
);

/// Drop records whose ownership back-reference names a missing owner.
/// Records whose owner exists but does not list them stay refused as
/// malformed: only a dangling reference degrades here.
fn drop_dangling_owner<T>(
    rows: &mut Vec<T>,
    arena: &str,
    owners: &std::collections::BTreeSet<String>,
    losses: &mut Vec<LossNote>,
) where
    T: DanglingOwner,
{
    let mut dropped = Vec::new();
    rows.retain(|row| {
        let keep = owners.contains(row.owner());
        if !keep {
            dropped.push((row.identity().to_owned(), row.owner().to_owned()));
        }
        keep
    });
    for (id, owner) in dropped {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "topology {arena} {id} omitted: owner {owner} is missing"
        )));
    }
}

/// Drop appearance bindings whose target record is absent, naming each.
/// A binding that names no record has no carrier to style; the writer
/// would otherwise drop it silently when no object claims it.
/// The curves a procedural surface construction references; the solved
/// carrier that replaces the construction holds none of them.
pub(super) fn construction_curves(
    model: &cadmpeg_ir::document::Model,
    construction: &str,
) -> Vec<String> {
    fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => out.push(text.clone()),
            serde_json::Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
            serde_json::Value::Object(fields) => {
                fields.values().for_each(|item| strings(item, out));
            }
            _ => {}
        }
    }
    let mut referenced = Vec::new();
    if let Some(definition) = model
        .procedural_surfaces
        .iter()
        .find(|row| row.id.as_str() == construction)
        .and_then(|row| serde_json::to_value(row.definition()).ok())
    {
        strings(&definition, &mut referenced);
    }
    referenced.retain(|id| model.curves.iter().any(|curve| curve.id.as_str() == id));
    referenced.dedup();
    referenced
}

fn drop_dangling_bindings(model: &mut cadmpeg_ir::document::Model, losses: &mut Vec<LossNote>) {
    let mut dropped: Vec<(String, &'static str)> = model
        .appearance_bindings
        .iter()
        .filter(|row| binding_target_missing(model, row))
        .map(|row| (row.id.as_str().to_owned(), "its target record is missing"))
        .collect();
    dropped.extend(
        model
            .appearance_bindings
            .iter()
            .filter(|row| {
                !model
                    .appearances
                    .iter()
                    .any(|appearance| appearance.id == row.appearance)
            })
            .map(|row| {
                (
                    row.id.as_str().to_owned(),
                    "its appearance record is missing",
                )
            }),
    );
    if !dropped.is_empty() {
        let gone: std::collections::BTreeSet<&str> =
            dropped.iter().map(|(id, _)| id.as_str()).collect();
        model
            .appearance_bindings
            .retain(|row| !gone.contains(row.id.as_str()));
    }
    for (id, reason) in dropped {
        losses.push(
            RhinoLossCode::WriterEntityOmitted
                .note(format!("appearance binding {id} omitted: {reason}")),
        );
    }
}

/// Whether an appearance binding names a record absent from the document.
fn binding_target_missing(
    model: &cadmpeg_ir::document::Model,
    row: &cadmpeg_ir::appearance::AppearanceBinding,
) -> bool {
    use cadmpeg_ir::appearance::AppearanceTarget;
    match &row.target {
        AppearanceTarget::Body(id) => !model.bodies.iter().any(|row| row.id == *id),
        AppearanceTarget::Occurrence(id) => !model.occurrences.iter().any(|row| row.id == *id),
        AppearanceTarget::Face(id) => !model.faces.iter().any(|row| row.id == *id),
        AppearanceTarget::Edge(id) => !model.edges.iter().any(|row| row.id == *id),
        AppearanceTarget::Vertex(id) => !model.vertices.iter().any(|row| row.id == *id),
        AppearanceTarget::Surface(id) => !model.surfaces.iter().any(|row| row.id == *id),
        AppearanceTarget::Curve(id) => !model.curves.iter().any(|row| row.id == *id),
        AppearanceTarget::Point(id) => !model.points.iter().any(|row| row.id == *id),
        AppearanceTarget::Tessellation(id) => {
            !model.tessellations.iter().any(|row| row.id.as_str() == id)
        }
        AppearanceTarget::TriangleGroup { tessellation, .. } => !model
            .tessellations
            .iter()
            .any(|row| row.id.as_str() == tessellation),
        AppearanceTarget::Record { arena, id } => !model.contains_record(*arena, id),
    }
}

/// An ownership back-reference a record degrades on when it dangles.
trait DanglingOwner: DedupeIdentity {
    fn owner(&self) -> &str;
}

macro_rules! dangling_owner {
    ($ty:ty, $field:ident) => {
        impl DanglingOwner for $ty {
            fn owner(&self) -> &str {
                self.$field.as_str()
            }
        }
    };
}

dangling_owner!(cadmpeg_ir::topology::Region, body);
dangling_owner!(cadmpeg_ir::topology::Shell, region);
dangling_owner!(cadmpeg_ir::topology::Face, shell);

pub(super) fn collect(ir: &CadIr) -> Result<(Vec<BodyScope>, Vec<LossNote>), super::WriteError> {
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
                for segment in segments {
                    if curves.insert(segment.curve.to_string()) {
                        frontier.push(segment.curve.to_string());
                    }
                }
            }
            if let Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(compound)) =
                curve.geometry.procedural_construction().and_then(|id| {
                    model
                        .procedural_curves
                        .iter()
                        .find(|row| row.id == *id)
                        .map(cadmpeg_ir::geometry::ProceduralCurve::definition)
                })
            {
                for component in compound.components() {
                    if curves.insert(component.component.to_string()) {
                        frontier.push(component.component.to_string());
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
        let member_order = scoped
            .model
            .shells
            .iter()
            .flat_map(cadmpeg_ir::topology::Shell::faces)
            .enumerate()
            .map(|(position, id)| (id.as_str(), position))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut scoped_faces = model
            .faces
            .iter()
            .filter(|entity| faces.contains(entity.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        scoped_faces.sort_by_key(|face| {
            member_order
                .get(face.id.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        scoped.model.faces = scoped_faces;
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
        let mut retained_compounds = BTreeSet::new();
        for curve in &mut scoped.model.curves {
            if let Some(construction) = curve.geometry.procedural_construction() {
                if model.procedural_curves.iter().any(|row| {
                    row.id == *construction
                        && matches!(
                            row.definition(),
                            cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(_)
                        )
                }) {
                    retained_compounds.insert(construction.clone());
                    continue;
                }
                let construction = construction.as_str().to_owned();
                if curve.geometry.solved().is_some() {
                    losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                        "curve {} procedural construction {construction} was reduced to a held solved carrier",
                        curve.id
                    )));
                } else {
                    losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                        "curve {} procedural construction {construction} has no held solved carrier",
                        curve.id
                    )));
                }
            }
            if let Some(cache) = curve.geometry.solved().cloned() {
                curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(cache);
            }
        }
        scoped.model.procedural_curves = model
            .procedural_curves
            .iter()
            .filter(|row| retained_compounds.contains(&row.id))
            .cloned()
            .collect();
        for surface in &mut scoped.model.surfaces {
            if let Some(construction) = surface.geometry.procedural_construction() {
                let construction = construction.as_str().to_owned();
                if surface.geometry.solved().is_some() {
                    let omitted = construction_curves(model, &construction)
                        .into_iter()
                        .filter(|id| !curves.contains(id.as_str()))
                        .collect::<Vec<_>>();
                    let children = if omitted.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "; its construction curve(s) {} are omitted",
                            omitted.join(", ")
                        )
                    };
                    losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                        "surface {} procedural construction {construction} was reduced to a held solved carrier{children}",
                        surface.id
                    )));
                } else {
                    losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                        "surface {} procedural construction {construction} has no held solved carrier",
                        surface.id
                    )));
                }
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
        match super::reduce::validate(&scoped) {
            Ok(()) => {}
            Err(super::WriteError::Omit(reason)) => {
                losses.push(super::omitted_body(&scoped, &reason, &BTreeSet::new()));
                continue;
            }
            Err(error) => return Err(error),
        }
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
    let faces = &scoped.model.faces;
    for surface in &mut scoped.model.surfaces {
        let charged = losses.len();
        resolve_surface(surface, losses);
        if losses.len() > charged {
            let carried = faces
                .iter()
                .filter(|face| face.surface == surface.id)
                .map(|face| face.id.as_str())
                .collect::<Vec<_>>();
            if !carried.is_empty() {
                losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                    "surface {} resolved carrier is held by face(s) {}",
                    surface.id,
                    carried.join(", ")
                )));
            }
        }
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
/// Drop loops whose directed coedge ring does not close, charging one loss
/// per loop: a traversal-broken loop has no valid Brep ring, so the writer
/// degrades at the smallest record instead of refusing the write (§5.2).
/// Loops with dangling references are left to `prune_pass`, which repairs
/// their rings; only fully-resolved rings are judged here.
pub(super) fn prune_open_loops(scoped: &mut CadIr, pruned: &mut impl FnMut(&str, &str, String)) {
    use cadmpeg_ir::topology::Sense;
    use std::collections::{BTreeMap, BTreeSet};
    let edges: BTreeMap<&str, _> = scoped
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect();
    let coedges: BTreeMap<&str, _> = scoped
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect();
    let mut drop = Vec::new();
    'outer: for loop_ in &scoped.model.loops {
        let ring = loop_.coedges();
        if ring.is_empty() {
            continue;
        }
        for (index, id) in ring.iter().enumerate() {
            let (Some(current), Some(next)) = (
                coedges.get(id.as_str()),
                coedges.get(ring[(index + 1) % ring.len()].as_str()),
            ) else {
                continue 'outer;
            };
            let (Some(edge), Some(next_edge)) = (
                edges.get(current.edge.as_str()),
                edges.get(next.edge.as_str()),
            ) else {
                continue 'outer;
            };
            let end = if current.sense == Sense::Forward {
                &edge.end
            } else {
                &edge.start
            };
            let start = if next.sense == Sense::Forward {
                &next_edge.start
            } else {
                &next_edge.end
            };
            if end != start {
                drop.push(loop_.id.as_str().to_owned());
                continue 'outer;
            }
        }
    }
    if drop.is_empty() {
        return;
    }
    let drop: BTreeSet<_> = drop.into_iter().collect();
    scoped.model.loops.retain(|row| {
        let keep = !drop.contains(row.id.as_str());
        if !keep {
            pruned(
                "loop",
                row.id.as_str(),
                "coedge traversal does not close".into(),
            );
        }
        keep
    });
}

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
        prune_invalid_radials(scoped, &mut pruned);
        prune_pass(scoped, &mut pruned, &points, &surfaces, &curves, &pcurves);
        prune_open_loops(scoped, &mut pruned);
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

fn prune_invalid_radials(scoped: &mut CadIr, pruned: &mut impl FnMut(&str, &str, String)) {
    use std::collections::{BTreeMap, BTreeSet};

    let mut groups = BTreeMap::<_, Vec<usize>>::new();
    for (index, coedge) in scoped.model.coedges.iter().enumerate() {
        groups.entry(coedge.edge.clone()).or_default().push(index);
    }
    let mut dropped = BTreeSet::new();
    for (edge, indices) in groups {
        let by_id: BTreeMap<_, _> = indices
            .iter()
            .map(|&index| {
                let coedge = &scoped.model.coedges[index];
                (coedge.id.clone(), coedge.radial_next.clone())
            })
            .collect();
        let start = scoped.model.coedges[indices[0]].id.clone();
        let mut current = start.clone();
        let mut visited = BTreeSet::new();
        while visited.insert(current.clone()) {
            let Some(next) = by_id.get(&current) else {
                break;
            };
            current = next.clone();
        }
        if current == start && visited.len() == indices.len() {
            continue;
        }
        for index in indices {
            let coedge = &scoped.model.coedges[index];
            pruned(
                "coedge",
                coedge.id.as_str(),
                format!("edge {edge} has an invalid radial cycle"),
            );
            dropped.insert(coedge.id.clone());
        }
    }
    scoped
        .model
        .coedges
        .retain(|coedge| !dropped.contains(&coedge.id));
}

/// Drop edges no surviving coedge uses and no shell lists as a wire edge,
/// charging one loss per edge: an unowned edge is pruning debris (a dropped
/// loop's ring edge, for example), not a malformed graph.
pub(super) fn prune_orphaned_edges(
    scoped: &mut CadIr,
    pruned: &mut impl FnMut(&str, &str, String),
) {
    use std::collections::BTreeSet;
    let live: BTreeSet<String> = scoped
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str().to_owned())
        .chain(
            scoped
                .model
                .shells
                .iter()
                .flat_map(|shell| shell.wire_edges().iter().map(|id| id.as_str().to_owned())),
        )
        .collect();
    scoped.model.edges.retain(|edge| {
        let keep = live.contains(edge.id.as_str());
        if !keep {
            pruned(
                "edge",
                edge.id.as_str(),
                "no owning coedge use or wire-edge membership remains".into(),
            );
        }
        keep
    });
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
        if let Some(use_) = coedge
            .use_curve
            .as_ref()
            .filter(|use_| !curves.contains(use_.curve.as_str()))
        {
            pruned(
                "coedge",
                coedge.id.as_str(),
                format!("curve {} is missing", use_.curve),
            );
            coedge.use_curve = None;
        }
    }
    scoped.model.coedges.retain(|coedge| {
        let missing =
            (!edges.contains(coedge.edge.as_str()))
                .then(|| format!("edge {} is missing", coedge.edge))
                .or_else(|| {
                    (!loops.contains(coedge.owner_loop.as_str()))
                        .then(|| format!("loop {} is missing", coedge.owner_loop))
                })
                .or_else(|| {
                    (!scoped.model.loops.iter().any(|row| {
                        row.id == coedge.owner_loop && row.coedges().contains(&coedge.id)
                    }))
                    .then(|| format!("loop {} does not own this coedge", coedge.owner_loop))
                });
        if let Some(missing) = missing {
            pruned("coedge", coedge.id.as_str(), missing);
            return false;
        }
        true
    });
    let coedge_owners = scoped
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.clone(), coedge.owner_loop.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    prune_orphaned_edges(scoped, pruned);
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
        if !scoped
            .model
            .faces
            .iter()
            .any(|face| face.id == row.face && face.loops.iter().any(|id| id == &row.id))
        {
            pruned(
                "loop",
                row.id.as_str(),
                format!("face {} does not own this loop", row.face),
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
                    .filter(|id| coedge_owners.get(*id) == Some(&row.id))
                    .cloned()
                    .collect::<Vec<_>>();
                for id in row.coedges() {
                    if coedge_owners.get(id) != Some(&row.id) {
                        pruned(
                            "loop",
                            row.id.as_str(),
                            format!("coedge {id} is missing or belongs to another loop"),
                        );
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
    let loop_owners = scoped
        .model
        .loops
        .iter()
        .map(|row| (row.id.clone(), row.face.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    scoped.model.faces.retain_mut(|row| {
        if !scoped
            .model
            .shells
            .iter()
            .any(|shell| shell.id == row.shell && shell.faces().contains(&row.id))
        {
            pruned(
                "face",
                row.id.as_str(),
                format!("shell {} does not own this face", row.shell),
            );
            return false;
        }
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
            .filter(|id| loop_owners.get(id) == Some(&row.id));
        let kept = kept.collect::<Vec<_>>();
        for id in &row.loops {
            if loop_owners.get(id) != Some(&row.id) {
                pruned(
                    "face",
                    row.id.as_str(),
                    format!("loop {id} is missing or belongs to another face"),
                );
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
    let face_owners = scoped
        .model
        .faces
        .iter()
        .map(|row| (row.id.clone(), row.shell.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    scoped.model.shells.retain_mut(|row| {
        if !scoped
            .model
            .regions
            .iter()
            .any(|region| region.id == row.region && region.shells.contains(&row.id))
        {
            pruned(
                "shell",
                row.id.as_str(),
                format!("region {} does not own this shell", row.region),
            );
            return false;
        }
        let faces = row
            .faces()
            .iter()
            .filter(|id| face_owners.get(*id) == Some(&row.id))
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
            if face_owners.get(id) != Some(&row.id) {
                pruned(
                    "shell",
                    row.id.as_str(),
                    format!("face {id} is missing or belongs to another shell"),
                );
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
    let shell_owners = scoped
        .model
        .shells
        .iter()
        .map(|row| (row.id.clone(), row.region.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    scoped.model.regions.retain_mut(|row| {
        if !scoped
            .model
            .bodies
            .iter()
            .any(|body| body.id == row.body && body.regions.contains(&row.id))
        {
            pruned(
                "region",
                row.id.as_str(),
                format!("body {} does not own this region", row.body),
            );
            return false;
        }
        for id in row.shells.clone() {
            if shell_owners.get(&id) != Some(&row.id) {
                pruned(
                    "region",
                    row.id.as_str(),
                    format!("shell {id} is missing or belongs to another region"),
                );
            }
        }
        row.shells
            .retain(|id| shell_owners.get(id) == Some(&row.id));
        if row.shells.is_empty() {
            pruned("region", row.id.as_str(), "no shells remain".to_owned());
            return false;
        }
        true
    });
    let region_owners = scoped
        .model
        .regions
        .iter()
        .map(|row| (row.id.clone(), row.body.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    for body in &mut scoped.model.bodies {
        for id in body.regions.clone() {
            if region_owners.get(&id) != Some(&body.id) {
                pruned(
                    "body",
                    body.id.as_str(),
                    format!("region {id} is missing or belongs to another body"),
                );
            }
        }
        body.regions
            .retain(|id| region_owners.get(id) == Some(&body.id));
    }
}
