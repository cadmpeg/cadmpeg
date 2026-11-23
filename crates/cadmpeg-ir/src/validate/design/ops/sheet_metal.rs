// SPDX-License-Identifier: Apache-2.0
//! Sheet metal (§6.6): the rule arena and the sheet-metal operations.

use std::collections::BTreeSet;

use super::picks::{self, Class};
use super::{assembly, error, warning, Site};
use crate::design::dimension::Dimension;
use crate::design::ids::SheetMetalRuleId;
use crate::design::names::NameKind;
use crate::design::ops::shared::{Limit, Span};
use crate::design::ops::sheet_metal::{
    BendAllowance, BendEquations, BendSet, BendTable, ConnectBetween, ConvertBend, ConvertFrom,
    Flange, FlangeHeight, FlangeWidth, HeightDatum, Hem, KFactorTable, Rip, RipPath, SheetMetal,
    SheetMetalKind, StampForm, StampOutline,
};
use crate::design::options::{Thickness, WallSide};
use crate::design::selection::{PickContext, SetSelection};
use crate::report::check::Finding;

use super::super::records::DesignRecords;
use super::options;

/// Rule ids are unique, and each rule's tables and equations are ordered.
pub(super) fn check_rules(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let mut ids = BTreeSet::new();
    for rule in &index.ir.design.sheet_metal_rules {
        let id = rule.id.as_str();
        if !ids.insert(id) {
            error(findings, id, format_args!("sheet-metal rule id repeats"));
        }
        match &rule.allowance {
            Some(BendAllowance::KFactorTable(table)) => check_k_factor_table(id, table, findings),
            Some(BendAllowance::Table(table)) => check_bend_table(id, table, findings),
            Some(BendAllowance::Equations(equations)) => {
                check_equations(id, equations, findings);
            }
            Some(BendAllowance::KFactor(_)) | None => {}
        }
    }
}

fn check_k_factor_table(id: &str, table: &KFactorTable, findings: &mut Vec<Finding>) {
    let increasing = table
        .rows
        .windows(2)
        .all(|pair| pair[0].ratio.get() < pair[1].ratio.get());
    if table.rows.is_empty() || !increasing {
        error(
            findings,
            id,
            format_args!("a KFactorTable is empty or its ratios do not increase strictly"),
        );
    }
}

fn strictly_increasing(values: impl IntoIterator<Item = f64>) -> bool {
    let values: Vec<f64> = values.into_iter().collect();
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn check_bend_table(id: &str, table: &BendTable, findings: &mut Vec<Finding>) {
    if table.blocks.is_empty()
        || !strictly_increasing(table.blocks.iter().map(|block| block.thickness.get()))
    {
        error(
            findings,
            id,
            format_args!(
                "a BendTable has no block or its blocks are not in strictly increasing thickness"
            ),
        );
    }
    for block in &table.blocks {
        let ordered = !block.radii.is_empty()
            && !block.angles.is_empty()
            && strictly_increasing(block.radii.iter().map(|radius| radius.get()))
            && strictly_increasing(block.angles.iter().map(|angle| angle.get()));
        let shaped = block.values.len() == block.angles.len()
            && block
                .values
                .iter()
                .all(|row| row.len() == block.radii.len());
        if !ordered || !shaped {
            error(
                findings,
                id,
                format_args!("a BendTable block's radii or angles are empty or not strictly increasing, or its values are not one row per angle of one value per radius"),
            );
        }
    }
}

fn check_equations(id: &str, equations: &BendEquations, findings: &mut Vec<Finding>) {
    if equations.ranges.is_empty() {
        error(findings, id, format_args!("a BendEquations has no range"));
    }
    for range in &equations.ranges {
        if range.from.get() >= range.to.get() {
            error(
                findings,
                id,
                format_args!("an AngleRange's from is not below its to"),
            );
        }
    }
    for pair in equations.ranges.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        let apart = first.to.get() < second.from.get()
            || (first.to.get() == second.from.get()
                && !(first.to_inclusive && second.from_inclusive));
        if !apart {
            error(
                findings,
                id,
                format_args!("BendEquations ranges are out of order or overlap"),
            );
        }
    }
}

