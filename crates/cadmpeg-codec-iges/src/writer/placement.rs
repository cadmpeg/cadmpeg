// SPDX-License-Identifier: Apache-2.0
//! Body transforms stated in the geometry that each body owns. The decoder
//! reads a Directory transformation back onto a body only from a Type 186, so
//! the writer places every other transformed body into its points, curves and
//! surfaces (CADIR §3.4).

use super::degrade::{naming, Unwritten};
use super::{
    curve_reference_span, edge_span, flatten_curve, is_decoder_free_geometry_body, lifted_pcurve,
    pcurve_orientation_context, rigid_placement, solved_curve, topology_edge_explicit_tolerance,
    TopologyIndex, UseDirection,
};
use crate::loss::IgesLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
    LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::geometry::sampled::GeometryLayoutError;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PointId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, IncreasingParameterInterval, LoopBoundary, LoopRing,
    ParameterInterval, PcurveUse, Sense, ShellMember,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// Relative bound under which a linear map counts as a similarity: the bound
/// that `Transform::is_proper_rigid` applies to a rigid motion.
const SIMILARITY_TOLERANCE: f64 = 1.0e-9;

/// Bound on the determinant of a linear map, relative to the product of its
/// column lengths, under which the map is singular.
const SINGULARITY_TOLERANCE: f64 = 1.0e-12;

/// The transform that the writer places into the geometry of `body`. A solid
/// keeps a rigid transform as the Type 124 of its Type 186.
fn placed_transform(body: &Body) -> Option<Transform> {
    let transform = body
        .transform
        .filter(|transform| *transform != Transform::identity())?;
    let kept = body.kind == BodyKind::Solid
        && !is_decoder_free_geometry_body(body)
        && rigid_placement(transform).is_some();
    (!kept).then_some(transform)
}

/// The records that one body owns, by arena.
#[derive(Default)]
struct Owned<'ir> {
    points: BTreeSet<&'ir str>,
    curves: BTreeSet<&'ir str>,
    surfaces: BTreeSet<&'ir str>,
    faces: BTreeSet<&'ir str>,
    edges: BTreeSet<&'ir str>,
    vertices: BTreeSet<&'ir str>,
    coedges: BTreeSet<&'ir str>,
}

fn owned<'ir>(ir: &'ir CadIr, body: &Body) -> Owned<'ir> {
    let geometry = &ir.geometry;
    let mut owned = Owned::default();
    for region in geometry.stated_body_regions(&body.id) {
        for shell_id in region.stated_shells() {
            for shell in geometry.shells.iter().filter(|shell| shell.id == *shell_id) {
                for member in &shell.members {
                    match member {
                        ShellMember::WireEdge { id } => {
                            owned.edges.insert(id.as_str());
                        }
                        ShellMember::FreeVertex { id } => {
                            owned.vertices.insert(id.as_str());
                        }
                        ShellMember::Subd { .. } => {}
                    }
                }
            }
            for face in geometry.stated_shell_faces(shell_id) {
                owned.faces.insert(face.id.as_str());
                owned.surfaces.insert(face.surface.as_str());
                for loop_ in face
                    .loops
                    .stated()
                    .filter_map(|id| geometry.loops.iter().find(|loop_| loop_.id == *id))
                {
                    owned
                        .vertices
                        .extend(loop_.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
                    for coedge in loop_
                        .coedges()
                        .iter()
                        .filter_map(|id| geometry.coedges.iter().find(|coedge| coedge.id == *id))
                    {
                        owned.coedges.insert(coedge.id.as_str());
                        owned.edges.insert(coedge.edge.as_str());
                        if let Some(use_curve) = &coedge.use_curve {
                            owned.curves.insert(use_curve.curve.as_str());
                        }
                    }
                }
            }
        }
    }
    for edge in geometry
        .edges
        .iter()
        .filter(|edge| owned.edges.contains(edge.id.as_str()))
    {
        owned
            .vertices
            .extend(edge.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
        if let Some(curve) = edge.curve() {
            owned.curves.insert(curve.as_str());
        }
    }
    let mut pending = owned.curves.iter().copied().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let Some(SolvedCurveGeometry::Composite { segments, .. }) = geometry
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| curve.geometry.solved())
        else {
            continue;
        };
        for segment in segments {
            if owned.curves.insert(segment.curve.as_str()) {
                pending.push(segment.curve.as_str());
            }
        }
    }
    owned.points.extend(
        geometry
            .vertices
            .iter()
            .filter(|vertex| owned.vertices.contains(vertex.id.as_str()))
            .map(|vertex| vertex.point.as_str()),
    );
    owned
}

/// The placed bodies and, for each record they own, its transform and body.
#[derive(Default)]
struct Placements<'ir> {
    bodies: BTreeMap<&'ir str, Transform>,
    points: BTreeMap<&'ir str, (Transform, &'ir str)>,
    curves: BTreeMap<&'ir str, (Transform, &'ir str)>,
    surfaces: BTreeMap<&'ir str, (Transform, &'ir str)>,
    faces: BTreeSet<&'ir str>,
}

impl<'ir> Placements<'ir> {
    fn of(ir: &'ir CadIr) -> Result<Self, Unwritten> {
        let mut placements = Self {
            bodies: ir
                .geometry
                .bodies
                .iter()
                .filter_map(|body| Some((body.id.as_str(), placed_transform(body)?)))
                .collect(),
            ..Self::default()
        };
        if placements.bodies.is_empty() {
            return Ok(placements);
        }
        let mut owners = BTreeMap::<(&str, &str), (Option<Transform>, &str)>::new();
        for body in &ir.geometry.bodies {
            let body_id = body.id.as_str();
            let transform = placements.bodies.get(body_id).copied();
            let owned = owned(ir, body);
            for (arena, ids) in [
                ("point", &owned.points),
                ("curve", &owned.curves),
                ("surface", &owned.surfaces),
            ] {
                for id in ids {
                    if let Some((other, other_body)) =
                        owners.insert((arena, id), (transform, body_id))
                    {
                        if other != transform {
                            return Err(naming(id)(CodecError::NotImplemented(format!(
                                "IGES writer places the transform of body {body_id} into its \
                                 geometry, and {arena} {id} is shared with body {other_body} \
                                 under another transform"
                            ))));
                        }
                    }
                }
            }
            let Some(transform) = transform else {
                continue;
            };
            for (ids, map) in [
                (&owned.points, &mut placements.points),
                (&owned.curves, &mut placements.curves),
                (&owned.surfaces, &mut placements.surfaces),
            ] {
                map.extend(ids.iter().map(|id| (*id, (transform, body_id))));
            }
            placements.faces.extend(&owned.faces);
        }
        Ok(placements)
    }
}

