// SPDX-License-Identifier: Apache-2.0
//! Swept solids and the loft (§6.3).

use crate::report::check::Ruled;
use std::f64::consts::TAU;

use super::options;
use super::{error, picks, Site};
use crate::design::dimension::Dimension;
use crate::design::ops::shared::{Limit, RadialDimension, Span};
use crate::design::ops::swept::{
    CenterlineAt, CornerTransition, DraftNeutral, Extrude, ExtrudeDraft, GuideFrame,
    HelicalSection, HelicalSweep, HoleDraft, Loft, PathExtent, Primitive, PrimitiveShape,
    ProfileFit, Revolve, Rib, ScalingLaw, SectionForm, SectionShape, SectionTurn, Spine, Sweep,
    SweepControls, SweepOrientation, SweepSection,
};
use crate::design::options::{
    BodyForm, Boolean, BooleanOp, SectionCondition, SolidOutput, Support,
};
use crate::report::check::Finding;
use crate::scalar::Angle;

use super::super::records::DesignRecords;

pub(super) fn check_extrude(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    extrude: &Extrude,
    findings: &mut Vec<Finding>,
) {
    options::check_profile(site, "Extrude.profile", &extrude.profile, findings);
    if let Some(span) = &extrude.span {
        options::check_length_span(site, "Extrude.span", span, findings);
    }
    if let Some(draft) = &extrude.draft {
        check_extrude_draft(site, draft, extrude.span.as_ref(), findings);
    }
    if let Some(fit) = &extrude.fit {
        check_fit(site, fit, findings);
    }
    if extrude.sheet_cut.is_some() && !cuts(&extrude.output) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.extrude-sheet-cut-occurs-only-extrude",
                "Extrude.sheet_cut occurs only in an extrude that cuts"
            ),
        );
    }
    options::check_solid_output(index, site, &extrude.output, findings);
}

fn cuts(output: &SolidOutput) -> bool {
    matches!(
        output.boolean,
        Some(Boolean::Apply {
            op: BooleanOp::Cut,
            ..
        })
    )
}

fn check_extrude_draft(
    site: Site<'_>,
    draft: &ExtrudeDraft,
    span: Option<&Span<crate::scalar::Length>>,
    findings: &mut Vec<Finding>,
) {
    let mut refuse = |message: Ruled<'_>| error(findings, site.entity, message);
    if draft.end.is_none() && draft.start.is_none() {
        refuse(crate::rule!(
            "design.extrudedraft-states-neither-end-nor-start",
            "ExtrudeDraft states neither end nor start; a draft that is off is draft: None"
        ));
    }
    let starts_at_origin =
        matches!(span, Some(Span::Between { start, .. }) if matches!(**start, Limit::AtOrigin));
    if draft.start.is_some() {
        if matches!(span, Some(Span::Symmetric { .. })) {
            refuse(crate::rule!(
                "design.extrudedraft-under-symmetric-span-states-start",
                "ExtrudeDraft under a symmetric span states start"
            ));
        }
        if starts_at_origin {
            refuse(crate::rule!(
                "design.extrudedraft-states-start-where-span-start",
                "ExtrudeDraft states start where the span's start is AtOrigin"
            ));
        }
    }
    if draft.neutral == Some(DraftNeutral::Start) {
        let between = matches!(span, Some(Span::Between { .. }));
        if !between || starts_at_origin || draft.start.is_some() {
            refuse(crate::rule!("design.extrudedraft-neutral-start-occurs-only-under", 
                "ExtrudeDraft.neutral Start occurs only under a Span::Between whose start is not AtOrigin, with no start draft"
            ));
        }
    }
    if let Some(HoleDraft::Angles { start: Some(_), .. }) = &draft.holes {
        if draft.start.is_none() {
            refuse(crate::rule!(
                "design.holedraft-angles-states-start-where-extrudedraft",
                "HoleDraft::Angles states start where its ExtrudeDraft does not"
            ));
        }
    }
}

