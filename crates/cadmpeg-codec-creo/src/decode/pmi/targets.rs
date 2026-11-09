// SPDX-License-Identifier: Apache-2.0
//! Geometric targets selected by typed tolerance placements.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{EdgeId, FaceId, SurfaceId};
use cadmpeg_ir::pmi::PmiTarget;

use crate::container::ContainerScan;
use crate::feature::pmi_framing::{
    dimension_attachments, prefixes_for_annotation_slots, AttachmentPrefix,
};

use super::{elements::Element, Tolerance};

const DISPLAY_VALUE_MATCH_TOLERANCE: f64 = 1.0e-9;
const SURFACE_ATTACHMENT: u32 = 0x25;
const CARRIER_COINCIDENCE_TOLERANCE_MM: f64 = 1.0e-6;
const AXIS_PARALLEL_TOLERANCE: f64 = 1.0e-9;
const VISIBLE_SURFACE_PREFIX: &str = "creo:visibgeom:surface#";
const EDGE_ATTACHMENT: u32 = 1;
const DIMENSION_PLACEMENT: u32 = 1;
const SURFACE_PLACEMENT: u32 = 2;

type Owner = (u32, u32);

/// The geometric targets of one tolerance and the placement that selected them.
#[derive(Debug, Clone)]
pub(super) enum ToleranceTarget {
    /// The unique face of the attached surface of a type-2 placement.
    Surface(PmiTarget),
    /// The targets of the dimension that a type-1 placement selects.
    Dimension(DimensionTarget),
}

impl ToleranceTarget {
    pub(super) fn faces(&self) -> Vec<PmiTarget> {
        match self {
            Self::Surface(face) => vec![face.clone()],
            Self::Dimension(target) => target.faces.clone(),
        }
    }
}

pub(super) fn tolerance_targets(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    owners: &BTreeMap<Owner, Option<(Element, &str)>>,
    tolerances: &BTreeMap<Owner, Tolerance<'_>>,
    dimension_targets: &BTreeMap<(&str, u32), DimensionTarget>,
    ir: &CadIr,
) -> Result<BTreeMap<Owner, ToleranceTarget>, CodecError> {
    let mut targets = BTreeMap::new();
    for name in ["BasFullData", "FullMData"] {
        let Some(section) = super::section(scan, name) else {
            continue;
        };
        let matching = || {
            owners.iter().filter_map(|(owner, element)| {
                let (element, section_name) = element.as_ref()?;
                (*section_name == name).then_some((*owner, element.annotation?))
            })
        };
        let count = matching().count();
        let mut slots = ctx.alloc_filled(count, 0_u32, "Creo tolerance annotation slots")?;
        for (slot, (_, annotation)) in slots.iter_mut().zip(matching()) {
            *slot = annotation;
        }
        let Some(prefixes) = prefixes_for_annotation_slots(ctx, section, &slots)? else {
            continue;
        };
        for ((owner, _), prefix) in matching().zip(prefixes) {
            let Some(prefix) = prefix else { continue };
            let Some(tolerance) = tolerances.get(&owner) else {
                continue;
            };
            let (Some(stored), Some(displayed)) = (
                tolerance.magnitude,
                super::text_magnitude(prefix.value_string),
            ) else {
                continue;
            };
            if (stored - displayed).abs() > DISPLAY_VALUE_MATCH_TOLERANCE {
                continue;
            }
            let Some(placement) = prefix.placement else {
                continue;
            };
            let target = match (placement.placement_type, placement.attachment) {
                (DIMENSION_PLACEMENT, _) => dimension_targets
                    .get(&(name, placement.id))
                    .cloned()
                    .map(ToleranceTarget::Dimension),
                (
                    SURFACE_PLACEMENT,
                    Some(AttachmentPrefix::Inline {
                        reference_type: SURFACE_ATTACHMENT,
                        id,
                    }),
                ) if placement.id == id => surface_faces(ir, id).and_then(|faces| {
                    let [face] = faces.as_slice() else {
                        return None;
                    };
                    Some(ToleranceTarget::Surface(PmiTarget::Face {
                        face: face.clone(),
                    }))
                }),
                _ => None,
            };
            if let Some(target) = target {
                targets.insert(owner, target);
            }
        }
    }
    Ok(targets)
}