/// The rule a sheet-metal kind names.
const fn named_rule(kind: &SheetMetalKind) -> Option<&SheetMetalRuleId> {
    match kind {
        SheetMetalKind::ApplyRule { rule, .. } => Some(rule),
        SheetMetalKind::Wall { rule, .. }
        | SheetMetalKind::ContourFlange { rule, .. }
        | SheetMetalKind::ExtrudedHole { rule, .. }
        | SheetMetalKind::Stamp { rule, .. }
        | SheetMetalKind::ToolForm { rule, .. } => rule.as_ref(),
        SheetMetalKind::Flange(Flange { rule, .. }) | SheetMetalKind::Hem(Hem { rule, .. }) => {
            rule.as_ref()
        }
        SheetMetalKind::Convert(convert) => convert.rule.as_ref(),
        _ => None,
    }
}

/// A selection holds one pick, of a kind `allowed` admits.
fn check_one(
    site: Site<'_>,
    field: &str,
    what: &str,
    selection: &SetSelection,
    allowed: impl Fn(Class) -> bool,
    findings: &mut Vec<Finding>,
) {
    let count = picks::count(selection);
    if count.is_some_and(|count| count != 1) || picks::refused(selection, allowed).is_some() {
        error(
            findings,
            site.entity,
            format_args!("{field} picks other than one {what}"),
        );
    }
}