fn check_fit(site: Site<'_>, fit: &ProfileFit, findings: &mut Vec<Finding>) {
    if fit.outer.is_none() && fit.inner.is_none() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.profilefit-states-neither-outer-nor-inner",
                "ProfileFit states neither outer nor inner"
            ),
        );
    }
    if fit.outer_join.is_some() && fit.outer.is_none() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.profilefit-states-outer-join-without-outer",
                "ProfileFit states outer_join without outer"
            ),
        );
    }
    if fit.inner_join.is_some() && fit.inner.is_none() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.profilefit-states-inner-join-without-inner",
                "ProfileFit states inner_join without inner"
            ),
        );
    }
}

pub(super) fn check_revolve(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    revolve: &Revolve,
    findings: &mut Vec<Finding>,
) {
    options::check_profile(site, "Revolve.profile", &revolve.profile, findings);
    if let Some(span) = &revolve.span {
        options::check_angle_span(site, "Revolve.span", span, findings);
    }
    if let Some(fit) = &revolve.fit {
        check_fit(site, fit, findings);
    }
    options::check_solid_output(index, site, &revolve.output, findings);
}

pub(super) fn check_sweep(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    sweep: &Sweep,
    findings: &mut Vec<Finding>,
) {
    match &sweep.section {
        SweepSection::Profile(profile) => {
            options::check_profile(site, "Sweep.section", profile, findings);
        }
        SweepSection::Generated { shape, turn } => {
            check_section_shape(site, shape, false, findings);
            if matches!(shape, SectionShape::Circle { .. }) && turn.is_some() {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.sweepsection-generated-states-turn-circle-section",
                        "SweepSection::Generated states a turn for a Circle section"
                    ),
                );
            }
            if *turn == Some(SectionTurn::ZStart) && !matches!(shape, SectionShape::Square { .. }) {
                error(
                    findings,
                    site.entity,
                    crate::rule!(
                        "design.sectionturn-zstart-occurs-only-square-section",
                        "SectionTurn::ZStart occurs only with a Square section"
                    ),
                );
            }
        }
    }
    options::check_path(site, "Sweep.path", &sweep.path, findings);
    check_controls(site, &sweep.controls, sweep.guides.len(), findings);
    for guide in &sweep.guides {
        options::check_path(site, "SweepGuide.path", &guide.path, findings);
        if let Some(extent) = &guide.extent {
            check_path_extent(site, extent, findings);
        }
    }
    if let Some(extent) = &sweep.extent {
        check_path_extent(site, extent, findings);
    }
    if sweep.transition == Some(CornerTransition::Open) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.sweep-transition-open-occurs-only-implicitsweep",
                "Sweep.transition is Open, which occurs only in ImplicitSweep.corner"
            ),
        );
    }
    options::check_solid_output(index, site, &sweep.output, findings);
}

fn check_controls(
    site: Site<'_>,
    controls: &SweepControls,
    guides: usize,
    findings: &mut Vec<Finding>,
) {
    match &controls.orientation {
        Some(SweepOrientation::Guides(frame)) => {
            let least = match frame {
                GuideFrame::PathToFirst => 1,
                GuideFrame::FirstToSecond => 2,
            };
            if guides < least {
                error(
                    findings,
                    site.entity,
                    crate::rule!("design.sweeporientation-guides-needs-least-guides-feature", 
                        "SweepOrientation::Guides needs at least {least} guides; the feature has {guides}"
                    ),
                );
            }
        }
        Some(SweepOrientation::Curve { path, .. }) => {
            options::check_path(site, "SweepOrientation::Curve.path", path, findings);
        }
        _ => {}
    }
    if let Some(ScalingLaw::Curve(path)) = &controls.scaling {
        options::check_path(site, "ScalingLaw::Curve", path, findings);
    }
}

