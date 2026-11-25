// SPDX-License-Identifier: Apache-2.0
//! Surfacing, wireframe, forms, and meshes (§6.6).

use super::options::{self, BooleanAt};
use super::picks;
use super::{error, swept, Site};
use crate::design::dimension::Dimension;
use crate::design::ops::shared::Law;
use crate::design::ops::surface::{
    BoundaryFill, CircleMode, CurveDefinition, CurveFeature, Extend, ExtendLimit, Form,
    ImplicitSection, ImplicitSweep, Knit, LineLength, LineMode, MeshFeature, MeshKind, Patch,
    PatchBounds, Unsew,
};
use crate::design::options::{BooleanOp, SectionCondition, Solidify};
use crate::design::selection::SetSelection;
use crate::report::check::{Check, Finding};

use super::super::records::DesignRecords;

pub(super) fn check_patch(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    patch: &Patch,
    findings: &mut Vec<Finding>,
) {
    let mut free = true;
    match &patch.bounds {
        PatchBounds::Profile(profile) => {
            options::check_profile(site, "PatchBounds::Profile", profile, findings);
        }
        PatchBounds::Loops { outer, inner } => {
            if outer.is_empty() || inner.iter().any(Vec::is_empty) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.patchbounds-loops-loop-empty",
                        "a PatchBounds::Loops loop is empty"
                    ),
                );
            }
            for edge in outer.iter().chain(inner.iter().flatten()) {
                options::check_path(site, "PatchEdge.curve", &edge.curve, findings);
                free &= matches!(edge.condition, None | Some(SectionCondition::Free { .. }));
            }
        }
    }
    if matches!(patch.planar, Some(Dimension::Literal(true))) && !free {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.patch-planar-true-where-condition-other",
                "Patch.planar is true where a condition is other than Free"
            ),
        );
    }
    options::check_sheet_output(index, site, &patch.output, findings);
}

pub(super) fn check_knit(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    knit: &Knit,
    findings: &mut Vec<Finding>,
) {
    let closes = matches!(
        knit.output.solid,
        Some(Solidify::IfClosed | Solidify::CapPlanarOpenings)
    );
    if !closes && picks::count(&knit.elements).is_some_and(|count| count < 2) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.knit-elements-holds-fewer-two-picks",
                "Knit.elements holds fewer than two picks"
            ),
        );
    }
    if knit.healing.is_some() && knit.tolerance.is_none() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.knit-healing-occurs-without-tolerance",
                "Knit.healing occurs without a tolerance"
            ),
        );
    }
    options::check_sheet_output(index, site, &knit.output, findings);
}

pub(super) fn check_unsew(site: Site<'_>, unsew: &Unsew, findings: &mut Vec<Finding>) {
    if picks::is_empty(&unsew.edges) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.unsew-edges-picks-no-edge",
                "Unsew.edges picks no edge"
            ),
        );
    }
    if let Some(class) = picks::refused(&unsew.edges, picks::is_edge) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.unsew-edges-picks-not-edge",
                "Unsew.edges picks {class}, not an edge"
            ),
        );
    }
}

/// `UpTo` limits select points, faces, or planes.
fn check_up_to(site: Site<'_>, field: &str, selection: &SetSelection, findings: &mut Vec<Finding>) {
    let allowed = |class| picks::is_point(class) || picks::is_plane(class);
    if let Some(class) = picks::refused(selection, allowed) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.selects-selects-points-faces-planes",
                "{field} selects {class}; it selects points, faces, or planes"
            ),
        );
    }
}

pub(super) fn check_extend(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    extend: &Extend,
    findings: &mut Vec<Finding>,
) {
    match &extend.limit {
        Some(ExtendLimit::Distance(distance)) => site.bound(findings, |values| {
            (values.get(distance)?.get() <= 0.0)
                .then(|| "ExtendLimit::Distance is not positive".to_owned())
        }),
        Some(ExtendLimit::UpTo(target)) => {
            check_up_to(site, "ExtendLimit::UpTo", target, findings);
        }
        _ => {}
    }
    if let Some(output) = &extend.output {
        options::check_sheet_output(index, site, output, findings);
    }
}

/// The values a constant or linear law states.
fn law_values<T>(law: &Law<T>) -> Vec<&Dimension<T>> {
    match law {
        Law::Constant(value) => vec![value],
        Law::Linear { start, end } => vec![start, end],
        _ => Vec::new(),
    }
}