pub(super) fn check_sheet_metal(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    sheet: &SheetMetal,
    findings: &mut Vec<Finding>,
) {
    if let Some(rule) = named_rule(&sheet.kind) {
        let found = index
            .ir
            .design
            .sheet_metal_rules
            .iter()
            .find(|record| &record.id == rule);
        let in_scope = found.is_some_and(|record| {
            site.scope
                .is_none_or(|scope| record.scope.as_str() == scope)
        });
        if !in_scope {
            error(
                findings,
                site.entity,
                format_args!(
                    "sheet-metal rule {} is no rule of the feature's scope",
                    rule.as_str()
                ),
            );
        }
    }
    match &sheet.kind {
        SheetMetalKind::Wall { profile, .. } => {
            options::check_profile(site, "SheetMetalKind::Wall.profile", profile, findings);
        }
        SheetMetalKind::ContourFlange {
            profile,
            edges,
            width,
            extent,
            thickness,
            vertex_bends,
            bend,
            ..
        } => {
            options::check_profile(site, "ContourFlange.profile", profile, findings);
            if let Some(thickness) = thickness {
                let allowed = matches!(
                    thickness,
                    Thickness::OneSide {
                        side: None | Some(WallSide::Left | WallSide::Right),
                        ..
                    } | Thickness::Symmetric(_)
                        | Thickness::Native(_)
                );
                if !allowed {
                    error(
                        findings,
                        site.entity,
                        format_args!("ContourFlange.thickness is other than OneSide with Left, Right, or no side, Symmetric, or Native"),
                    );
                }
            }
            match (edges, extent) {
                (Some(_), Some(_)) => error(
                    findings,
                    site.entity,
                    format_args!("a ContourFlange with edges states an extent"),
                ),
                (None, None) => error(
                    findings,
                    site.entity,
                    format_args!("a ContourFlange without edges states no extent"),
                ),
                (None, Some(_)) if width.is_some() => error(
                    findings,
                    site.entity,
                    format_args!("a ContourFlange without edges states a width"),
                ),
                _ => {}
            }
            if let Some(extent) = extent {
                options::check_length_span(site, "ContourFlange.extent", extent, findings);
            }
            if let Some(Span::Between { start, end }) = extent {
                for limit in [start, end] {
                    let fits = match &**limit {
                        Limit::AtOrigin
                        | Limit::ToPoint(_)
                        | Limit::ToPlane(_)
                        | Limit::ToGeometry { .. } => true,
                        Limit::Amount(Dimension::Literal(amount)) => amount.get() > 0.0,
                        Limit::Amount(_) => true,
                        _ => false,
                    };
                    if !fits {
                        error(
                            findings,
                            site.entity,
                            format_args!("a ContourFlange.extent end is other than AtOrigin, a positive Amount, ToPoint, ToPlane, or ToGeometry"),
                        );
                    }
                }
            }
            if let (Some(FlangeWidth::PerEdge(pairs)), Some(edges)) = (width, edges) {
                if picks::count(edges).is_some_and(|count| count != pairs.len()) {
                    error(
                        findings,
                        site.entity,
                        format_args!(
                            "FlangeWidth::PerEdge has other than one pair per selected edge"
                        ),
                    );
                }
            }
            for (at, vertex_bend) in vertex_bends.iter().enumerate() {
                check_one(
                    site,
                    "VertexBend.vertex",
                    "vertex",
                    &vertex_bend.vertex,
                    picks::is_vertex,
                    findings,
                );
                if vertex_bends[..at]
                    .iter()
                    .any(|other| picks::share_pick(&other.vertex, &vertex_bend.vertex))
                {
                    error(
                        findings,
                        site.entity,
                        format_args!("two VertexBends pick one vertex"),
                    );
                }
                if bend.radius.as_ref() == Some(&vertex_bend.radius)
                    && bend.radius_side == vertex_bend.radius_side
                {
                    warning(
                        findings,
                        site.entity,
                        format_args!("a VertexBend restates the flange's bend radius and side"),
                    );
                }
            }
            check_relief(site, bend, findings);
        }
        SheetMetalKind::Flange(flange) => check_flange(site, flange, findings),
        SheetMetalKind::Hem(hem) => {
            if !matches!(
                hem.position,
                None | Some(
                    crate::design::ops::sheet_metal::BendPosition::AtEdge
                        | crate::design::ops::sheet_metal::BendPosition::TangentToSide
                )
            ) {
                error(
                    findings,
                    site.entity,
                    format_args!("Hem.position is other than AtEdge or TangentToSide"),
                );
            }
        }
        SheetMetalKind::Bend {
            line,
            angle,
            flat_length,
            ..
        } => {
            if angle.is_some() && flat_length.is_some() {
                error(
                    findings,
                    site.entity,
                    format_args!("a Bend states both angle and flat_length"),
                );
            }
            let allowed =
                |class| matches!(class, Class::SketchEntity | Class::Name(NameKind::Edge));
            if let Some(class) = picks::refused(line, allowed) {
                error(
                    findings,
                    site.entity,
                    format_args!(
                        "SheetMetalKind::Bend.line picks {class}; it picks sketch lines or edges"
                    ),
                );
            }
        }
        SheetMetalKind::Connect {
            between,
            extension,
            double,
            ..
        } => match between {
            ConnectBetween::Apart([first, second]) => {
                for selection in [first, second] {
                    check_one(
                        site,
                        "ConnectBetween::Apart",
                        "edge",
                        selection,
                        picks::is_edge,
                        findings,
                    );
                }
                if picks::named(first).is_some() && picks::named(first) == picks::named(second) {
                    error(
                        findings,
                        site.entity,
                        format_args!("ConnectBetween::Apart's two selections are one"),
                    );
                }
            }
            ConnectBetween::SharedEdge(edges) => {
                if let Some(class) = picks::refused(edges, picks::is_edge) {
                    error(
                        findings,
                        site.entity,
                        format_args!("ConnectBetween::SharedEdge picks {class}, not an edge"),
                    );
                }
                if double.is_some() || extension.is_some() {
                    error(
                        findings,
                        site.entity,
                        format_args!("a SharedEdge Connect states double or extension"),
                    );
                }
            }
        },
        SheetMetalKind::Rip(rip) => check_rip(site, rip, false, findings),
        SheetMetalKind::Convert(convert) => {
            check_one(
                site,
                "Convert.fixed",
                "face",
                &convert.fixed,
                picks::is_face,
                findings,
            );
            if let ConvertFrom::Faces { bends, .. } = &convert.from {
                for bend in bends {
                    match bend {
                        ConvertBend::Edge { edge, .. } => {
                            check_one(
                                site,
                                "ConvertBend::Edge.edge",
                                "edge",
                                edge,
                                picks::is_edge,
                                findings,
                            );
                        }
                        ConvertBend::Face { face } => {
                            check_one(
                                site,
                                "ConvertBend::Face.face",
                                "face",
                                face,
                                picks::is_face,
                                findings,
                            );
                        }
                    }
                }
            }
            for rip in &convert.rips {
                if !matches!(rip.path, RipPath::Edges(_) | RipPath::Sketched { .. }) {
                    error(
                        findings,
                        site.entity,
                        format_args!("a ConvertRip.path is other than Edges or Sketched"),
                    );
                }
                check_rip_path(site, &rip.path, findings);
            }
            check_relief(site, &convert.bend, findings);
        }
        SheetMetalKind::BreakCorner { corners, .. } => {
            if picks::is_empty(corners) {
                error(
                    findings,
                    site.entity,
                    format_args!("BreakCorner.corners is empty"),
                );
            }
            if let Some(class) = picks::refused(corners, picks::is_edge) {
                error(
                    findings,
                    site.entity,
                    format_args!("BreakCorner.corners picks {class}, not an edge"),
                );
            }
        }
        SheetMetalKind::ExtendWall { edge, .. } => {
            check_one(
                site,
                "ExtendWall.edge",
                "edge",
                edge,
                picks::is_edge,
                findings,
            );
        }
        SheetMetalKind::ExtrudedHole { bore, entry, .. } => {
            if let Some(entry) = entry {
                check_one(
                    site,
                    "ExtrudedHole.entry",
                    "planar face",
                    entry,
                    picks::is_face,
                    findings,
                );
            }
            check_outline(site, bore, findings);
        }
        SheetMetalKind::Stamp { entry, form, .. } => {
            if let Some(entry) = entry {
                check_one(
                    site,
                    "Stamp.entry",
                    "planar face",
                    entry,
                    picks::is_face,
                    findings,
                );
            }
            match form {
                StampForm::Raised { outline, .. } => check_outline(site, outline, findings),
                StampForm::Louver {
                    profile, open_edge, ..
                } => {
                    options::check_profile(site, "StampForm::Louver.profile", profile, findings);
                    let allowed = |class| class == Class::SketchEntity;
                    check_one(
                        site,
                        "Louver.open_edge",
                        "curve of its profile's sketch",
                        open_edge,
                        allowed,
                        findings,
                    );
                }
                StampForm::Bead { path, .. } => {
                    options::check_profile(site, "StampForm::Bead.path", path, findings);
                }
                StampForm::StiffeningRib { bend, .. } => {
                    check_one(
                        site,
                        "StiffeningRib.bend",
                        "bend face",
                        bend,
                        picks::is_face,
                        findings,
                    );
                }
                StampForm::Bridge { .. } => {}
            }
        }
        SheetMetalKind::ToolForm {
            tool, constraints, ..
        } => {
            if site.scope.is_some_and(|scope| {
                index
                    .spanned_definitions(scope)
                    .contains(tool.definition.as_str())
            }) {
                error(
                    findings,
                    site.entity,
                    format_args!(
                        "ToolForm.tool names {}, a definition the feature's scope spans",
                        tool.definition.as_str()
                    ),
                );
            }
            for constraint in constraints {
                assembly::check_joint(index, site, constraint, findings);
                let names_tool = constraint.connectors.iter().any(|connector| {
                    let wire = super::wire_strings(connector);
                    wire.contains(tool.definition.as_str())
                });
                let has_context = constraint.connectors.iter().any(|connector| match connector {
                    crate::design::ops::shared::JointConnector::Entity(selection) => picks::named(selection)
                        .is_some_and(|picks| {
                            picks.iter().any(|pick| matches!(&pick.context, Some(PickContext::Definition(d)) if d.definition == tool.definition))
                        }),
                    _ => false,
                });
                if !names_tool && !has_context {
                    error(
                        findings,
                        site.entity,
                        format_args!("a ToolForm constraint has no connector whose picks carry a Definition context naming its tool"),
                    );
                }
            }
        }
        SheetMetalKind::Unfold { fixed, bends } => {
            if let Some(fixed) = fixed {
                check_one(
                    site,
                    "Unfold.fixed",
                    "planar face",
                    fixed,
                    picks::is_face,
                    findings,
                );
            }
            check_bends(site, bends.as_ref(), findings);
        }
        SheetMetalKind::Refold { bends } => check_bends(site, bends.as_ref(), findings),
        SheetMetalKind::FlatPattern { fixed: Some(fixed) } => {
            check_one(
                site,
                "FlatPattern.fixed",
                "planar face",
                fixed,
                picks::is_face,
                findings,
            );
        }
        // The attached plate, bend and unchanged corner operands are checked
        // by the attached sheet-metal pass.
        SheetMetalKind::AttachedPlate { .. }
        | SheetMetalKind::AttachedBend { .. }
        | SheetMetalKind::UnchangedCorner { .. }
        | SheetMetalKind::ApplyRule { .. }
        | SheetMetalKind::FlatPattern { fixed: None }
        | SheetMetalKind::Native(_) => {}
    }
}

