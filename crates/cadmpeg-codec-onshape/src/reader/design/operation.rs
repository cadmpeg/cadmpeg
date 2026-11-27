// SPDX-License-Identifier: Apache-2.0
//! The design operation of one Part Studio feature (§6).

use std::collections::BTreeSet;

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, InputReference, InputRole,
    PlaneValue, Relation, ScalarDimension, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::body::Combine;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::ops::shared::ToolDisposition;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{
    Extrude, ExtrudeDirection, ExtrudeOrigin, Revolve, SpineLimits, Sweep, SweepControls,
    SweepSection,
};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput, SymmetricMeasure, Targets,
};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::extents::{
    AngularTermination, ExtrudeDirection as V10Direction, ExtrudeExtent, ExtrudeStart,
    LinearTermination,
};
use cadmpeg_ir::features::geometry::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::features::operation_settings::BooleanOp as V10Op;
use cadmpeg_ir::features::operation_settings::{BooleanKind, CombineDestination};
use cadmpeg_ir::features::profiles::{PathRef, ProfileRef};
use cadmpeg_ir::features::revolve::{RevolutionAxis, RevolveConstruction, RevolveExtent};
use cadmpeg_ir::features::sweeps::{SolidSweepOperation, SweepSection as V10Section, SweepShape};
use cadmpeg_ir::features::{FeatureDefinition, ParameterId};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::units::UnitVector3;
use serde_json::Value;

use super::Source;
use crate::loss::OnshapeLossCode;

mod dressup;
mod picks;

use picks::{pick, selection};

/// A full turn within this many radians is a full revolve.
const FULL_TURN_TOLERANCE_RAD: f64 = 1e-12;

/// What the design states before a feature.
pub(super) struct Context<'c> {
    /// The sketches the design states.
    pub(super) sketches: &'c BTreeSet<&'c str>,
    /// The sketch of each sketch entity the design states.
    pub(super) entities: &'c std::collections::BTreeMap<&'c str, &'c str>,
    /// The sketches of the earlier timeline members.
    pub(super) preceding: &'c BTreeSet<String>,
    /// The parameter each feature owns for an input, by the feature and the
    /// input's `FeatureScript` role.
    pub(super) owned: &'c std::collections::BTreeMap<(&'c str, &'c str), &'c ParameterId>,
    /// The stored input states of the features.
    pub(super) inputs: &'c [cadmpeg_ir::features::evaluation::FeatureInputTopology],
    /// The model sketch entities, which a trimmed region is cut against.
    pub(super) model_entities: &'c [cadmpeg_ir::sketches::SketchEntity],
    /// The sketch arrangements built so far.
    pub(super) arrangements: &'c cadmpeg_ir::design::regions::Arrangements,
}

impl Context<'_> {
    /// The parameter `feature` owns for the first of `roles` it states.
    fn owned(&self, feature: &v10::Feature, roles: &[&str]) -> Option<ParameterId> {
        roles
            .iter()
            .find_map(|role| self.owned.get(&(feature.id.as_str(), *role)))
            .map(|id| (*id).clone())
    }
}

/// The operation a feature falls back to, with the loss that names what the
/// model record carries beyond it.
pub(super) struct Fallback {
    pub(super) operation: FeatureOperation,
    pub(super) code: OnshapeLossCode,
    pub(super) reason: String,
}

/// The family of the Onshape standard feature type `kind`
/// (`FeatureScript` standard library).
fn family(kind: &str) -> Option<OperationFamily> {
    Some(match kind {
        "newSketch" => OperationFamily::Sketch,
        "cPlane" | "mateConnector" => OperationFamily::Datum,
        "extrude" => OperationFamily::Extrude,
        "revolve" => OperationFamily::Revolve,
        "sweep" => OperationFamily::Sweep,
        "loft" => OperationFamily::Loft,
        "rib" => OperationFamily::Rib,
        "fillet" => OperationFamily::Fillet,
        "chamfer" => OperationFamily::Chamfer,
        "shell" => OperationFamily::Shell,
        "draft" => OperationFamily::Draft,
        "thicken" => OperationFamily::Thicken,
        "offsetSurface" => OperationFamily::Offset,
        "hole" => OperationFamily::Hole,
        "linearPattern" | "circularPattern" | "curvePattern" | "mirror" => OperationFamily::Pattern,
        "booleanBodies" => OperationFamily::Combine,
        "splitPart" => OperationFamily::Split,
        "deleteBodies" => OperationFamily::DeleteBodies,
        "transform" => OperationFamily::TransformBodies,
        "moveFace" => OperationFamily::MoveFaces,
        "deleteFace" => OperationFamily::DeleteFaces,
        "replaceFace" => OperationFamily::ReplaceFaces,
        "fill" => OperationFamily::BoundaryFill,
        "helix" => OperationFamily::Curve,
        _ => return None,
    })
}