/// Face targets of annotation dimensions keyed by section and `dim_array` slot.
pub(super) fn dimension_targets<'a>(
    scan: &ContainerScan<'_>,
    element_tables: &[(&'a str, Vec<Option<Element>>)],
    ir: &CadIr,
) -> BTreeMap<(&'a str, u32), DimensionTarget> {
    let mut targets = BTreeMap::new();
    for (name, elements) in element_tables {
        let Some(section) = super::section(scan, name) else {
            continue;
        };
        let Some(spans) = crate::feature::dimensions::element_spans(section.window()) else {
            continue;
        };
        for element in elements.iter().flatten() {
            let (Some(8), Some(slot)) = (element.annotation_type, element.annotation) else {
                continue;
            };
            let Some(span) = spans
                .iter()
                .find(|span| u32::try_from(span.slot).ok() == Some(slot))
            else {
                continue;
            };
            let Some(attachments) =
                dimension_attachments(section, *span, (element.owner, element.owner_type))
            else {
                continue;
            };
            if let Some(target) = resolve_dimension(&attachments, ir) {
                targets.insert((*name, slot), target);
            }
        }
    }
    targets
}

/// The faces that a dimension's two attachment links select.
#[derive(Debug, Clone)]
pub(super) struct DimensionTarget {
    pub(super) faces: Vec<PmiTarget>,
    /// The faces come from edge links and the shared-face exclusion.
    pub(super) from_edges: bool,
}

fn resolve_dimension(attachments: &[(u32, u32)], ir: &CadIr) -> Option<DimensionTarget> {
    match attachments {
        [(SURFACE_ATTACHMENT, first), (SURFACE_ATTACHMENT, second)] => {
            let mut faces = surface_faces(ir, *first)?;
            for face in surface_faces(ir, *second)? {
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
            Some(DimensionTarget {
                faces: faces
                    .into_iter()
                    .map(|face| PmiTarget::Face { face })
                    .collect(),
                from_edges: false,
            })
        }
        [(EDGE_ATTACHMENT, first), (EDGE_ATTACHMENT, second)] => {
            let first = edge_faces(ir, *first)?;
            let second = edge_faces(ir, *second)?;
            first.intersection(&second).next()?;
            let mut faces = Vec::new();
            for (own, other) in [(&first, &second), (&second, &first)] {
                let mut unshared = own.difference(other);
                let (Some(face), None) = (unshared.next(), unshared.next()) else {
                    return None;
                };
                faces.push(PmiTarget::Face { face: face.clone() });
            }
            Some(DimensionTarget {
                faces,
                from_edges: true,
            })
        }
        _ => None,
    }
}

/// The bounded faces of surface `id`. A visible surface supplies its own
/// faces. A datum cylinder supplies the visible faces whose cylinder carrier
/// has its axis line and radius. An identifier that names a visible surface
/// with faces and a datum surface selects no faces.
fn surface_faces(ir: &CadIr, id: u32) -> Option<Vec<FaceId>> {
    let visible =
        crate::identity::compose_checked::<SurfaceId>(&crate::identity::VISIBGEOM_SURFACE, id)
            .ok()?;
    let datum =
        crate::identity::compose_checked::<SurfaceId>(&crate::identity::ACTDATUM_SURFACE, id)
            .ok()?;
    let faces_on = |predicate: &dyn Fn(&SurfaceId) -> bool| -> Vec<FaceId> {
        ir.model
            .faces
            .iter()
            .filter(|face| predicate(&face.surface))
            .map(|face| face.id.clone())
            .collect()
    };
    let faces = faces_on(&|surface| *surface == visible);
    if !faces.is_empty() {
        let names_datum = ir.model.surfaces.iter().any(|surface| surface.id == datum);
        return (!names_datum).then_some(faces);
    }
    let datum_cylinder = cylinder(ir, &datum)?;
    let faces = faces_on(&|surface| {
        surface.as_str().starts_with(VISIBLE_SURFACE_PREFIX)
            && cylinder(ir, surface).is_some_and(|carrier| same_cylinder(carrier, datum_cylinder))
    });
    (!faces.is_empty()).then_some(faces)
}

fn cylinder<'a>(ir: &'a CadIr, id: &SurfaceId) -> Option<&'a CylinderSurface> {
    match &ir
        .model
        .surfaces
        .iter()
        .find(|surface| &surface.id == id)?
        .geometry
    {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => Some(cylinder),
        _ => None,
    }
}

/// Whether two cylinders share their axis line and radius.
fn same_cylinder(first: &CylinderSurface, second: &CylinderSurface) -> bool {
    let axis = <[f64; 3]>::from(*first.frame().axis().as_raw());
    let other_axis = <[f64; 3]>::from(*second.frame().axis().as_raw());
    let origin = <[f64; 3]>::from(first.origin().get());
    let other_origin = <[f64; 3]>::from(second.origin().get());
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let delta = [0, 1, 2].map(|index| other_origin[index] - origin[index]);
    let along = dot(delta, axis);
    let offset = [0, 1, 2].map(|index| delta[index] - axis[index] * along);
    (first.radius().get() - second.radius().get()).abs() <= CARRIER_COINCIDENCE_TOLERANCE_MM
        && (dot(axis, other_axis).abs() - 1.0).abs() <= AXIS_PARALLEL_TOLERANCE
        && dot(offset, offset).sqrt() <= CARRIER_COINCIDENCE_TOLERANCE_MM
}

fn edge_faces(ir: &CadIr, id: u32) -> Option<BTreeSet<FaceId>> {
    let edge =
        crate::identity::compose_checked::<EdgeId>(&crate::identity::VISIBGEOM_EDGE, id).ok()?;
    let loops: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge)
        .map(|coedge| &coedge.owner_loop)
        .collect();
    let faces: BTreeSet<_> = ir
        .model
        .loops
        .iter()
        .filter(|item| loops.contains(&item.id))
        .map(|item| item.face.clone())
        .collect();
    (!faces.is_empty()).then_some(faces)
}

/// The faces of every dimension slot whose targets are the same in each section
/// that defines the slot.
pub(super) fn dimension_targets_by_slot(
    targets: &BTreeMap<(&str, u32), DimensionTarget>,
) -> BTreeMap<u32, Vec<PmiTarget>> {
    let mut by_slot = BTreeMap::<u32, Option<Vec<PmiTarget>>>::new();
    for ((_, slot), target) in targets {
        by_slot
            .entry(*slot)
            .and_modify(|stored| {
                if stored.as_ref() != Some(&target.faces) {
                    *stored = None;
                }
            })
            .or_insert_with(|| Some(target.faces.clone()));
    }
    by_slot
        .into_iter()
        .filter_map(|(slot, faces)| Some((slot, faces?)))
        .collect()
}

#[cfg(test)]
mod tests;
