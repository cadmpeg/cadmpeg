// SPDX-License-Identifier: Apache-2.0
//! Body, face, and pattern operations.

use std::collections::BTreeMap;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::{body as new, shared::ToolDisposition};
use cadmpeg_ir::features::{self as old, FeatureOperation as LegacyOperation};
use cadmpeg_ir::scalar::Length;

use super::{selections, swept, Projected};

mod patterns;

pub(super) fn operation(value: &LegacyOperation) -> Option<Projected<FeatureOperation>> {
    if !matches!(
        value,
        LegacyOperation::Combine { .. }
            | LegacyOperation::SplitBody { .. }
            | LegacyOperation::SplitFace { .. }
            | LegacyOperation::TrimBodies { .. }
            | LegacyOperation::TrimBodiesAtPlane { .. }
            | LegacyOperation::MoveBody { .. }
            | LegacyOperation::MoveFace { .. }
            | LegacyOperation::DeleteFace { .. }
            | LegacyOperation::MirrorShape { .. }
            | LegacyOperation::Pattern { .. }
    ) {
        return None;
    }
    Some((|| {
        Ok(match value {
            LegacyOperation::Combine {
                operands,
                op,
                keep_tools,
                destination,
            } => {
                if *destination != old::CombineDestination::TargetComponent {
                    return Err("the combine destination has no resolved v11 component");
                }
                FeatureOperation::Combine(Box::new(new::Combine {
                    target: Some(selections::set(selections::bodies(operands.target())?)?),
                    tools: selections::bodies(operands.tools())?,
                    op: Some(match op {
                        old::BooleanKind::Join => cadmpeg_ir::design::options::BooleanOp::Join,
                        old::BooleanKind::Cut => cadmpeg_ir::design::options::BooleanOp::Cut,
                        old::BooleanKind::Intersect => {
                            cadmpeg_ir::design::options::BooleanOp::Intersect
                        }
                    }),
                    tools_after: Some(if *keep_tools {
                        ToolDisposition::Kept
                    } else {
                        ToolDisposition::Consumed
                    }),
                    new_component: None,
                    native_options: BTreeMap::default(),
                    source_settings: BTreeMap::default(),
                }))
            }
            LegacyOperation::SplitBody {
                extend_tool: Some(_),
                ..
            } => return Err("the split body states a tool extension"),
            LegacyOperation::SplitBody {
                targets,
                tool,
                extend_tool: None,
            } => split(
                selections::set(selections::bodies(targets)?)?,
                match tool {
                    old::SplitBodyTool::Plane { plane } => {
                        new::SplitTool::Plane(Box::new(swept::datum_plane(plane)?))
                    }
                    old::SplitBodyTool::Faces(faces) => {
                        new::SplitTool::Geometry(selections::face(faces)?)
                    }
                },
                Some(new::Kept::All {
                    form: Some(new::PieceForm::Bodies),
                }),
            ),
            LegacyOperation::SplitFace { method, .. }
                if !matches!(
                    method,
                    cadmpeg_ir::features::surfaces::SplitFaceMethod::Unresolved {}
                ) =>
            {
                return Err("the split face states a splitting method")
            }
            LegacyOperation::SplitFace { targets, tool, .. } => {
                let tool = match tool {
                    old::SplitFaceTool::Plane { plane } => {
                        new::SplitTool::Plane(Box::new(swept::datum_plane(plane)?))
                    }
                    old::SplitFaceTool::Path(_) => {
                        return Err("the split-face path has no stated projection direction")
                    }
                    old::SplitFaceTool::Planes { .. } => {
                        return Err("the split face has multiple plane tools")
                    }
                    old::SplitFaceTool::Faces(faces) => {
                        new::SplitTool::Geometry(selections::face(faces)?)
                    }
                    old::SplitFaceTool::Native(_) => return Err("the split-face tool is native"),
                };
                split(
                    selections::face(targets)?,
                    tool,
                    Some(new::Kept::All {
                        form: Some(new::PieceForm::Faces),
                    }),
                )
            }
            LegacyOperation::TrimBodies { operands, keep } => split(
                selections::set(selections::bodies(operands.targets())?)?,
                new::SplitTool::Geometry(selections::set(selections::bodies(operands.tools())?)?),
                kept(*keep),
            ),
            LegacyOperation::TrimBodiesAtPlane {
                targets,
                plane,
                keep,
            } => split(
                selections::set(selections::bodies(targets)?)?,
                new::SplitTool::Plane(Box::new(swept::datum_plane(plane)?)),
                kept(*keep),
            ),
            LegacyOperation::MoveBody {
                bodies,
                translation,
                rotation,
                copies,
            } => {
                if rotation.is_some() && *translation != old::FiniteVector3::ZERO {
                    return Err("the body move has no stated order for translation and rotation");
                }
                let delta = translation.get();
                let mut steps = vec![new::TransformStep::Components {
                    frame: None,
                    delta: [delta.x, delta.y, delta.z]
                        .map(|value| {
                            Length::new(value)
                                .map(Dimension::Literal)
                                .ok_or("the translation is invalid")
                        })
                        .into_iter()
                        .collect::<Projected<Vec<_>>>()?
                        .try_into()
                        .map_err(|_| "the translation has invalid arity")?,
                }];
                if let Some(rotation) = rotation {
                    steps.push(new::TransformStep::Rotate {
                        axis: swept::axis(rotation.origin, rotation.direction)?,
                        angle: Dimension::Literal(rotation.angle),
                    });
                }
                FeatureOperation::TransformBodies(Box::new(new::TransformBodies {
                    bodies: selections::set(selections::bodies(bodies)?)?,
                    transformation: new::Transformation {
                        steps,
                        resolved: None,
                    },
                    mode: Some(if *copies == 0 {
                        new::TransformMode::Move
                    } else {
                        new::TransformMode::Copy {
                            count: Dimension::Literal(*copies),
                            divisions: None,
                        }
                    }),
                    native_options: BTreeMap::default(),
                    source_settings: BTreeMap::default(),
                }))
            }
            LegacyOperation::MoveFace { faces, motion } => {
                if let old::FaceMotion::Offset { distance } = motion {
                    FeatureOperation::Offset(Box::new(cadmpeg_ir::design::ops::dressup::Offset {
                        sets: vec![cadmpeg_ir::design::ops::dressup::OffsetSet {
                            faces: selections::face(faces)?,
                            distance: Some(Dimension::Literal(*distance)),
                        }],
                        result: cadmpeg_ir::design::ops::dressup::OffsetResult::Move {},
                        join: None,
                        face_meeting: None,
                        native_options: BTreeMap::default(),
                        source_settings: BTreeMap::default(),
                    }))
                } else {
                    let step = match motion {
                        old::FaceMotion::Translate {
                            direction,
                            distance,
                        } => new::TransformStep::Translate {
                            direction: swept::direction(*direction)?,
                            distance: Dimension::Literal(*distance),
                        },
                        old::FaceMotion::Rotate {
                            axis_origin,
                            axis_dir,
                            angle,
                        } => new::TransformStep::Rotate {
                            axis: swept::axis(*axis_origin, *axis_dir)?,
                            angle: Dimension::Literal(*angle),
                        },
                        old::FaceMotion::Transform { transform } => {
                            new::TransformStep::Fixed(*transform)
                        }
                        old::FaceMotion::Offset { .. } => {
                            return Err("the offset has no rigid transformation")
                        }
                    };
                    FeatureOperation::MoveFaces(Box::new(new::MoveFaces {
                        faces: selections::face(faces)?,
                        transformation: new::Transformation {
                            steps: vec![step],
                            resolved: None,
                        },
                        native_options: BTreeMap::default(),
                        source_settings: BTreeMap::default(),
                    }))
                }
            }
            LegacyOperation::DeleteFace { faces, heal } => {
                FeatureOperation::DeleteFaces(Box::new(new::DeleteFaces {
                    faces: selections::face(faces)?,
                    heal: Some(if *heal {
                        new::Heal::Extend
                    } else {
                        new::Heal::Open
                    }),
                    native_options: BTreeMap::default(),
                    source_settings: BTreeMap::default(),
                }))
            }
            LegacyOperation::MirrorShape { .. } => {
                return Err("the mirror source has no v11 copy or replacement disposition")
            }
            LegacyOperation::Pattern { seeds, pattern } => {
                FeatureOperation::Pattern(Box::new(patterns::project(seeds, pattern)?))
            }
            _ => return Err("unhandled body operation"),
        })
    })())
}

fn split(
    targets: cadmpeg_ir::design::selection::SetSelection,
    tools: new::SplitTool,
    kept: Option<new::Kept<Dimension<bool>>>,
) -> FeatureOperation {
    FeatureOperation::Split(Box::new(new::Split {
        mode: new::SplitMode::ByTools {
            targets,
            tools,
            method: Some(new::SplitMethod::Intersect),
            kept,
        },
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    }))
}

fn kept(side: old::BodyTrimSide) -> Option<new::Kept<Dimension<bool>>> {
    match side {
        old::BodyTrimSide::Unresolved => None,
        old::BodyTrimSide::Forward => Some(new::Kept::Side {
            side: Dimension::Literal(false),
        }),
        old::BodyTrimSide::Reverse => Some(new::Kept::Side {
            side: Dimension::Literal(true),
        }),
    }
}
