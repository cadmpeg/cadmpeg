// SPDX-License-Identifier: Apache-2.0
//! Validation of an IR document using only in-IR arithmetic.
//!
//! These checks need no geometry kernel: referential integrity of the topology
//! graph, loop-ring closure, coedge pairing, unit presence, and cheap geometric
//! sanity (non-degenerate directions, positive radii, well-formed NURBS pole
//! counts). Anything requiring true geometric evaluation (does a pcurve lie on
//! its surface, do faces actually bound a closed solid) is out of scope and is
//! deliberately *not* faked here.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::document::CadIr;
use crate::geometry::{CurveGeometry, SurfaceGeometry};
use crate::math::Vector3;
use crate::report::{Check, Finding, LossNote, Severity, ValidationReport};
use crate::topology::Coedge;
use crate::units::LengthUnit;
use sha2::{Digest, Sha256};

/// A radius/length that is not a finite positive number is invalid geometry.
/// Written without a negated comparison operator so it stays clippy-clean while
/// still rejecting NaN and non-positive values.
fn nonpositive(x: f64) -> bool {
    !(x.is_finite() && x > 0.0)
}

/// Validate `ir`, returning a report. `losses` are propagated into the report
/// unchanged (e.g. loss notes from the decode that produced `ir`).
pub fn validate(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    let mut findings = Vec::new();

    let ids = IdSets::build(ir);
    check_units(ir, &mut findings);
    check_references(ir, &ids, &mut findings);
    check_loops(ir, &mut findings);
    check_coedge_pairing(ir, &mut findings);
    check_bounds(ir, &mut findings);
    check_tessellations(ir, &mut findings);
    check_feature_input_lanes(ir, &mut findings);
    check_design_records(ir, &mut findings);
    check_unknown_payloads(ir, &mut findings);

    ValidationReport {
        entity_counts: entity_counts(ir),
        findings,
        losses,
    }
}

fn check_design_records(ir: &CadIr, findings: &mut Vec<Finding>) {
    let record_indices = ir
        .design_record_headers
        .iter()
        .map(|record| record.record_index)
        .collect::<HashSet<_>>();
    for header in &ir.design_entity_headers {
        if let Some(declared) = header.declared_reference_count {
            if declared as usize != header.reference_indices.len() {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: "sketch container reference count does not match its reference run"
                        .into(),
                    entity: Some(header.entity_id.clone()),
                });
            }
        }
        if header
            .reference_indices
            .iter()
            .any(|index| !record_indices.contains(index))
        {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "sketch container references an absent Design record".into(),
                entity: Some(header.entity_id.clone()),
            });
        }
    }
    let sketch_owners = ir
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(crate::design::DesignObjectKind::Sketch))
        .map(|header| header.entity_suffix as u32)
        .collect::<HashSet<_>>();
    for relation in &ir.sketch_relations {
        if !sketch_owners.contains(&relation.owner_reference) || relation.raw_bytes.len() != 101 {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "sketch relation references an absent owner or has an invalid byte frame"
                    .into(),
                entity: Some(relation.record_index.to_string()),
            });
        }
    }
    for point in &ir.sketch_points {
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "sketch point contains a non-finite coordinate".into(),
                entity: Some(point.record_index.to_string()),
            });
        }
    }
    for curve in &ir.sketch_curve_identities {
        let valid = match &curve.geometry {
            None => true,
            Some(crate::design::SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            }) => {
                [start.x, start.y, start.z, end.x, end.y, end.z]
                    .into_iter()
                    .all(f64::is_finite)
                    && (direction.norm() - 1.0).abs() <= 1.0e-9
                    && (normal.norm() - 1.0).abs() <= 1.0e-9
                    && ((end.x - start.x).powi(2)
                        + (end.y - start.y).powi(2)
                        + (end.z - start.z).powi(2))
                    .sqrt()
                        > 0.0
            }
            Some(crate::design::SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            }) => {
                [
                    center.x,
                    center.y,
                    center.z,
                    *radius,
                    *start_angle,
                    *end_angle,
                ]
                .into_iter()
                .all(f64::is_finite)
                    && *radius > 0.0
                    && (normal.norm() - 1.0).abs() <= 1.0e-9
                    && (reference_direction.norm() - 1.0).abs() <= 1.0e-9
            }
            Some(crate::design::SketchCurveGeometry::Nurbs {
                degree,
                fit_tolerance,
                knots,
                weights,
                control_points,
                ..
            }) => {
                fit_tolerance.is_finite()
                    && knots.len() == control_points.len() + *degree as usize + 1
                    && (weights.is_empty() || weights.len() == control_points.len())
                    && knots.windows(2).all(|pair| pair[0] <= pair[1])
                    && weights
                        .iter()
                        .all(|weight| weight.is_finite() && *weight > 0.0)
            }
        };
        if !valid {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "sketch curve contains an invalid exact geometry frame".into(),
                entity: Some(curve.record_index.to_string()),
            });
        }
    }
}