/// The document with a copy, for each body, of each point, curve and surface
/// that the body shares with a body under another transform, and the body's
/// references moved to its copies. Fails naming a vertex, edge, coedge,
/// face or composite curve that two such bodies share.
fn separated_bodies(ir: &CadIr) -> Result<Cow<'_, CadIr>, Unwritten> {
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| (body, placed_transform(body), owned(ir, body)))
        .collect::<Vec<_>>();
    if bodies.iter().all(|(_, transform, _)| transform.is_none()) {
        return Ok(Cow::Borrowed(ir));
    }
    let mut owners = BTreeMap::<(&str, &str), Option<Transform>>::new();
    let mut topology = BTreeMap::<(&'static str, String), BTreeSet<&str>>::new();
    let mut copies = Vec::new();
    for (body, transform, owned) in &bodies {
        let mut shared = BTreeMap::new();
        for (arena, ids) in [
            ("point", &owned.points),
            ("curve", &owned.curves),
            ("surface", &owned.surfaces),
        ] {
            for id in ids {
                match owners.get(&(arena, *id)) {
                    None => {
                        owners.insert((arena, *id), *transform);
                    }
                    Some(first) if first != transform => {
                        let copy = format!(
                            "iges:writer:{arena}#{}@{}",
                            id.replace('#', "/"),
                            body.id.as_str().replace('#', "/")
                        );
                        shared.insert(*id, copy);
                    }
                    Some(_) => {}
                }
            }
        }
        for (arena, ids) in [
            ("vertex", &owned.vertices),
            ("edge", &owned.edges),
            ("coedge", &owned.coedges),
            ("face", &owned.faces),
            ("curve", &owned.curves),
        ] {
            for id in ids {
                topology
                    .entry((arena, (*id).to_owned()))
                    .or_default()
                    .insert(body.id.as_str());
            }
        }
        if !shared.is_empty() {
            copies.push((body.id.as_str(), owned, shared));
        }
    }
    if copies.is_empty() {
        return Ok(Cow::Borrowed(ir));
    }
    let mut separated = ir.clone();
    let geometry = &mut separated.geometry;
    for (body, owned, shared) in copies {
        let failure = |record: &str, reason: &str| {
            naming(record)(CodecError::NotImplemented(format!(
                "IGES writer places the transform of body {body} into its geometry, and {reason}"
            )))
        };
        let exclusive = |arena: &'static str, id: &str| {
            topology
                .get(&(arena, id.to_owned()))
                .is_none_or(|owners| owners.len() == 1)
        };
        let invalid = |id: &str| failure(id, &format!("the copy of {id} has no valid identity"));
        for point in ir
            .geometry
            .points
            .iter()
            .filter(|point| shared.contains_key(point.id.as_str()))
        {
            let mut copy = point.clone();
            copy.id = PointId::try_from(shared[point.id.as_str()].clone())
                .map_err(|_| invalid(point.id.as_str()))?;
            geometry.points.push(copy);
        }
        for curve in ir
            .geometry
            .curves
            .iter()
            .filter(|curve| shared.contains_key(curve.id.as_str()))
        {
            let mut copy = curve.clone();
            copy.id = CurveId::try_from(shared[curve.id.as_str()].clone())
                .map_err(|_| invalid(curve.id.as_str()))?;
            geometry.curves.push(copy);
        }
        for surface in ir
            .geometry
            .surfaces
            .iter()
            .filter(|surface| shared.contains_key(surface.id.as_str()))
        {
            let mut copy = surface.clone();
            copy.id = SurfaceId::try_from(shared[surface.id.as_str()].clone())
                .map_err(|_| invalid(surface.id.as_str()))?;
            geometry.surfaces.push(copy);
        }
        let moved = |arena: &'static str,
                     record: &str,
                     reference: &str|
         -> Result<Option<String>, Unwritten> {
            let Some(copy) = shared.get(reference) else {
                return Ok(None);
            };
            if !exclusive(arena, record) {
                return Err(failure(
                    record,
                    &format!("{arena} {record} is shared with a body under another transform"),
                ));
            }
            Ok(Some(copy.clone()))
        };
        for vertex in geometry
            .vertices
            .iter_mut()
            .filter(|vertex| owned.vertices.contains(vertex.id.as_str()))
        {
            if let Some(copy) = moved("vertex", vertex.id.as_str(), vertex.point.as_str())? {
                vertex.point = PointId::try_from(copy).map_err(|_| invalid(vertex.id.as_str()))?;
            }
        }
        for edge in geometry
            .edges
            .iter_mut()
            .filter(|edge| owned.edges.contains(edge.id.as_str()))
        {
            let Some(curve) = edge.curve() else {
                continue;
            };
            if let Some(copy) = moved("edge", edge.id.as_str(), curve.as_str())? {
                let copy = CurveId::try_from(copy).map_err(|_| invalid(edge.id.as_str()))?;
                edge.map_curve(|_| copy);
            }
        }
        for coedge in geometry
            .coedges
            .iter_mut()
            .filter(|coedge| owned.coedges.contains(coedge.id.as_str()))
        {
            let Some(use_curve) = &mut coedge.use_curve else {
                continue;
            };
            if let Some(copy) = moved("coedge", coedge.id.as_str(), use_curve.curve.as_str())? {
                use_curve.curve =
                    CurveId::try_from(copy).map_err(|_| invalid(coedge.id.as_str()))?;
            }
        }
        for face in geometry
            .faces
            .iter_mut()
            .filter(|face| owned.faces.contains(face.id.as_str()))
        {
            if let Some(copy) = moved("face", face.id.as_str(), face.surface.as_str())? {
                face.surface = SurfaceId::try_from(copy).map_err(|_| invalid(face.id.as_str()))?;
            }
        }
        // Each composite of the body by its identity in the separated
        // document, with the identity of its source record.
        let composites = owned
            .curves
            .iter()
            .filter(|id| !shared.contains_key(**id))
            .map(|id| ((*id).to_owned(), (*id).to_owned()))
            .chain(
                shared
                    .iter()
                    .map(|(source, copy)| (copy.clone(), (*source).to_owned())),
            )
            .collect::<BTreeMap<_, _>>();
        for curve in &mut geometry.curves {
            let Some(source) = composites.get(curve.id.as_str()) else {
                continue;
            };
            let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
                &mut curve.geometry
            else {
                continue;
            };
            let own_copy = shared.contains_key(source.as_str());
            for segment in segments.iter_mut() {
                let Some(copy) = shared.get(segment.curve.as_str()) else {
                    continue;
                };
                if !own_copy && !exclusive("curve", source.as_str()) {
                    return Err(failure(
                        source.as_str(),
                        &format!(
                            "composite curve {source} is shared with a body under another \
                             transform"
                        ),
                    ));
                }
                segment.curve =
                    CurveId::try_from(copy.clone()).map_err(|_| invalid(source.as_str()))?;
            }
        }
    }
    Ok(Cow::Owned(separated))
}