/// The design operation of a thin extrude, admitted where every sketch
/// entity its profile picks is a stated entity of a sketch earlier in the
/// timeline.
pub(super) fn thin(thin: &Value, context: &Context<'_>) -> Result<FeatureOperation, String> {
    let operation: FeatureOperation = json::from_value(thin.clone())
        .map_err(|error| format!("the thin design operation is not supported yet ({error})"))?;
    let FeatureOperation::Extrude(extrude) = &operation else {
        return Err("the thin design feature is no extrusion".into());
    };
    let Picks::Named(picks) = &extrude.profile.picks else {
        return Err("the thin profile names no picks".into());
    };
    for pick in picks.as_slice() {
        let PickTarget::SketchEntity(entity) = &pick.target else {
            return Err("the thin profile picks no sketch entity".into());
        };
        let Some(sketch) = context.entities.get(entity.as_str()) else {
            return Err("the thin profile's sketch is not stated in the design".into());
        };
        if !context.preceding.contains(*sketch) {
            return Err("the thin profile's sketch does not precede it in the timeline".into());
        }
    }
    Ok(operation)
}

/// The design operation of `model` and the assumption it states, or the
/// operation it falls back to.
pub(super) fn project(
    model: &v10::Feature,
    source: Option<&Source>,
    context: &Context<'_>,
) -> Result<(FeatureOperation, Option<&'static str>), Fallback> {
    let kind = source.and_then(|source| source.feature_type.as_deref());
    let FeatureDefinition::Operation(operation) = model.evaluation.definition() else {
        return Err(unresolved(
            kind,
            None,
            "the model record holds no operation",
        ));
    };
    let typed = match operation {
        v10::FeatureOperation::Sketch { sketch } => {
            return match sketch.id() {
                Some(id) if context.sketches.contains(id.as_str()) => {
                    Ok((FeatureOperation::Sketch(id.clone()), None))
                }
                _ => Err(unresolved(
                    kind,
                    Some(OperationFamily::Sketch),
                    "the sketch has no resolved frame and curves",
                )),
            };
        }
        v10::FeatureOperation::Native { .. } => {
            return Err(Fallback {
                operation: family_operation(kind),
                code: OnshapeLossCode::DesignOperationNative,
                reason: String::new(),
            });
        }
        v10::FeatureOperation::TreeNode { .. } => {
            return Err(Fallback {
                operation: native(kind.unwrap_or("feature")),
                code: OnshapeLossCode::DesignOperationNative,
                reason: String::new(),
            });
        }
        v10::FeatureOperation::StoredGeometry {} => {
            return Err(Fallback {
                operation: native(kind.unwrap_or("feature")),
                code: OnshapeLossCode::DesignOperationNative,
                reason: "the derived feature's captured design stays in its model record".into(),
            });
        }
        v10::FeatureOperation::Extrude { .. } => {
            let depth = context.owned(model, &["AlongDistance", "TotalDistance"]);
            extrude(
                operation,
                source.is_some_and(|source| source.next),
                depth.as_ref(),
                context,
            )
            .map(|extrude| FeatureOperation::Extrude(Box::new(extrude)))
        }
        v10::FeatureOperation::Revolve { construction, op } => revolve(
            construction,
            *op,
            context.owned(model, &["AlongAngle"]),
            context,
        )
        .map(|revolve| FeatureOperation::Revolve(Box::new(revolve))),
        v10::FeatureOperation::DatumOffsetPlane { .. } => {
            offset_plane(model).map(|datum| FeatureOperation::Datum(Box::new(datum)))
        }
        v10::FeatureOperation::Fillet { .. } => dressup::fillet(model, operation, source, context),
        v10::FeatureOperation::Chamfer { .. } => {
            return dressup::chamfer(model, operation, source, context)
                .map(|(chamfer, sided)| (chamfer, sided.then_some(dressup::CHAMFER_SIDE)))
                .map_err(|reason| unresolved(kind, None, reason));
        }
        v10::FeatureOperation::Shell { .. } => dressup::shell(model, operation, context),
        v10::FeatureOperation::Hole { .. } => dressup::hole(model, operation, context),
        v10::FeatureOperation::Sweep { .. } => {
            sweep(operation, context).map(|sweep| FeatureOperation::Sweep(Box::new(sweep)))
        }
        v10::FeatureOperation::Combine { .. } => {
            combine(operation).map(|combine| FeatureOperation::Combine(Box::new(combine)))
        }
        _ => Err("the design operation of this feature is not projected yet"),
    };
    typed
        .map(|operation| (operation, None))
        .map_err(|reason| unresolved(kind, None, reason))
}

