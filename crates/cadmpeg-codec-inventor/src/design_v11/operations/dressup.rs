// SPDX-License-Identifier: Apache-2.0
//! Dress-up and drilling operands.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{kinds, Construction};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::dressup as new;
use cadmpeg_ir::design::options::{Thickness, WallSide};
use cadmpeg_ir::features::{self as old, FeatureOperation as LegacyOperation};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::sketches::Sketch;

use super::{selections, swept, Projected};

fn controls() -> new::BlendControls {
    new::BlendControls {
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

fn radius(value: &old::edge_treatments::RadiusSpec) -> Projected<new::FilletSize> {
    Ok(match value {
        old::edge_treatments::RadiusSpec::Constant { radius } => {
            new::FilletSize::Radius(Dimension::Literal(*radius))
        }
        old::edge_treatments::RadiusSpec::Chordal { chord_length } => {
            new::FilletSize::Chord(Dimension::Literal(*chord_length))
        }
        old::edge_treatments::RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } => new::FilletSize::Offsets {
            first: Dimension::Literal(*offset_one),
            second: Dimension::Literal(*offset_two),
        },
        old::edge_treatments::RadiusSpec::Variable { points } => {
            new::FilletSize::Variable(new::Stations {
                points: points
                    .as_slice()
                    .iter()
                    .map(|point| new::ValueStation {
                        at: new::StationAt::Parameter {
                            along: new::Along::Chain {},
                            fraction: Dimension::Literal(point.parameter),
                        },
                        value: Dimension::Literal(point.radius),
                    })
                    .collect(),
                law: None,
                chain_start: None,
            })
        }
        old::edge_treatments::RadiusSpec::Unresolved { .. } => {
            return Err("the fillet size is unresolved")
        }
    })
}

fn thickness(value: cadmpeg_ir::scalar::PositiveLength, outward: Option<bool>) -> Thickness {
    Thickness::OneSide {
        value: Dimension::Literal(value),
        side: outward.map(|value| {
            if value {
                WallSide::Outside
            } else {
                WallSide::Inside
            }
        }),
    }
}

pub(super) fn operation(
    value: &LegacyOperation,
    sketches: &[Sketch],
) -> Option<Projected<FeatureOperation>> {
    if !matches!(
        value,
        LegacyOperation::CosmeticThread { .. }
            | LegacyOperation::Fillet { .. }
            | LegacyOperation::FullRoundFillet { .. }
            | LegacyOperation::FaceBlend { .. }
            | LegacyOperation::Chamfer { .. }
            | LegacyOperation::Shell { .. }
            | LegacyOperation::Thicken { .. }
            | LegacyOperation::Hole { .. }
    ) {
        return None;
    }
    Some((|| {
        Ok(match value {
            LegacyOperation::Fillet { groups } => FeatureOperation::Fillet(Box::new(new::Fillet {
                sets: groups
                    .iter()
                    .map(|group| {
                        Ok(new::FilletSet::Edges {
                            edges: selections::edge(&group.edges)?,
                            section: new::FilletSection {
                                size: Some(radius(&group.radius)?),
                                profile: None,
                                weight: group
                                    .tangency_weight
                                    .map(|value| {
                                        PositiveReal::new(value.get())
                                            .map(Dimension::Literal)
                                            .ok_or("the fillet weight is outside the v11 range")
                                    })
                                    .transpose()?,
                            },
                            spine: None,
                            hold: [None, None],
                        })
                    })
                    .collect::<Projected<_>>()?,
                controls: controls(),
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
            })),
            LegacyOperation::FullRoundFillet { groups } => {
                let side = |value: &old::edge_treatments::FullRoundSideSelection| -> Projected<new::RoundSide> {
                    Ok(match value {
                        old::edge_treatments::FullRoundSideSelection::Automatic => new::RoundSide::Automatic {},
                        old::edge_treatments::FullRoundSideSelection::Explicit(faces) => {
                            new::RoundSide::Faces(selections::face(faces)?)
                        }
                        old::edge_treatments::FullRoundSideSelection::Unresolved => {
                            return Err("the full-round side is unresolved")
                        }
                    })
                };
                FeatureOperation::Fillet(Box::new(new::Fillet {
                    sets: groups
                        .iter()
                        .map(|group| {
                            Ok(new::FilletSet::FullRound {
                                first: side(group.side_one_faces())?,
                                center: selections::face(group.center_faces())?,
                                second: side(group.side_two_faces())?,
                            })
                        })
                        .collect::<Projected<_>>()?,
                    controls: controls(),
                    native_options: BTreeMap::default(),
                    source_settings: BTreeMap::default(),
                }))
            }
            LegacyOperation::FaceBlend {
                operands,
                radius: size,
            } => FeatureOperation::Fillet(Box::new(new::Fillet {
                sets: vec![new::FilletSet::Faces {
                    first: new::FaceSide {
                        faces: selections::face(operands.first_faces())?,
                        hold: None,
                    },
                    second: Box::new(new::FaceSide {
                        faces: selections::face(operands.second_faces())?,
                        hold: None,
                    }),
                    section: new::FilletSection {
                        size: Some(radius(size)?),
                        profile: None,
                        weight: None,
                    },
                    spine: None,
                    near: None,
                    trim: None,
                }],
                controls: controls(),
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
            })),
            LegacyOperation::Chamfer {
                groups,
                flip_direction,
            } => FeatureOperation::Chamfer(Box::new(new::Chamfer {
                sets: groups
                    .iter()
                    .map(|group| {
                        Ok(new::ChamferSet {
                            edges: selections::set(selections::edge(&group.edges)?)?,
                            size: Some(match group.spec {
                                old::edge_treatments::ChamferSpec::Distance { distance } => {
                                    new::ChamferSize::Distance(Dimension::Literal(distance))
                                }
                                old::edge_treatments::ChamferSpec::TwoDistances {
                                    first,
                                    second,
                                } if first == second => {
                                    new::ChamferSize::Distance(Dimension::Literal(first))
                                }
                                old::edge_treatments::ChamferSpec::TwoDistances { .. }
                                | old::edge_treatments::ChamferSpec::DistanceAngle { .. } => {
                                    return Err(
                                        "the chamfer support side has no v11 reference selection",
                                    )
                                }
                                old::edge_treatments::ChamferSpec::Unresolved { .. } => {
                                    return Err("the chamfer size is unresolved")
                                }
                            }),
                            reference: None,
                            flip: Some(Dimension::Literal(*flip_direction)),
                            measure: None,
                        })
                    })
                    .collect::<Projected<_>>()?,
                controls: controls(),
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
            })),
            LegacyOperation::Shell {
                bodies,
                removed_faces,
                thickness: wall,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            } => {
                if mode.is_some()
                    || join.is_some()
                    || resolve_intersections.is_some()
                    || allow_self_intersections.is_some()
                {
                    return Err("the shell offset controls have no v11 fields");
                }
                if wall.is_none() && outward.is_some() {
                    return Err("the shell side has no thickness");
                }
                FeatureOperation::Shell(Box::new(new::ShellFeature {
                    bodies: bodies
                        .as_ref()
                        .map(|value| selections::set(selections::bodies(value)?))
                        .transpose()?,
                    removed: selections::face(removed_faces)?,
                    thickness: wall.map(|value| thickness(value, *outward)),
                    faces: Vec::new(),
                    join: None,
                    face_meeting: None,
                    native_options: BTreeMap::default(),
                    source_settings: BTreeMap::default(),
                }))
            }
            LegacyOperation::Thicken {
                faces,
                thickness: wall,
                side,
                result,
                tangent_chain,
            } => {
                if tangent_chain.is_some() {
                    return Err("the thicken tangent-chain control requires a selection rule");
                }
                let wall = match (wall, side) {
                    (None, None) => None,
                    (Some(value), None) => Some(thickness(*value, None)),
                    (
                        Some(value),
                        Some(cadmpeg_ir::features::direct_edit::ThickenSide::Forward),
                    ) => Some(thickness(*value, Some(true))),
                    (
                        Some(value),
                        Some(cadmpeg_ir::features::direct_edit::ThickenSide::Reverse),
                    ) => Some(thickness(*value, Some(false))),
                    (Some(value), Some(cadmpeg_ir::features::direct_edit::ThickenSide::Both)) => {
                        Some(Thickness::Symmetric(Dimension::Literal(*value)))
                    }
                    _ => return Err("the thicken side has no thickness"),
                };
                let boolean = result
                    .as_ref()
                    .map(|value| -> Projected<_> {
                        Ok(match value {
                            cadmpeg_ir::features::operation_settings::SolidFeatureResult::NewBody {} => {
                                cadmpeg_ir::design::options::Boolean::NewBody
                            }
                            cadmpeg_ir::features::operation_settings::SolidFeatureResult::Boolean { operation, targets } => {
                                cadmpeg_ir::design::options::Boolean::Apply {
                                    op: match operation {
                                        cadmpeg_ir::features::operation_settings::BooleanKind::Join => {
                                            cadmpeg_ir::design::options::BooleanOp::Join
                                        }
                                        cadmpeg_ir::features::operation_settings::BooleanKind::Cut => {
                                            cadmpeg_ir::design::options::BooleanOp::Cut
                                        }
                                        cadmpeg_ir::features::operation_settings::BooleanKind::Intersect => {
                                            cadmpeg_ir::design::options::BooleanOp::Intersect
                                        }
                                    },
                                    targets: Some(cadmpeg_ir::design::options::Targets::Selected(
                                        selections::set(selections::bodies(targets)?)?,
                                    )),
                                }
                            }
                        })
                    })
                    .transpose()?;
                FeatureOperation::Thicken(Box::new(new::Thicken {
                    faces: selections::face(faces)?,
                    thickness: wall,
                    boolean,
                    native_options: BTreeMap::default(),
                    source_settings: BTreeMap::default(),
                }))
            }
            LegacyOperation::Hole {
                profile,
                profile_filter,
                face,
                direction,
                placements,
                shape,
                extent,
                bottom,
                taper_angle,
                allow_multi_profile_faces,
            } => {
                if taper_angle.is_some() || shape.exit_kind().is_some() {
                    return Err("the hole taper or exit treatment has no equivalent v11 bore");
                }
                let (positions, drilling) = if let Some(placements) = placements {
                    if profile.is_some() || profile_filter.is_some() {
                        return Err("the hole has both profile and placement operands");
                    }
                    let mut drilling = direction.map(swept::direction).transpose()?;
                    let mut points = Vec::new();
                    for placement in placements {
                        let old::holes::HolePlacement::Directed {
                            position,
                            direction,
                        } = placement
                        else {
                            return Err("the hole placement axis is unoriented");
                        };
                        let candidate = swept::direction(*direction)?;
                        if drilling.as_ref().is_some_and(|value| value != &candidate) {
                            return Err("the hole placements have different drilling directions");
                        }
                        drilling = Some(candidate);
                        points.push(Construction::<kinds::Point> {
                            resolved: Some(*position),
                            ..Construction::default()
                        });
                    }
                    (new::HolePositions::Placed(points), drilling)
                } else {
                    let profile = profile.as_ref().ok_or("the hole has no positions")?;
                    let centers = profile_filter.map(|value| {
                        use new::CenterSource::{Arcs, Circles, Points};
                        match value {
                            old::holes::HoleProfileFilter::Points => vec![Points],
                            old::holes::HoleProfileFilter::Circles => vec![Circles],
                            old::holes::HoleProfileFilter::PointsAndCircles => {
                                vec![Points, Circles]
                            }
                            old::holes::HoleProfileFilter::Arcs => vec![Arcs],
                            old::holes::HoleProfileFilter::PointsAndArcs => vec![Points, Arcs],
                            old::holes::HoleProfileFilter::CirclesAndArcs => vec![Circles, Arcs],
                            old::holes::HoleProfileFilter::All => vec![Points, Circles, Arcs],
                        }
                        .into_iter()
                        .collect()
                    });
                    (
                        new::HolePositions::Points {
                            points: selections::planar(profile, sketches)?,
                            centers,
                        },
                        direction.map(swept::direction).transpose()?,
                    )
                };
                let old::holes::HoleConstruction::Form {
                    kind,
                    specification: None,
                } = shape.construction()
                else {
                    return Err("the hole specification requires additional v11 thread operands");
                };
                let mut bore = bore(kind, shape.diameter())?;
                if let Some(bottom) = bottom {
                    if bore.bottom.is_some() {
                        return Err("the hole stores two bottom rules");
                    }
                    bore.bottom = Some(match bottom {
                        old::holes::HoleBottom::Flat => new::HoleBottom::Flat,
                        old::holes::HoleBottom::Angled {
                            included_angle,
                            depth_to_tip,
                        } => new::HoleBottom::DrillPoint {
                            angle: Some(Dimension::Literal(*included_angle)),
                            depth_to: Some(if *depth_to_tip {
                                new::DepthTo::Tip
                            } else {
                                new::DepthTo::Shoulder
                            }),
                        },
                    });
                }
                let mut settings = BTreeMap::default();
                swept::setting(
                    &mut settings,
                    cadmpeg_core::nonblank_literal!("inventor:allow_multi_profile_faces"),
                    *allow_multi_profile_faces,
                );
                FeatureOperation::Hole(Box::new(new::Hole {
                    positions,
                    start: face.as_ref().map(selections::face).transpose()?,
                    direction: drilling,
                    shape: new::HoleShape::Bore(bore),
                    extent: extent.as_ref().map(swept::linear).transpose()?,
                    standard: None,
                    thread: None,
                    targets: None,
                    series: Vec::new(),
                    native_options: BTreeMap::default(),
                    source_settings: settings,
                }))
            }
            LegacyOperation::CosmeticThread {
                face,
                standard,
                designation,
                diameter,
                pitch,
                modeled,
                extent,
            } => FeatureOperation::Thread(Box::new(new::Thread {
                face: selections::face(face)?,
                start: None,
                spec: new::ThreadSpec {
                    standard: match (standard, designation) {
                        (Some(standard), designation) => Some(new::StandardSize {
                            standard: standard.clone(),
                            kind: None,
                            size: designation.clone().map(Dimension::Literal),
                            class: None,
                        }),
                        (None, Some(_)) => return Err("the thread designation has no standard"),
                        (None, None) => None,
                    },
                    major_diameter: diameter.map(Dimension::Literal),
                    minor_diameter: None,
                    pitch_diameter: None,
                    pitch: pitch.map(Dimension::Literal),
                    hand: None,
                    length: extent.map(|value| match value {
                        old::CosmeticThreadExtent::Blind { length } => {
                            new::ThreadLength::Blind(Dimension::Literal(length))
                        }
                        old::CosmeticThreadExtent::Through {} => new::ThreadLength::Full,
                    }),
                    modeled: modeled.map(Dimension::Literal),
                },
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
            })),
            _ => return Err("unhandled dress-up operation"),
        })
    })())
}

fn bore(
    kind: &old::holes::HoleKind,
    diameter: Option<cadmpeg_ir::scalar::PositiveLength>,
) -> Projected<new::Bore> {
    use old::holes::HoleKind;
    let head = match kind {
        HoleKind::Simple | HoleKind::SimpleDrilled { .. } => Some(new::HoleHead::Plain),
        HoleKind::Counterbore { diameter, depth }
        | HoleKind::CounterboreDrilled {
            diameter, depth, ..
        } => Some(new::HoleHead::Counterbore {
            diameter: Some(Dimension::Literal(*diameter)),
            depth: Some(Dimension::Literal(*depth)),
        }),
        HoleKind::Countersink { diameter, angle } => Some(new::HoleHead::Countersink(new::Cone {
            diameter: Some(Dimension::Literal(*diameter)),
            depth: None,
            angle: Some(Dimension::Literal(*angle)),
        })),
        _ => return Err("the hole head requires additional v11 treatment operands"),
    };
    let bottom = match kind {
        HoleKind::SimpleDrilled { drill_point_angle }
        | HoleKind::CounterboreDrilled {
            drill_point_angle, ..
        } => Some(new::HoleBottom::DrillPoint {
            angle: Some(Dimension::Literal(*drill_point_angle)),
            depth_to: None,
        }),
        _ => None,
    };
    Ok(new::Bore {
        diameter: diameter.map(Dimension::Literal),
        head,
        taper: None,
        bottom,
        near_chamfer: None,
        neck_chamfer: None,
        far_chamfer: None,
    })
}