/// The cylinder, cone, sphere and torus supports of bodies that the writer
/// places under a transform that is not a similarity. The exact image of
/// each is a rational B-spline surface.
pub(super) fn dissimilar_supports(ir: &CadIr) -> BTreeSet<String> {
    let mut supports = BTreeSet::new();
    for body in &ir.geometry.bodies {
        let Some(transform) = placed_transform(body) else {
            continue;
        };
        if Linear::of(transform).similarity.is_some() {
            continue;
        }
        for surface in owned(ir, body).surfaces {
            if ir.geometry.surfaces.iter().any(|record| {
                record.id.as_str() == surface
                    && matches!(
                        record.geometry.solved(),
                        Some(
                            SolvedSurfaceGeometry::Cylinder(_)
                                | SolvedSurfaceGeometry::Cone(_)
                                | SolvedSurfaceGeometry::Sphere(_)
                                | SolvedSurfaceGeometry::Torus(_)
                        )
                    )
            }) {
                supports.insert(surface.to_owned());
            }
        }
    }
    supports
}

/// The document with the transform of each body that IGES does not state on
/// the body placed into the geometry that the body owns, and a
/// `procedural.reduced` note for each procedural carrier that the placement
/// states by its solved form. Geometry that a body shares with a body under
/// another transform is placed as a copy for each body.
pub(super) fn placed_bodies(ir: &CadIr) -> Result<(Cow<'_, CadIr>, Vec<LossNote>), Unwritten> {
    let separated = separated_bodies(ir)?;
    match placed(separated.as_ref())? {
        Some((placed, losses)) => Ok((Cow::Owned(placed), losses)),
        None => Ok((separated, Vec::new())),
    }
}