/// The dimension of `value`, bound to the parameter `parameter` where the
/// feature owns one for it.
fn bound<Q>(parameter: Option<ParameterId>, value: Q) -> Dimension<Q> {
    parameter.map_or(Dimension::Literal(value), Dimension::Parameter)
}

/// A native operation of the Onshape feature type `kind`, or of the kind
/// `onshape:feature` where `kind` holds only white space.
fn native(kind: &str) -> FeatureOperation {
    FeatureOperation::Native {
        kind: cadmpeg_ir::design::names::CodecName::new(
            &cadmpeg_ir::codec_id!(crate::dialect::FORMAT),
            kind,
        )
        .unwrap_or_else(|| cadmpeg_ir::codec_name!("onshape:feature")),
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }
}

/// The family operation of the feature type `kind`, or a native operation
/// where Onshape's type has no family.
fn family_operation(kind: Option<&str>) -> FeatureOperation {
    match kind.and_then(family) {
        Some(family) => FeatureOperation::Unresolved { family },
        None => native(kind.unwrap_or("feature")),
    }
}

fn unresolved(kind: Option<&str>, family: Option<OperationFamily>, reason: &str) -> Fallback {
    let operation = family.map_or_else(
        || family_operation(kind),
        |family| FeatureOperation::Unresolved { family },
    );
    let code = if matches!(operation, FeatureOperation::Unresolved { .. }) {
        OnshapeLossCode::DesignOperationUnresolved
    } else {
        OnshapeLossCode::DesignOperationNative
    };
    Fallback {
        operation,
        code,
        reason: format!("{reason}; the model record carries the decoded operands"),
    }
}

fn boolean(op: V10Op) -> Option<Boolean> {
    let apply = |op| Boolean::Apply {
        op,
        targets: Some(Targets::All),
    };
    match op {
        V10Op::NewBody => Some(Boolean::NewBody),
        V10Op::Join => Some(apply(BooleanOp::Join)),
        V10Op::Cut => Some(apply(BooleanOp::Cut)),
        V10Op::Intersect => Some(apply(BooleanOp::Intersect)),
        V10Op::Unresolved => None,
    }
}

fn length(value: f64) -> Result<Dimension<Length>, &'static str> {
    Ok(Dimension::Literal(
        Length::new(value).ok_or("the length is not finite")?,
    ))
}

fn linear_limit(
    termination: &LinearTermination,
    next: bool,
    depth: Option<&ParameterId>,
) -> Result<Limit<Length>, &'static str> {
    Ok(match termination {
        LinearTermination::Blind { .. } if depth.is_some() => Limit::Amount(Dimension::Parameter(
            depth.cloned().ok_or("the depth is missing")?,
        )),
        LinearTermination::Blind { length: value } => Limit::Amount(length(value.get())?),
        LinearTermination::ThroughAll {} => Limit::ThroughAll,
        LinearTermination::ToFace { .. } if next => Limit::Next,
        LinearTermination::ToFace { face, offset: None } => Limit::ToGeometry {
            target: picks::faces(face)?,
            offset: None,
            extend: None,
            far: None,
        },
        _ => {
            return Err(
                "the extrude limit is neither blind, through all, up to next, nor up to a face",
            )
        }
    })
}

