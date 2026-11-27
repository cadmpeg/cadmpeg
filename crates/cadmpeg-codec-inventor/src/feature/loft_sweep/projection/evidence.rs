// SPDX-License-Identifier: Apache-2.0
//! Exact generator joins and complete result-face construction checks.

use crate::feature::history_views::{ResultStates, StateModel};
use crate::feature::profile::selection::PathCurve;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::geometry::lofts::LoftMemberForm;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{BodyKind, Face};
use cadmpeg_ir::CadIr;
use std::collections::{HashMap, HashSet};

const EPS_RESULT_POINT_MM: f64 = 1.0e-5;
const EPS_SWEEP_RADIUS_MM: f64 = 1.0e-5;
/// Largest distance between a side-face axis or center and the joined path
/// curve.
const EPS_SWEEP_PATH_MM: f64 = 1.0e-5;
/// Largest sine of the angle between a cylinder axis and its path line.
const EPS_SWEEP_AXIS: f64 = 1.0e-6;

struct NamedFace<'a> {
    face: &'a Face,
    family: &'a str,
    values: Vec<i64>,
}

fn owned_faces(ir: &CadIr, generator: u32) -> Option<Vec<NamedFace<'_>>> {
    owned_faces_in(ir, generator, false)
}

/// Faces of the carrier with the schema-44 naming attributes of `generator`.
/// A joined result body also holds faces of the body it merged with, so
/// `joined` admits faces that no attribute of the generator names.
fn owned_faces_in(ir: &CadIr, generator: u32, joined: bool) -> Option<Vec<NamedFace<'_>>> {
    let mut names = HashMap::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target.entity else {
            continue;
        };
        let [_, _, _, _, _, Some(AttributeValue::String(family)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(44)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(source)), Some(AttributeValue::Integer(-1)), rest @ ..] =
            attribute.values.as_slice()
        else {
            continue;
        };
        if attribute.name != "ATTRIB_CUSTOM-attrib"
            || *source != i64::from(generator)
            || !matches!(
                family.as_str(),
                "INV_NMX_GEN_TAG" | "INV_NMX_SWEEPGENERATED_TAG" | "INV_NMX_ORDERED_GEN_TAG"
            )
        {
            continue;
        }
        let values = rest
            .iter()
            .map(|value| match value {
                Some(AttributeValue::Integer(value)) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if names
            .insert(face.as_str(), (family.as_str(), values))
            .is_some()
        {
            return None;
        }
    }
    let mut selected = Vec::new();
    let mut body = None;
    for face in &ir.geometry.faces {
        let Some((family, values)) = names.remove(face.id.as_str()) else {
            continue;
        };
        let shell = ir
            .geometry
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)?;
        let region = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.id == shell.region)?;
        if body.is_some_and(|body| body != &region.body) {
            return None;
        }
        body = Some(&region.body);
        selected.push(NamedFace {
            face,
            family,
            values,
        });
    }
    let body = body?;
    if !names.is_empty()
        || ir
            .geometry
            .bodies
            .iter()
            .find(|candidate| &candidate.id == body)?
            .kind
            != BodyKind::Solid
    {
        return None;
    }
    let shells = ir
        .geometry
        .shells
        .iter()
        .filter(|shell| {
            ir.geometry
                .regions
                .iter()
                .any(|region| region.id == shell.region && &region.body == body)
        })
        .map(|shell| &shell.id)
        .collect::<HashSet<_>>();
    if !joined
        && ir
            .geometry
            .faces
            .iter()
            .filter(|face| shells.contains(&face.shell))
            .count()
            != selected.len()
    {
        return None;
    }
    Some(selected)
}