fn check_unknown_payloads(ir: &CadIr, findings: &mut Vec<Finding>) {
    for record in &ir.unknowns {
        let Some(data) = &record.data else { continue };
        let hash = Sha256::digest(data)
            .iter()
            .fold(String::new(), |mut acc, byte| {
                use std::fmt::Write as _;
                let _ = write!(acc, "{byte:02x}");
                acc
            });
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: "preserved payload length or hash does not match its record".into(),
                entity: Some(record.id.0.clone()),
            });
        }
    }
}

macro_rules! define_registered_entity_counts {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
        fn registered_entity_counts(ir: &CadIr) -> BTreeMap<String, usize> {
            BTreeMap::from([
                $((stringify!($field).into(), ir.$field.len())),*
            ])
        }
    };
}
crate::document::arena_registry!(define_registered_entity_counts);

fn entity_counts(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut m = registered_entity_counts(ir);
    m.insert(
        "asm_history_states".into(),
        ir.asm_histories
            .iter()
            .map(|history| history.states.len())
            .sum(),
    );
    m.insert(
        "asm_history_changes".into(),
        ir.asm_histories
            .iter()
            .flat_map(|history| &history.states)
            .flat_map(|state| &state.bulletin_boards)
            .map(|board| board.changes.len())
            .sum(),
    );
    m.insert(
        "asm_history_records".into(),
        ir.asm_histories
            .iter()
            .flat_map(|history| &history.states)
            .map(|state| state.records.len())
            .sum(),
    );
    let unknown_surfaces = ir
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Unknown { .. }))
        .count();
    m.insert("surfaces_unknown_geometry".into(), unknown_surfaces);
    m
}

fn check_feature_input_lanes(ir: &CadIr, findings: &mut Vec<Finding>) {
    const MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];
    for lane in &ir.feature_input_lanes {
        for entity in &lane.sketch_entities {
            let Ok(offset) = usize::try_from(entity.offset) else {
                findings.push(Finding {
                    check: Check::Bounds,
                    severity: Severity::Error,
                    message: "feature-input entity offset exceeds address space".into(),
                    entity: Some(lane.id.clone()),
                });
                continue;
            };
            let marker_matches = offset
                .checked_add(MARKER.len())
                .and_then(|end| lane.native_payload.get(offset..end))
                == Some(MARKER);
            let field_in_bounds = offset
                .checked_add(21)
                .is_some_and(|end| end <= lane.native_payload.len());
            if !marker_matches || !field_in_bounds {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: "feature-input entity is outside its native payload".into(),
                    entity: Some(lane.id.clone()),
                });
            }
        }
    }
}

fn check_tessellations(ir: &CadIr, findings: &mut Vec<Finding>) {
    for mesh in &ir.tessellations {
        if mesh
            .vertices
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "contains a non-finite tessellation vertex".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .triangles
            .iter()
            .flatten()
            .any(|index| *index as usize >= mesh.vertices.len())
        {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "contains an out-of-range tessellation index".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .normals
            .iter()
            .any(|normal| !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "contains a non-finite tessellation normal".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh.channels.iter().any(|channel| {
            channel.data.len() != channel.item_size as usize * channel.count as usize
        }) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: "contains a malformed tessellation channel".into(),
                entity: Some(mesh.id.clone()),
            });
        }
    }
}