fn extrude(
    operation: &v10::FeatureOperation,
    next: bool,
    depth: Option<&ParameterId>,
    context: &Context<'_>,
) -> Result<Extrude, &'static str> {
    let v10::FeatureOperation::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid,
        ..
    } = operation
    else {
        return Err("the model record is no extrusion");
    };
    let ProfileRef::Planar(profile) = profile else {
        return Err("the extrude profile is not planar");
    };
    let profile = picks::profile(profile, context)?;
    let reversed = match direction {
        V10Direction::ProfileNormal {} => false,
        V10Direction::ReversedProfileNormal {} => true,
        _ => return Err("the extrude direction is not the profile normal"),
    };
    if !matches!(start, ExtrudeStart::ProfilePlane {}) {
        return Err("the extrude starts off the profile plane");
    }
    let span = match extent {
        ExtrudeExtent::OneSided { side } if side.draft.is_none() => Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(linear_limit(&side.termination, next, depth)?),
        },
        ExtrudeExtent::Symmetric { side } if side.draft.is_none() => {
            match linear_limit(&side.termination, false, depth)? {
                Limit::Amount(amount) => Span::Symmetric {
                    amount,
                    measure: Some(SymmetricMeasure::Total),
                },
                _ => return Err("the symmetric extrude limit is not blind"),
            }
        }
        _ => return Err("the extrude extent is not projected yet"),
    };
    Ok(Extrude {
        profile,
        loops: None,
        direction: Some(ExtrudeDirection::ProfileNormal {
            reversed: Some(Dimension::Literal(reversed)),
        }),
        origin: Some(ExtrudeOrigin::ProfilePlane),
        span: Some(span),
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: solid.map(|solid| {
                if solid {
                    BodyForm::Solid
                } else {
                    BodyForm::Sheet
                }
            }),
            boolean: boolean(*op),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}

fn revolve(
    construction: &RevolveConstruction,
    op: V10Op,
    bound: Option<ParameterId>,
    context: &Context<'_>,
) -> Result<Revolve, &'static str> {
    let RevolveConstruction::Resolved {
        profile,
        axis,
        extent,
        solid,
        ..
    } = construction
    else {
        return Err("the revolve construction is unresolved");
    };
    let profile = picks::profile(profile, context)?;
    let angle = match extent {
        RevolveExtent::OneSided {
            termination: AngularTermination::Angle { angle },
        } => angle.get(),
        _ => return Err("the revolve extent is not one angle"),
    };
    let end = if (angle.abs() - std::f64::consts::TAU).abs() <= FULL_TURN_TOLERANCE_RAD {
        Limit::ThroughAll
    } else if let (Some(parameter), true) = (bound, angle > 0.0) {
        Limit::Amount(Dimension::Parameter(parameter))
    } else {
        Limit::Amount(Dimension::Literal(
            Angle::new(angle).ok_or("the revolve angle is not finite")?,
        ))
    };
    Ok(Revolve {
        profile,
        loops: None,
        axis: Some(axis_construction(axis)?),
        span: Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(end),
        }),
        fit: None,
        output: SolidOutput {
            body: Some(if solid.unwrap_or(true) {
                BodyForm::Solid
            } else {
                BodyForm::Sheet
            }),
            boolean: boolean(op),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}

/// The revolve axis: its resolved line, and a `Coincident` pick of the
/// sketch line it was read from, which runs along it.
fn axis_construction(axis: &RevolutionAxis) -> Result<Construction<kinds::Axis>, &'static str> {
    let direction = UnitVector3::normalized_nonzero(axis.direction.into())
        .ok_or("the revolve axis is degenerate")?;
    let mut construction = Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: Some(AxisValue {
            origin: axis.origin,
            direction,
        }),
        resolved_in: std::collections::BTreeMap::new(),
    };
    if let Some(PathRef::SketchCurves { curves, .. }) = &axis.reference {
        let [line] = curves.as_slice() else {
            return Err("the revolve axis picks more than one sketch line");
        };
        construction.inputs.push(ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(vec![pick(
                PickTarget::SketchEntity(line.clone()),
            )])?)),
            value: None,
            reversed: None,
        });
    }
    Ok(construction)
}

/// The plane of a model support frame (`origin`, `normal`, `u_axis`).
fn plane_value(frame: &Value) -> Result<PlaneValue, &'static str> {
    let vector = |key: &str| -> Result<UnitVector3, &'static str> {
        let value: FiniteVector3 = json::from_value(
            frame
                .get(key)
                .cloned()
                .ok_or("the plane frame is incomplete")?,
        )
        .map_err(|_| "the plane frame is unreadable")?;
        UnitVector3::normalized_nonzero(value).ok_or("the plane frame is degenerate")
    };
    Ok(PlaneValue {
        origin: json::from_value(
            frame
                .get("origin")
                .cloned()
                .ok_or("the plane origin is missing")?,
        )
        .map_err(|_| "the plane origin is unreadable")?,
        normal: vector("normal")?,
        x_axis: vector("u_axis")?,
    })
}