pub(in crate::feature::loft_sweep) fn sweep(
    ir: &CadIr,
    generator: u32,
    radius: f64,
    profile: i64,
    path: &[(i64, Option<PathCurve>)],
    endpoints: [Point3; 2],
) -> Option<()> {
    let geometry = &ir.geometry;
    let faces = owned_faces(ir, generator)?;
    let joined = |span: i64| path.iter().find(|(name, _)| *name == span)?.1;
    let mut caps = HashMap::new();
    let mut spans = HashSet::new();
    for named in &faces {
        if named.family != "INV_NMX_SWEEPGENERATED_TAG" {
            return None;
        }
        let [section, span, role, tail @ ..] = named.values.as_slice() else {
            return None;
        };
        if tail != [0; 7] {
            return None;
        }
        let surface = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        let SurfaceGeometry::Solved(geometry) = &surface.geometry else {
            return None;
        };
        match (section, span, role, geometry) {
            (0, 0, 4 | 12, SolvedSurfaceGeometry::Plane(_)) => {
                if caps
                    .insert(*role, cap_center(&ir.geometry, named.face, radius)?)
                    .is_some()
                {
                    return None;
                }
            }
            (section, span, 20, SolvedSurfaceGeometry::Cylinder(cylinder))
                if *section == profile
                    && (cylinder.radius().get() - radius).abs() <= EPS_SWEEP_RADIUS_MM
                    && joined(*span).is_none_or(|curve| along_line(cylinder, curve)) =>
            {
                if !spans.insert(*span) {
                    return None;
                }
            }
            (section, span, 20, SolvedSurfaceGeometry::Torus(torus))
                if *section == profile
                    && (torus.minor_radius().get().abs() - radius).abs() <= EPS_SWEEP_RADIUS_MM
                    && joined(*span).is_none_or(|curve| about_arc(torus, curve)) =>
            {
                if !spans.insert(*span) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if caps.len() != 2
        || spans.len() != path.len()
        || !path.iter().all(|(name, _)| spans.contains(name))
    {
        return None;
    }
    if !((same_point(endpoints[0], caps[&4]) && same_point(endpoints[1], caps[&12]))
        || (same_point(endpoints[1], caps[&4]) && same_point(endpoints[0], caps[&12])))
    {
        return None;
    }
    Some(())
}

/// Whether the cylinder axis is the line of a line-joined span.
fn along_line(
    cylinder: &cadmpeg_ir::geometry::analytic::CylinderSurface,
    curve: PathCurve,
) -> bool {
    let PathCurve::Line { start, end } = curve else {
        return false;
    };
    let axis = *cylinder.frame().axis().as_raw();
    let line = end.vector_from(start);
    let length = line.norm();
    let off_axis = |point: Point3| {
        let offset = point.vector_from(cylinder.origin().get());
        offset.cross(axis).norm()
    };
    length > 0.0
        && line.cross(axis).norm() <= EPS_SWEEP_AXIS * length
        && off_axis(start) <= EPS_SWEEP_PATH_MM
        && off_axis(end) <= EPS_SWEEP_PATH_MM
}

/// Whether the torus center and major radius are those of an arc-joined span.
fn about_arc(torus: &cadmpeg_ir::geometry::analytic::TorusSurface, curve: PathCurve) -> bool {
    let PathCurve::Arc { center, radius } = curve else {
        return false;
    };
    same_point_within(torus.center().get(), center, EPS_SWEEP_PATH_MM)
        && (torus.major_radius().get() - radius).abs() <= EPS_SWEEP_RADIUS_MM
}

fn same_point(left: Point3, right: Point3) -> bool {
    same_point_within(left, right, EPS_RESULT_POINT_MM)
}

fn same_point_within(left: Point3, right: Point3, tolerance: f64) -> bool {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
        <= tolerance
}

fn cap_center(geometry: &Topology, face: &Face, radius: f64) -> Option<Point3> {
    let face_loops = face.loops.boundary()?;
    if face_loops.len() != 1 {
        return None;
    }
    let id = face_loops.iter().next()?;
    let ring = geometry
        .loops
        .iter()
        .find(|ring| &ring.id == id && ring.face == face.id)?;
    if ring.coedges().is_empty() {
        return None;
    }
    let mut center = None;
    for id in ring.coedges() {
        let coedge = geometry
            .coedges
            .iter()
            .find(|coedge| &coedge.id == id && coedge.owner_loop == ring.id)?;
        if coedge.use_curve.is_some() {
            return None;
        }
        let edge = geometry.edges.iter().find(|edge| edge.id == coedge.edge)?;
        let curve = geometry
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.carrier.curve())?;
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &curve.geometry else {
            return None;
        };
        if (circle.radius().get() - radius).abs() > EPS_SWEEP_RADIUS_MM {
            return None;
        }
        let point = circle.center().get();
        if center.is_some_and(|center| !same_point(center, point)) {
            return None;
        }
        center = Some(point);
    }
    center
}

pub(in crate::feature::loft_sweep) fn loft(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    curve_member: impl Fn(usize, i64) -> bool,
) -> Option<()> {
    loft_in(ir, generator, sections, curve_member, false)
}

fn loft_in(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    curve_member: impl Fn(usize, i64) -> bool,
    joined: bool,
) -> Option<()> {
    let geometry = &ir.geometry;
    let faces = owned_faces_in(ir, generator, joined)?;
    let mut caps = HashSet::new();
    let mut sides = HashSet::new();
    for named in faces {
        let surface = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        match (named.family, named.values.as_slice()) {
            ("INV_NMX_GEN_TAG", [0, section, 260])
                if matches!(
                    &surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                ) =>
            {
                if !sections.iter().any(|source| i64::from(*source) == *section)
                    || !caps.insert(*section)
                {
                    return None;
                }
            }
            ("INV_NMX_ORDERED_GEN_TAG", values)
                if side_tag(values).is_some_and(|tag| {
                    tag.first_section == i64::from(sections[0])
                        && tag.last_section == i64::from(sections[1])
                        && curve_member(0, tag.first_curve)
                        && curve_member(1, tag.last_curve)
                }) =>
            {
                let member = side_tag(values)?.member;
                side_surface_is_loft(geometry, &surface.geometry)?;
                if !sides.insert(usize::try_from(member).ok()?) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    (sections[0] != sections[1]
        && caps.len() == 2
        && !sides.is_empty()
        && (1..=sides.len()).all(|member| sides.contains(&member)))
    .then_some(())
}

/// Whether a side-face surface is an unconstrained procedural loft with a
/// NURBS solution.
fn side_surface_is_loft(geometry: &Topology, surface: &SurfaceGeometry) -> Option<()> {
    let construction = surface.procedural_construction()?;
    let procedural = geometry
        .procedural_surfaces
        .iter()
        .find(|procedural| &procedural.id == construction)?;
    let ProceduralSurfaceDefinition::Loft(payload) = procedural.definition() else {
        return None;
    };
    (matches!(surface.solved()?, SolvedSurfaceGeometry::Nurbs(_)) && unconstrained_loft(payload))
        .then_some(())
}

/// Loft result proof from the final model alone, for the validator. The
/// final model holds the result faces unless a later feature consumed or
/// split a cap, or copied faces with a pattern or a mirror. The history state
/// after the loft holds them then (spec section 13.2), and the validator
/// cannot decode the state. `history_bound` says that the loft has a bound
/// history-state record of the loft kind. The loft stays proved then when no
/// generator-owned face of the final model contradicts it.
///
/// A loft proved this way is screened for contradicting faces, not proved
/// again: the state is not persisted with its faces, so the validator has
/// nothing to prove the result against. The projector proved it when it
/// decoded the state.
pub(in crate::feature::loft_sweep) fn loft_final_proof(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
    history_bound: bool,
) -> Option<()> {
    if loft_result(ir, generator, sections, &section_curves).is_some() {
        return Some(());
    }
    (history_bound && loft_owned_consistent(ir, generator, sections, &section_curves)).then_some(())
}

/// Whether no generator-owned face of `ir` contradicts the loft. A later
/// feature removes a cap or a side face, splits it, or copies it with a
/// pattern or a mirror. Removal leaves gaps in the member indices and a copy
/// repeats a name, so neither contradicts the loft, and a face that is gone
/// proves nothing. A named face contradicts the loft when no such operation
/// produces it: a cap of a section that the loft does not use, a side face
/// whose first or last section or curve differs from the loft, a side member
/// index below one, or a name of another family. An empty owned set is
/// consistent.
///
/// The screen reads names and tails only, not surfaces. The projector proves
/// the surface of each cap and side face on the state after the loft, and the
/// final model can hold a face with another surface when a later feature
/// re-surfaced it, which section 13.2 does not forbid. A surface of the final
/// model therefore does not contradict a loft that the state proved. A legacy
/// naming attribute that the projector refuses (a form other than the
/// count-prefixed pairs of integers, or a source count below one) refuses
/// the screen as well, whichever generator it names.
pub(in crate::feature::loft_sweep) fn loft_owned_consistent(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> bool {
    if sections[0] == sections[1] {
        return false;
    }
    let first_curves = section_curves(0);
    let last_curves = section_curves(1);
    let uses = |section: i64| sections.iter().any(|source| i64::from(*source) == section);
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .collect::<HashSet<_>>();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(id) = &attribute.target.entity else {
            continue;
        };
        let name = attribute.name.as_str();
        let integers = |values: &[Option<AttributeValue>]| {
            values
                .iter()
                .map(|value| match value {
                    Some(AttributeValue::Integer(value)) => Some(*value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        };
        let consistent = if name == "ATTRIB_CUSTOM-attrib" {
            let [_, _, _, _, _, Some(AttributeValue::String(family)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(44)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(source)), Some(AttributeValue::Integer(-1)), rest @ ..] =
                attribute.values.as_slice()
            else {
                continue;
            };
            if *source != i64::from(generator)
                || !matches!(
                    family.as_str(),
                    "INV_NMX_GEN_TAG" | "INV_NMX_SWEEPGENERATED_TAG" | "INV_NMX_ORDERED_GEN_TAG"
                )
            {
                continue;
            }
            if !faces.contains(id.as_str()) {
                return false;
            }
            match (family.as_str(), integers(rest)) {
                ("INV_NMX_GEN_TAG", Some(values)) => {
                    matches!(values[..], [0, section, 260] if uses(section))
                }
                ("INV_NMX_ORDERED_GEN_TAG", Some(values)) => side_tag(&values).is_some_and(|tag| {
                    tag.first_section == i64::from(sections[0])
                        && tag.last_section == i64::from(sections[1])
                        && first_curves.contains(&tag.first_curve)
                        && last_curves.contains(&tag.last_curve)
                        && tag.member >= 1
                }),
                _ => false,
            }
        } else if name == LEGACY_CAP_CLASS || name == LEGACY_SIDE_CLASS {
            // `legacy_owned_faces` refuses the whole loft for a legacy
            // attribute of another form, so the screen does too.
            let [_, _, _, _, _, Some(AttributeValue::Integer(_)), Some(AttributeValue::Integer(count)), rest @ ..] =
                attribute.values.as_slice()
            else {
                return false;
            };
            let Some(integers) = integers(rest) else {
                return false;
            };
            let Some(pairs) = usize::try_from(*count)
                .ok()
                .and_then(|count| count.checked_mul(2))
                .filter(|pairs| *count >= 1 && *pairs <= integers.len())
            else {
                return false;
            };
            let (names, tail) = integers.split_at(pairs);
            if !names
                .chunks_exact(2)
                .any(|pair| pair[0] == i64::from(generator))
            {
                continue;
            }
            if !faces.contains(id.as_str()) {
                return false;
            }
            *count == 1
                && names[1] == -1
                && if name == LEGACY_CAP_CLASS {
                    matches!(tail[..], [0, section, 260] if uses(section))
                } else {
                    legacy_side_groups(tail, sections).is_some_and(|(first, last)| {
                        first.iter().all(|curve| first_curves.contains(curve))
                            && last.iter().all(|curve| last_curves.contains(curve))
                    })
                }
        } else {
            continue;
        };
        if !consistent {
            return false;
        }
    }
    true
}

/// Loft result check for either attribute carrier. `section_curves` lists the
/// associative curve names of a section boundary.
pub(in crate::feature::loft_sweep) fn loft_result(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Option<()> {
    loft(ir, generator, sections, |section, name| {
        section_curves(section).contains(&name)
    })
    .or_else(|| legacy_loft(ir, generator, sections, &section_curves))
}

/// Why a loft result is not proved.
#[derive(Debug)]
pub(in crate::feature::loft_sweep) enum Unproved {
    /// Neither the final model nor the state after the loft holds a complete
    /// generator-owned result.
    Result,
    /// The history state of the loft does not decode.
    StateFailed,
    /// The views of history states passed their work cap.
    Bounded,
    /// The state after the loft holds a complete generator-owned result, and
    /// the result body does not merge with a body of the preceding state.
    NotJoined,
    /// The decode of a state failed.
    Codec(CodecError),
}

/// Loft result proof. The final model holds the result faces unless a later
/// feature removed or copied them. The history state that first names the
/// loft holds them in that case (spec section 13.2).
pub(in crate::feature::loft_sweep) fn loft_proof(
    ctx: &DecodeContext<'_>,
    final_model: Option<&CadIr>,
    states: Option<&dyn ResultStates>,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Result<(), Unproved> {
    if final_model
        .is_some_and(|model| loft_result(model, generator, sections, &section_curves).is_some())
    {
        return Ok(());
    }
    let Some(states) = states else {
        return Err(Unproved::Result);
    };
    match &*states.after(ctx, generator).map_err(Unproved::Codec)? {
        StateModel::Model(model) => {
            loft_result(model, generator, sections, &section_curves).ok_or(Unproved::Result)
        }
        StateModel::Unavailable => Err(Unproved::Result),
        StateModel::Failed => Err(Unproved::StateFailed),
        StateModel::Bounded => Err(Unproved::Bounded),
    }
}

/// Joined loft result proof (spec section 13.2). The state after the loft
/// names the cap and side faces of the loft in one solid body (the legacy form
/// can leave merged faces unnamed, see `legacy_loft_in`), and that body
/// holds at least one face that the loft does not name: the oldest state that
/// lists the loft holds the features before it and the loft only, so such a
/// face belongs to a body of the state before the loft, and the loft merged
/// with that body. The state before the loft holds a solid body with at least
/// as many faces as the foreign faces of the result body. The final model is
/// not consulted, because later features can change the merged body.
pub(in crate::feature::loft_sweep) fn loft_join_proof(
    ctx: &DecodeContext<'_>,
    states: Option<&dyn ResultStates>,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Result<(), Unproved> {
    let states = states.ok_or(Unproved::Result)?;
    let after = states.after(ctx, generator).map_err(Unproved::Codec)?;
    let foreign = match &*after {
        StateModel::Model(model) => {
            joined_result(model, generator, sections, &section_curves).ok_or(Unproved::Result)?
        }
        StateModel::Unavailable => return Err(Unproved::Result),
        StateModel::Failed => return Err(Unproved::StateFailed),
        StateModel::Bounded => return Err(Unproved::Bounded),
    };
    if foreign == 0 {
        return Err(Unproved::NotJoined);
    }
    let before = states.before(ctx, generator).map_err(Unproved::Codec)?;
    match &*before {
        StateModel::Model(model) if largest_solid_faces(model) >= foreign => Ok(()),
        StateModel::Model(_) | StateModel::Unavailable => Err(Unproved::NotJoined),
        StateModel::Failed => Err(Unproved::StateFailed),
        StateModel::Bounded => Err(Unproved::Bounded),
    }
}

/// Faces of the solid body of `ir` with the most faces. The state before the
/// loft holds the joined body as one solid body. The states do not carry a body
/// identity across the history, so the size of the body stands for it.
fn largest_solid_faces(ir: &CadIr) -> usize {
    let mut per_body: HashMap<&str, usize> = HashMap::new();
    for face in &ir.geometry.faces {
        let body = ir
            .geometry
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)
            .and_then(|shell| ir.geometry.regions.iter().find(|r| r.id == shell.region))
            .and_then(|region| {
                ir.geometry
                    .bodies
                    .iter()
                    .find(|body| body.id == region.body)
            })
            .filter(|body| body.kind == BodyKind::Solid);
        if let Some(body) = body {
            *per_body.entry(body.id.as_str()).or_default() += 1;
        }
    }
    per_body.into_values().max().unwrap_or(0)
}

/// Number of faces of the result body that the loft does not name, when the
/// loft names a complete result in that body.
fn joined_result(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: &impl Fn(usize) -> Vec<i64>,
) -> Option<usize> {
    loft_in(
        ir,
        generator,
        sections,
        |section, name| section_curves(section).contains(&name),
        true,
    )
    .or_else(|| legacy_loft_in(ir, generator, sections, section_curves, true))?;
    let owned = owned_faces_in(ir, generator, true)
        .map(|faces| {
            faces
                .into_iter()
                .map(|named| named.face)
                .collect::<Vec<_>>()
        })
        .or_else(|| {
            Some(
                legacy_owned_faces(ir, generator)?
                    .into_iter()
                    .map(|named| named.face)
                    .collect(),
            )
        })?;
    let body_of = |face: &Face| {
        let shell = ir
            .geometry
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)?;
        let region = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.id == shell.region)?;
        Some(&region.body)
    };
    let body = body_of(owned.first()?)?;
    let in_body = ir
        .geometry
        .faces
        .iter()
        .filter(|face| body_of(face) == Some(body))
        .count();
    in_body.checked_sub(owned.len())
}

const LEGACY_CAP_CLASS: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
const LEGACY_SIDE_CLASS: &str =
    "NMx_Loft_Surface_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";

/// A face that carries a generator-owned naming attribute of the legacy class chains.
struct LegacyFace<'a> {
    face: &'a Face,
    cap: bool,
    tail: Vec<i64>,
}

/// Faces named by `generator` in a legacy carrier, in model order. The faces
/// belong to one solid body. Other faces of the body can belong to later features.
fn legacy_owned_faces(ir: &CadIr, generator: u32) -> Option<Vec<LegacyFace<'_>>> {
    let mut names = HashMap::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target.entity else {
            continue;
        };
        let cap = match attribute.name.as_str() {
            LEGACY_CAP_CLASS => true,
            LEGACY_SIDE_CLASS => false,
            _ => continue,
        };
        let [_, _, _, _, _, Some(AttributeValue::Integer(_)), Some(AttributeValue::Integer(count)), rest @ ..] =
            attribute.values.as_slice()
        else {
            return None;
        };
        let pairs = usize::try_from(*count).ok()?.checked_mul(2)?;
        let integers = rest
            .iter()
            .map(|value| match value {
                Some(AttributeValue::Integer(value)) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if *count < 1 || pairs > integers.len() {
            return None;
        }
        let (names_part, tail) = integers.split_at(pairs);
        let named = names_part
            .chunks_exact(2)
            .any(|pair| pair[0] == i64::from(generator));
        if !named {
            continue;
        }
        if *count != 1 || names_part[1] != -1 {
            return None;
        }
        if names.insert(face.as_str(), (cap, tail.to_vec())).is_some() {
            return None;
        }
    }
    let mut selected = Vec::new();
    let mut body = None;
    for face in &ir.geometry.faces {
        let Some((cap, tail)) = names.remove(face.id.as_str()) else {
            continue;
        };
        let shell = ir
            .geometry
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)?;
        let region = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.id == shell.region)?;
        if body.is_some_and(|body| body != &region.body) {
            return None;
        }
        body = Some(&region.body);
        selected.push(LegacyFace { face, cap, tail });
    }
    let body = body?;
    (names.is_empty()
        && ir
            .geometry
            .bodies
            .iter()
            .find(|candidate| &candidate.id == body)?
            .kind
            == BodyKind::Solid)
        .then_some(selected)
}

/// Loft result check for a legacy carrier. The generator names both caps and
/// one planar side face for each curve of both section boundaries. Later
/// features can add faces to the body.
pub(in crate::feature::loft_sweep) fn legacy_loft(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Option<()> {
    legacy_loft_in(ir, generator, sections, section_curves, false)
}

/// Most section curves of one section boundary that a joined loft result can
/// leave without a side face: the side face on the joined end merges into a
/// face of the body that the loft joins.
const JOINED_UNNAMED_CURVES_PER_SECTION: usize = 1;

/// Legacy loft result check. With `joined`, the result body also holds the
/// faces of the body that the loft joined, and the join merges faces of the
/// loft into faces of that body. A cap face can stay unnamed or split into
/// several named faces of one section, and each section boundary can leave at
/// most [`JOINED_UNNAMED_CURVES_PER_SECTION`] curve without a side face. At least one
/// cap face is named, and every named face still names a curve or a section
/// of the loft.
fn legacy_loft_in(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
    joined: bool,
) -> Option<()> {
    let geometry = &ir.geometry;
    if sections[0] == sections[1] {
        return None;
    }
    let mut caps = HashSet::new();
    let mut first_curves = HashSet::new();
    let mut last_curves = HashSet::new();
    for named in legacy_owned_faces(ir, generator)? {
        let surface = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        if !matches!(
            &surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) {
            return None;
        }
        if named.cap {
            let [0, section, 260] = named.tail[..] else {
                return None;
            };
            if !sections.iter().any(|source| i64::from(*source) == section)
                || (!caps.insert(section) && !joined)
            {
                return None;
            }
        } else {
            let (first, last) = legacy_side_groups(&named.tail, sections)?;
            if !first.into_iter().all(|curve| first_curves.insert(curve))
                || !last.into_iter().all(|curve| last_curves.insert(curve))
            {
                return None;
            }
        }
    }
    let complete = |found: &HashSet<i64>, section: usize| {
        let expected = section_curves(section);
        let unnamed = expected.iter().filter(|name| !found.contains(name)).count();
        !expected.is_empty()
            && found.iter().all(|name| expected.contains(name))
            && unnamed
                <= if joined {
                    JOINED_UNNAMED_CURVES_PER_SECTION
                } else {
                    0
                }
    };
    let caps_named = if joined {
        !caps.is_empty()
    } else {
        caps.len() == 2
    };
    (caps_named && complete(&first_curves, 0) && complete(&last_curves, 1)).then_some(())
}

/// Number of integers in one group of a legacy loft side tail:
/// `curve, section, 258, 0, 0`.
const LEGACY_GROUP_LEN: usize = 5;

/// The curve names of a legacy loft side tail, split by section. The tail
/// holds the group count `n`, then `n` groups `curve, section, 258, 0, 0`. The
/// count is 2, with one group for each section, or 3, with two groups of the
/// first section followed by one group of the last section.
fn legacy_side_groups(tail: &[i64], sections: [u32; 2]) -> Option<(Vec<i64>, Vec<i64>)> {
    let (&count, groups) = tail.split_first()?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| (2..=3).contains(count))?;
    if groups.len() != count.checked_mul(LEGACY_GROUP_LEN)? {
        return None;
    }
    let (first_section, last_section) = (i64::from(sections[0]), i64::from(sections[1]));
    let mut curves = Vec::new();
    let mut section_of = Vec::new();
    for group in groups.chunks_exact(LEGACY_GROUP_LEN) {
        let [curve, section, 258, 0, 0] = *group else {
            return None;
        };
        curves.push(curve);
        section_of.push(section);
    }
    let (last, first) = (curves.pop()?, curves);
    let first_ok = section_of[..count - 1].iter().all(|s| *s == first_section);
    (first_ok && section_of[count - 1] == last_section).then_some((first, vec![last]))
}

/// The section groups and member index of a loft side-face tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SideTag {
    first_curve: i64,
    first_section: i64,
    last_curve: i64,
    last_section: i64,
    member: i64,
}

/// Reads a loft side-face tag. The tag holds two groups
/// `curve, section, 258, tail_0, tail_1` in loft-section order. One group has
/// tail `0, 0`, and the other group has tail `4, member`.
fn side_tag(values: &[i64]) -> Option<SideTag> {
    let [2, first_curve, first_section, 258, a_0, a_1, last_curve, last_section, 258, b_0, b_1] =
        *values
    else {
        return None;
    };
    let (([0, 0], [4, member]) | ([4, member], [0, 0])) = ([a_0, a_1], [b_0, b_1]) else {
        return None;
    };
    Some(SideTag {
        first_curve,
        first_section,
        last_curve,
        last_section,
        member,
    })
}

fn unconstrained_loft(
    payload: &cadmpeg_ir::geometry::surface_payloads::LoftSurfacePayload,
) -> bool {
    if payload.closures() != &[0, 0] || *payload.mode() != 0 || !payload.bridge().is_empty() {
        return false;
    }
    let [primary, secondary] = payload.sections();
    if primary.entries.len() != 2 || !secondary.entries.is_empty() {
        return false;
    }
    for entry in &primary.entries {
        if entry.path.path.is_some()
            || !entry.path.auxiliaries.is_empty()
            || entry.profile.len() != 1
        {
            return false;
        }
        if !matches!(
            &entry.profile[0].form,
            LoftMemberForm::Support {
                type_code: 1,
                surface: None,
                support_bounds: [None, None, None, None],
                pcurve: None,
                first_flag: false,
                direction: None,
                ..
            }
        ) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
