// SPDX-License-Identifier: Apache-2.0
//! Dress-up and holes (§6.4).

use super::options::{self, BooleanAt};
use super::{error, picks, warning, Site};
use crate::design::construction::{InputReference, Relation};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::names::{GenerationRole, HoleFaceRole, TopoName};
use crate::design::ops::datum::DatumElement;
use crate::design::ops::dressup::{
    Along, BlendControls, Bore, Chamfer, ChamferSize, ConicShape, Decal, Dome, Draft, DraftAngle,
    Emboss, Fillet, FilletProfile, FilletSet, FilletSize, Hole, HoleBottom, HoleHead,
    HolePositions, HoleShape, Offset, OffsetResult, RoundSide, Setbacks, ShellFeature, SlotLength,
    StageHole, StationAt, Stations, Thicken, Thread, ThreadLength, ThreadSpec, Transition,
    TransitionKind,
};
use crate::design::ops::shared::Limit;
use crate::design::selection::{DatumPart, PickTarget, PrincipalEntity};
use crate::report::check::Finding;
use crate::scalar::PositiveLength;

use super::super::records::DesignRecords;

pub(super) fn check_fillet(site: Site<'_>, fillet: &Fillet, findings: &mut Vec<Finding>) {
    if fillet.sets.is_empty() {
        error(findings, site.entity, format_args!("Fillet.sets is empty"));
    }
    for set in &fillet.sets {
        check_fillet_set(site, set, findings);
    }
    check_controls(site, &fillet.controls, fillet.sets.len(), findings);
    for set in &fillet.sets {
        if let (Some(keep), FilletSet::Edges { edges, .. }) = (&fillet.controls.keep, set) {
            if picks::share_pick(keep, edges) {
                error(
                    findings,
                    site.entity,
                    format_args!("BlendControls.keep shares a pick with a set's edges"),
                );
            }
        }
    }
}

fn check_fillet_set(site: Site<'_>, set: &FilletSet, findings: &mut Vec<Finding>) {
    let (section, holds) = match set {
        FilletSet::Edges { section, hold, .. } => (
            Some(section),
            hold.iter().filter(|hold| hold.is_some()).count(),
        ),
        FilletSet::Faces {
            first,
            second,
            section,
            ..
        } => {
            if picks::share_pick(&first.faces, &second.faces) {
                error(
                    findings,
                    site.entity,
                    format_args!("Faces.first.faces and Faces.second.faces share a pick"),
                );
            }
            let holds = usize::from(first.hold.is_some()) + usize::from(second.hold.is_some());
            (Some(section), holds)
        }
        FilletSet::FullRound {
            first,
            center,
            second,
        } => {
            for side in [first, second] {
                if let RoundSide::Faces(faces) = side {
                    if picks::share_pick(center, faces) {
                        error(
                            findings,
                            site.entity,
                            format_args!("a FullRound's center and side selections share a pick"),
                        );
                    }
                }
            }
            (None, 0)
        }
    };
    let Some(section) = section else {
        return;
    };
    match &section.size {
        Some(FilletSize::Held) if holds == 0 => error(
            findings,
            site.entity,
            format_args!("FilletSize::Held occurs in a set with no hold"),
        ),
        Some(FilletSize::Variable(stations)) => {
            check_stations(site, stations, findings);
            site.bound(findings, |values| {
                for station in &stations.points {
                    if values.get(&station.value)?.get() > 0.0 {
                        return None;
                    }
                }
                Some("a Variable radius has no positive station value".to_owned())
            });
        }
        _ => {}
    }
    let center_and_rho = matches!(
        section.profile,
        Some(FilletProfile::Conic(ConicShape::CenterAndRho { .. }))
    );
    if matches!(section.size, Some(FilletSize::ByProfile)) != center_and_rho {
        error(
            findings,
            site.entity,
            format_args!(
                "FilletSize::ByProfile occurs exactly where the profile is Conic(CenterAndRho)"
            ),
        );
    }
    if let Some(FilletProfile::Conic(
        ConicShape::Rho { rho: Some(rho) } | ConicShape::CenterAndRho { rho, .. },
    )) = &section.profile
    {
        site.bound(findings, |values| {
            let rho = values.get(rho)?.get();
            (!(rho > 0.0 && rho < 1.0)).then(|| format!("a conic rho of {rho} lies outside (0, 1)"))
        });
    }
}