pub(super) fn check_implicit_sweep(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    sweep: &ImplicitSweep,
    findings: &mut Vec<Finding>,
) {
    if sweep.solution == Some(0) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.implicitsweep-solution-0-least-1",
                "ImplicitSweep.solution is 0; it is at least 1"
            ),
        );
    }
    swept::check_corner(
        site,
        "ImplicitSweep.corner",
        sweep.corner.as_ref(),
        findings,
    );
    if let Some(spine) = &sweep.spine {
        options::check_path(site, "ImplicitSweep.spine", spine, findings);
    }
    swept::check_spine_limits(site, "ImplicitSweep.limits", &sweep.limits, findings);
    match &sweep.section {
        ImplicitSection::Line { mode, lengths } => {
            let angled = match mode {
                LineMode::Curve { from, angle, .. } => from.is_none() || angle.is_some(),
                _ => true,
            };
            if !angled {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.linemode-curve-occurs-without-angle",
                        "LineMode::Curve.from occurs without an angle"
                    ),
                );
            }
            for length in lengths.iter().flatten() {
                if let LineLength::UpTo(target) = length {
                    check_up_to(site, "LineLength::UpTo", target, findings);
                }
            }
        }
        ImplicitSection::Circle(CircleMode::CenterAngles { from, angles, .. }) => {
            let stated = angles.iter().filter(|angle| angle.is_some()).count();
            if stated == 1 {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.circlemode-centerangles-states-one-angle-states",
                        "CircleMode::CenterAngles states one angle; it states both or neither"
                    ),
                );
            }
            if from.is_some() && stated == 0 {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.circlemode-centerangles-occurs-without-angle",
                        "CircleMode::CenterAngles.from occurs without an angle"
                    ),
                );
            }
        }
        ImplicitSection::Conic {
            guides, parameter, ..
        } => {
            if !(2..=5).contains(&guides.len()) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.implicitsection-conic-guides-two-five",
                        "an ImplicitSection::Conic has {} guides; it has two to five",
                        guides.len()
                    ),
                );
            }
            if parameter.is_some() && guides.len() != 2 {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.implicitsection-conic-states-parameter-other-two",
                        "an ImplicitSection::Conic states a parameter with other than two guides"
                    ),
                );
            }
            for value in parameter.iter().flat_map(law_values) {
                site.bound(findings, |values| {
                    let value = values.get(value)?.get();
                    (!(value > 0.0 && value < 1.0)).then(|| {
                        "an ImplicitSection::Conic parameter lies outside (0, 1)".to_owned()
                    })
                });
            }
        }
        _ => {}
    }
    options::check_solid_output(index, site, &sweep.output, findings);
}

pub(super) fn check_boundary_fill(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    fill: &BoundaryFill,
    findings: &mut Vec<Finding>,
) {
    if fill.cells.is_empty() || fill.cells.iter().any(picks::is_empty) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.boundaryfill-cells-one-cell-selections-empty",
                "BoundaryFill.cells or one of its cell selections is empty"
            ),
        );
    }
    if let Some(boolean) = &fill.boolean {
        options::check_boolean(index, site, boolean, BooleanAt::Own, findings);
    }
}

pub(super) fn check_curve(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    curve: &CurveFeature,
    findings: &mut Vec<Finding>,
) {
    match &curve.definition {
        CurveDefinition::Spline(spline) => {
            let closed = matches!(spline.closed, Some(Dimension::Literal(true)));
            let least = if closed { 3 } else { 2 };
            if spline.points.len() < least {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.splinecurve-points-holds-points-holds-least",
                        "SplineCurve.points holds {} points; it holds at least {least}",
                        spline.points.len()
                    ),
                );
            }
        }
        CurveDefinition::Helix(helix) => {
            if let Some(crate::design::ops::swept::HelixStart::Circle(circle)) = &helix.start {
                swept::check_helix_circle(index, site, circle, findings);
            }
        }
        CurveDefinition::Bridge { ends } => {
            let [first, second] = ends;
            if picks::named(&first.curve).is_some()
                && picks::named(&first.curve) == picks::named(&second.curve)
                && first.at == second.at
            {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.two-bridgeend-selections-one-pick-one",
                        "the two BridgeEnd selections are one pick with one at"
                    ),
                );
            }
        }
        CurveDefinition::Polygon { sides, .. } => site.bound(findings, |values| {
            let sides = values.get(sides)?;
            (sides < 3).then(|| {
                format!("CurveDefinition::Polygon has {sides} sides; a count is at least 3")
            })
        }),
        CurveDefinition::Offset { corner, .. } => {
            swept::check_corner(
                site,
                "CurveDefinition::Offset.corner",
                corner.as_ref(),
                findings,
            );
        }
        _ => {}
    }
}

pub(super) fn check_form(site: Site<'_>, form: &Form, findings: &mut Vec<Finding>) {
    let cage = &form.cage;
    if cage.subds.is_empty() || cage.entity_count() != cage.subds.len() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.form-cage-holds-records-other-subds",
                "Form.cage holds records other than subds, or no subds record"
            ),
        );
    }
    let mut nested = Vec::new();
    crate::validate::topology::check_subdivision_schemes(cage, &mut nested);
    for finding in nested {
        findings.push(crate::validate::nested_finding(
            finding,
            Check::Design,
            site.entity,
            "Form.cage",
        ));
    }
}

pub(super) fn check_mesh(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    mesh: &MeshFeature,
    findings: &mut Vec<Finding>,
) {
    match &mesh.kind {
        MeshKind::Combine { tools, op } => {
            if op.as_ref().is_some_and(|op| {
                !matches!(op, BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect)
            }) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.meshkind-combine-op-other-join-cut",
                        "MeshKind::Combine.op is other than Join, Cut, or Intersect"
                    ),
                );
            }
            if picks::is_empty(tools) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.meshkind-combine-tools-empty",
                        "MeshKind::Combine.tools is empty"
                    ),
                );
            }
        }
        MeshKind::Section { sketch }
            if !index
                .ir
                .design
                .sketches
                .iter()
                .any(|record| &record.id == sketch) =>
        {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.meshkind-section-names-sketch-no-sketch",
                    "MeshKind::Section names sketch {}, which is no sketch record",
                    sketch.as_str()
                ),
            );
        }
        _ => {}
    }
}