fn placed(ir: &CadIr) -> Result<Option<(CadIr, Vec<LossNote>)>, Unwritten> {
    let placements = Placements::of(ir)?;
    if placements.bodies.is_empty() {
        return Ok(None);
    }
    if let Some(body) = placements
        .bodies
        .iter()
        .find_map(|(body, transform)| Linear::of(*transform).singular().then_some(*body))
    {
        return Err(naming(body)(CodecError::NotImplemented(format!(
            "IGES writer places the transform of body {body} into its geometry, and the \
             transform is singular, so the geometry has no image of its dimension"
        ))));
    }
    let failure = |record: &str, body: &str, reason: String| {
        naming(record)(CodecError::NotImplemented(format!(
            "IGES writer places the transform of body {body} into its geometry, and {reason}"
        )))
    };
    let mut placed = ir.clone();
    let mut losses = Vec::new();
    let mut reduced = BTreeSet::new();

    let composite = |geometry: &CurveGeometry| {
        matches!(
            geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
        )
    };
    let mut curve_maps = BTreeMap::<String, (ParameterMap, &str)>::new();
    for curve in &mut placed.geometry.curves {
        let Some(&(transform, body)) = placements.curves.get(curve.id.as_str()) else {
            continue;
        };
        if composite(&curve.geometry) {
            continue;
        }
        let (geometry, map, construction) =
            placed_curve(&curve.geometry, transform).map_err(|reason| {
                failure(
                    curve.id.as_str(),
                    body,
                    format!("curve {} {reason}", curve.id),
                )
            })?;
        if let Some(range) = curve.parameter_range {
            curve.parameter_range = Some(
                IncreasingParameterInterval::new(map.interval(range.endpoints())).ok_or_else(
                    || {
                        failure(
                            curve.id.as_str(),
                            body,
                            format!("curve {} has no placed range", curve.id),
                        )
                    },
                )?,
            );
        }
        curve.geometry = geometry;
        if let Some(construction) = construction {
            losses.push(IgesLossCode::ProceduralReduced.note(format!(
                "IGES writer states procedural curve {construction} of body {body} by its \
                 placed solved form"
            )));
            reduced.insert(construction);
        }
        curve_maps.insert(curve.id.as_str().to_owned(), (map, body));
    }
    let mut pending = placed
        .geometry
        .curves
        .iter()
        .enumerate()
        .filter(|(_, curve)| {
            composite(&curve.geometry) && placements.curves.contains_key(curve.id.as_str())
        })
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    while let Some(&first) = pending.first() {
        let mut waiting = Vec::new();
        for &position in &pending {
            let curve = &placed.geometry.curves[position];
            let (_, body) = placements.curves[curve.id.as_str()];
            let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
                &curve.geometry
            else {
                continue;
            };
            if segments
                .iter()
                .any(|segment| !curve_maps.contains_key(segment.curve.as_str()))
            {
                waiting.push(position);
                continue;
            }
            let map = composite_map(ir, segments, &curve_maps).map_err(|reason| {
                failure(
                    curve.id.as_str(),
                    body,
                    format!("composite curve {} {reason}", curve.id),
                )
            })?;
            let range = curve
                .parameter_range
                .map(|range| {
                    IncreasingParameterInterval::new(map.interval(range.endpoints())).ok_or_else(
                        || {
                            failure(
                                curve.id.as_str(),
                                body,
                                format!("curve {} has no placed range", curve.id),
                            )
                        },
                    )
                })
                .transpose()?;
            let id = curve.id.as_str().to_owned();
            placed.geometry.curves[position].parameter_range = range;
            curve_maps.insert(id, (map, body));
        }
        if waiting.len() == pending.len() {
            let curve = &placed.geometry.curves[first];
            let (_, body) = placements.curves[curve.id.as_str()];
            return Err(failure(
                curve.id.as_str(),
                body,
                format!(
                    "composite curve {} has a child that the writer does not place",
                    curve.id
                ),
            ));
        }
        pending = waiting;
    }
    for point in &mut placed.geometry.points {
        let Some(&(transform, body)) = placements.points.get(point.id.as_str()) else {
            continue;
        };
        let position = transform
            .apply_point(point.position().get())
            .ok_or_else(|| {
                failure(
                    point.id.as_str(),
                    body,
                    format!("point {} leaves the finite range", point.id),
                )
            })?;
        point.set_position(position);
    }
    for edge in &mut placed.geometry.edges {
        let (Some(curve), Some(range)) = (edge.curve(), edge.param_range()) else {
            continue;
        };
        let Some((map, body)) = curve_maps.get(curve.as_str()) else {
            continue;
        };
        let range = ParameterInterval::new(map.interval(range.get())).map_err(|_| {
            failure(
                edge.id.as_str(),
                body,
                format!("edge {} has no placed range", edge.id),
            )
        })?;
        edge.set_param_range(Some(range));
    }
    for coedge in &mut placed.geometry.coedges {
        let Some(use_curve) = &coedge.use_curve else {
            continue;
        };
        let Some((map, body)) = curve_maps.get(use_curve.curve.as_str()) else {
            continue;
        };
        let range = ParameterInterval::new(map.interval(use_curve.parameter_range.endpoints()))
            .map_err(|_| {
                failure(
                    coedge.id.as_str(),
                    body,
                    format!("coedge {} has no placed range", coedge.id),
                )
            })?;
        if let Some(use_curve) = &mut coedge.use_curve {
            use_curve.parameter_range = range;
        }
    }

    let mut surface_maps = BTreeMap::new();
    for surface in &mut placed.geometry.surfaces {
        let Some(&(transform, body)) = placements.surfaces.get(surface.id.as_str()) else {
            continue;
        };
        let (geometry, map, construction) =
            placed_surface(&surface.geometry, transform).map_err(|reason| {
                failure(
                    surface.id.as_str(),
                    body,
                    format!("surface {} {reason}", surface.id),
                )
            })?;
        surface.geometry = geometry;
        if let Some(construction) = construction {
            losses.push(IgesLossCode::ProceduralReduced.note(format!(
                "IGES writer states procedural surface {construction} of body {body} by its \
                 placed solved form"
            )));
            reduced.insert(construction);
        }
        surface_maps.insert(surface.id.as_str().to_owned(), (map, transform, body));
    }
    let constructions = placed
        .geometry
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            CurveGeometry::Procedural { construction, .. } => Some(construction.as_str()),
            CurveGeometry::Solved(_) => None,
        })
        .chain(
            placed
                .geometry
                .surfaces
                .iter()
                .filter_map(|surface| match &surface.geometry {
                    SurfaceGeometry::Procedural { construction, .. } => Some(construction.as_str()),
                    SurfaceGeometry::Solved(_) => None,
                }),
        )
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    reduced.retain(|construction| !constructions.contains(construction));
    placed
        .geometry
        .procedural_curves
        .retain(|procedural| !reduced.contains(procedural.id.as_str()));
    placed
        .geometry
        .procedural_surfaces
        .retain(|procedural| !reduced.contains(procedural.id.as_str()));

    // A reflection turns each loop against its face's placed normal, so the
    // writer reverses the loops of a reflected face.
    let mut reflected_loops = BTreeMap::new();
    let mut pcurve_maps = BTreeMap::<String, (UvMap, &str)>::new();
    for face in &mut placed.geometry.faces {
        if !placements.faces.contains(face.id.as_str()) {
            continue;
        }
        let Some(&(map, transform, body)) = surface_maps.get(face.surface.as_str()) else {
            continue;
        };
        let reflected = Linear::of(transform).determinant < 0.0;
        if reflected != (map.determinant() < 0.0) {
            face.sense = opposite(face.sense);
        }
        for loop_ in face
            .loops
            .stated()
            .filter_map(|id| ir.geometry.loops.iter().find(|loop_| loop_.id == *id))
        {
            if reflected {
                reflected_loops.insert(loop_.id.as_str(), body);
            }
            for pcurve in loop_pcurves(ir, loop_) {
                if let Some((other, _)) = pcurve_maps.insert(pcurve.to_owned(), (map, body)) {
                    if other != map {
                        return Err(failure(
                            pcurve,
                            body,
                            format!("pcurve {pcurve} has two placed images"),
                        ));
                    }
                }
            }
        }
    }
    let against = against_edge_chains(ir, &reflected_loops)?;
    let unparametric = |uses: &mut [PcurveUse]| {
        for use_ in uses {
            if pcurve_maps
                .get(use_.pcurve.as_str())
                .is_some_and(|(map, _)| !map.is_diagonal())
            {
                use_.isoparametric = None;
            }
        }
    };
    for loop_ in &mut placed.geometry.loops {
        let reflected = reflected_loops.get(loop_.id.as_str()).copied();
        let id = loop_.id.as_str().to_owned();
        let flip = |uses: &[PcurveUse], body: &str| {
            flipped(ir, uses)
                .map_err(|reason| failure(&id, body, format!("reflected loop {id} {reason}")))
        };
        match &mut loop_.boundary {
            LoopBoundary::Vertex { pcurves, .. } => {
                unparametric(pcurves.as_mut_slice());
                if let Some(body) = reflected {
                    *pcurves = flip(pcurves.as_slice(), body)?;
                }
            }
            LoopBoundary::Ring(ring) => {
                let coedges = ring.coedges().to_vec();
                let mut vertex_uses = ring.vertex_uses().to_vec();
                for vertex_use in &mut vertex_uses {
                    unparametric(vertex_use.pcurves.as_mut_slice());
                }
                let (coedges, vertex_uses) = match reflected {
                    None => (coedges, vertex_uses),
                    Some(body) => {
                        // The pole after coedge `i` lies after coedge `i + 1` in the
                        // reversed ring.
                        let count = coedges.len();
                        let mut anchored = Vec::new();
                        for position in (0..count).rev() {
                            for vertex_use in vertex_uses
                                .iter()
                                .rev()
                                .filter(|vertex_use| vertex_use.after == coedges[position])
                            {
                                anchored.push(AnchoredVertexUse {
                                    vertex: vertex_use.vertex.clone(),
                                    after: coedges[(position + 1) % count].clone(),
                                    pcurves: flip(vertex_use.pcurves.as_slice(), body)?,
                                });
                            }
                        }
                        (coedges.into_iter().rev().collect(), anchored)
                    }
                };
                *ring = LoopRing::new(coedges, vertex_uses).map_err(|error| {
                    failure(
                        &id,
                        reflected.unwrap_or_default(),
                        format!("placed loop {id} is invalid: {error}"),
                    )
                })?;
            }
        }
    }
    for coedge in &mut placed.geometry.coedges {
        unparametric(coedge.pcurves.as_mut_slice());
        let Some(&body) = reflected_loops.get(coedge.owner_loop.as_str()) else {
            continue;
        };
        coedge.sense = opposite(coedge.sense);
        if against.contains(coedge.id.as_str()) {
            let pcurves = flipped(ir, &coedge.pcurves).map_err(|reason| {
                failure(
                    coedge.id.as_str(),
                    body,
                    format!("reflected coedge {} {reason}", coedge.id),
                )
            })?;
            coedge.pcurves = pcurves;
        }
    }
    for pcurve in &mut placed.geometry.pcurves {
        let Some(&(map, body)) = pcurve_maps.get(pcurve.id.as_str()) else {
            continue;
        };
        if map == UvMap::IDENTITY {
            continue;
        }
        *pcurve = mapped_pcurve(pcurve, map).map_err(|reason| {
            failure(
                pcurve.id.as_str(),
                body,
                format!("pcurve {} {reason}", pcurve.id),
            )
        })?;
    }
    for body in &mut placed.geometry.bodies {
        if placements.bodies.contains_key(body.id.as_str()) {
            body.transform = None;
        }
    }
    Ok(Some((placed, losses)))
}

/// The parameter curves that `loop_` uses.
fn loop_pcurves<'ir>(ir: &'ir CadIr, loop_: &'ir cadmpeg_ir::topology::Loop) -> Vec<&'ir str> {
    match &loop_.boundary {
        LoopBoundary::Vertex { pcurves, .. } => {
            pcurves.iter().map(|use_| use_.pcurve.as_str()).collect()
        }
        LoopBoundary::Ring(ring) => ring
            .coedges()
            .iter()
            .filter_map(|id| ir.geometry.coedges.iter().find(|coedge| coedge.id == *id))
            .flat_map(|coedge| coedge.pcurves.iter())
            .chain(
                ring.vertex_uses()
                    .iter()
                    .flat_map(|use_| use_.pcurves.iter()),
            )
            .map(|use_| use_.pcurve.as_str())
            .collect(),
    }
}

