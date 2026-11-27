// SPDX-License-Identifier: Apache-2.0
//! Fillet, chamfer, shell, and hole operations (§6.4).

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::dressup::{
    BlendControls, Bore, Chamfer, ChamferSet, ChamferSize, Cone, DepthTo, Fillet, FilletProfile,
    FilletSection, FilletSet, FilletSize, Hole, HoleBottom, HoleHead, HolePositions, HoleShape,
    ShellFeature, StandardSize, ThreadLength, ThreadSpec,
};
use cadmpeg_ir::design::ops::shared::Limit;
use cadmpeg_ir::design::options::{Hand, NativeOption, NativeOptions, Thickness, WallSide};
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::edge_treatments::{ChamferSpec, RadiusSpec};
use cadmpeg_ir::features::extents::LinearTermination;
use cadmpeg_ir::features::holes::{
    HoleBottom as V10Bottom, HoleConstruction, HoleKind, HoleSpecification, HoleThreadDepth,
    ThreadHand,
};
use cadmpeg_ir::scalar::Length;

use super::picks::{self, Mapping};
use super::{bound, Context};
use crate::reader::design::Source;

/// The assumption a chamfer with two setbacks states: the first lies on
/// the face whose loop runs along the edge's own direction (§6.4), swapped
/// by the stored `oppositeDirection`.
pub(super) const CHAMFER_SIDE: &str =
    "the first chamfer setback is read on the face whose loop runs along the edge, swapped by oppositeDirection (ONSHAPE-013)";