fn check_stations<Q>(site: Site<'_>, stations: &Stations<Q>, findings: &mut Vec<Finding>) {
    if stations.points.len() < 2 {
        error(
            findings,
            site.entity,
            format_args!("Stations.points holds fewer than two entries"),
        );
    }
    let on_chain = stations.points.iter().any(|station| {
        matches!(
            &station.at,
            StationAt::Parameter {
                along: Along::Chain {},
                ..
            }
        )
    });
    site.bound(findings, |values| {
        let mut last: Vec<(&Along, f64)> = Vec::new();
        for station in &stations.points {
            let StationAt::Parameter { along, fraction } = &station.at else {
                continue;
            };
            let Some(fraction) = values.get(fraction) else {
                continue;
            };
            let fraction = fraction.get();
            if let Some((_, previous)) = last.iter_mut().find(|(other, _)| *other == along) {
                if fraction <= *previous {
                    return Some(
                        "station fractions on one Along do not strictly increase".to_owned(),
                    );
                }
                *previous = fraction;
            } else {
                last.push((along, fraction));
            }
        }
        None
    });
    if stations.chain_start.is_some() && !on_chain {
        error(
            findings,
            site.entity,
            format_args!("Stations.chain_start occurs where no station is on Along::Chain"),
        );
    }
}

/// The controls shared by fillet and chamfer: corners, transitions.
fn check_controls(
    site: Site<'_>,
    controls: &BlendControls,
    sets: usize,
    findings: &mut Vec<Finding>,
) {
    for corner in &controls.corners {
        if let Some(class) = picks::refused(&corner.vertex, picks::is_vertex) {
            error(
                findings,
                site.entity,
                format_args!("Corner.vertex picks {class}, not a vertex"),
            );
        }
        if let Some(Setbacks::PerEdge(setbacks)) = &corner.setbacks {
            for (at, setback) in setbacks.iter().enumerate() {
                if let Some(class) = picks::refused(&setback.edge, picks::is_edge) {
                    error(
                        findings,
                        site.entity,
                        format_args!("EdgeSetback.edge picks {class}, not an edge"),
                    );
                }
                if setbacks[..at]
                    .iter()
                    .any(|other| picks::share_pick(&other.edge, &setback.edge))
                {
                    error(
                        findings,
                        site.entity,
                        format_args!("Setbacks::PerEdge names one edge twice"),
                    );
                }
            }
        }
    }
    for (at, transition) in controls.transitions.iter().enumerate() {
        check_transition(site, at, sets, transition, findings);
    }
}

/// A transition's members by its kind, and its kind's own counts.
fn check_transition(
    site: Site<'_>,
    at: usize,
    sets: usize,
    transition: &Transition,
    findings: &mut Vec<Finding>,
) {
    let members = transition.between.len();
    let fits = match &transition.kind {
        Some(TransitionKind::Intersect | TransitionKind::CornerSphere { .. }) => members >= 2,
        Some(TransitionKind::Blend) => members == 2,
        Some(TransitionKind::RollingBall { .. } | TransitionKind::SweptAround { .. }) => {
            (1..=3).contains(&members)
        }
        _ => members >= 1,
    };
    let in_range = transition
        .between
        .iter()
        .all(|&set| usize::try_from(set).is_ok_and(|set| set < sets));
    if !fits || !in_range {
        error(
            findings,
            site.entity,
            format_args!(
                "transition {at} has {members} members or names a set past the {sets} sets; its kind takes other counts"
            ),
        );
    }
    if matches!(
        transition.kind,
        Some(TransitionKind::SweptAround { against: None, .. })
    ) && members == 2
    {
        error(
            findings,
            site.entity,
            format_args!("transition {at} is a SweptAround of two members with no against"),
        );
    }
    if let Some(TransitionKind::CornerSphere { lengths, .. }) = &transition.kind {
        if !matches!(lengths.len(), 0 | 3) {
            error(
                findings,
                site.entity,
                format_args!(
                    "transition {at} is a CornerSphere with {} lengths; it states none or three",
                    lengths.len()
                ),
            );
        }
    }
}

