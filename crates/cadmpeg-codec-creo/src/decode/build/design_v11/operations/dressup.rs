// SPDX-License-Identifier: Apache-2.0
//! Rounds, chamfers, and holes.
//!
//! The v10 projection reads one constant-radius set per round and one
//! constant-distance set per chamfer. It always states a chamfer's flip as
//! false without reading it, so the flip is unstated here.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{kinds, Construction};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::NameKind;
use cadmpeg_ir::design::ops::dressup::{
    BlendControls, Bore, Chamfer, ChamferSet, ChamferSize, Fillet, FilletSection, FilletSet,
    FilletSize, Hole, HoleBottom, HoleHead, HolePositions, HoleShape,
};
use cadmpeg_ir::design::selection::{
    Anchor, MemberOrder, Members, OrderedSelection, Picks, Selection, SetSelection,
};
use cadmpeg_ir::features::edge_treatments::{ChamferSpec, RadiusSpec};
use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind, HolePlacement, PartialPair};
use cadmpeg_ir::features::{self as old, FeatureId, FeatureOperation as LegacyOperation};
use cadmpeg_ir::scalar::Length;

use super::super::names;
use super::{selections, Stated};

pub(super) fn controls() -> BlendControls {
    BlendControls {
        corner_style: None,
        corners: Vec::new(),
        limits: Vec::new(),
        keep: None,
        transitions: Vec::new(),
        overflow: None,
        keep_features: None,
        round_corners: None,
    }
}

/// Prefixes of the feature-local ids of a generated edge (its curve-topology
/// id) and face (its surface id) in the v10 selection.
const GENERATED_EDGE: &str = "curve#";
const GENERATED_FACE: &str = "surface#";