fn check_path_extent(site: Site<'_>, extent: &PathExtent, findings: &mut Vec<Finding>) {
    for fraction in [&extent.along, &extent.against].into_iter().flatten() {
        site.bound(findings, |values| {
            let value = values.get(fraction)?.get();
            (!(0.0..=1.0).contains(&value))
                .then(|| format!("PathExtent fraction {value} lies outside [0, 1]"))
        });
    }
    if let (Some(along), Some(against)) = (&extent.along, &extent.against) {
        site.bound(findings, |values| {
            (values.get(along)?.get() + values.get(against)?.get() <= 0.0)
                .then(|| "PathExtent has along + against not above zero".to_owned())
        });
    }
}

fn check_section_shape(
    site: Site<'_>,
    shape: &SectionShape,
    helical: bool,
    findings: &mut Vec<Finding>,
) {
    match shape {
        SectionShape::Circle {
            diameter,
            inner_diameter: Some(inner),
        } => site.bound(findings, |values| {
            (values.get(inner)?.get() >= values.get(diameter)?.get()).then(|| {
                "SectionShape::Circle.inner_diameter is not less than its diameter".to_owned()
            })
        }),
        SectionShape::Triangle { apex: Some(_), .. } if !helical => error(
            findings,
            site.entity,
            crate::rule!(
                "design.sectionshape-triangle-apex-occurs-only-helicalsection",
                "SectionShape::Triangle.apex occurs only in a HelicalSection::Generated"
            ),
        ),
        _ => {}
    }
}

pub(super) fn check_helical_sweep(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    sweep: &HelicalSweep,
    findings: &mut Vec<Finding>,
) {
    match &sweep.section {
        HelicalSection::Profile(profile) => {
            options::check_profile(site, "HelicalSweep.section", profile, findings);
        }
        HelicalSection::Generated { shape, .. } => {
            check_section_shape(site, shape, true, findings);
        }
    }
    if let Some(crate::design::ops::swept::HelixStart::Circle(circle)) = &sweep.helix.start {
        check_helix_circle(index, site, circle, findings);
    }
    options::check_solid_output(index, site, &sweep.output, findings);
}

/// `HelixStart::Circle` selects exactly one circle or arc. An edge or a
/// curve name does not state its curve's shape, so only a sketch entity's
/// curve is read.
pub(super) fn check_helix_circle(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    circle: &crate::design::selection::OrderedSelection,
    findings: &mut Vec<Finding>,
) {
    if picks::count(circle).is_some_and(|count| count != 1) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.helixstart-circle-selects-other-exactly-one",
                "HelixStart::Circle selects other than exactly one circle or arc"
            ),
        );
    }
    let allowed = |class| {
        matches!(
            class,
            picks::Class::SketchEntity
                | picks::Class::SpatialSketchEntity
                | picks::Class::Name(
                    crate::design::names::NameKind::Edge | crate::design::names::NameKind::Curve
                )
        )
    };
    if let Some(class) = picks::refused(circle, allowed) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.helixstart-circle-picks-not-circle-arc",
                "HelixStart::Circle picks {class}, not a circle or arc"
            ),
        );
    }
    let entities = picks::named(circle).unwrap_or_default();
    if entities
        .iter()
        .any(|pick| super::sketches::picks_no_circle(index, &pick.target))
    {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.helixstart-circle-picks-sketch-entity-curve",
                "HelixStart::Circle picks a sketch entity whose curve is no circle or arc"
            ),
        );
    }
}

pub(super) fn check_rib(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    rib: &Rib,
    findings: &mut Vec<Finding>,
) {
    options::check_profile(site, "Rib.profile", &rib.profile, findings);
    if let Some(extent) = &rib.extent {
        if !matches!(
            extent,
            Limit::Next | Limit::Amount(_) | Limit::ToGeometry { .. } | Limit::ToPlane(_)
        ) {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.rib-extent-other-next-amount-togeometry",
                    "Rib.extent is other than Next, Amount, ToGeometry, or ToPlane"
                ),
            );
        }
        options::check_limit(site, "Rib.extent", extent, false, findings);
    }
    if rib.thickness_at.is_some() && rib.draft.is_none() {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.rib-thickness-occurs-only-draft",
                "Rib.thickness_at occurs only with draft"
            ),
        );
    }
    if let Some(thickness) = &rib.thickness {
        options::check_thickness(site, thickness, findings);
    }
    if let Some(boolean) = &rib.boolean {
        options::check_boolean(index, site, boolean, options::BooleanAt::Own, findings);
    }
}

