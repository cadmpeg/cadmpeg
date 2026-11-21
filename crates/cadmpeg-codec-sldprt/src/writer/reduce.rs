// SPDX-License-Identifier: Apache-2.0
//! Content the semantic writer cannot hold (§3.4, "Content the target cannot
//! hold"): each item is dropped or degraded with a loss naming it, and the
//! rest of the document is written.

use std::collections::HashMap;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::edit_dropped;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::{Annotations, Exactness};

use crate::loss::SldprtLossCode;

/// The linear resolution within which a vertex fixes an edge end on its
/// carrier when the vertex states no larger tolerance (`sldprt.md`, B-rep
/// edge ranges), in millimetres.
const LINEAR_RESOLUTION_MM: f64 = 1.0e-5;
/// Relative roundoff within which a range end equals a NURBS domain end.
const DOMAIN_END_ROUNDOFF_RELATIVE: f64 = 1.0e-9;

/// The writer bounds every edge by its vertices. A range the decode derives
/// from those vertices is stated by them; any other range is not written.
pub(super) fn edge_ranges_omitted(
    ir: &CadIr,
    annotations: &Annotations,
) -> Result<Option<LossNote>, ResourceLimit> {
    let points = ir
        .geometry
        .points
        .iter()
        .map(|point| (point.id.as_str(), point.position().get()))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .geometry
        .vertices
        .iter()
        .filter_map(|vertex| {
            let bound = vertex.tolerance.map_or(LINEAR_RESOLUTION_MM, |tolerance| {
                tolerance.get().max(LINEAR_RESOLUTION_MM)
            });
            Some((
                vertex.id.as_str(),
                (*points.get(vertex.point.as_str())?, bound),
            ))
        })
        .collect::<HashMap<_, _>>();
    let curves = ir
        .geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let mut omitted = Vec::new();
    for edge in &ir.geometry.edges {
        if edge.param_range().is_none() || derived_range(annotations, edge) {
            continue;
        }
        let Some(curve) = edge.curve() else {
            omitted.push(edge.id.as_str());
            continue;
        };
        let stated = match curves.get(curve.as_str()) {
            Some(CurveGeometry::Solved(geometry)) => {
                range_stated_by_vertices(edge, geometry, &vertices)?
            }
            _ => false,
        };
        if !stated {
            omitted.push(edge.id.as_str());
        }
    }
    Ok((!omitted.is_empty()).then(|| {
        SldprtLossCode::WriterEdgeRangesOmitted.note(format!(
            "{} edge(s) carry a parameter range their vertices do not fix on the curve; each is \
             written bounded by its vertices, without its range: {}",
            omitted.len(),
            omitted.join(", ")
        ))
    }))
}

fn derived_range(annotations: &Annotations, edge: &Edge) -> bool {
    annotations
        .exactness()
        .get(edge.id.as_str())
        .is_some_and(|note| {
            note.entity() == Exactness::Derived
                || note.fields().get("carrier.param_range") == Some(&Exactness::Derived)
        })
}

/// The curve at each range end lies within its vertex's bound. A NURBS
/// carrier that closes on itself gives its closure vertex two parameters, so
/// only its whole domain is stated, by a closed edge.
fn range_stated_by_vertices(
    edge: &Edge,
    geometry: &SolvedCurveGeometry,
    vertices: &HashMap<&str, (Point3, f64)>,
) -> Result<bool, ResourceLimit> {
    let (Some(range), Some(ends)) = (edge.param_range(), edge.ends.as_ref()) else {
        return Ok(false);
    };
    let [start_t, end_t] = range.get();
    let (Some(&(start, start_bound)), Some(&(end, end_bound))) = (
        vertices.get(ends.start.as_str()),
        vertices.get(ends.end.as_str()),
    ) else {
        return Ok(false);
    };
    // A point the carrier states no finite value for fixes no range end.
    let at =
        |parameter| match curve_point_solved(EvaluationAdmission::Standard, geometry, parameter) {
            Ok(point) => Ok(Some(point.get())),
            Err(failure) => failure.non_finite().map(|_| None),
        };
    let (Some(at_start), Some(at_end)) = (at(start_t)?, at(end_t)?) else {
        return Ok(false);
    };
    if Point3::distance(at_start, start) > start_bound || Point3::distance(at_end, end) > end_bound
    {
        return Ok(false);
    }
    let SolvedCurveGeometry::Nurbs(nurbs) = geometry else {
        return Ok(true);
    };
    let Some(domain) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs) else {
        return Ok(false);
    };
    let [low, high] = domain.endpoints();
    let (Some(first), Some(last)) = (at(low)?, at(high)?) else {
        return Ok(false);
    };
    if Point3::distance(first, last) > start_bound.max(end_bound) {
        return Ok(true);
    }
    let roundoff = (high - low) * DOMAIN_END_ROUNDOFF_RELATIVE;
    Ok(ends.start == ends.end
        && (start_t - low).abs() <= roundoff
        && (end_t - high).abs() <= roundoff)
}