pub(super) fn check_chamfer(site: Site<'_>, chamfer: &Chamfer, findings: &mut Vec<Finding>) {
    if chamfer.sets.is_empty() {
        error(findings, site.entity, format_args!("Chamfer.sets is empty"));
    }
    for set in &chamfer.sets {
        if let Some(ChamferSize::Vertex(setbacks)) = &set.size {
            if setbacks.len() != 3 {
                error(
                    findings,
                    site.entity,
                    format_args!(
                        "ChamferSize::Vertex holds {} entries; it holds three",
                        setbacks.len()
                    ),
                );
            }
        }
        if let Some(keep) = &chamfer.controls.keep {
            if picks::share_pick(keep, &set.edges) {
                error(
                    findings,
                    site.entity,
                    format_args!("BlendControls.keep shares a pick with a set's edges"),
                );
            }
        }
    }
    check_controls(site, &chamfer.controls, chamfer.sets.len(), findings);
}

pub(super) fn check_shell(site: Site<'_>, shell: &ShellFeature, findings: &mut Vec<Finding>) {
    for (at, faces) in shell.faces.iter().enumerate() {
        let shared = shell.faces[..at]
            .iter()
            .any(|other| picks::share_pick(&other.faces, &faces.faces))
            || picks::share_pick(&faces.faces, &shell.removed);
        if shared {
            error(
                findings,
                site.entity,
                format_args!(
                    "ShellFeature.faces selections share a pick with each other or with removed"
                ),
            );
        }
    }
    if let Some(thickness) = &shell.thickness {
        options::check_thickness(site, thickness, findings);
    }
}

pub(super) fn check_draft(site: Site<'_>, draft: &Draft, findings: &mut Vec<Finding>) {
    if draft.sets.is_empty() {
        error(findings, site.entity, format_args!("Draft.sets is empty"));
    }
    let anchored = draft.neutral.is_some() || draft.parting.is_some();
    for set in &draft.sets {
        if matches!(
            set.angle,
            Some(DraftAngle::Two { .. } | DraftAngle::Symmetric(_))
        ) && !anchored
        {
            error(
                findings,
                site.entity,
                format_args!("a Two or Symmetric draft angle needs neutral or parting"),
            );
        }
        if set.faces.is_none() && set.fixed.is_none() {
            error(
                findings,
                site.entity,
                format_args!("DraftSet.faces is None where fixed is absent"),
            );
        }
        if let Some(DraftAngle::Variable(stations)) = &set.angle {
            check_stations(site, stations, findings);
        }
    }
    if !anchored && draft.sets.iter().all(|set| set.fixed.is_none()) {
        warning(
            findings,
            site.entity,
            format_args!("the draft states none of neutral, parting, or a set's fixed"),
        );
    }
    if draft.steps.is_some() && draft.parting.is_none() {
        error(
            findings,
            site.entity,
            format_args!("Draft.steps occurs only with parting"),
        );
    }
}

pub(super) fn check_thicken(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    thicken: &Thicken,
    findings: &mut Vec<Finding>,
) {
    if let Some(thickness) = &thicken.thickness {
        options::check_thickness(site, thickness, findings);
    }
    if let Some(boolean) = &thicken.boolean {
        options::check_boolean(index, site, boolean, BooleanAt::Own, findings);
    }
}