fn controls() -> BlendControls {
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

/// A constant-radius circular fillet of the one edge group of `operation`.
pub(super) fn fillet(
    model: &v10::Feature,
    operation: &v10::FeatureOperation,
    source: Option<&Source>,
    context: &Context<'_>,
) -> Mapping<FeatureOperation> {
    let v10::FeatureOperation::Fillet { groups } = operation else {
        return Err("the model record is no fillet");
    };
    let [group] = groups.as_slice() else {
        return Err("the fillet has more than one edge group");
    };
    let RadiusSpec::Constant { radius } = &group.radius else {
        return Err("the fillet radius is not constant");
    };
    if group.tangency_weight.is_some() {
        return Err("the fillet states a tangency weight");
    }
    Ok(FeatureOperation::Fillet(Box::new(Fillet {
        sets: vec![FilletSet::Edges {
            edges: picks::blend_edges(
                &group.edges,
                source.and_then(|source| source.tangent),
                source.map_or(&[], |source| source.seeds.as_slice()),
                context,
            )?,
            section: FilletSection {
                size: Some(FilletSize::Radius(bound(
                    context.owned(model, &["Radius"]),
                    *radius,
                ))),
                profile: Some(FilletProfile::Circular),
                weight: None,
            },
            spine: None,
            hold: [None, None],
        }],
        controls: controls(),
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

/// A chamfer of the one edge group of `operation`, and whether its size
/// states the assumed side of [`CHAMFER_SIDE`].
pub(super) fn chamfer(
    model: &v10::Feature,
    operation: &v10::FeatureOperation,
    source: Option<&Source>,
    context: &Context<'_>,
) -> Mapping<(FeatureOperation, bool)> {
    let v10::FeatureOperation::Chamfer {
        groups,
        flip_direction,
    } = operation
    else {
        return Err("the model record is no chamfer");
    };
    let [group] = groups.as_slice() else {
        return Err("the chamfer has more than one edge group");
    };
    let owned = |role: &str| context.owned(model, &[role]);
    let (size, sided) = match group.spec {
        ChamferSpec::Distance { distance } => (
            ChamferSize::Distance(bound(owned("Distance"), distance)),
            false,
        ),
        ChamferSpec::TwoDistances { first, second } => (
            ChamferSize::Distances {
                first: bound(owned("FirstDistance"), first),
                second: bound(owned("SecondDistance"), second),
            },
            true,
        ),
        ChamferSpec::DistanceAngle { distance, angle } => (
            ChamferSize::DistanceAngle {
                distance: bound(owned("Distance"), distance),
                angle: bound(owned("Angle"), angle),
            },
            true,
        ),
        ChamferSpec::Unresolved { .. } => return Err("the chamfer size is unresolved"),
    };
    Ok((
        FeatureOperation::Chamfer(Box::new(Chamfer {
            sets: vec![ChamferSet {
                edges: picks::blend_edges(
                    &group.edges,
                    source.and_then(|source| source.tangent),
                    source.map_or(&[], |source| source.seeds.as_slice()),
                    context,
                )?,
                size: Some(size),
                reference: None,
                flip: Some(Dimension::Literal(*flip_direction)),
                measure: None,
            }],
            controls: controls(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
        sided,
    ))
}

/// A shell that removes faces, with an inward or outward wall.
pub(super) fn shell(
    model: &v10::Feature,
    operation: &v10::FeatureOperation,
    context: &Context<'_>,
) -> Mapping<FeatureOperation> {
    let v10::FeatureOperation::Shell {
        bodies: None,
        removed_faces,
        thickness,
        outward,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    } = operation
    else {
        return Err("the shell states bodies or offset controls");
    };
    let thickness = thickness.ok_or("the shell has no thickness")?;
    Ok(FeatureOperation::Shell(Box::new(ShellFeature {
        bodies: None,
        removed: picks::faces(removed_faces)?,
        thickness: Some(Thickness::OneSide {
            value: bound(context.owned(model, &["Thickness"]), thickness),
            side: outward.map(|outward| {
                if outward {
                    WallSide::Outside
                } else {
                    WallSide::Inside
                }
            }),
        }),
        faces: Vec::new(),
        join: None,
        face_meeting: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

/// A hole at sketch points: its bore, head, bottom, extent, and its
/// clearance standard or thread.
pub(super) fn hole(
    model: &v10::Feature,
    operation: &v10::FeatureOperation,
    context: &Context<'_>,
) -> Mapping<FeatureOperation> {
    let v10::FeatureOperation::Hole {
        profile: Some(profile),
        face: None,
        direction: None,
        placements: None,
        shape,
        extent,
        bottom,
        taper_angle: None,
        ..
    } = operation
    else {
        return Err("the hole is not placed at sketch points");
    };
    if shape.exit_kind().is_some() {
        return Err("the hole states an exit treatment");
    }
    let owned = |role: &str| context.owned(model, &[role]);
    let HoleConstruction::Form {
        kind,
        specification,
    } = shape.construction()
    else {
        return Err("the hole construction is no form");
    };
    let head = match kind {
        HoleKind::Simple => HoleHead::Plain,
        HoleKind::Counterbore { diameter, depth } => HoleHead::Counterbore {
            diameter: Some(bound(owned("CounterboreDiameter"), *diameter)),
            depth: Some(bound(owned("CounterboreDepth"), *depth)),
        },
        HoleKind::Countersink { diameter, angle } => HoleHead::Countersink(Cone {
            diameter: Some(bound(owned("CountersinkDiameter"), *diameter)),
            depth: None,
            angle: Some(bound(owned("CountersinkAngle"), *angle)),
        }),
        _ => return Err("the hole head is neither plain, counterbore, nor countersink"),
    };
    let bottom = bottom.as_ref().map(|bottom| match bottom {
        V10Bottom::Flat => HoleBottom::Flat,
        V10Bottom::Angled {
            included_angle,
            depth_to_tip,
        } => HoleBottom::DrillPoint {
            angle: Some(bound(owned("TipAngle"), *included_angle)),
            depth_to: Some(if *depth_to_tip {
                DepthTo::Tip
            } else {
                DepthTo::Shoulder
            }),
        },
    });
    let extent = match extent {
        Some(LinearTermination::ThroughAll {}) => Limit::ThroughAll,
        Some(LinearTermination::Blind { length }) => Limit::Amount(bound(
            owned("Depth"),
            Length::new(length.get()).ok_or("the hole depth is not finite")?,
        )),
        _ => return Err("the hole extent is neither blind nor through all"),
    };
    let mut native_options = NativeOptions::new();
    let (standard, thread) = match specification.as_deref() {
        None => (None, None),
        Some(specification) => standard(specification, &mut native_options)?,
    };
    Ok(FeatureOperation::Hole(Box::new(Hole {
        positions: HolePositions::Points {
            points: picks::profile(profile, context)?,
            centers: None,
        },
        start: None,
        direction: None,
        shape: HoleShape::Bore(Bore {
            diameter: shape
                .diameter()
                .map(|diameter| bound(owned("Diameter"), diameter)),
            head: Some(head),
            taper: None,
            bottom,
            near_chamfer: None,
            neck_chamfer: None,
            far_chamfer: None,
        }),
        extent: Some(extent),
        standard,
        thread,
        targets: None,
        series: Vec::new(),
        native_options,
        source_settings: NativeOptions::new(),
    })))
}

fn text(value: &str) -> Mapping<NonBlankString> {
    NonBlankString::new(value.trim()).ok_or("a hole standard text is blank")
}

fn option(options: &mut NativeOptions, key: &str, value: NativeOption) -> Mapping<()> {
    let name = cadmpeg_ir::design::names::CodecName::new(
        &cadmpeg_ir::codec_id!(crate::dialect::FORMAT),
        key,
    )
    .ok_or("a native option name is invalid")?;
    options.insert(name, value);
    Ok(())
}

fn hand(hand: ThreadHand) -> Hand {
    match hand {
        ThreadHand::Right => Hand::Right,
        ThreadHand::Left => Hand::Left,
    }
}

/// The clearance standard or the thread of a hole's standard value. The
/// standard's presentation facts, whose meaning ONSHAPE-005 leaves open,
/// are native options.
fn standard(
    specification: &HoleSpecification,
    options: &mut NativeOptions,
) -> Mapping<(Option<StandardSize>, Option<ThreadSpec>)> {
    let size = |designation: &Option<String>| -> Mapping<_> {
        designation
            .as_deref()
            .map(|size| text(size).map(Dimension::Literal))
            .transpose()
    };
    let class = |class: &Option<String>| -> Mapping<_> { class.as_deref().map(text).transpose() };
    Ok(match specification {
        HoleSpecification::Clearance {
            standard,
            designation,
            fit,
            modeled,
            cosmetic,
            hand: source_hand,
            depth: _,
            clearance: None,
        } => {
            option(
                options,
                "hole_hand",
                NativeOption::Text(Dimension::Literal(text(match source_hand {
                    ThreadHand::Right => "right",
                    ThreadHand::Left => "left",
                })?)),
            )?;
            option(
                options,
                "hole_modeled",
                NativeOption::Bool(Dimension::Literal(*modeled)),
            )?;
            option(
                options,
                "hole_cosmetic",
                NativeOption::Bool(Dimension::Literal(*cosmetic)),
            )?;
            (
                Some(StandardSize {
                    standard: standard.clone(),
                    kind: None,
                    size: size(designation)?,
                    class: class(fit)?,
                }),
                None,
            )
        }
        HoleSpecification::Threaded {
            standard,
            designation,
            class: thread_class,
            modeled,
            cosmetic,
            pitch,
            major_diameter,
            hand: source_hand,
            depth,
            clearance: None,
        } => {
            option(
                options,
                "thread_cosmetic",
                NativeOption::Bool(Dimension::Literal(*cosmetic)),
            )?;
            (
                None,
                Some(ThreadSpec {
                    standard: Some(StandardSize {
                        standard: standard.clone(),
                        kind: None,
                        size: size(designation)?,
                        class: class(thread_class)?,
                    }),
                    major_diameter: major_diameter.map(Dimension::Literal),
                    minor_diameter: None,
                    pitch_diameter: None,
                    pitch: pitch.map(Dimension::Literal),
                    hand: Some(hand(*source_hand)),
                    length: Some(match depth {
                        HoleThreadDepth::HoleDepth => ThreadLength::Full,
                        HoleThreadDepth::Blind { depth } => {
                            ThreadLength::Blind(Dimension::Literal(*depth))
                        }
                        HoleThreadDepth::TappedStandard => ThreadLength::Standard,
                    }),
                    modeled: Some(Dimension::Literal(*modeled)),
                }),
            )
        }
        _ => return Err("the hole standard states a radial clearance"),
    })
}