/// The reversed coedges of `loops` whose parameter curve chain runs in the
/// coedge's traversal, against its edge curve. A reflection makes each such
/// coedge forward, so its chain must then run along the edge curve.
fn against_edge_chains(
    ir: &CadIr,
    loops: &BTreeMap<&str, &str>,
) -> Result<BTreeSet<String>, Unwritten> {
    let mut against = BTreeSet::new();
    if loops.is_empty() {
        return Ok(against);
    }
    let index = TopologyIndex::new(ir);
    for coedge in ir.geometry.coedges.iter().filter(|coedge| {
        loops.contains_key(coedge.owner_loop.as_str())
            && coedge.sense == Sense::Reversed
            && !coedge.pcurves.is_empty()
    }) {
        let missing = || {
            naming(coedge.id.as_str())(CodecError::malformed(format_args!(
                "IGES reflected coedge {} references a missing record",
                coedge.id
            )))
        };
        let surface = index
            .loops
            .get(coedge.owner_loop.as_str())
            .and_then(|loop_| index.faces.get(loop_.face.as_str()))
            .and_then(|face| index.surfaces.get(face.surface.as_str()))
            .copied()
            .ok_or_else(missing)?;
        let edge = index
            .edges
            .get(coedge.edge.as_str())
            .copied()
            .ok_or_else(missing)?;
        let curve = edge
            .curve()
            .and_then(|curve| index.curves.get(curve.as_str()))
            .copied()
            .ok_or_else(missing)?;
        let orientation = solved_curve(curve)
            .and_then(flatten_curve)
            .and_then(|geometry| edge_span(&index, edge, &geometry))
            .and_then(|span| {
                pcurve_orientation_context(
                    &index,
                    &surface.geometry,
                    span.start,
                    span.end,
                    UseDirection::traversed(coedge.sense),
                    topology_edge_explicit_tolerance(&index, edge),
                    coedge.id.as_str(),
                )
                .orientation(&coedge.pcurves)
            })
            .map_err(naming(coedge.id.as_str()))?;
        if orientation == Sense::Reversed {
            against.insert(coedge.id.as_str().to_owned());
        }
    }
    Ok(against)
}

/// `uses` traversed the other way: in reverse order, each use directed
/// against its previous direction.
fn flipped(ir: &CadIr, uses: &[PcurveUse]) -> Result<Vec<PcurveUse>, String> {
    uses.iter()
        .rev()
        .map(|use_| {
            let [start, end] = match use_.parameter_range {
                Some(range) => range.endpoints(),
                None => ir
                    .geometry
                    .pcurves
                    .iter()
                    .find(|pcurve| pcurve.id == use_.pcurve)
                    .and_then(Pcurve::parameter_range)
                    .ok_or_else(|| format!("uses pcurve {} with no parameter range", use_.pcurve))?
                    .get(),
            };
            Ok(PcurveUse {
                parameter_range: Some(
                    DirectedParameterRange::new([end, start]).map_err(|error| error.to_string())?,
                ),
                ..use_.clone()
            })
        })
        .collect()
}

/// The placed composite parameter of each composite parameter, which runs
/// over the reference spans of the children in turn.
fn composite_map(
    ir: &CadIr,
    segments: &[cadmpeg_ir::geometry::CompositeCurveSegment],
    maps: &BTreeMap<String, (ParameterMap, &str)>,
) -> Result<ParameterMap, String> {
    let mut breaks = vec![[0.0, 0.0]];
    for segment in segments {
        let child = ir
            .geometry
            .curves
            .iter()
            .find(|curve| curve.id == segment.curve)
            .ok_or_else(|| format!("references missing child {}", segment.curve))?;
        let geometry = solved_curve(child)
            .and_then(flatten_curve)
            .map_err(|error| error.to_string())?;
        let span = curve_reference_span(ir, &segment.curve, &geometry)
            .map_err(|error| error.to_string())?;
        let [start, end] = span.range.finite_components().map(FiniteReal::get);
        let (map, _) = maps
            .get(segment.curve.as_str())
            .ok_or_else(|| format!("has unplaced child {}", segment.curve))?;
        let [placed_start, placed_end] = map.interval([start, end]);
        let [source, target] = breaks.last().copied().unwrap_or([0.0, 0.0]);
        breaks.push([
            source + (end - start),
            target + (placed_end - placed_start).abs(),
        ]);
    }
    Ok(ParameterMap::Piecewise(breaks))
}

pub(super) const fn opposite(sense: Sense) -> Sense {
    match sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}

/// `value`, or `target` where the two agree within the similarity bound.
fn snapped(value: f64, target: f64) -> f64 {
    if (value - target).abs() <= SIMILARITY_TOLERANCE * target.abs().max(1.0) {
        target
    } else {
        value
    }
}

/// The linear part of a transform, by the images of the unit axes.
struct Linear {
    columns: [Vector3; 3],
    determinant: f64,
    /// The scale of a similarity.
    similarity: Option<f64>,
}

impl Linear {
    fn of(transform: Transform) -> Self {
        let rows = transform.affine_rows();
        let columns =
            [0, 1, 2].map(|column| Vector3::new(rows[0][column], rows[1][column], rows[2][column]));
        let determinant = columns[0].dot(columns[1].cross(columns[2]));
        let squares = columns.map(|column| column.dot(column));
        let mean = (squares[0] + squares[1] + squares[2]) / 3.0;
        let bound = SIMILARITY_TOLERANCE * mean;
        let similar = mean > 0.0
            && squares.iter().all(|square| (square - mean).abs() <= bound)
            && columns[0].dot(columns[1]).abs() <= bound
            && columns[0].dot(columns[2]).abs() <= bound
            && columns[1].dot(columns[2]).abs() <= bound;
        Self {
            columns,
            determinant,
            similarity: similar.then(|| snapped(mean.sqrt(), 1.0)),
        }
    }

    /// Whether the map collapses a direction.
    fn singular(&self) -> bool {
        let bound = self.columns.iter().map(Vector3::norm).product::<f64>();
        self.determinant.abs() <= SINGULARITY_TOLERANCE * bound
    }

