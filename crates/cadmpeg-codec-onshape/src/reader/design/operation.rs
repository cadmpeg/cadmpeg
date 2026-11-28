// SPDX-License-Identifier: Apache-2.0
//! The design operation of one Part Studio feature (§6).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, InputReference, InputRole,
    PlaneValue, Relation, ScalarDimension, Subject,
};
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    Expression, Measure, MeasureKind, MeasureTarget, MulOp, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::body::Combine;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::ops::shared::ToolDisposition;
use cadmpeg_ir::design::ops::shared::{Law, Limit, Span};
use cadmpeg_ir::design::ops::swept::{
    DraftNeutral, Extrude, ExtrudeDirection, ExtrudeDraft, ExtrudeOrigin, Revolve, ScalingLaw,
    SpineLimits, Sweep, SweepControls, SweepOrientation, SweepSection, Twist,
};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, LimitOffset, NativeOptions, SolidOutput, SymmetricMeasure,
    Targets,
};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterOwner};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::datums::DatumPlaneReference;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::profiles::{PathRef, PlanarProfileRef};
use cadmpeg_ir::features::revolve::RevolutionAxis;
use cadmpeg_ir::features::{FeatureDefinition, ParameterId};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length, NonNegativeReal};
use cadmpeg_ir::units::UnitVector3;
use serde_json::Value;

use super::Source;
use crate::loss::OnshapeLossCode;
use crate::reader::{features, Mapping as SourceMapping};

mod dressup;
mod picks;

use picks::{pick, selection};

/// A full turn within this many radians is a full revolve.
const FULL_TURN_TOLERANCE_RAD: f64 = 1e-12;

/// What the design states before a feature.
pub(super) struct Context<'c, 'a> {
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
    pub(super) topology: &'c crate::reader::topology::Inputs<'a>,
    pub(super) model: &'c cadmpeg_ir::document::Model,
    pub(super) scope: &'c cadmpeg_ir::ids::ProductDefinitionId,
    pub(super) derived_parameters: &'c RefCell<Vec<DesignParameter>>,
}