pub(super) fn check_primitive(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    primitive: &Primitive,
    findings: &mut Vec<Finding>,
) {
    let angles: Vec<&Option<Dimension<Angle>>> = match &primitive.shape {
        PrimitiveShape::Cylinder { angle, .. } | PrimitiveShape::Cone { angle, .. } => vec![angle],
        PrimitiveShape::Sphere { longitude, .. }
        | PrimitiveShape::Torus { longitude, .. }
        | PrimitiveShape::Ellipsoid { longitude, .. } => vec![longitude],
        _ => Vec::new(),
    };
    for angle in angles.into_iter().flatten() {
        site.bound(findings, |values| {
            let angle = values.get(angle)?.get();
            (!(angle > 0.0 && angle <= TAU))
                .then(|| format!("primitive angle {angle} lies outside (0, 2π]"))
        });
    }
    match &primitive.shape {
        PrimitiveShape::Cone { radii, .. } => {
            site.bound(findings, |values| {
                let zero = |radius: &RadialDimension<crate::scalar::NonNegativeLength>| {
                    let (RadialDimension::Radius(size) | RadialDimension::Diameter(size)) = radius;
                    values.get(size).map(|size| size.get() == 0.0)
                };
                (zero(&radii[0])? && zero(&radii[1])?)
                    .then(|| "PrimitiveShape::Cone has both radii zero".to_owned())
            });
        }
        PrimitiveShape::Prism { sides, .. } => site.bound(findings, |values| {
            let sides = values.get(sides)?;
            (sides < 3)
                .then(|| format!("PrimitiveShape::Prism has {sides} sides; a count is at least 3"))
        }),
        PrimitiveShape::Wedge { y, base, top } => check_wedge(site, y, base, top, findings),
        PrimitiveShape::Plane { .. }
            if !matches!(primitive.output.body, None | Some(BodyForm::Sheet)) =>
        {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.plane-primitive-output-body-other-sheet",
                    "a Plane primitive's output body is other than Sheet or None"
                ),
            );
        }
        _ => {}
    }
    options::check_solid_output(index, site, &primitive.output, findings);
}

fn check_wedge(
    site: Site<'_>,
    y: &[Dimension<crate::scalar::Length>; 2],
    base: &[Dimension<crate::scalar::Length>; 4],
    top: &[Dimension<crate::scalar::Length>; 4],
    findings: &mut Vec<Finding>,
) {
    site.bound(findings, |values| {
        let read = |dimensions: &[Dimension<crate::scalar::Length>]| {
            dimensions
                .iter()
                .map(|dimension| values.get(dimension).map(crate::scalar::Length::get))
                .collect::<Option<Vec<f64>>>()
        };
        let (y, base, top) = (read(y)?, read(base)?, read(top)?);
        let ordered = y[0] < y[1]
            && base[0] <= base[1]
            && base[2] <= base[3]
            && top[0] <= top[1]
            && top[2] <= top[3];
        let degenerate_x = base[0] == base[1] && top[0] == top[1];
        let degenerate_z = base[2] == base[3] && top[2] == top[3];
        (!ordered || degenerate_x || degenerate_z)
            .then(|| "a Wedge's bounds are out of order or degenerate in one axis".to_owned())
    });
}

