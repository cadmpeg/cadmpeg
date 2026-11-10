// SPDX-License-Identifier: Apache-2.0
//! Exact generator joins and complete result-face construction checks.

use crate::feature::profile::selection::PathCurve;
use cadmpeg_ir::{
    attributes::{AttributeTarget, AttributeValue},
    document::Model,
    geometry::{
        CurveGeometry, LoftMemberForm, ProceduralSurfaceDefinition, SolvedCurveGeometry,
        SolvedSurfaceGeometry, SurfaceGeometry,
    },
    math::Point3,
    topology::{BodyKind, Face},
};
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

fn owned_faces(model: &Model, generator: u32) -> Option<Vec<NamedFace<'_>>> {
    let mut names = HashMap::new();
    for attribute in &model.attributes {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let [_, _, _, _, _, AttributeValue::String(family), AttributeValue::Integer(1), AttributeValue::Integer(44), AttributeValue::Integer(1), AttributeValue::Integer(source), AttributeValue::Integer(-1), rest @ ..] =
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
                AttributeValue::Integer(value) => Some(*value),
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
    for face in &model.faces {
        let Some((family, values)) = names.remove(face.id.as_str()) else {
            continue;
        };
        let shell = model.shells.iter().find(|shell| shell.id == face.shell)?;
        let region = model
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
        || model
            .bodies
            .iter()
            .find(|candidate| &candidate.id == body)?
            .kind
            != BodyKind::Solid
    {
        return None;
    }
    let shells = model
        .shells
        .iter()
        .filter(|shell| {
            model
                .regions
                .iter()
                .any(|region| region.id == shell.region && &region.body == body)
        })
        .map(|shell| &shell.id)
        .collect::<HashSet<_>>();
    if model
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
    model: &Model,
    generator: u32,
    radius: f64,
    profile: i64,
    path: &[(i64, Option<PathCurve>)],
    endpoints: [Point3; 2],
) -> Option<()> {
    let faces = owned_faces(model, generator)?;
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
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        let SurfaceGeometry::Solved(geometry) = &surface.geometry else {
            return None;
        };
        match (section, span, role, geometry) {
            (0, 0, 4 | 12, SolvedSurfaceGeometry::Plane(_)) => {
                if caps
                    .insert(*role, cap_center(model, named.face, radius)?)
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

fn cap_center(model: &Model, face: &Face, radius: f64) -> Option<Point3> {
    if face.loops.len() != 1 {
        return None;
    }
    let id = face.loops.iter().next()?;
    let ring = model
        .loops
        .iter()
        .find(|ring| &ring.id == id && ring.face == face.id)?;
    if ring.coedges().is_empty() {
        return None;
    }
    let mut center = None;
    for id in ring.coedges() {
        let coedge = model
            .coedges
            .iter()
            .find(|coedge| &coedge.id == id && coedge.owner_loop == ring.id)?;
        if coedge.use_curve.is_some() {
            return None;
        }
        let edge = model.edges.iter().find(|edge| edge.id == coedge.edge)?;
        let curve = model
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
    model: &Model,
    generator: u32,
    sections: [u32; 2],
    curve_member: impl Fn(usize, i64) -> bool,
) -> Option<()> {
    let faces = owned_faces(model, generator)?;
    let mut caps = HashSet::new();
    let mut sides = HashSet::new();
    for named in faces {
        let surface = model
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
                let construction = surface.geometry.procedural_construction()?;
                let procedural = model
                    .procedural_surfaces
                    .iter()
                    .find(|procedural| &procedural.id == construction)?;
                let ProceduralSurfaceDefinition::Loft(payload) = procedural.definition() else {
                    return None;
                };
                if !matches!(surface.geometry.solved()?, SolvedSurfaceGeometry::Nurbs(_))
                    || !unconstrained_loft(payload)
                {
                    return None;
                }
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

/// Loft result check for either attribute carrier. `section_curves` lists the
/// associative curve names of a section boundary.
pub(in crate::feature::loft_sweep) fn loft_result(
    model: &Model,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Option<()> {
    loft(model, generator, sections, |section, name| {
        section_curves(section).contains(&name)
    })
    .or_else(|| legacy_loft(model, generator, sections, &section_curves))
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
fn legacy_owned_faces(model: &Model, generator: u32) -> Option<Vec<LegacyFace<'_>>> {
    let mut names = HashMap::new();
    for attribute in &model.attributes {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let cap = match attribute.name.as_str() {
            LEGACY_CAP_CLASS => true,
            LEGACY_SIDE_CLASS => false,
            _ => continue,
        };
        let [_, _, _, _, _, AttributeValue::Integer(_), AttributeValue::Integer(count), rest @ ..] =
            attribute.values.as_slice()
        else {
            return None;
        };
        let pairs = usize::try_from(*count).ok()?.checked_mul(2)?;
        let integers = rest
            .iter()
            .map(|value| match value {
                AttributeValue::Integer(value) => Some(*value),
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
    for face in &model.faces {
        let Some((cap, tail)) = names.remove(face.id.as_str()) else {
            continue;
        };
        let shell = model.shells.iter().find(|shell| shell.id == face.shell)?;
        let region = model
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
        && model
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
    model: &Model,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Option<()> {
    if sections[0] == sections[1] {
        return None;
    }
    let mut caps = HashSet::new();
    let mut first_curves = HashSet::new();
    let mut last_curves = HashSet::new();
    for named in legacy_owned_faces(model, generator)? {
        let surface = model
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
            if !sections.iter().any(|source| i64::from(*source) == section) || !caps.insert(section)
            {
                return None;
            }
        } else {
            let [2, first_curve, first_section, 258, 0, 0, last_curve, last_section, 258, 0, 0] =
                named.tail[..]
            else {
                return None;
            };
            if first_section != i64::from(sections[0])
                || last_section != i64::from(sections[1])
                || !first_curves.insert(first_curve)
                || !last_curves.insert(last_curve)
            {
                return None;
            }
        }
    }
    let complete = |found: &HashSet<i64>, section: usize| {
        let expected = section_curves(section);
        !expected.is_empty()
            && found.len() == expected.len()
            && expected.iter().all(|name| found.contains(name))
    };
    (caps.len() == 2 && complete(&first_curves, 0) && complete(&last_curves, 1)).then_some(())
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