/// Presence sets for every arena, keyed by the string id.
struct IdSets {
    bodies: HashSet<String>,
    lumps: HashSet<String>,
    shells: HashSet<String>,
    faces: HashSet<String>,
    loops: HashSet<String>,
    coedges: HashSet<String>,
    edges: HashSet<String>,
    vertices: HashSet<String>,
    points: HashSet<String>,
    surfaces: HashSet<String>,
    curves: HashSet<String>,
    pcurves: HashSet<String>,
    appearances: HashSet<String>,
    unknowns: HashSet<String>,
}

impl IdSets {
    fn build(ir: &CadIr) -> Self {
        IdSets {
            bodies: ir.bodies.iter().map(|e| e.id.0.clone()).collect(),
            lumps: ir.lumps.iter().map(|e| e.id.0.clone()).collect(),
            shells: ir.shells.iter().map(|e| e.id.0.clone()).collect(),
            faces: ir.faces.iter().map(|e| e.id.0.clone()).collect(),
            loops: ir.loops.iter().map(|e| e.id.0.clone()).collect(),
            coedges: ir.coedges.iter().map(|e| e.id.0.clone()).collect(),
            edges: ir.edges.iter().map(|e| e.id.0.clone()).collect(),
            vertices: ir.vertices.iter().map(|e| e.id.0.clone()).collect(),
            points: ir.points.iter().map(|e| e.id.0.clone()).collect(),
            surfaces: ir.surfaces.iter().map(|e| e.id.0.clone()).collect(),
            curves: ir.curves.iter().map(|e| e.id.0.clone()).collect(),
            pcurves: ir.pcurves.iter().map(|e| e.id.0.clone()).collect(),
            appearances: ir.appearances.iter().map(|e| e.id.0.clone()).collect(),
            unknowns: ir.unknowns.iter().map(|e| e.id.0.clone()).collect(),
        }
    }
}

fn ref_error(findings: &mut Vec<Finding>, owner: &str, target_kind: &str, target: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: format!("references missing {target_kind} `{target}`"),
        entity: Some(owner.to_string()),
    });
}

fn check_units(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.units.length != LengthUnit::Millimeter {
        findings.push(Finding {
            check: Check::Units,
            severity: Severity::Warning,
            message: format!(
                "document length unit is {:?}, not the canonical millimeter",
                ir.units.length
            ),
            entity: None,
        });
    }
    if nonpositive(ir.tolerances.resabs) {
        findings.push(Finding {
            check: Check::Units,
            severity: Severity::Warning,
            message: "resabs tolerance is not positive".into(),
            entity: None,
        });
    }
}