    /// The largest singular value: the square root of the largest eigenvalue
    /// of the symmetric `MᵀM`, by the trigonometric solution of its
    /// characteristic cubic.
    fn largest_stretch(&self) -> f64 {
        let [x, y, z] = self.columns;
        let gram = [
            [x.dot(x), x.dot(y), x.dot(z)],
            [y.dot(x), y.dot(y), y.dot(z)],
            [z.dot(x), z.dot(y), z.dot(z)],
        ];
        let mean = (gram[0][0] + gram[1][1] + gram[2][2]) / 3.0;
        let off = gram[0][1].powi(2) + gram[0][2].powi(2) + gram[1][2].powi(2);
        let spread = ((gram[0][0] - mean).powi(2)
            + (gram[1][1] - mean).powi(2)
            + (gram[2][2] - mean).powi(2)
            + 2.0 * off)
            / 6.0;
        if spread <= 0.0 {
            return mean.max(0.0).sqrt();
        }
        let width = spread.sqrt();
        let shifted = |row: usize, column: usize| {
            (gram[row][column] - if row == column { mean } else { 0.0 }) / width
        };
        let determinant = shifted(0, 0)
            * shifted(1, 1).mul_add(shifted(2, 2), -(shifted(1, 2) * shifted(2, 1)))
            - shifted(0, 1)
                * shifted(1, 0).mul_add(shifted(2, 2), -(shifted(1, 2) * shifted(2, 0)))
            + shifted(0, 2)
                * shifted(1, 0).mul_add(shifted(2, 1), -(shifted(1, 1) * shifted(2, 0)));
        let angle = (0.5 * determinant).clamp(-1.0, 1.0).acos() / 3.0;
        (2.0 * width).mul_add(angle.cos(), mean).max(0.0).sqrt()
    }

    fn apply(&self, vector: Vector3) -> Vector3 {
        self.columns[0].scale(vector.x)
            + self.columns[1].scale(vector.y)
            + self.columns[2].scale(vector.z)
    }
}

/// The placed curve parameter of a curve parameter.
#[derive(Clone)]
enum ParameterMap {
    /// `scale t + shift`.
    Affine { scale: f64, shift: f64 },
    /// Linear between consecutive `[source, target]` breaks, which increase.
    Piecewise(Vec<[f64; 2]>),
}

impl ParameterMap {
    const IDENTITY: Self = Self::Affine {
        scale: 1.0,
        shift: 0.0,
    };

    fn at(&self, parameter: f64) -> f64 {
        match self {
            Self::Affine { scale, shift } => scale.mul_add(parameter, *shift),
            Self::Piecewise(breaks) => breaks
                .windows(2)
                .find(|pair| parameter <= pair[1][0])
                .or_else(|| breaks.windows(2).last())
                .map_or(parameter, |pair| {
                    let ([source, target], [next_source, next_target]) = (pair[0], pair[1]);
                    (parameter - source)
                        .mul_add((next_target - target) / (next_source - source), target)
                }),
        }
    }

    fn interval(&self, [lower, upper]: [f64; 2]) -> [f64; 2] {
        [self.at(lower), self.at(upper)]
    }
}

/// The placed surface parameters `(a u + b v, c u + d v)` of `(u, v)`.
#[derive(Clone, Copy, PartialEq)]
struct UvMap {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl UvMap {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };

    const fn scaled(u: f64, v: f64) -> Self {
        Self {
            a: u,
            b: 0.0,
            c: 0.0,
            d: v,
        }
    }

    fn determinant(self) -> f64 {
        self.a.mul_add(self.d, -(self.b * self.c))
    }

    fn is_diagonal(self) -> bool {
        self.b == 0.0 && self.c == 0.0
    }

    fn apply(self, point: Point2) -> Point2 {
        Point2::new(
            self.a.mul_add(point.u, self.b * point.v),
            self.c.mul_add(point.u, self.d * point.v),
        )
    }
}

/// The unit axis along `axis` and the unit reference in its normal plane
/// nearest `reference`.
fn unit_frame(axis: Vector3, reference: Vector3) -> Result<(Vector3, Vector3), String> {
    let axis = UnitVector3::normalized(axis).ok_or("has a degenerate placed axis")?;
    let axis = *axis.as_raw();
    let reference = reference - axis.scale(reference.dot(axis));
    let reference =
        UnitVector3::normalized(reference).ok_or("has a degenerate placed reference")?;
    Ok((axis, *reference.as_raw()))
}

fn placed_point(transform: Transform, point: FinitePoint3) -> Result<Point3, String> {
    transform
        .apply_point(point.get())
        .map(FinitePoint3::get)
        .ok_or_else(|| "leaves the finite range".to_owned())
}

fn placed_curve(
    geometry: &CurveGeometry,
    transform: Transform,
) -> Result<(CurveGeometry, ParameterMap, Option<String>), String> {
    match geometry {
        CurveGeometry::Procedural {
            construction,
            cache,
        } => {
            let cache = cache
                .as_ref()
                .ok_or("is a procedural carrier with no solved form")?;
            let (solved, map) = placed_solved_curve(cache, transform)?;
            Ok((
                CurveGeometry::Solved(solved),
                map,
                Some(construction.as_str().to_owned()),
            ))
        }
        CurveGeometry::Solved(solved) => {
            let (solved, map) = placed_solved_curve(solved, transform)?;
            Ok((CurveGeometry::Solved(solved), map, None))
        }
    }
}