pub(super) fn check_offset(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    offset: &Offset,
    findings: &mut Vec<Finding>,
) {
    if offset.sets.is_empty() {
        error(findings, site.entity, format_args!("Offset.sets is empty"));
    }
    if let OffsetResult::Copy(output) = &offset.result {
        options::check_sheet_output(index, site, output, findings);
    }
}

pub(super) fn check_hole(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    hole: &Hole,
    findings: &mut Vec<Finding>,
) {
    match &hole.positions {
        HolePositions::Points { points, centers } => {
            let whole = picks::named(points).is_some_and(|picks| {
                picks
                    .iter()
                    .any(|pick| matches!(pick.target, PickTarget::Sketch(_)))
            });
            if centers.is_some() && !whole && picks::named(points).is_some() {
                error(
                    findings,
                    site.entity,
                    format_args!("HolePositions::Points.centers is present where no pick targets a whole sketch"),
                );
            }
            let allowed = |class| picks::is_point(class) || class == picks::Class::Sketch;
            if let Some(class) = picks::refused(points, allowed) {
                error(
                    findings,
                    site.entity,
                    format_args!("HolePositions::Points picks {class}, not a point or a sketch"),
                );
            }
            let no_point = picks::named(points).unwrap_or_default().iter().any(|pick| {
                super::sketches::picks_no_point(index, &pick.target)
                    || picks_no_datum_point(index, &pick.target)
                    || matches!(
                        pick.target,
                        PickTarget::SketchFrame { entity, .. } if entity != PrincipalEntity::Origin
                    )
            });
            if no_point {
                error(
                    findings,
                    site.entity,
                    format_args!("HolePositions::Points picks a sketch entity, a datum, or a sketch frame entity that is no point"),
                );
            }
        }
        HolePositions::Placed(placed) => {
            if placed.is_empty() {
                error(
                    findings,
                    site.entity,
                    format_args!("HolePositions::Placed holds no construction"),
                );
            }
            if let Some(start) = &hole.start {
                let restates = placed.iter().flat_map(|c| &c.inputs).any(|input| {
                    let coincident = input
                        .role
                        .as_ref()
                        .is_some_and(|role| role.relation == Relation::Coincident);
                    let same = match &input.reference {
                        Some(InputReference::Picks(selection)) => {
                            picks::named(selection).is_some()
                                && picks::named(selection) == picks::named(start)
                        }
                        _ => false,
                    };
                    coincident && same
                });
                if restates {
                    error(
                        findings,
                        site.entity,
                        format_args!(
                            "a placed hole position is Coincident with Hole.start, which states it"
                        ),
                    );
                }
            }
        }
    }
    check_hole_shape(site, &hole.shape, hole.extent.as_ref(), findings);
    if let Some(extent) = &hole.extent {
        options::check_limit(site, "Hole.extent", extent, false, findings);
    }
    if let Some(thread) = &hole.thread {
        check_thread_spec(site, thread, findings);
        if hole.standard.is_some() {
            error(
                findings,
                site.entity,
                format_args!("a tapped hole states Hole.standard; its designation is only in thread.standard"),
            );
        }
    }
    if let Some(targets) = &hole.targets {
        options::check_targets(index, site, targets, findings);
    }
    check_series(index, site, hole, findings);
}

/// Whether a pick targets a datum, or a part of one, that its feature's
/// stated element makes no point. A datum feature whose operation is not
/// stated is not refused.
fn picks_no_datum_point(index: &DesignRecords<'_>, target: &PickTarget) -> bool {
    let PickTarget::Datum { feature, part } = target else {
        return false;
    };
    let Some(FeatureOperation::Datum(datum)) = index
        .features
        .get(feature.as_str())
        .map(|feature| &feature.definition.operation)
    else {
        return false;
    };
    !matches!(
        (part, &datum.element),
        (None | Some(DatumPart::Point(_)), DatumElement::Points(_))
            | (
                Some(DatumPart::Principal(PrincipalEntity::Origin)),
                DatumElement::Frame(_)
            )
    )
}

