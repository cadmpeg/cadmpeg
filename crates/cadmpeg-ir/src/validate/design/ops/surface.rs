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
use crate::scalar::Fraction;

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
                    format_args!("a PatchBounds::Loops loop is empty"),
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
            format_args!("Patch.planar is true where a condition is other than Free"),
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
            format_args!("Knit.elements holds fewer than two picks"),
        );
    }
    if knit.healing.is_some() && knit.tolerance.is_none() {
        error(
            findings,
            site.entity,
            format_args!("Knit.healing occurs without a tolerance"),
        );
    }
    options::check_sheet_output(index, site, &knit.output, findings);
}

pub(super) fn check_unsew(site: Site<'_>, unsew: &Unsew, findings: &mut Vec<Finding>) {
    if picks::is_empty(&unsew.edges) {
        error(
            findings,
            site.entity,
            format_args!("Unsew.edges picks no edge"),
        );
    }
    if let Some(class) = picks::refused(&unsew.edges, picks::is_edge) {
        error(
            findings,
            site.entity,
            format_args!("Unsew.edges picks {class}, not an edge"),
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
            format_args!("{field} selects {class}; it selects points, faces, or planes"),
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
        Some(ExtendLimit::Distance(Dimension::Literal(distance))) if distance.get() <= 0.0 => {
            error(
                findings,
                site.entity,
                format_args!("a literal ExtendLimit::Distance is not positive"),
            );
        }
        Some(ExtendLimit::UpTo(target)) => {
            check_up_to(site, "ExtendLimit::UpTo", target, findings);
        }
        _ => {}
    }
    if let Some(output) = &extend.output {
        options::check_sheet_output(index, site, output, findings);
    }
}

fn law_fraction_outside(law: &Law<Fraction>) -> bool {
    let outside = |value: &Dimension<Fraction>| matches!(value, Dimension::Literal(value) if !(value.get() > 0.0 && value.get() < 1.0));
    match law {
        Law::Constant(value) => outside(value),
        Law::Linear { start, end } => outside(start) || outside(end),
        _ => false,
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
            format_args!("ImplicitSweep.solution is 0; it is at least 1"),
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
                    format_args!("LineMode::Curve.from occurs without an angle"),
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
                    format_args!(
                        "CircleMode::CenterAngles states one angle; it states both or neither"
                    ),
                );
            }
            if from.is_some() && stated == 0 {
                error(
                    findings,
                    site.entity,
                    format_args!("CircleMode::CenterAngles.from occurs without an angle"),
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
                    format_args!(
                        "an ImplicitSection::Conic has {} guides; it has two to five",
                        guides.len()
                    ),
                );
            }
            if parameter.is_some() && guides.len() != 2 {
                error(
                    findings,
                    site.entity,
                    format_args!(
                        "an ImplicitSection::Conic states a parameter with other than two guides"
                    ),
                );
            }
            if parameter.as_ref().is_some_and(law_fraction_outside) {
                error(
                    findings,
                    site.entity,
                    format_args!("an ImplicitSection::Conic parameter lies outside (0, 1)"),
                );
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
            format_args!("BoundaryFill.cells or one of its cell selections is empty"),
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
                    format_args!(
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
                    format_args!("the two BridgeEnd selections are one pick with one at"),
                );
            }
        }
        CurveDefinition::Polygon {
            sides: Dimension::Literal(sides),
            ..
        } if *sides < 3 => error(
            findings,
            site.entity,
            format_args!(
                "CurveDefinition::Polygon has {sides} sides; a literal count is at least 3"
            ),
        ),
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
            format_args!("Form.cage holds records other than subds, or no subds record"),
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
                    format_args!("MeshKind::Combine.op is other than Join, Cut, or Intersect"),
                );
            }
            if picks::is_empty(tools) {
                error(
                    findings,
                    site.entity,
                    format_args!("MeshKind::Combine.tools is empty"),
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
                format_args!(
                    "MeshKind::Section names sketch {}, which is no sketch record",
                    sketch.as_str()
                ),
            );
        }
        _ => {}
    }
}