fn check_references(ir: &CadIr, ids: &IdSets, findings: &mut Vec<Finding>) {
    for b in &ir.bodies {
        for l in &b.lumps {
            if !ids.lumps.contains(&l.0) {
                ref_error(findings, &b.id.0, "lump", &l.0);
            }
        }
    }
    for l in &ir.lumps {
        if !ids.bodies.contains(&l.body.0) {
            ref_error(findings, &l.id.0, "body", &l.body.0);
        }
        for s in &l.shells {
            if !ids.shells.contains(&s.0) {
                ref_error(findings, &l.id.0, "shell", &s.0);
            }
        }
    }
    for s in &ir.shells {
        if !ids.lumps.contains(&s.lump.0) {
            ref_error(findings, &s.id.0, "lump", &s.lump.0);
        }
        for f in &s.faces {
            if !ids.faces.contains(&f.0) {
                ref_error(findings, &s.id.0, "face", &f.0);
            }
        }
        for e in &s.wire_edges {
            if !ids.edges.contains(&e.0) {
                ref_error(findings, &s.id.0, "wire edge", &e.0);
            }
        }
        for v in &s.free_vertices {
            if !ids.vertices.contains(&v.0) {
                ref_error(findings, &s.id.0, "free vertex", &v.0);
            }
        }
    }
    for f in &ir.faces {
        if !ids.shells.contains(&f.shell.0) {
            ref_error(findings, &f.id.0, "shell", &f.shell.0);
        }
        if !ids.surfaces.contains(&f.surface.0) {
            ref_error(findings, &f.id.0, "surface", &f.surface.0);
        }
        for lp in &f.loops {
            if !ids.loops.contains(&lp.0) {
                ref_error(findings, &f.id.0, "loop", &lp.0);
            }
        }
    }
    for lp in &ir.loops {
        if !ids.faces.contains(&lp.face.0) {
            ref_error(findings, &lp.id.0, "face", &lp.face.0);
        }
        for ce in &lp.coedges {
            if !ids.coedges.contains(&ce.0) {
                ref_error(findings, &lp.id.0, "coedge", &ce.0);
            }
        }
    }
    for ce in &ir.coedges {
        if !ids.loops.contains(&ce.owner_loop.0) {
            ref_error(findings, &ce.id.0, "loop", &ce.owner_loop.0);
        }
        if !ids.edges.contains(&ce.edge.0) {
            ref_error(findings, &ce.id.0, "edge", &ce.edge.0);
        }
        if !ids.coedges.contains(&ce.next.0) {
            ref_error(findings, &ce.id.0, "coedge(next)", &ce.next.0);
        }
        if !ids.coedges.contains(&ce.previous.0) {
            ref_error(findings, &ce.id.0, "coedge(previous)", &ce.previous.0);
        }
        if let Some(p) = &ce.partner {
            if !ids.coedges.contains(&p.0) {
                ref_error(findings, &ce.id.0, "coedge(partner)", &p.0);
            }
        }
        if let Some(radial_next) = &ce.radial_next {
            if !ids.coedges.contains(&radial_next.0) {
                ref_error(findings, &ce.id.0, "coedge(radial_next)", &radial_next.0);
            }
        }
        if let Some(pc) = &ce.pcurve {
            if !ids.pcurves.contains(&pc.0) {
                ref_error(findings, &ce.id.0, "pcurve", &pc.0);
            }
        }
    }
    for e in &ir.edges {
        if let Some(c) = &e.curve {
            if !ids.curves.contains(&c.0) {
                ref_error(findings, &e.id.0, "curve", &c.0);
            }
        }
        if !ids.vertices.contains(&e.start.0) {
            ref_error(findings, &e.id.0, "vertex(start)", &e.start.0);
        }
        if !ids.vertices.contains(&e.end.0) {
            ref_error(findings, &e.id.0, "vertex(end)", &e.end.0);
        }
    }
    for v in &ir.vertices {
        if !ids.points.contains(&v.point.0) {
            ref_error(findings, &v.id.0, "point", &v.point.0);
        }
    }
    for binding in &ir.appearance_bindings {
        use crate::appearance::AppearanceTarget;
        let owner = format!("appearance-binding:{}", binding.appearance.0);
        if !ids.appearances.contains(&binding.appearance.0) {
            ref_error(findings, &owner, "appearance", &binding.appearance.0);
        }
        match &binding.target {
            AppearanceTarget::Body(body) if !ids.bodies.contains(&body.0) => {
                ref_error(findings, &owner, "body", &body.0);
            }
            AppearanceTarget::Face(face) if !ids.faces.contains(&face.0) => {
                ref_error(findings, &owner, "face", &face.0);
            }
            _ => {}
        }
    }
    for attribute in &ir.attributes {
        use crate::attributes::AttributeTarget;
        let owner = &attribute.id.0;
        match &attribute.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if !ids.bodies.contains(&id.0) => {
                ref_error(findings, owner, "body", &id.0);
            }
            AttributeTarget::Face(id) if !ids.faces.contains(&id.0) => {
                ref_error(findings, owner, "face", &id.0);
            }
            AttributeTarget::Coedge(id) if !ids.coedges.contains(&id.0) => {
                ref_error(findings, owner, "coedge", &id.0);
            }
            AttributeTarget::Edge(id) if !ids.edges.contains(&id.0) => {
                ref_error(findings, owner, "edge", &id.0);
            }
            AttributeTarget::Vertex(id) if !ids.vertices.contains(&id.0) => {
                ref_error(findings, owner, "vertex", &id.0);
            }
            _ => {}
        }
    }
    for link in &ir.persistent_design_links {
        use crate::attributes::AttributeTarget;
        let owner = format!("persistent-design-link:{}", link.design_id);
        match &link.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if !ids.bodies.contains(&id.0) => {
                ref_error(findings, &owner, "body", &id.0);
            }
            AttributeTarget::Face(id) if !ids.faces.contains(&id.0) => {
                ref_error(findings, &owner, "face", &id.0);
            }
            AttributeTarget::Coedge(id) if !ids.coedges.contains(&id.0) => {
                ref_error(findings, &owner, "coedge", &id.0);
            }
            AttributeTarget::Edge(id) if !ids.edges.contains(&id.0) => {
                ref_error(findings, &owner, "edge", &id.0);
            }
            AttributeTarget::Vertex(id) if !ids.vertices.contains(&id.0) => {
                ref_error(findings, &owner, "vertex", &id.0);
            }
            _ => {}
        }
    }
    for s in &ir.surfaces {
        if let SurfaceGeometry::Unknown { record: Some(u) } = &s.geometry {
            if !ids.unknowns.contains(&u.0) {
                ref_error(findings, &s.id.0, "unknown record", &u.0);
            }
        }
    }
    for parameterization in &ir.surface_parameterizations {
        if !ids.surfaces.contains(&parameterization.surface.0) {
            ref_error(
                findings,
                &parameterization.surface.0,
                "surface",
                &parameterization.surface.0,
            );
        }
    }
    for procedural in &ir.procedural_surfaces {
        if !ids.surfaces.contains(&procedural.surface.0) {
            ref_error(
                findings,
                &procedural.surface.0,
                "surface",
                &procedural.surface.0,
            );
        }
    }
    for procedural in &ir.procedural_curves {
        if !ids.curves.contains(&procedural.curve.0) {
            ref_error(findings, &procedural.curve.0, "curve", &procedural.curve.0);
        }
    }
    for link in &ir.sketch_curve_links {
        if !ids.coedges.contains(&link.coedge.0) {
            ref_error(findings, &link.coedge.0, "coedge", &link.coedge.0);
        }
    }
}