fn check_series(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    hole: &Hole,
    findings: &mut Vec<Finding>,
) {
    if hole.series.is_empty() {
        return;
    }
    if !site.assembly {
        error(
            findings,
            site.entity,
            format_args!("a Hole with a series is in a definition that states no assembly"),
        );
    }
    for (at, stage) in hole.series.iter().enumerate() {
        if hole.series[..at]
            .iter()
            .any(|other| picks::share_pick(&other.parts, &stage.parts))
        {
            error(
                findings,
                site.entity,
                format_args!("two series stages pick one participant"),
            );
        }
        match &stage.hole {
            StageHole::PartFeature(feature) => {
                if picks::count(&stage.parts).is_some_and(|count| count != 1) {
                    error(
                        findings,
                        site.entity,
                        format_args!("a StageHole::PartFeature stage picks other than one part"),
                    );
                }
                let is_hole = index
                    .features
                    .get(feature.as_str())
                    .is_some_and(|f| matches!(f.definition.operation, FeatureOperation::Hole(_)));
                if !is_hole {
                    error(
                        findings,
                        site.entity,
                        format_args!(
                            "StageHole::PartFeature names {}, which is no Hole feature",
                            feature.as_str()
                        ),
                    );
                }
            }
            StageHole::Stated {
                shape,
                extent,
                standard,
                thread,
            } => {
                check_hole_shape(site, shape, extent.as_deref(), findings);
                if let Some(thread) = thread {
                    check_thread_spec(site, thread, findings);
                    if standard.is_some() {
                        error(
                            findings,
                            site.entity,
                            format_args!("a tapped series stage states StageHole::Stated.standard; its designation is only in thread.standard"),
                        );
                    }
                }
            }
        }
    }
}

fn check_hole_shape(
    site: Site<'_>,
    shape: &HoleShape,
    extent: Option<&Limit<crate::scalar::Length>>,
    findings: &mut Vec<Finding>,
) {
    let bore = match shape {
        HoleShape::Bore(bore) => bore,
        HoleShape::Slot {
            bore,
            length,
            measure,
            ..
        } => {
            if let (Some(SlotLength::TangentToTangent), Some(diameter)) = (measure, &bore.diameter)
            {
                site.bound(findings, |values| {
                    (values.get(length)?.get() <= values.get(diameter)?.get()).then(|| {
                        "a TangentToTangent slot's length does not exceed its bore diameter"
                            .to_owned()
                    })
                });
            }
            bore
        }
        HoleShape::Revolved(profile) => {
            options::check_profile(site, "HoleShape::Revolved", profile, findings);
            return;
        }
    };
    check_bore(site, bore, findings);
    if matches!(bore.bottom, Some(HoleBottom::Trimmed))
        && matches!(extent, Some(Limit::Amount(_) | Limit::AtOrigin))
    {
        error(
            findings,
            site.entity,
            format_args!("HoleBottom::Trimmed occurs with an extent that ends on no face"),
        );
    }
}

fn check_bore(site: Site<'_>, bore: &Bore, findings: &mut Vec<Finding>) {
    let cones = [&bore.near_chamfer, &bore.neck_chamfer, &bore.far_chamfer];
    let head_cone = match &bore.head {
        Some(HoleHead::Countersink(cone)) => Some(cone),
        _ => None,
    };
    for cone in cones.into_iter().flatten().chain(head_cone) {
        let stated = usize::from(cone.diameter.is_some())
            + usize::from(cone.depth.is_some())
            + usize::from(cone.angle.is_some());
        if stated > 2 {
            error(
                findings,
                site.entity,
                format_args!("a Cone states diameter, depth, and angle; it states at most two"),
            );
        }
    }
    let head = match &bore.head {
        Some(
            HoleHead::Counterbore { diameter, .. }
            | HoleHead::Counterdrill { diameter, .. }
            | HoleHead::Spotface { diameter, .. },
        ) => diameter.as_ref(),
        Some(HoleHead::Countersink(cone)) => cone.diameter.as_ref(),
        _ => None,
    };
    if let (Some(bore_diameter), Some(head)) = (&bore.diameter, head) {
        site.bound(findings, |values| {
            (values.get(head)?.get() <= values.get(bore_diameter)?.get())
                .then(|| "a hole head's diameter does not exceed the bore diameter".to_owned())
        });
    }
}