impl Context<'_, '_> {
    fn input(
        &self,
        model: &v10::Feature,
    ) -> Option<&cadmpeg_ir::features::evaluation::FeatureInputTopology> {
        self.inputs.iter().find(|input| input.input_of == model.id)
    }

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
pub(super) fn thin(
    thin: &Value,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> Result<FeatureOperation, String> {
    let mut operation: FeatureOperation = json::from_value(thin.clone())
        .map_err(|error| format!("the thin design operation is not supported yet ({error})"))?;
    let FeatureOperation::Extrude(extrude) = &mut operation else {
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
    match &mut extrude.output.boolean {
        Some(Boolean::Apply { targets: scope, .. }) => {
            *scope = targets(source, "defaultScope", "booleanScope")?;
        }
        Some(Boolean::NewBody) => source.usage.account(&["defaultScope", "booleanScope"]),
        _ => {}
    }
    Ok(operation)
}

/// The design operation of `model` and the assumption it states, or the
/// operation it falls back to.
pub(super) fn project(
    model: &v10::Feature,
    source: Option<&Source<'_>>,
    context: &Context<'_, '_>,
) -> Result<(FeatureOperation, Option<&'static str>), Fallback> {
    let kind = source.and_then(Source::kind);
    if let Some(source) = source.filter(|source| !crate::reader::features::is_custom(source.record))
    {
        if Source::independently_projected(source.record) {
            let mut seen = BTreeSet::new();
            for parameter in crate::reader::rows(source.record, "parameters")
                .map_err(|reason| unresolved(kind, None, &reason))?
            {
                let key = crate::reader::text(parameter, "parameterId")
                    .map_err(|reason| unresolved(kind, None, &reason))?;
                if !seen.insert(key) {
                    return Err(unresolved(kind, None, "feature parameter keys repeat"));
                }
            }
        }
        let typed = match kind {
            Some("extrude") => {
                Some(extrude(model, source, context).map(|operation| (operation, None)))
            }
            Some("revolve") => {
                Some(revolve(model, source, context).map(|operation| (operation, None)))
            }
            Some("fillet") => {
                Some(dressup::fillet(model, source, context).map(|operation| (operation, None)))
            }
            Some("chamfer") => Some(
                dressup::chamfer(model, source, context)
                    .map(|(operation, sided)| (operation, sided.then_some(dressup::CHAMFER_SIDE))),
            ),
            Some("hole") => {
                Some(dressup::hole(model, source, context).map(|operation| (operation, None)))
            }
            Some("sweep") => Some(sweep(model, source, context).map(|operation| (operation, None))),
            Some("shell") => {
                Some(dressup::shell(model, source, context).map(|operation| (operation, None)))
            }
            Some("booleanBodies") => {
                Some(combine(source, context).map(|operation| (operation, None)))
            }
            Some("importDerived") => Some(linked(source).map(|operation| (operation, None))),
            _ => None,
        };
        if let Some(typed) = typed {
            return typed.map_err(|reason| unresolved(kind, None, &reason));
        }
    }
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
        v10::FeatureOperation::DatumOffsetPlane {
            reference,
            distance,
        } => offset_plane(reference.as_ref(), *distance)
            .map(|datum| FeatureOperation::Datum(Box::new(datum))),
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
            &cadmpeg_ir::codec_id!(crate::dialect::registry_ids::FORMAT),
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

fn boolean(source: &Source<'_>) -> SourceMapping<Boolean> {
    let op = match source.tag("operationType") {
        Some("NEW") => {
            source.usage.account(&["defaultScope", "booleanScope"]);
            return Ok(Boolean::NewBody);
        }
        Some("ADD") => BooleanOp::Join,
        Some("REMOVE") => BooleanOp::Cut,
        Some("INTERSECT") => BooleanOp::Intersect,
        _ => return Err("operationType does not name a supported Boolean".into()),
    };
    Ok(Boolean::Apply {
        op,
        targets: targets(source, "defaultScope", "booleanScope")?,
    })
}

fn targets(
    source: &Source<'_>,
    default: &'static str,
    key: &'static str,
) -> SourceMapping<Option<Targets>> {
    if source.flag(default)? || features::parameter(source.record, default).is_err() {
        source.usage.account(&[key]);
        return Ok(Some(Targets::All));
    }
    let Ok(parameter) = features::parameter(source.record, key) else {
        return Ok(None);
    };
    let ids = picks::ids(source, key)?;
    if ids.is_empty() && crate::reader::query::has_unresolved_ids(parameter) {
        return Ok(None);
    }
    Ok(Some(Targets::Selected(picks::named_ids(ids)?)))
}

fn source_profile(
    source: &Source<'_>,
    key: &'static str,
    context: &Context<'_, '_>,
) -> SourceMapping<cadmpeg_ir::design::selection::OrderedSelection> {
    let profile: PlanarProfileRef = json::from_value(features::profile(
        source.record,
        key,
        context.model,
        context.topology,
    )?)
    .map_err(|error| format!("{key}: {error}"))?;
    let picks = picks::profile(&profile, context)?;
    source.usage.account(&[key]);
    Ok(picks)
}

fn signed<Q>(
    source: &Source<'_>,
    key: &'static str,
    parameter: Option<ParameterId>,
    literal: impl FnOnce() -> SourceMapping<Q>,
    reverse: bool,
) -> SourceMapping<Dimension<Q>> {
    let dimension = match parameter {
        Some(id) if reverse => Dimension::Negated(id),
        Some(id) => Dimension::Parameter(id),
        None => Dimension::Literal(literal()?),
    };
    source.usage.account(&[key]);
    Ok(dimension)
}

fn extrude(
    feature: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<FeatureOperation> {
    let body = match source.tag("bodyType") {
        Some("SOLID") => BodyForm::Solid,
        Some("SURFACE") => BodyForm::Sheet,
        _ => return Err("bodyType is outside the solid and surface extrusion forms".into()),
    };
    source
        .usage
        .ignore_value(&["domain"], &Value::from("MODEL"));
    source.usage.ignore_false(&["hasExtrudeDirection"]);
    if source.peek_flag("hasExtrudeDirection") == Ok(false) {
        source.usage.account(&["extrudeDirection"]);
    }
    if source
        .peek_tag("domain")
        .is_none_or(|domain| domain == "MODEL")
    {
        source.usage.account(&["flatOperationType"]);
    }
    source.usage.account(&[
        "wallShape",
        "midplane",
        "flipWall",
        "thickness",
        "thickness1",
        "thickness2",
    ]);
    if matches!(body, BodyForm::Sheet) {
        source
            .usage
            .account(&["entities", "operationType", "defaultScope", "booleanScope"]);
    } else {
        source.usage.account(&[
            "surfaceEntities",
            "surfaceOperationType",
            "defaultSurfaceScope",
            "booleanSurfaceScope",
        ]);
    }
    let profile = source_profile(
        source,
        if matches!(body, BodyForm::Sheet) {
            "surfaceEntities"
        } else {
            "entities"
        },
        context,
    )?;
    let distance = |key, role, reverse| {
        signed(
            source,
            key,
            context.owned(feature, &[role]),
            || {
                source.quantity_value(key, |value| {
                    Length::new(if reverse { -value } else { value })
                })
            },
            reverse,
        )
    };
    let symmetric = source.symmetric_extrude()?;
    let bound = source.tag("endBound");
    if matches!(bound, Some("BLIND" | "THROUGH_ALL")) {
        source.usage.account(&["symmetric"]);
    } else {
        source.usage.ignore_false(&["symmetric"]);
    }
    for (key, active_bound) in [
        ("depth", "BLIND"),
        ("endBoundEntityFace", "UP_TO_SURFACE"),
        ("endBoundEntityBody", "UP_TO_BODY"),
        ("endBoundEntityVertex", "UP_TO_VERTEX"),
    ] {
        if bound != Some(active_bound) {
            source.usage.account(&[key]);
        }
    }
    let end_offset_active = matches!(bound, Some("UP_TO_NEXT" | "UP_TO_SURFACE" | "UP_TO_BODY"));
    if !end_offset_active {
        source.usage.account(&["hasOffset"]);
    } else if bound == Some("UP_TO_NEXT") {
        source.usage.ignore_false(&["hasOffset"]);
    }
    if !end_offset_active || source.peek_flag("hasOffset") == Ok(false) {
        source
            .usage
            .account(&["offsetDistance", "offsetOppositeDirection"]);
    }
    let end = match bound {
        Some("BLIND") => Limit::Amount(distance(
            "depth",
            if symmetric {
                "TotalDistance"
            } else {
                "AlongDistance"
            },
            false,
        )?),
        Some("THROUGH_ALL") => Limit::ThroughAll,
        Some("UP_TO_NEXT") => Limit::Next,
        Some(bound @ ("UP_TO_SURFACE" | "UP_TO_BODY")) => Limit::ToGeometry {
            target: picks::selected(
                source,
                if bound == "UP_TO_SURFACE" {
                    "endBoundEntityFace"
                } else {
                    "endBoundEntityBody"
                },
            )?,
            offset: source
                .flag("hasOffset")?
                .then(|| {
                    distance(
                        "offsetDistance",
                        "LimitOffset",
                        !source.flag("offsetOppositeDirection")?,
                    )
                    .map(LimitOffset::Translated)
                })
                .transpose()?,
            extend: (bound == "UP_TO_SURFACE").then_some(Dimension::Literal(true)),
            far: None,
        },
        _ => return Err("endBound is outside the admitted extrusion limits".into()),
    };
    let reversed = if features::parameter(source.record, "oppositeExtrudeDirection").is_ok() {
        source.usage.account(&["oppositeDirection"]);
        source.flag("oppositeExtrudeDirection")?
    } else {
        source.flag("oppositeDirection")?
    };
    let second = !symmetric && source.flag("hasSecondDirection")?;
    if !second {
        source.usage.account(&[
            "hasSecondDirection",
            "secondDirectionBound",
            "hasSecondDirectionDraft",
        ]);
    }
    let second_bound = second.then(|| source.tag("secondDirectionBound")).flatten();
    for (key, active_bound) in [
        ("secondDirectionDepth", "BLIND"),
        ("secondDirectionBoundEntityFace", "UP_TO_SURFACE"),
        ("secondDirectionBoundEntityBody", "UP_TO_BODY"),
        ("secondDirectionBoundEntityVertex", "UP_TO_VERTEX"),
    ] {
        if second_bound != Some(active_bound) {
            source.usage.account(&[key]);
        }
    }
    if !second {
        source.usage.account(&[
            "secondDirectionOppositeDirection",
            "secondDirectionOppositeExtrudeDirection",
        ]);
    }
    let second_offset_active = second_bound == Some("UP_TO_NEXT");
    if second_offset_active {
        source.usage.ignore_false(&["hasSecondDirectionOffset"]);
    } else {
        source.usage.account(&["hasSecondDirectionOffset"]);
    }
    if !second_offset_active || source.peek_flag("hasSecondDirectionOffset") == Ok(false) {
        source.usage.account(&[
            "secondDirectionOffsetDistance",
            "secondDirectionOffsetOppositeDirection",
        ]);
    }
    let span = match end {
        Limit::Amount(amount) if symmetric => Span::Symmetric {
            amount,
            measure: Some(SymmetricMeasure::Total),
        },
        end => Span::Between {
            start: Box::new(if second {
                match second_bound {
                    Some("BLIND") => Limit::Amount(distance(
                        "secondDirectionDepth",
                        "AgainstDistance",
                        source.flag("secondDirectionOppositeDirection")? != reversed,
                    )?),
                    Some("THROUGH_ALL") => Limit::ThroughAll,
                    Some("UP_TO_NEXT") => Limit::Next,
                    _ => {
                        return Err(
                            "secondDirectionBound is outside the admitted extrusion limits".into(),
                        )
                    }
                }
            } else if symmetric && matches!(end, Limit::ThroughAll) {
                Limit::ThroughAll
            } else {
                Limit::AtOrigin
            }),
            end: Box::new(end),
        },
    };
    let span_unresolved = (bound == Some("UP_TO_NEXT")
        && source.peek_flag("hasOffset") != Ok(false))
        || (second_offset_active && source.peek_flag("hasSecondDirectionOffset") != Ok(false));
    let origin = if source.flag("startOffset")? {
        match source.tag("startOffsetBound") {
            Some("BLIND") => {
                source.usage.account(&["startOffsetEntity"]);
                ExtrudeOrigin::Offset(distance(
                    "startOffsetDistance",
                    "StartOffset",
                    source.flag("startOffsetOppositeDirection")?,
                )?)
            }
            Some("ENTITY") => {
                source.usage.account(&["startOffsetDistance"]);
                source.usage.account(&["startOffsetOppositeDirection"]);
                ExtrudeOrigin::Faces {
                    faces: picks::selected(source, "startOffsetEntity")?,
                    offset: None,
                }
            }
            _ => return Err("startOffsetBound does not name an admitted start".into()),
        }
    } else {
        source.usage.account(&[
            "startOffsetBound",
            "startOffsetDistance",
            "startOffsetOppositeDirection",
            "startOffsetEntity",
        ]);
        ExtrudeOrigin::ProfilePlane
    };
    let boolean = if matches!(body, BodyForm::Sheet) {
        match source.tag("surfaceOperationType") {
            None | Some("NEW") => {
                source
                    .usage
                    .account(&["defaultSurfaceScope", "booleanSurfaceScope"]);
                Boolean::NewBody
            }
            Some("ADD") => Boolean::Apply {
                op: BooleanOp::Join,
                targets: targets(source, "defaultSurfaceScope", "booleanSurfaceScope")?,
            },
            _ => return Err("surfaceOperationType does not name a supported operation".into()),
        }
    } else {
        boolean(source)?
    };
    let draft_angle = |key, role, inward: bool| {
        signed(
            source,
            key,
            context.owned(feature, &[role]),
            || source.quantity_value(key, |value| Angle::new(if inward { value } else { -value })),
            !inward,
        )
    };
    let end_draft = source
        .flag("hasDraft")?
        .then(|| {
            draft_angle(
                "draftAngle",
                "DraftAngle",
                source.flag("draftPullDirection")?,
            )
        })
        .transpose()?;
    if end_draft.is_none() {
        source.usage.account(&["draftAngle", "draftPullDirection"]);
    }
    let second_draft = second && source.flag("hasSecondDirectionDraft")?;
    if !second_draft {
        source.usage.account(&[
            "secondDirectionDraftAngle",
            "secondDirectionDraftPullDirection",
        ]);
    }
    let start_draft = if symmetric && matches!(span, Span::Between { .. }) {
        end_draft.clone()
    } else {
        second_draft
            .then(|| {
                draft_angle(
                    "secondDirectionDraftAngle",
                    "AgainstDraftAngle",
                    source.flag("secondDirectionDraftPullDirection")?,
                )
            })
            .transpose()?
    };
    let draft = (start_draft.is_some() || end_draft.is_some()).then_some(ExtrudeDraft {
        end: end_draft,
        start: start_draft,
        neutral: Some(DraftNeutral::Origin),
        holes: None,
    });
    Ok(FeatureOperation::Extrude(Box::new(Extrude {
        profile,
        loops: None,
        direction: (source.peek_flag("hasExtrudeDirection") == Ok(false)).then_some(
            ExtrudeDirection::ProfileNormal {
                reversed: Some(Dimension::Literal(reversed)),
            },
        ),
        origin: Some(origin),
        span: (!span_unresolved).then_some(span),
        draft,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: Some(body),
            boolean: Some(boolean),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

fn revolve(
    feature: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<FeatureOperation> {
    if source.tag("bodyType") != Some("SOLID") {
        return Err("bodyType is outside the solid revolve form".into());
    }
    source.usage.account(&[
        "surfaceEntities",
        "surfaceOperationType",
        "defaultSurfaceScope",
        "booleanSurfaceScope",
        "wallShape",
        "midplane",
        "flipWall",
        "thickness",
        "thickness1",
        "thickness2",
    ]);
    let profile = source_profile(source, "entities", context)?;
    let queries = features::queries(source.record, "axis")?;
    let sketch_axis = queries.iter().any(|query| {
        crate::reader::query::resolved(query)
            .is_ok_and(|query| query["queryType"] == "SKETCH_ENTITY")
    });
    let axis: RevolutionAxis = json::from_value(if sketch_axis {
        features::sketch_axis(source.record, context.model)?
    } else {
        context
            .topology
            .load(source.ordinal, source.record)?
            .axis(source.record, "axis")?
    })
    .map_err(|error| format!("axis: {error}"))?;
    let axis = axis_construction(&axis)?;
    source.usage.account(&["axis"]);
    let reverse = source.peek_flag("oppositeDirection")?;
    let angle = |key, role, reverse| {
        signed(
            source,
            key,
            context.owned(feature, &[role]),
            || {
                source.quantity_value(key, |value| {
                    Angle::new(if reverse { -value } else { value })
                })
            },
            reverse,
        )
    };
    let revolve_type = source.peek_tag("revolveType");
    let full = revolve_type == Some("FULL") || source.flag("fullRevolve")?;
    let end_bound = source.peek_tag("endBound");
    let blind = revolve_type == Some("ONE_DIRECTION")
        || end_bound == Some("BLIND")
        || end_bound.is_none() && features::parameter(source.record, "fullRevolve").is_ok();
    if !full
        && source
            .quantity_value("angle", Some)
            .is_ok_and(|angle| angle < 0.0)
    {
        return Err("the direction convention for a negative revolve angle is unresolved".into());
    }
    source.usage.ignore_false(&[
        "endBoundHasOffset",
        "startBoundHasOffset",
        "startOppositeDirection",
    ]);
    let end = if full {
        source.usage.account(&[
            "angle",
            "endBound",
            "hasStartBound",
            "fullRevolve",
            "oppositeDirection",
        ]);
        if revolve_type == Some("FULL") {
            source.usage.account(&["revolveType"]);
        }
        Limit::ThroughAll
    } else if blind {
        if revolve_type == Some("ONE_DIRECTION") {
            source.usage.account(&["revolveType"]);
        }
        if end_bound == Some("BLIND") {
            source.usage.account(&["endBound"]);
        }
        source.usage.account(&["oppositeDirection"]);
        let amount = angle("angle", "AlongAngle", reverse)?;
        if matches!(amount, Dimension::Literal(angle) if
            (angle.get().abs() - std::f64::consts::TAU).abs() <= FULL_TURN_TOLERANCE_RAD)
        {
            Limit::ThroughAll
        } else {
            Limit::Amount(amount)
        }
    } else {
        return Err("endBound does not name a full or blind revolve".into());
    };
    let span = match end {
        Limit::Amount(amount) if source.flag("symmetric")? => {
            source
                .usage
                .account(&["hasStartBound", "startBound", "angleBack"]);
            Span::Symmetric {
                amount,
                measure: Some(SymmetricMeasure::Total),
            }
        }
        end => Span::Between {
            start: Box::new(if !full && source.flag("hasStartBound")? {
                if source.tag("startBound") != Some("BLIND") {
                    return Err("startBound does not name a blind revolve".into());
                }
                Limit::Amount(angle("angleBack", "AgainstAngle", !reverse)?)
            } else {
                source.usage.account(&["startBound", "angleBack"]);
                Limit::AtOrigin
            }),
            end: Box::new(end),
        },
    };
    source.usage.ignore_false(&["symmetric"]);
    Ok(FeatureOperation::Revolve(Box::new(Revolve {
        profile,
        loops: None,
        axis: Some(axis),
        span: Some(span),
        fit: None,
        output: SolidOutput {
            body: Some(BodyForm::Solid),
            boolean: Some(boolean(source)?),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
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

/// An offset input on the resolved support plane (§6.2).
fn offset_plane(
    reference: Option<&DatumPlaneReference>,
    distance: Length,
) -> Result<Datum, &'static str> {
    let Some(DatumPlaneReference::ResolvedPlane { frame }) = reference else {
        return Err("the datum plane's support is not a resolved plane");
    };
    let support = PlaneValue {
        origin: frame.origin(),
        normal: UnitVector3::normalized_nonzero(frame.normal().into())
            .ok_or("the plane frame is degenerate")?,
        x_axis: UnitVector3::normalized_nonzero(frame.u_axis().into())
            .ok_or("the plane frame is degenerate")?,
    };
    let normal = support.normal.as_raw();
    let origin = support.origin.get();
    let offset = FinitePoint3::new(cadmpeg_ir::math::Point3::new(
        origin.x + distance.get() * normal.x,
        origin.y + distance.get() * normal.y,
        origin.z + distance.get() * normal.z,
    ))
    .ok_or("the offset plane origin is not finite")?;
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
                value: Some(ScalarDimension::Length(Dimension::Literal(distance))),
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

fn sweep(
    feature: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<FeatureOperation> {
    crate::reader::sweep::validate_record(source.record)?;
    if source.tag("bodyType") != Some("SOLID") {
        return Err("bodyType is outside the solid sweep form".into());
    }
    source.usage.account(&[
        "surfaceProfiles",
        "surfaceOperationType",
        "defaultSurfaceScope",
        "booleanSurfaceScope",
        "wallShape",
        "midplane",
        "thickness",
        "thickness1",
        "thickness2",
        "flipWall",
        "trimEnds",
    ]);
    source.usage.ignore_false(&["extendToFullPath"]);
    let (profile, path) =
        crate::reader::sweep::operands(source.record, "profiles", context.model, context.topology)?;
    let profile = json::from_value(profile).map_err(|error| format!("profiles: {error}"))?;
    let mut path: PathRef = json::from_value(path).map_err(|error| format!("path: {error}"))?;
    if let PathRef::SketchCurves { curves, .. } = &mut path {
        if source.flag("pathOppositeDirection")? {
            *curves = curves
                .iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .try_into()
                .map_err(|_| "path repeats a curve")?;
        }
    }
    let path = picks::path(&path, context)?;
    source.usage.account(&["profiles", "path"]);
    let profile_control = source.tag("profileControl");
    if profile_control != Some("LOCK_FACES") {
        source.usage.account(&["lockFaces"]);
    }
    if profile_control != Some("LOCK_DIRECTION") {
        source.usage.account(&["lockDirectionQuery"]);
    }
    let orientation = match profile_control {
        None => None,
        Some("NONE") => Some(SweepOrientation::FollowPath),
        Some("KEEP_ORIENTATION") => Some(SweepOrientation::Fixed),
        Some("LOCK_FACES") => Some(SweepOrientation::Surface(picks::selected(
            source,
            "lockFaces",
        )?)),
        Some("LOCK_DIRECTION") => {
            let axis: RevolutionAxis = json::from_value(
                context
                    .topology
                    .load(source.ordinal, source.record)?
                    .axis(source.record, "lockDirectionQuery")?,
            )
            .map_err(|error| format!("lockDirectionQuery: {error}"))?;
            let direction = UnitVector3::normalized_nonzero(axis.direction.into())
                .ok_or("lockDirectionQuery is degenerate")?;
            Some(SweepOrientation::Direction(Construction {
                inputs: vec![ConstructionInput {
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation: Relation::Parallel,
                    }),
                    reference: Some(InputReference::Picks(picks::selected(
                        source,
                        "lockDirectionQuery",
                    )?)),
                    value: None,
                    reversed: None,
                }],
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: Some(direction),
                resolved_in: BTreeMap::new(),
            }))
        }
        _ => return Err("profileControl does not name a supported sweep orientation".into()),
    };
    let can_twist = !matches!(
        orientation,
        Some(SweepOrientation::Surface(_) | SweepOrientation::Direction(_))
    );
    let scaling = source
        .flag("hasScale")?
        .then(|| {
            Ok::<_, String>(ScalingLaw::Law(Law::Linear {
                start: Dimension::Literal(FiniteReal::ONE),
                end: signed(
                    source,
                    "scaleFactor",
                    context.owned(feature, &["ScaleFactor"]),
                    || source.quantity_value("scaleFactor", FiniteReal::new),
                    false,
                )?,
            }))
        })
        .transpose()?;
    if scaling.is_none() {
        source.usage.account(&["scaleFactor"]);
    }
    let section = SweepSection::Profile(picks::profile(&profile, context)?);
    let output = SolidOutput {
        body: Some(BodyForm::Solid),
        boolean: Some(boolean(source)?),
    };
    let twist = if can_twist && source.flag("hasTwist")? {
        Some(sweep_twist(feature, source, context, &path)?)
    } else {
        source
            .usage
            .account(&["hasTwist", "twistType", "angle", "turns", "pitch", "ccw"]);
        None
    };
    Ok(FeatureOperation::Sweep(Box::new(Sweep {
        section,
        path,
        controls: SweepControls {
            orientation,
            twist,
            scaling,
        },
        guides: Vec::new(),
        extent: None,
        placement: None,
        limits: SpineLimits {
            start: None,
            end: None,
        },
        transition: None,
        output,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

fn sweep_twist(
    feature: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
    path: &cadmpeg_ir::design::selection::OrderedSelection,
) -> SourceMapping<Twist> {
    let reverse = !source.flag("ccw")?;
    let angle = |value: f64| Angle::new(if reverse { -value } else { value });
    let kind = source.tag("twistType").unwrap_or("TURNS");
    for (key, active_kind) in [("angle", "ANGLE"), ("turns", "TURNS"), ("pitch", "PITCH")] {
        if kind != active_kind {
            source.usage.account(&[key]);
        }
    }
    let revolutions = match kind {
        "ANGLE" => {
            return Ok(Twist::Angle(signed(
                source,
                "angle",
                context.owned(feature, &["TwistAngle"]),
                || source.quantity_value("angle", angle),
                reverse,
            )?))
        }
        "TURNS" => match context.owned(feature, &["TwistTurns"]) {
            Some(id) => {
                source.usage.account(&["turns"]);
                parameter_node(id)
            }
            None => {
                return Ok(Twist::Angle(Dimension::Literal(
                    source.quantity("turns", |turns| angle(turns * std::f64::consts::TAU))?,
                )))
            }
        },
        "PITCH" => {
            let pitch = match context.owned(feature, &["TwistPitch"]) {
                Some(id) => {
                    source.usage.account(&["pitch"]);
                    parameter_node(id)
                }
                None => number_node(
                    source.quantity("pitch", NonNegativeReal::new)?,
                    UnitSymbol::Mm,
                )?,
            };
            let length = Node::Measure(Box::new(Measure {
                kind: MeasureKind::Length,
                of: MeasureTarget::Selection(path.clone()),
                cached: None,
            }));
            Node::mul(length, MulOp::Divide, pitch)
        }
        _ => {
            return Err(format!(
                "twistType {kind} is outside the sweep twist definition"
            ))
        }
    };
    let turn = number_node(
        NonNegativeReal::new(std::f64::consts::TAU).ok_or("the full turn is not finite")?,
        UnitSymbol::Rad,
    )?;
    let total = Node::mul(revolutions, MulOp::Multiply, turn);
    let total = if reverse {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(total),
        }
    } else {
        total
    };
    let native = crate::reader::text(source.record, "featureId")?;
    let id = ParameterId::mint(crate::reader::id(
        "parameter",
        &format!("{native}/total-twist"),
    ))
    .map_err(|error| error.to_string())?;
    context
        .derived_parameters
        .borrow_mut()
        .push(DesignParameter {
            id: id.clone(),
            scope: context.scope.clone(),
            owner: Some(ParameterOwner::Feature(feature.id.clone())),
            position: None,
            name: Some(cadmpeg_core::nonblank_const!("Total twist")),
            kind: ValueKind::Quantity(Quantity::ANGLE),
            unit: None,
            definition: ParameterDefinition::Expression(Expression::Neutral(Neutral(total))),
            value: None,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        });
    Ok(Twist::Angle(Dimension::Parameter(id)))
}

fn parameter_node(id: ParameterId) -> Node<ParameterRef> {
    Node::Reference(ParameterRef {
        target: RefTarget::Parameter(id),
        through: None,
    })
}

fn number_node(value: NonNegativeReal, symbol: UnitSymbol) -> SourceMapping<Node<ParameterRef>> {
    Ok(Node::Number {
        value,
        unit: super::parameters::unit(symbol).ok_or("the literal unit power is invalid")?,
    })
}

fn combine(
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> crate::reader::Mapping<FeatureOperation> {
    let crate::reader::derived::BooleanInputs {
        operation,
        keep,
        target,
        tools,
    } = crate::reader::derived::boolean_inputs(
        source.record,
        source.ordinal,
        context.topology,
        Some(&source.usage),
    )?;
    Ok(FeatureOperation::Combine(Box::new(Combine {
        target: (!target.is_empty())
            .then(|| picks::bodies(&target))
            .transpose()?,
        tools: picks::bodies(&tools)?,
        op: Some(match operation {
            "UNION" => BooleanOp::Join,
            "SUBTRACTION" => BooleanOp::Cut,
            "INTERSECTION" => BooleanOp::Intersect,
            _ => return Err("the Boolean operation is not supported yet".into()),
        }),
        tools_after: Some(if keep {
            ToolDisposition::Kept
        } else {
            ToolDisposition::Consumed
        }),
        new_component: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

fn linked(source: &Source<'_>) -> crate::reader::Mapping<FeatureOperation> {
    use cadmpeg_ir::design::feature::{Link, StoredOrigin};
    use cadmpeg_ir::design::selection::DefinitionRef;

    let (document, version, element) = crate::reader::derived::reference(source.record)?;
    let part_studio = features::parameter(source.record, "partStudio")?;
    source.usage.account(&["partStudio"]);
    let modern = source.peek_flag("newUI") != Ok(false)
        || features::parameter(source.record, "newUI").is_err();
    if modern
        && part_studio.get("partQuery").is_some_and(|query| {
            !query.is_null()
                && (!query.is_object()
                    || query
                        .get("queries")
                        .is_some_and(|rows| rows.as_array().is_none_or(|rows| !rows.is_empty())))
        })
    {
        source
            .usage
            .partial("partStudio", "has an unprojected partQuery selection");
    } else if modern
        && part_studio
            .get("configuration")
            .is_some_and(|config| config.as_array().is_none_or(|entries| !entries.is_empty()))
    {
        source
            .usage
            .partial("partStudio", "has an unprojected configuration");
    }
    source.usage.ignore_value(&["newUI"], &Value::Bool(true));
    source.usage.ignore_false(&[
        "includeProperties",
        "includeMateConnectors",
        "preserveActiveSheetMetal",
    ]);
    source.usage.ignore_empty_queries(&["parts", "location"]);
    if matches!(source.peek_tag("placement"), None | Some("AT_ORIGIN")) {
        source.usage.account(&[
            "placement",
            "mateConnectorIndex",
            "mateConnectorId",
            "mateConnectorIndexInFeature",
        ]);
    }
    Ok(FeatureOperation::Stored {
        bodies: None,
        tessellations: Vec::new(),
        origin: StoredOrigin::Linked(Box::new(Link {
            source: None,
            document: Some(DefinitionRef {
                definition: super::external_definition_id(document, version, element)?,
                configuration: None,
            }),
            transform: None,
            scale: None,
            associative: None,
            state: None,
            native_ref: None,
        })),
    })
}