fn check_loops(ir: &CadIr, findings: &mut Vec<Finding>) {
    let by_id: HashMap<&str, &Coedge> = ir.coedges.iter().map(|c| (c.id.0.as_str(), c)).collect();

    for lp in &ir.loops {
        if lp.coedges.is_empty() {
            findings.push(Finding {
                check: Check::LoopClosure,
                severity: Severity::Error,
                message: "loop has no coedges".into(),
                entity: Some(lp.id.0.clone()),
            });
            continue;
        }
        // Walk the `next` chain from the first listed coedge and confirm it is a
        // simple cycle whose members are exactly the loop's coedge set.
        let expected: HashSet<&str> = lp.coedges.iter().map(|c| c.0.as_str()).collect();
        let start = lp.coedges[0].0.as_str();
        let mut visited: HashSet<&str> = HashSet::new();
        let mut cur = start;
        let mut broke = false;
        for _ in 0..lp.coedges.len() {
            if !visited.insert(cur) {
                break; // returned early to an already-seen node
            }
            match by_id.get(cur) {
                Some(ce) => cur = ce.next.0.as_str(),
                None => {
                    broke = true; // dangling next; referential check already flags it
                    break;
                }
            }
        }
        if broke {
            continue;
        }
        if visited != expected || cur != start {
            findings.push(Finding {
                check: Check::LoopClosure,
                severity: Severity::Error,
                message: format!(
                    "coedge `next` ring does not close over the loop's {} coedges",
                    lp.coedges.len()
                ),
                entity: Some(lp.id.0.clone()),
            });
        }
    }
}