fn check_thread_spec(site: Site<'_>, spec: &ThreadSpec, findings: &mut Vec<Finding>) {
    site.bound(findings, |values| {
        let read = |diameter: Option<&Dimension<PositiveLength>>| {
            diameter
                .and_then(|diameter| values.get(diameter))
                .map(PositiveLength::get)
        };
        let minor = read(spec.minor_diameter.as_ref());
        let pitch = read(spec.pitch_diameter.as_ref());
        let major = read(spec.major_diameter.as_ref());
        let ordered = |low: Option<f64>, high: Option<f64>| match (low, high) {
            (Some(low), Some(high)) => low < high,
            _ => true,
        };
        (!(ordered(minor, pitch) && ordered(pitch, major) && ordered(minor, major)))
            .then(|| "thread diameters are not minor < pitch < major".to_owned())
    });
    if let Some(ThreadLength::ToElement { element, .. }) = &spec.length {
        check_face_edge_or_plane(site, "ThreadLength::ToElement.element", element, findings);
    }
}

fn check_face_edge_or_plane(
    site: Site<'_>,
    field: &str,
    selection: &crate::design::selection::SetSelection,
    findings: &mut Vec<Finding>,
) {
    let allowed = |class| picks::is_edge(class) || picks::is_plane(class);
    if let Some(class) = picks::refused(selection, allowed) {
        error(
            findings,
            site.entity,
            format_args!("{field} picks {class}; it resolves to faces, edges, or planes"),
        );
    }
}

pub(super) fn check_thread(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    thread: &Thread,
    findings: &mut Vec<Finding>,
) {
    check_thread_spec(site, &thread.spec, findings);
    if let Some(start) = &thread.start {
        check_face_edge_or_plane(site, "Thread.start", start, findings);
    }
    let Some(faces) = picks::named(&thread.face) else {
        return;
    };
    let threaded_bore = |name: &TopoName| {
        match name {
        TopoName::Generated {
            feature,
            role: Some(GenerationRole::Hole(HoleFaceRole::BoreWall)),
            ..
        } => index.features.get(feature.as_str()).is_some_and(|f| {
            matches!(&f.definition.operation, FeatureOperation::Hole(hole) if hole.thread.is_some())
        }),
        _ => false,
    }
    };
    let only_bores = !faces.is_empty()
        && faces
            .iter()
            .all(|pick| matches!(&pick.target, PickTarget::Entity(name) if threaded_bore(name)));
    if only_bores {
        error(
            findings,
            site.entity,
            format_args!("a Thread selects only bore faces of a hole whose thread is present"),
        );
    }
}

pub(super) fn check_emboss(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    emboss: &Emboss,
    findings: &mut Vec<Finding>,
) {
    options::check_profile(site, "Emboss.profile", &emboss.profile, findings);
    if let Some(targets) = &emboss.targets {
        options::check_targets(index, site, targets, findings);
    }
}

pub(super) fn check_dome(site: Site<'_>, dome: &Dome, findings: &mut Vec<Finding>) {
    if let Some(class) = picks::refused(&dome.faces, picks::is_face) {
        error(
            findings,
            site.entity,
            format_args!("Dome.faces picks {class}, not a face"),
        );
    }
    if dome.points.is_some() && dome.height.is_some() {
        error(
            findings,
            site.entity,
            format_args!("a Dome with points states a height"),
        );
    }
}

pub(super) fn check_decal(site: Site<'_>, decal: &Decal, findings: &mut Vec<Finding>) {
    if let Some(class) = picks::refused(&decal.faces, picks::is_face) {
        error(
            findings,
            site.entity,
            format_args!("Decal.faces picks {class}, not a face"),
        );
    }
}