pub(super) fn check_loft(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    loft: &Loft,
    findings: &mut Vec<Finding>,
) {
    let stations =
        loft.sections.len() + usize::from(loft.start.is_some()) + usize::from(loft.end.is_some());
    if stations < 2 {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.loft-sections-point-ends-needs-least",
                "Loft has {stations} sections and point ends; it needs at least 2"
            ),
        );
    }
    if loft.cross.len() == 1 {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.loft-cross-holds-one-section-empty",
                "Loft.cross holds one section; it is empty or holds at least 2"
            ),
        );
    }
    if loft.shape.section_law.is_some() && stations <= 2 {
        error(
            findings,
            site.entity,
            crate::rule!("design.loftshape-section-law-occurs-only-loft", "LoftShape.section_law occurs only in a loft of more than two sections and point ends"),
        );
    }
    if let Some(smoothing) = &loft.shape.smoothing {
        if smoothing.angle.is_none() && smoothing.deviation.is_none() {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.loftsmoothing-states-neither-angle-nor-deviation",
                    "LoftSmoothing states neither angle nor deviation"
                ),
            );
        }
    }
    if loft.shape.style.is_some() && matches!(loft.shape.ruled, Some(Dimension::Literal(true))) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.loftshape-style-occurs-ruled-true",
                "LoftShape.style occurs with ruled: true"
            ),
        );
    }
    let guides = loft.guides.len();
    for guide in &loft.guides {
        options::check_path(site, "Guide.path", &guide.path, findings);
        if guide.influence == Some(crate::design::ops::swept::GuideInfluence::NextGuide)
            && guides < 2
        {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.guide-influence-nextguide-occurs-only-loft",
                    "a guide's influence NextGuide occurs only in a loft with at least two guides"
                ),
            );
        }
    }
    for section in loft.sections.iter().chain(&loft.cross) {
        match &section.form {
            SectionForm::Profile(profile) | SectionForm::Placed { profile, .. } => {
                options::check_profile(site, "Section.form", profile, findings);
            }
        }
        if section.spine_point.is_some() && loft.spine.is_none() {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.section-spine-point-occurs-only-loft",
                    "Section.spine_point occurs only in a loft with a spine"
                ),
            );
        }
        let faces = matches!(
            section.condition,
            Some(
                SectionCondition::Free {
                    support: Some(Support::Faces(_))
                } | SectionCondition::Tangent {
                    support: Some(Support::Faces(_)),
                    ..
                } | SectionCondition::Curvature {
                    support: Some(Support::Faces(_)),
                    ..
                }
            )
        );
        if section.trim_support.is_some() && !faces {
            error(
                findings,
                site.entity,
                crate::rule!(
                    "design.section-trim-support-occurs-only-where",
                    "Section.trim_support occurs only where its condition names a Faces support"
                ),
            );
        }
    }
    match &loft.spine {
        Some(Spine::Path { path, controls }) => {
            options::check_path(site, "Spine::Path.path", path, findings);
            check_controls(site, controls, guides, findings);
        }
        Some(Spine::Centerline { path, stations, .. }) => {
            options::check_path(site, "Spine::Centerline.path", path, findings);
            let mut seen: Vec<&CenterlineAt> = Vec::new();
            for station in stations {
                if seen.contains(&&station.at) {
                    error(
                        findings,
                        site.entity,
                        crate::rule!(
                            "design.two-area-stations-centerline-name-one",
                            "two area stations of a Centerline name one position"
                        ),
                    );
                }
                seen.push(&station.at);
                if let CenterlineAt::Fraction(fraction) = &station.at {
                    site.bound(findings, |values| {
                        (!(0.0..=1.0).contains(&values.get(fraction)?.get()))
                            .then(|| "a Centerline station fraction lies outside [0, 1]".to_owned())
                    });
                }
            }
        }
        Some(Spine::Computed) | None => {}
    }
    options::check_solid_output(index, site, &loft.output, findings);
}

/// Where a `CornerTransition` other than a sweep's occurs.
pub(super) fn check_corner(
    site: Site<'_>,
    field: &str,
    corner: Option<&CornerTransition>,
    findings: &mut Vec<Finding>,
) {
    if corner == Some(&CornerTransition::Carried) {
        error(
            findings,
            site.entity,
            crate::rule!(
                "design.carried-occurs-only-sweep-transition",
                "{field} is Carried, which occurs only in Sweep.transition"
            ),
        );
    }
}