fn check_coedge_pairing(ir: &CadIr, findings: &mut Vec<Finding>) {
    let by_id: HashMap<&str, &Coedge> = ir.coedges.iter().map(|c| (c.id.0.as_str(), c)).collect();

    for ce in &ir.coedges {
        let Some(partner_id) = &ce.partner else {
            continue;
        };
        let Some(partner) = by_id.get(partner_id.0.as_str()) else {
            continue; // dangling; referential check flags it
        };
        // Partner must point back at us.
        match &partner.partner {
            Some(back) if back.0 == ce.id.0 => {}
            _ => findings.push(Finding {
                check: Check::CoedgePairing,
                severity: Severity::Error,
                message: format!("partner `{}` does not point back", partner_id.0),
                entity: Some(ce.id.0.clone()),
            }),
        }
        // Partners must share the same edge.
        if partner.edge.0 != ce.edge.0 {
            findings.push(Finding {
                check: Check::CoedgePairing,
                severity: Severity::Error,
                message: format!("partner `{}` references a different edge", partner_id.0),
                entity: Some(ce.id.0.clone()),
            });
        }
        // Partners on a manifold edge run in opposite senses.
        if partner.sense == ce.sense {
            findings.push(Finding {
                check: Check::CoedgePairing,
                severity: Severity::Warning,
                message: format!(
                    "partner `{}` has the same sense (non-manifold or mis-decoded)",
                    partner_id.0
                ),
                entity: Some(ce.id.0.clone()),
            });
        }
    }
}

fn degenerate(v: &Vector3) -> bool {
    v.norm() <= f64::EPSILON
}