/// A datum plane offset by its distance along the normal of a support plane
/// the model resolved: an `Offset` input on that plane, its value the
/// distance (§6.2).
fn offset_plane(model: &v10::Feature) -> Result<Datum, &'static str> {
    let wire = json::to_value(model.evaluation.definition())
        .map_err(|_| "the datum plane is unreadable")?;
    let value = wire
        .pointer("/value/value")
        .ok_or("the datum plane is missing")?;
    if value.pointer("/reference/kind").and_then(Value::as_str) != Some("resolved_plane") {
        return Err("the datum plane's support is not a resolved plane");
    }
    let support = plane_value(
        value
            .pointer("/reference/frame")
            .ok_or("the support frame is missing")?,
    )?;
    let distance = value
        .get("distance")
        .and_then(Value::as_f64)
        .ok_or("the plane offset is missing")?;
    let normal = support.normal.as_raw();
    let origin = support.origin.get();
    let offset: FinitePoint3 = json::from_value(cadmpeg_core::json!({
        "x": origin.x + distance * normal.x,
        "y": origin.y + distance * normal.y,
        "z": origin.z + distance * normal.z,
    }))
    .map_err(|_| "the offset plane origin is not finite")?;
    let resolved = PlaneValue {
        origin: offset,
        normal: support.normal,
        x_axis: support.x_axis,
    };
    let support = Construction::<kinds::Plane> {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: Some(support),
        resolved_in: std::collections::BTreeMap::new(),
    };
    Ok(Datum {
        element: DatumElement::Plane(Construction {
            inputs: vec![ConstructionInput {
                role: Some(InputRole {
                    subject: Subject::Whole,
                    relation: Relation::Offset,
                }),
                reference: Some(InputReference::Built(Box::new(AnyConstruction::Plane(
                    support,
                )))),
                value: Some(ScalarDimension::Length(length(distance)?)),
                reversed: None,
            }],
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: Some(resolved),
            resolved_in: std::collections::BTreeMap::new(),
        }),
        link: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}

/// A solid sweep of one planar profile along a chain of sketch curves, with
/// no orientation, twist, or scaling control.
fn sweep(operation: &v10::FeatureOperation, context: &Context<'_>) -> Result<Sweep, &'static str> {
    let v10::FeatureOperation::Sweep {
        shape:
            SweepShape::Solid {
                op,
                section: V10Section::Profile(profile),
                sections,
            },
        path: Some(path),
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        ..
    } = operation
    else {
        return Err("the sweep states controls beyond a plain solid sweep");
    };
    if !sections.is_empty() {
        return Err("the sweep has more than one section");
    }
    Ok(Sweep {
        section: SweepSection::Profile(picks::profile(profile, context)?),
        path: picks::path(path, context)?,
        controls: SweepControls {
            orientation: None,
            twist: None,
            scaling: None,
        },
        guides: Vec::new(),
        extent: None,
        placement: None,
        limits: SpineLimits {
            start: None,
            end: None,
        },
        transition: None,
        output: SolidOutput {
            body: Some(BodyForm::Solid),
            boolean: boolean(match op {
                SolidSweepOperation::NewBody => V10Op::NewBody,
                SolidSweepOperation::Join => V10Op::Join,
                SolidSweepOperation::Cut => V10Op::Cut,
                SolidSweepOperation::Intersect => V10Op::Intersect,
            }),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}

/// A Boolean of generated bodies whose result stays with its target.
fn combine(operation: &v10::FeatureOperation) -> Result<Combine, &'static str> {
    let v10::FeatureOperation::Combine {
        operands,
        op,
        keep_tools,
        destination: CombineDestination::TargetComponent,
    } = operation
    else {
        return Err("the Boolean makes a new component");
    };
    Ok(Combine {
        target: Some(picks::bodies(operands.target())?),
        tools: picks::bodies(operands.tools())?,
        op: Some(match op {
            BooleanKind::Join => BooleanOp::Join,
            BooleanKind::Cut => BooleanOp::Cut,
            BooleanKind::Intersect => BooleanOp::Intersect,
        }),
        tools_after: Some(if *keep_tools {
            ToolDisposition::Kept
        } else {
            ToolDisposition::Consumed
        }),
        new_component: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}