/// Remove the content the `SolidWorks` form has no field for from the document
/// the writer writes, and name each record.
pub(super) fn unwritable_content(ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let mut named = Vec::new();
    for face in &mut ir.geometry.faces {
        if face.name.take().is_some() {
            named.push(face.id.as_str().to_owned());
        }
    }
    losses.extend(note(
        SldprtLossCode::WriterFaceNamesOmitted,
        "face(s) have a name the SLDPRT B-rep has no field for; each is written unnamed",
        &named,
    ));
    let mut named = Vec::new();
    for body in &mut ir.geometry.bodies {
        if body.color.is_none() && body.name.take().is_some() {
            named.push(body.id.as_str().to_owned());
        }
    }
    losses.extend(note(
        SldprtLossCode::WriterBodyNamesOmitted,
        "body or bodies have a name and no material, and the SLDPRT writer stores a body name \
         only as its material's; each is written unnamed",
        &named,
    ));
    let mut named = Vec::new();
    for appearance in &mut ir.model.appearances {
        let fields = [
            ("asset_guid", appearance.asset_guid.take().is_some()),
            ("visual_guid", appearance.visual_guid.take().is_some()),
            ("physical_token", appearance.physical_token.take().is_some()),
            ("category", appearance.category.take().is_some()),
            (
                "properties",
                !std::mem::take(&mut appearance.properties).is_empty(),
            ),
        ];
        let dropped = fields
            .iter()
            .filter(|(_, dropped)| *dropped)
            .map(|(field, _)| *field)
            .collect::<Vec<_>>();
        if !dropped.is_empty() {
            named.push(format!(
                "{} ({})",
                appearance.id.as_str(),
                dropped.join(", ")
            ));
        }
    }
    losses.extend(note(
        SldprtLossCode::WriterAppearanceFieldsOmitted,
        "appearance(s) have fields beside a name and base colour that the SLDPRT material \
         record has no field for; each is written with its name and base colour",
        &named,
    ));
    losses.extend(note(
        SldprtLossCode::WriterSubdOmitted,
        "subdivision surface(s) have no SLDPRT form and are not written",
        &std::mem::take(&mut ir.geometry.subds)
            .iter()
            .map(|subd| subd.id.as_str().to_owned())
            .collect::<Vec<_>>(),
    ));
    losses.extend(pmi(ir)?);
    losses.extend(active_configuration(ir));
    Ok(losses)
}

/// PMI with no SWIFT baseline comes from another format: the writer has no
/// PMI encoder, and the arena coverage (`Omitted` off the patched path)
/// charges it. PMI that differs from its baseline is an edit the retained
/// SWIFT records do not hold.
fn pmi(ir: &mut CadIr) -> Result<Option<LossNote>, CodecError> {
    let baseline = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get(super::PMI_LOCAL_DIGEST_ATTRIBUTE))
        .cloned();
    let Some(baseline) = baseline else {
        ir.model.pmi.clear();
        return Ok(None);
    };
    if baseline == super::pmi_local_sha256(ir)? {
        return Ok(None);
    }
    let mut ids = ir
        .model
        .pmi
        .iter()
        .map(|pmi| pmi.id.as_str().to_owned())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        ids.push("PMISemanticDataDB".to_owned());
    }
    Ok(edit_dropped(
        EntityKind::PmiAnnotation,
        "SLDPRT",
        "has no encoder for an edit of the retained SWIFT PMI and holds the PMI as the source \
         file stores it; the PMI digest does not isolate the edited record among these",
        ids,
    ))
}

/// `SolidWorks` stores one active configuration. Where the document states none
/// or several, the first by ordinal among the candidates is written active.
fn active_configuration(ir: &mut CadIr) -> Option<LossNote> {
    let configurations = &mut ir.model.configurations;
    let active = configurations
        .iter()
        .filter(|configuration| configuration.active)
        .count();
    if configurations.is_empty() || active == 1 {
        return None;
    }
    let chosen = configurations
        .iter()
        .filter(|configuration| active == 0 || configuration.active)
        .min_by_key(|configuration| configuration.ordinal)?
        .id
        .clone();
    let stated = configurations
        .iter()
        .filter(|configuration| configuration.active)
        .map(|configuration| configuration.id.as_str().to_owned())
        .collect::<Vec<_>>();
    for configuration in configurations.iter_mut() {
        configuration.active = configuration.id == chosen;
    }
    Some(
        SldprtLossCode::WriterActiveConfigurationAssumed.note(format!(
        "the document states {active} active configuration(s) ({}), and the SLDPRT writer stores \
         exactly one; it writes {} active, the first by ordinal",
        stated.join(", "),
        chosen.as_str()
    )),
    )
}

fn note(code: SldprtLossCode, reason: &str, ids: &[String]) -> Option<LossNote> {
    (!ids.is_empty()).then(|| code.note(format!("{} {reason}: {}", ids.len(), ids.join(", "))))
}