fn check_bounds(ir: &CadIr, findings: &mut Vec<Finding>) {
    for (id, tolerance) in ir
        .vertices
        .iter()
        .map(|entity| (&entity.id.0, entity.tolerance))
        .chain(
            ir.edges
                .iter()
                .map(|entity| (&entity.id.0, entity.tolerance)),
        )
        .chain(
            ir.faces
                .iter()
                .map(|entity| (&entity.id.0, entity.tolerance)),
        )
    {
        if tolerance.is_some_and(|value| !value.is_finite() || value < 0.0) {
            bounds_err(findings, id, "topology tolerance is negative or non-finite");
        }
    }
    for s in &ir.surfaces {
        match &s.geometry {
            SurfaceGeometry::Plane { normal, u_axis, .. } => {
                if degenerate(normal) {
                    bounds_err(findings, &s.id.0, "plane normal is degenerate");
                }
                if u_axis.is_some_and(|direction| degenerate(&direction)) {
                    bounds_err(findings, &s.id.0, "plane u axis is degenerate");
                }
            }
            SurfaceGeometry::Cylinder {
                axis,
                ref_direction,
                radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "cylinder axis is degenerate");
                }
                if ref_direction.is_some_and(|direction| degenerate(&direction)) {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "cylinder reference direction is degenerate",
                    );
                }
                if nonpositive(*radius) {
                    bounds_err(findings, &s.id.0, "cylinder radius is not positive");
                }
            }
            SurfaceGeometry::Cone {
                axis,
                ref_direction,
                radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "cone axis is degenerate");
                }
                if ref_direction.is_some_and(|direction| degenerate(&direction)) {
                    bounds_err(findings, &s.id.0, "cone reference direction is degenerate");
                }
                if *radius < 0.0 {
                    bounds_err(findings, &s.id.0, "cone radius is negative");
                }
            }
            SurfaceGeometry::Sphere {
                axis,
                ref_direction,
                radius,
                ..
            } => {
                if axis.is_some_and(|direction| degenerate(&direction)) {
                    bounds_err(findings, &s.id.0, "sphere axis is degenerate");
                }
                if ref_direction.is_some_and(|direction| degenerate(&direction)) {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "sphere reference direction is degenerate",
                    );
                }
                if radius.abs() <= f64::EPSILON {
                    bounds_err(findings, &s.id.0, "sphere radius is zero");
                }
            }
            SurfaceGeometry::Torus {
                axis,
                ref_direction,
                major_radius,
                minor_radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "torus axis is degenerate");
                }
                if ref_direction.is_some_and(|direction| degenerate(&direction)) {
                    bounds_err(findings, &s.id.0, "torus reference direction is degenerate");
                }
                if nonpositive(*major_radius) || minor_radius.abs() <= f64::EPSILON {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "torus major radius is not positive or minor radius is zero",
                    );
                }
            }
            SurfaceGeometry::Nurbs(n) => {
                let expected = (n.u_count as usize) * (n.v_count as usize);
                if n.control_points.len() != expected {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "NURBS surface pole count does not match u_count*v_count",
                    );
                }
                check_knots(findings, &s.id.0, &n.u_knots, "u");
                check_knots(findings, &s.id.0, &n.v_knots, "v");
            }
            // An unknown surface carries no numeric geometry to bounds-check; its
            // record link is checked in `check_references`. A face resting on it
            // is legal (topology known, shape opaque).
            SurfaceGeometry::Unknown { .. } => {}
        }
    }
    for c in &ir.curves {
        match &c.geometry {
            CurveGeometry::Line { direction, .. } => {
                if degenerate(direction) {
                    bounds_err(findings, &c.id.0, "line direction is degenerate");
                }
            }
            CurveGeometry::Circle { axis, radius, .. } => {
                if degenerate(axis) {
                    bounds_err(findings, &c.id.0, "circle axis is degenerate");
                }
                if nonpositive(*radius) {
                    bounds_err(findings, &c.id.0, "circle radius is not positive");
                }
            }
            CurveGeometry::Ellipse {
                major_radius,
                minor_radius,
                ..
            } => {
                if nonpositive(*major_radius) || nonpositive(*minor_radius) {
                    bounds_err(findings, &c.id.0, "ellipse radius is not positive");
                }
            }
            CurveGeometry::Parabola {
                axis,
                major_direction,
                focal_distance,
                ..
            } => {
                if degenerate(axis) || degenerate(major_direction) {
                    bounds_err(findings, &c.id.0, "parabola frame is degenerate");
                }
                if nonpositive(*focal_distance) {
                    bounds_err(findings, &c.id.0, "parabola focal distance is not positive");
                }
            }
            CurveGeometry::Hyperbola {
                axis,
                major_direction,
                major_radius,
                minor_radius,
                ..
            } => {
                if degenerate(axis) || degenerate(major_direction) {
                    bounds_err(findings, &c.id.0, "hyperbola frame is degenerate");
                }
                if nonpositive(*major_radius) || nonpositive(*minor_radius) {
                    bounds_err(findings, &c.id.0, "hyperbola radius is not positive");
                }
            }
            CurveGeometry::Nurbs(n) => {
                if n.control_points.len() < (n.degree as usize + 1) {
                    bounds_err(
                        findings,
                        &c.id.0,
                        "NURBS curve has too few poles for its degree",
                    );
                }
                check_knots(findings, &c.id.0, &n.knots, "");
            }
        }
    }
}

fn check_knots(findings: &mut Vec<Finding>, id: &str, knots: &[f64], dir: &str) {
    if knots.windows(2).any(|w| w[1] < w[0]) {
        let label = if dir.is_empty() {
            "knot vector is not non-decreasing".to_string()
        } else {
            format!("{dir}-knot vector is not non-decreasing")
        };
        bounds_err(findings, id, &label);
    }
}

fn bounds_err(findings: &mut Vec<Finding>, id: &str, msg: &str) {
    findings.push(Finding {
        check: Check::Bounds,
        severity: Severity::Error,
        message: msg.to_string(),
        entity: Some(id.to_string()),
    });
}