fn placed_solved_curve(
    geometry: &SolvedCurveGeometry,
    transform: Transform,
) -> Result<(SolvedCurveGeometry, ParameterMap), String> {
    let linear = Linear::of(transform);
    let invalid = |error: &str| format!("has no valid placed form: {error}");
    match geometry {
        SolvedCurveGeometry::Transformed(placed) => {
            let composed = transform
                .compose(*placed.transform())
                .map_err(|_| "composes to a non-finite transform".to_owned())?;
            placed_solved_curve(placed.basis(), composed)
        }
        SolvedCurveGeometry::Line(line) => {
            let origin = placed_point(transform, line.origin())?;
            let image = linear.apply(*line.direction().as_raw());
            let length = image.norm();
            let line = LineCurve::try_new(origin, image.scale(1.0 / length)).map_err(invalid)?;
            Ok((
                SolvedCurveGeometry::Line(line),
                ParameterMap::Affine {
                    scale: snapped(length, 1.0),
                    shift: 0.0,
                },
            ))
        }
        SolvedCurveGeometry::Circle(circle) => {
            let radius = circle.radius().get();
            elliptic(
                placed_point(transform, circle.center())?,
                linear.apply(*circle.frame().reference().as_raw()),
                linear.apply(*circle.frame().binormal().as_raw()),
                [radius, radius],
            )
        }
        SolvedCurveGeometry::Ellipse(ellipse) => elliptic(
            placed_point(transform, ellipse.center())?,
            linear.apply(*ellipse.frame().reference().as_raw()),
            linear.apply(*ellipse.frame().binormal().as_raw()),
            [ellipse.major_radius().get(), ellipse.minor_radius().get()],
        ),
        SolvedCurveGeometry::Hyperbola(hyperbola) => {
            let center = placed_point(transform, hyperbola.center())?;
            let radii = [
                hyperbola.major_radius().get(),
                hyperbola.minor_radius().get(),
            ];
            let major = linear
                .apply(*hyperbola.frame().reference().as_raw())
                .scale(radii[0]);
            let minor = linear
                .apply(*hyperbola.frame().binormal().as_raw())
                .scale(radii[1]);
            // `A cosh t + B sinh t` is `A' cosh(t - t0) + B' sinh(t - t0)`
            // with `A' ⊥ B'` where `tanh 2 t0 = -2 A·B / (|A|² + |B|²)`.
            let shift = if similar_in_plane(major, minor, radii) {
                0.0
            } else {
                0.5 * (-2.0 * major.dot(minor) / (major.dot(major) + minor.dot(minor))).atanh()
            };
            let (sinh, cosh) = (shift.sinh(), shift.cosh());
            let major_image = major.scale(cosh) + minor.scale(sinh);
            let minor_image = major.scale(sinh) + minor.scale(cosh);
            let (axis, reference) = unit_frame(major_image.cross(minor_image), major_image)?;
            let hyperbola = HyperbolaCurve::try_new(
                center,
                axis,
                reference,
                major_image.norm(),
                minor_image.norm(),
            )
            .map_err(invalid)?;
            Ok((
                SolvedCurveGeometry::Hyperbola(hyperbola),
                ParameterMap::Affine {
                    scale: 1.0,
                    shift: -shift,
                },
            ))
        }
        SolvedCurveGeometry::Parabola(parabola) => {
            let vertex = placed_point(transform, parabola.vertex())?;
            let focal = parabola.focal_distance().get();
            // `V + t² A + t B` with `A = f X`, `B = 2 f Y` restated as
            // `V' + f' τ² X' + 2 f' τ Y'` with `τ = α t + β`.
            let quadratic = linear
                .apply(*parabola.frame().reference().as_raw())
                .scale(focal);
            let linear_term = linear
                .apply(*parabola.frame().binormal().as_raw())
                .scale(2.0 * focal);
            let (axis, reference) = unit_frame(quadratic.cross(linear_term), quadratic)?;
            let binormal = axis.cross(reference);
            let along = linear_term.dot(reference);
            let across = linear_term.dot(binormal);
            let curvature = quadratic.norm();
            let (scale, shift) = if similar_in_plane(quadratic, linear_term, [1.0, 2.0]) {
                (1.0, 0.0)
            } else {
                (2.0 * curvature / across, along / across)
            };
            let focal = across * across / (4.0 * curvature);
            let offset =
                reference.scale(focal * shift * shift) + binormal.scale(2.0 * focal * shift);
            let vertex = Point3::new(
                vertex.x - offset.x,
                vertex.y - offset.y,
                vertex.z - offset.z,
            );
            let parabola =
                ParabolaCurve::try_new(vertex, axis, reference, focal).map_err(invalid)?;
            Ok((
                SolvedCurveGeometry::Parabola(parabola),
                ParameterMap::Affine { scale, shift },
            ))
        }
        SolvedCurveGeometry::Degenerate(degenerate) => Ok((
            SolvedCurveGeometry::Degenerate(
                DegenerateCurve::try_new(placed_point(transform, degenerate.point())?)
                    .map_err(invalid)?,
            ),
            ParameterMap::IDENTITY,
        )),
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            nurbs.try_map_control_points(|_, point| {
                transform
                    .apply_point(point.get())
                    .ok_or_else(|| "leaves the finite range".to_owned())
            })?;
            Ok((SolvedCurveGeometry::Nurbs(nurbs), ParameterMap::IDENTITY))
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let mut polyline = polyline.clone();
            polyline
                .edit_samples(|samples| {
                    samples.edit_points(|point| {
                        *point = transform
                            .apply_point(*point)
                            .ok_or_else(|| {
                                GeometryLayoutError::EditRefused(
                                    "a placed polyline point is not finite".into(),
                                )
                            })?
                            .get();
                        Ok(())
                    })
                })
                .map_err(|error| invalid(&error.to_string()))?;
            Ok((
                SolvedCurveGeometry::Polyline(polyline),
                ParameterMap::IDENTITY,
            ))
        }
        SolvedCurveGeometry::Composite { .. } => {
            Err("is a transformed composite carrier, which the writer does not place".to_owned())
        }
        SolvedCurveGeometry::Unknown { .. } => {
            Err("has no exact form that the writer places".to_owned())
        }
    }
}

/// Whether the linear map takes the conjugate semi-axes `x`, `y` of radii
/// `radii` to perpendicular images in the same ratio, so that the placed
/// conic keeps its parameter.
fn similar_in_plane(x_image: Vector3, y_image: Vector3, radii: [f64; 2]) -> bool {
    let [x, y] = [x_image.norm(), y_image.norm()];
    x_image.dot(y_image).abs() <= SIMILARITY_TOLERANCE * x * y
        && (x * radii[1] - y * radii[0]).abs() <= SIMILARITY_TOLERANCE * x * radii[1]
}

/// The placed circle or ellipse `C + A cos t + B sin t` with `A`, `B` the
/// images of the scaled frame reference and binormal.
fn elliptic(
    center: Point3,
    reference_image: Vector3,
    binormal_image: Vector3,
    radii: [f64; 2],
) -> Result<(SolvedCurveGeometry, ParameterMap), String> {
    let invalid = |error: &str| format!("has no valid placed form: {error}");
    let major = reference_image.scale(radii[0]);
    let minor = binormal_image.scale(radii[1]);
    // `A cos t + B sin t` is `A' cos(t - t0) + B' sin(t - t0)` with
    // `A' ⊥ B'` and `|A'| ≥ |B'|` where
    // `tan 2 t0 = 2 A·B / (|A|² - |B|²)`.
    let shift = if similar_in_plane(reference_image, binormal_image, [1.0, 1.0]) {
        0.0
    } else {
        0.5 * (2.0 * major.dot(minor)).atan2(major.dot(major) - minor.dot(minor))
    };
    let (sin, cos) = shift.sin_cos();
    let major_image = major.scale(cos) + minor.scale(sin);
    let minor_image = minor.scale(cos) - major.scale(sin);
    let (axis, reference) = unit_frame(major_image.cross(minor_image), major_image)?;
    let major_radius = major_image.norm();
    let minor_radius = minor_image.norm().min(major_radius);
    let map = ParameterMap::Affine {
        scale: 1.0,
        shift: -shift,
    };
    if major_radius - minor_radius <= SIMILARITY_TOLERANCE * major_radius {
        let circle =
            CircleCurve::try_new(center, axis, reference, major_radius).map_err(invalid)?;
        return Ok((SolvedCurveGeometry::Circle(circle), map));
    }
    let ellipse = EllipseCurve::try_new(center, axis, reference, major_radius, minor_radius)
        .map_err(invalid)?;
    Ok((SolvedCurveGeometry::Ellipse(ellipse), map))
}