fn check_bends(site: Site<'_>, bends: Option<&BendSet>, findings: &mut Vec<Finding>) {
    if let Some(BendSet::Selected(selection)) = bends {
        if let Some(class) = picks::refused(selection, picks::is_face) {
            error(
                findings,
                site.entity,
                format_args!("BendSet::Selected selects {class}, not a bend face"),
            );
        }
    }
}

fn check_outline(site: Site<'_>, outline: &StampOutline, findings: &mut Vec<Finding>) {
    if let StampOutline::Profile(profile) = outline {
        options::check_profile(site, "StampOutline::Profile", profile, findings);
    }
}

fn check_relief(
    site: Site<'_>,
    bend: &crate::design::ops::sheet_metal::BendSpec,
    findings: &mut Vec<Finding>,
) {
    if let Some(relief) = &bend.relief {
        if relief.width.is_some() && relief.width_ratio.is_some() {
            error(
                findings,
                site.entity,
                format_args!("a Relief states both width and width_ratio"),
            );
        }
    }
}

fn check_flange(site: Site<'_>, flange: &Flange, findings: &mut Vec<Finding>) {
    let distance = matches!(flange.height, Some(FlangeHeight::Distance(_)));
    if flange.height_datum.is_some() && !distance && flange.outline.is_none() {
        error(
            findings,
            site.entity,
            format_args!("Flange.height_datum occurs without a Distance height or an outline"),
        );
    }
    if let Some(outline) = &flange.outline {
        options::check_profile(site, "Flange.outline", outline, findings);
        let one_edge = picks::count(&flange.edges).is_none_or(|count| count == 1)
            && picks::refused(&flange.edges, picks::is_edge).is_none();
        let datum = matches!(
            flange.height_datum,
            Some(HeightDatum::InnerSharp | HeightDatum::OuterSharp)
        );
        if !one_edge
            || !datum
            || flange.height.is_some()
            || flange.width.is_some()
            || flange.angle.is_some()
        {
            error(
                findings,
                site.entity,
                format_args!("a Flange with an outline picks other than one edge, states a height datum other than InnerSharp or OuterSharp, or states a height, width, or angle"),
            );
        }
    }
    let edges = picks::count(&flange.edges);
    match (&flange.width, edges) {
        (Some(FlangeWidth::PerEdge(pairs)), Some(edges)) if pairs.len() != edges => error(
            findings,
            site.entity,
            format_args!(
                "FlangeWidth::PerEdge has {} pairs for {edges} edges",
                pairs.len()
            ),
        ),
        (Some(FlangeWidth::CenteredPerEdge(widths)), Some(edges)) if widths.len() != edges => {
            error(
                findings,
                site.entity,
                format_args!(
                    "FlangeWidth::CenteredPerEdge has {} widths for {edges} edges",
                    widths.len()
                ),
            );
        }
        _ => {}
    }
    check_relief(site, &flange.bend, findings);
}