/// Picks of entities by their geometry ids, with the source's record of the
/// selection; `None` where a record id carries no geometry id.
fn by_ids<'a, O: MemberOrder>(
    ids: impl IntoIterator<Item = &'a str>,
    prefix: &str,
    kind: NameKind,
    native: &str,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<Selection<O>> {
    let picks = ids
        .into_iter()
        .map(|id| {
            let id = crate::identity::numbered_identity(id, prefix)?;
            Some(stated.names.pick(kind, id, holder))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Selection {
        native: selections::unread::<O>(native)?.native,
        ..selections::named(picks)?
    })
}

/// An edge operand: the edges the round or chamfer lists in the current
/// model or in their producers' results, each by its name, and by the
/// selection's native record where the listed edges are neither.
pub(super) fn edges(
    value: &cadmpeg_ir::features::selections::EdgeSelection,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<OrderedSelection> {
    match value {
        cadmpeg_ir::features::selections::EdgeSelection::Resolved { edges, native } => by_ids(
            edges.iter().map(cadmpeg_ir::ids::EdgeId::as_str),
            names::EDGE,
            NameKind::Edge,
            native,
            holder,
            stated,
        )
        .or_else(|| selections::unread(native)),
        cadmpeg_ir::features::selections::EdgeSelection::Native(native) => {
            selections::unread(native)
        }
        cadmpeg_ir::features::selections::EdgeSelection::Generated { edges, native } => by_ids(
            edges.iter().map(|edge| edge.local_id.as_str()),
            GENERATED_EDGE,
            NameKind::Edge,
            native.as_str(),
            holder,
            stated,
        )
        .or_else(|| selections::unread(native.as_str())),
        _ => None,
    }
}

/// A face operand: the faces of the surfaces the source names, by their
/// names.
pub(super) fn faces(
    value: &cadmpeg_ir::features::selections::FaceSelection,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<SetSelection> {
    match value {
        cadmpeg_ir::features::selections::FaceSelection::Resolved { faces, native } => by_ids(
            faces.iter().map(cadmpeg_ir::ids::FaceId::as_str),
            names::FACE,
            NameKind::Face,
            native,
            holder,
            stated,
        )
        .or_else(|| selections::unread(native)),
        cadmpeg_ir::features::selections::FaceSelection::Native(native) => by_ids(
            [native.as_str()],
            names::SURFACE,
            NameKind::Face,
            native,
            holder,
            stated,
        )
        .or_else(|| selections::unread(native)),
        cadmpeg_ir::features::selections::FaceSelection::Generated { faces, native } => by_ids(
            faces.iter().map(|face| face.local_id.as_str()),
            GENERATED_FACE,
            NameKind::Face,
            native.as_str(),
            holder,
            stated,
        )
        .or_else(|| selections::unread(native.as_str())),
        _ => None,
    }
}
fn set(selection: OrderedSelection) -> Option<SetSelection> {
    Some(SetSelection {
        at: selection.at,
        picks: match selection.picks {
            Picks::Named(members) => Picks::Named(Members::new(members.into_vec()).ok()?),
            Picks::Unread => Picks::Unread,
            Picks::Rule { .. } => return None,
        },
        native: selection.native,
    })
}

fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: None,
    }
}

pub(super) fn operation(
    value: &LegacyOperation,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<FeatureOperation> {
    match value {
        LegacyOperation::Fillet { groups } => fillet(groups.iter(), holder, stated),
        LegacyOperation::Chamfer { groups, .. } => chamfer(groups.iter(), holder, stated),
        LegacyOperation::Hole { .. } => hole(value, holder, stated),
        _ => None,
    }
}

fn fillet<'a>(
    groups: impl Iterator<Item = &'a old::edge_treatments::FilletGroup>,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<FeatureOperation> {
    let mut sets = Vec::new();
    let mut read = false;
    for group in groups {
        if group.tangency_weight.is_some() {
            return None;
        }
        let edges = edges(&group.edges, holder, stated);
        let size = match &group.radius {
            RadiusSpec::Constant { radius } => {
                Some(FilletSize::Radius(Dimension::Literal(*radius)))
            }
            RadiusSpec::Unresolved { .. } => None,
            _ => return None,
        };
        read |= edges.is_some() || size.is_some();
        sets.push(FilletSet::Edges {
            edges: edges.unwrap_or_else(unread),
            section: FilletSection {
                size,
                profile: None,
                weight: None,
            },
            spine: None,
            hold: [None, None],
        });
    }
    read.then(|| {
        FeatureOperation::Fillet(Box::new(Fillet {
            sets,
            controls: controls(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    })
}

fn chamfer<'a>(
    groups: impl Iterator<Item = &'a old::edge_treatments::ChamferGroup>,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<FeatureOperation> {
    let mut sets = Vec::new();
    let mut read = false;
    for group in groups {
        let edges = edges(&group.edges, holder, stated).and_then(set);
        let size = match group.spec {
            ChamferSpec::Distance { distance } => {
                Some(ChamferSize::Distance(Dimension::Literal(distance)))
            }
            ChamferSpec::Unresolved { .. } => None,
            _ => return None,
        };
        read |= edges.is_some() || size.is_some();
        sets.push(ChamferSet {
            edges: edges.unwrap_or_else(unread),
            size,
            reference: None,
            flip: None,
            measure: None,
        });
    }
    read.then(|| {
        FeatureOperation::Chamfer(Box::new(Chamfer {
            sets,
            controls: controls(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }))
    })
}

fn head(kind: &HoleKind) -> Option<HoleHead> {
    Some(match kind {
        HoleKind::Simple | HoleKind::SimpleDrilled { .. } => HoleHead::Plain,
        HoleKind::Counterbore { diameter, depth } => HoleHead::Counterbore {
            diameter: Some(Dimension::Literal(*diameter)),
            depth: Some(Dimension::Literal(*depth)),
        },
        HoleKind::PartialCounterbore(PartialPair::First(diameter)) => HoleHead::Counterbore {
            diameter: Some(Dimension::Literal(*diameter)),
            depth: None,
        },
        HoleKind::PartialCounterbore(PartialPair::Second(depth)) => HoleHead::Counterbore {
            diameter: None,
            depth: Some(Dimension::Literal(*depth)),
        },
        _ => return None,
    })
}

/// A hole placed at each position the v10 projection reads, by the point it
/// reads there. The drilling sense of a placement is the axis of the bore's
/// carrier, which the decoder does not set against the start face, so the
/// direction is unstated.
fn hole(
    value: &LegacyOperation,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<FeatureOperation> {
    let LegacyOperation::Hole {
        profile: None,
        profile_filter: None,
        face,
        direction: None,
        placements: Some(placements),
        shape,
        extent,
        bottom,
        taper_angle: None,
        allow_multi_profile_faces: None,
    } = value
    else {
        return None;
    };
    if shape.exit_kind().is_some() {
        return None;
    }
    let HoleConstruction::Form {
        kind,
        specification: None,
    } = shape.construction()
    else {
        return None;
    };
    let positions = placements
        .iter()
        .map(|placement| match placement {
            HolePlacement::Directed { position, .. } => Some(Construction::<kinds::Point> {
                inputs: vec![super::swept::unread_input()],
                resolved: Some(*position),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: BTreeMap::new(),
            }),
            HolePlacement::Axis { .. } => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let drill_point = match kind {
        HoleKind::SimpleDrilled { drill_point_angle } => Some(HoleBottom::DrillPoint {
            angle: Some(Dimension::Literal(*drill_point_angle)),
            depth_to: None,
        }),
        _ => None,
    };
    let bottom = match (bottom, drill_point) {
        (Some(_), Some(_)) => return None,
        (Some(old::holes::HoleBottom::Flat), None) => Some(HoleBottom::Flat),
        (Some(_), None) => return None,
        (None, drill_point) => drill_point,
    };
    let extent = match extent {
        Some(cadmpeg_ir::features::extents::LinearTermination::Blind { length }) => {
            Some(cadmpeg_ir::design::ops::shared::Limit::Amount(
                Dimension::Literal(Length::new(length.get())?),
            ))
        }
        Some(cadmpeg_ir::features::extents::LinearTermination::ThroughAll {}) => {
            Some(cadmpeg_ir::design::ops::shared::Limit::ThroughAll)
        }
        _ => None,
    };
    Some(FeatureOperation::Hole(Box::new(Hole {
        positions: HolePositions::Placed(positions),
        start: face.as_ref().and_then(|face| faces(face, holder, stated)),
        direction: None,
        shape: HoleShape::Bore(Bore {
            diameter: shape.diameter().map(Dimension::Literal),
            head: head(kind),
            taper: None,
            bottom,
            near_chamfer: None,
            neck_chamfer: None,
            far_chamfer: None,
        }),
        extent,
        standard: None,
        thread: None,
        targets: None,
        series: Vec::new(),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}