fn placed_surface(
    geometry: &SurfaceGeometry,
    transform: Transform,
) -> Result<(SurfaceGeometry, UvMap, Option<String>), String> {
    match geometry {
        SurfaceGeometry::Procedural {
            construction,
            cache,
        } => {
            let cache = cache
                .as_ref()
                .ok_or("is a procedural carrier with no solved form")?;
            let (solved, map) = placed_solved_surface(cache, transform)?;
            Ok((
                SurfaceGeometry::Solved(solved),
                map,
                Some(construction.as_str().to_owned()),
            ))
        }
        SurfaceGeometry::Solved(solved) => {
            let (solved, map) = placed_solved_surface(solved, transform)?;
            Ok((SurfaceGeometry::Solved(solved), map, None))
        }
    }
}

fn placed_solved_surface(
    geometry: &SolvedSurfaceGeometry,
    transform: Transform,
) -> Result<(SolvedSurfaceGeometry, UvMap), String> {
    let linear = Linear::of(transform);
    let invalid = |error: &str| format!("has no valid placed form: {error}");
    // A circular carrier keeps its parameters under a similarity of scale
    // `s`; a reflection turns the azimuth.
    let revolved = |frame: &cadmpeg_ir::units::OrthonormalFrame3| {
        let scale = linear.similarity.ok_or(
            "a cylinder, cone, sphere or torus under a transform that is not a similarity is \
             written only as the rational Type 128 that the writer restates for its faces",
        )?;
        let (axis, reference) = unit_frame(
            linear.apply(*frame.axis().as_raw()),
            linear.apply(*frame.reference().as_raw()),
        )?;
        let turn = if linear.determinant < 0.0 { -1.0 } else { 1.0 };
        Ok::<_, String>((axis, reference, scale, turn))
    };
    match geometry {
        SolvedSurfaceGeometry::Transformed(placed) => {
            let composed = transform
                .compose(*placed.transform())
                .map_err(|_| "composes to a non-finite transform".to_owned())?;
            placed_solved_surface(placed.basis(), composed)
        }
        SolvedSurfaceGeometry::Plane(plane) => {
            let origin = placed_point(transform, plane.origin())?;
            let u_image = linear.apply(*plane.frame().reference().as_raw());
            let v_image = linear.apply(*plane.frame().binormal().as_raw());
            let u_length = u_image.norm();
            let reference = u_image.scale(1.0 / u_length);
            let shear = v_image.dot(reference);
            let across = v_image - reference.scale(shear);
            let (axis, reference) = unit_frame(reference.cross(across), reference)?;
            let plane = PlaneSurface::try_new(origin, axis, reference).map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Plane(plane),
                UvMap {
                    a: snapped(u_length, 1.0),
                    b: snapped(shear, 0.0),
                    c: 0.0,
                    d: snapped(across.norm(), 1.0),
                },
            ))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let (axis, reference, scale, turn) = revolved(cylinder.frame())?;
            let cylinder = CylinderSurface::try_new(
                placed_point(transform, cylinder.origin())?,
                axis,
                reference,
                cylinder.radius().get() * scale,
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Cylinder(cylinder),
                UvMap::scaled(turn, scale),
            ))
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            let (axis, reference, scale, turn) = revolved(cone.frame())?;
            let cone = ConeSurface::try_new(
                placed_point(transform, cone.origin())?,
                axis,
                reference,
                cone.radius().get() * scale,
                cone.ratio().get(),
                cone.half_angle().get(),
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Cone(cone),
                UvMap::scaled(turn, scale),
            ))
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let (axis, reference, scale, turn) = revolved(sphere.frame())?;
            let sphere = SphereSurface::try_new(
                placed_point(transform, sphere.center())?,
                axis,
                reference,
                sphere.radius().get() * scale,
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Sphere(sphere),
                UvMap::scaled(turn, 1.0),
            ))
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let (axis, reference, scale, turn) = revolved(torus.frame())?;
            let torus = TorusSurface::try_new(
                placed_point(transform, torus.center())?,
                axis,
                reference,
                torus.major_radius().get() * scale,
                torus.minor_radius().get() * scale,
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Torus(torus),
                UvMap::scaled(turn, 1.0),
            ))
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            nurbs.try_map_control_points(|_, point| {
                transform
                    .apply_point(point.get())
                    .ok_or_else(|| "leaves the finite range".to_owned())
            })?;
            Ok((SolvedSurfaceGeometry::Nurbs(nurbs), UvMap::IDENTITY))
        }
        SolvedSurfaceGeometry::Polygonal(polygonal) => {
            let mut polygonal = polygonal.clone();
            let failure = |error: GeometryLayoutError| invalid(&error.to_string());
            polygonal
                .edit_vertices(|vertices| {
                    for vertex in vertices.iter_mut() {
                        *vertex = transform
                            .apply_point(*vertex)
                            .ok_or_else(|| {
                                GeometryLayoutError::EditRefused(
                                    "a placed vertex leaves the finite range".to_owned(),
                                )
                            })?
                            .get();
                    }
                    Ok(())
                })
                .map_err(failure)?;
            // The image of a deviation is at most the largest singular value
            // of the map times as long.
            let stretch = linear
                .similarity
                .unwrap_or_else(|| linear.largest_stretch());
            polygonal
                .set_chordal_deflection(polygonal.chordal_deflection().get() * stretch)
                .map_err(failure)?;
            Ok((SolvedSurfaceGeometry::Polygonal(polygonal), UvMap::IDENTITY))
        }
        SolvedSurfaceGeometry::Unknown { .. } => {
            Err("has no exact form that the writer places".to_owned())
        }
    }
}

/// `pcurve` under the support parameter map `map`.
fn mapped_pcurve(pcurve: &Pcurve, map: UvMap) -> Result<Pcurve, String> {
    if pcurve.uv_offset().is_some() {
        return Err("states a support chart offset".to_owned());
    }
    let range = pcurve
        .parameter_range()
        .ok_or("states no parameter range")?;
    let mut nurbs = lifted_pcurve(pcurve, range).map_err(|error| error.to_string())?;
    nurbs.try_map_control_points(|_, point| {
        let point = point.get();
        let image = map.apply(Point2::new(point.x, point.y));
        FinitePoint3::new(Point3::new(image.u, image.v, 0.0))
            .ok_or_else(|| "has a non-finite placed control point".to_owned())
    })?;
    let points = nurbs
        .control_points()
        .into_iter()
        .map(|point| {
            let point = point.get();
            Point2::new(point.x, point.y)
        })
        .collect();
    let weights = nurbs.weights().map(|weights| {
        weights
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect()
    });
    let knots = nurbs
        .knots()
        .finite_knots()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    let nurbs = PcurveNurbs::from_lanes(nurbs.degree(), knots, points, weights, nurbs.periodic())
        .map_err(|error| error.to_string())?;
    Ok(Pcurve {
        id: pcurve.id.clone(),
        geometry: PcurveGeometry::Nurbs { nurbs },
        metadata: PcurveMetadata::general(None, Some(range), None),
        native_ref: pcurve.native_ref.clone(),
    })
}