/// A rip's faces pick faces and are not empty; a ruling or a line picks one
/// face, and a line's ends differ.
pub(super) fn check_rip(site: Site<'_>, rip: &Rip, convert: bool, findings: &mut Vec<Finding>) {
    match rip {
        Rip::Faces { faces } => {
            if picks::is_empty(faces) {
                error(
                    findings,
                    site.entity,
                    format_args!("Rip::Faces.faces is empty"),
                );
            }
            if let Some(class) = picks::refused(faces, picks::is_face) {
                error(
                    findings,
                    site.entity,
                    format_args!("Rip::Faces.faces picks {class}, not a face"),
                );
            }
        }
        Rip::Along { path, .. } => {
            if !convert && matches!(path, RipPath::Edges(_)) {
                error(
                    findings,
                    site.entity,
                    format_args!(
                        "a Rip::Along's path is Edges, which is a Rip::Faces or a ConvertRip"
                    ),
                );
            }
            check_rip_path(site, path, findings);
        }
    }
}

fn check_rip_path(site: Site<'_>, path: &RipPath, findings: &mut Vec<Finding>) {
    match path {
        RipPath::Ruling { face, .. } => {
            check_one(
                site,
                "RipPath::Ruling.face",
                "face",
                face,
                picks::is_face,
                findings,
            );
        }
        RipPath::Line { face, from, to } => {
            check_one(
                site,
                "RipPath::Line.face",
                "face",
                face,
                picks::is_face,
                findings,
            );
            if from == to {
                error(
                    findings,
                    site.entity,
                    format_args!("a RipPath::Line's from and to are one point"),
                );
            }
        }
        RipPath::Edges(edges) => {
            if picks::is_empty(edges) {
                error(
                    findings,
                    site.entity,
                    format_args!("RipPath::Edges is empty"),
                );
            }
        }
        RipPath::Sketched { profile, .. } => {
            options::check_profile(site, "RipPath::Sketched.profile", profile, findings);
        }
    }
}
