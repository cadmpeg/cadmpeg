// SPDX-License-Identifier: Apache-2.0
//! Body, face, and pattern operations.

use std::collections::BTreeMap;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::{body as new, shared::ToolDisposition};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
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
            | LegacyOperation::SewBodies { .. }
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
                if *destination
                    != cadmpeg_ir::features::operation_settings::CombineDestination::TargetComponent
                {
                    return Err("the combine destination has no resolved v11 component");
                }
                FeatureOperation::Combine(Box::new(new::Combine {
                    target: Some(selections::set(selections::bodies(operands.target())?)?),
                    tools: selections::bodies(operands.tools())?,
                    op: Some(match op {
                        cadmpeg_ir::features::operation_settings::BooleanKind::Join => {
                            cadmpeg_ir::design::options::BooleanOp::Join
                        }
                        cadmpeg_ir::features::operation_settings::BooleanKind::Cut => {
                            cadmpeg_ir::design::options::BooleanOp::Cut
                        }
                        cadmpeg_ir::features::operation_settings::BooleanKind::Intersect => {
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
                targets,
                tool,
                extend_tool,
            } => split(
                selections::set(selections::bodies(targets)?)?,
                match tool {
                    cadmpeg_ir::features::direct_edit::SplitBodyTool::Plane { .. }
                        if extend_tool.is_some() =>
                    {
                        return Err("the split body states a tool extension beside a plane")
                    }
                    cadmpeg_ir::features::direct_edit::SplitBodyTool::Plane { plane } => {
                        new::SplitTool::Plane(Box::new(swept::datum_plane(plane)?))
                    }
                    cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(faces) => {
                        new::SplitTool::Geometry {
                            tools: selections::face(faces)?,
                            extend: extend_tool.map(|extended| {
                                if extended {
                                    new::ToolExtension::Extended { method: None }
                                } else {
                                    new::ToolExtension::Off
                                }
                            }),
                        }
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
                    cadmpeg_ir::features::direct_edit::SplitFaceTool::Plane { plane } => {
                        new::SplitTool::Plane(Box::new(swept::datum_plane(plane)?))
                    }
                    cadmpeg_ir::features::direct_edit::SplitFaceTool::Path(_) => {
                        return Err("the split-face path has no stated projection direction")
                    }
                    cadmpeg_ir::features::direct_edit::SplitFaceTool::Planes { .. } => {
                        return Err("the split face has multiple plane tools")
                    }
                    cadmpeg_ir::features::direct_edit::SplitFaceTool::Faces(faces) => {
                        new::SplitTool::Geometry {
                            tools: selections::face(faces)?,
                            extend: None,
                        }
                    }
                    cadmpeg_ir::features::direct_edit::SplitFaceTool::Native(_) => {
                        return Err("the split-face tool is native")
                    }
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
                new::SplitTool::Geometry {
                    tools: selections::set(selections::bodies(operands.tools())?)?,
                    extend: None,
                },
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
                let delta = translation.get();
                let components = new::TransformStep::Components {
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
                };
                let rotate = rotation
                    .as_ref()
                    .map(|rotation| {
                        Ok::<_, &'static str>(new::TransformStep::Rotate {
                            axis: swept::axis(rotation.origin, rotation.direction)?,
                            angle: Dimension::Literal(rotation.angle),
                        })
                    })
                    .transpose()?;
                // The stored matrix maps `p` to `R*p + t`: the rotation applies
                // first, then the translation. A zero translation leaves the
                // order free, and keeps the translation step first.
                let steps = match rotate {
                    Some(rotate)
                        if *translation != cadmpeg_ir::features::geometry::FiniteVector3::ZERO =>
                    {
                        vec![rotate, components]
                    }
                    Some(rotate) => vec![components, rotate],
                    None => vec![components],
                };
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
                if let cadmpeg_ir::features::direct_edit::FaceMotion::Offset { distance } = motion {
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
                        cadmpeg_ir::features::direct_edit::FaceMotion::Translate {
                            direction,
                            distance,
                        } => new::TransformStep::Translate {
                            direction: swept::direction(*direction)?,
                            distance: Dimension::Literal(*distance),
                        },
                        cadmpeg_ir::features::direct_edit::FaceMotion::Rotate {
                            axis_origin,
                            axis_dir,
                            angle,
                        } => new::TransformStep::Rotate {
                            axis: swept::axis(*axis_origin, *axis_dir)?,
                            angle: Dimension::Literal(*angle),
                        },
                        cadmpeg_ir::features::direct_edit::FaceMotion::Transform { transform } => {
                            new::TransformStep::Fixed(*transform)
                        }
                        cadmpeg_ir::features::direct_edit::FaceMotion::Offset { .. } => {
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
            LegacyOperation::SewBodies {
                bodies,
                gap_tolerance,
            } => FeatureOperation::Knit(Box::new(cadmpeg_ir::design::ops::surface::Knit {
                elements: selections::bodies(bodies)?,
                tolerance: gap_tolerance.as_ref().map(|gap| {
                    Dimension::Literal(cadmpeg_ir::scalar::NonNegativeLength::from(*gap))
                }),
                simplify: None,
                healing: None,
                federation: None,
                output: cadmpeg_ir::design::options::SheetOutput {
                    solid: None,
                    boolean: None,
                },
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
            })),
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
            targets: Some(targets),
            tools,
            method: Some(new::SplitMethod::Intersect),
            kept,
        },
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    }))
}

fn kept(
    side: cadmpeg_ir::features::operation_settings::BodyTrimSide,
) -> Option<new::Kept<Dimension<bool>>> {
    match side {
        cadmpeg_ir::features::operation_settings::BodyTrimSide::Unresolved => None,
        cadmpeg_ir::features::operation_settings::BodyTrimSide::Forward => Some(new::Kept::Side {
            side: Dimension::Literal(false),
        }),
        cadmpeg_ir::features::operation_settings::BodyTrimSide::Reverse => Some(new::Kept::Side {
            side: Dimension::Literal(true),
        }),
    }
}
