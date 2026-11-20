// SPDX-License-Identifier: Apache-2.0
//! Extrudes and revolves.
//!
//! A Creo extrude travels along its section's normal and spans from the
//! section plane. The v10 projection states the travel as a model-space
//! vector, which is the section normal or its reverse; it is stated here
//! against the v11 sketch's frame.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{
    AxisValue, Construction, ConstructionInput, InputReference, kinds,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin, Revolve};
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp, SolidOutput, SymmetricMeasure};
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::features::{
    self as old, AngularTermination, ExtrudeExtent, FeatureOperation as LegacyOperation,
    LinearTermination, RevolveExtent,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::units::UnitVector3;

use super::{Stated, selections};

/// The largest departure from ±1 of the cosine between the travel and the
/// section normal at which the travel is the section normal.
const PARALLEL: f64 = 1e-9;

pub(super) fn boolean(op: old::BooleanOp) -> Option<Boolean> {
    let apply = |op| Boolean::Apply { op, targets: None };
    Some(match op {
        old::BooleanOp::NewBody => Boolean::NewBody,
        old::BooleanOp::Join => apply(BooleanOp::Join),
        old::BooleanOp::Cut => apply(BooleanOp::Cut),
        old::BooleanOp::Intersect => apply(BooleanOp::Intersect),
        old::BooleanOp::Unresolved => return None,
    })
}

pub(super) fn output(op: old::BooleanOp, solid: Option<bool>) -> SolidOutput {
    SolidOutput {
        body: solid.map(|solid| {
            if solid {
                BodyForm::Solid
            } else {
                BodyForm::Sheet
            }
        }),
        boolean: boolean(op),
    }
}

fn linear(value: &LinearTermination) -> Option<Limit<Length>> {
    Some(match value {
        LinearTermination::Blind { length } => {
            Limit::Amount(Dimension::Literal(Length::new(length.get())?))
        }
        LinearTermination::ThroughAll {} => Limit::ThroughAll,
        LinearTermination::ToFirst {} => Limit::Next,
        LinearTermination::ToLast {} => Limit::Last,
        _ => return None,
    })
}

fn angular(value: &AngularTermination) -> Option<Limit<Angle>> {
    Some(match value {
        AngularTermination::Angle { angle } => {
            Limit::Amount(Dimension::Literal(Angle::new(angle.get())?))
        }
        AngularTermination::ThroughAll {} => Limit::ThroughAll,
        AngularTermination::ToFirst {} => Limit::Next,
        AngularTermination::ToLast {} => Limit::Last,
        _ => return None,
    })
}

fn opposite<Q>(limit: Limit<Q>, negate: impl FnOnce(Q) -> Option<Q>) -> Option<Limit<Q>> {
    Some(match limit {
        Limit::Amount(Dimension::Literal(value)) => {
            Limit::Amount(Dimension::Literal(negate(value)?))
        }
        Limit::Amount(_) => return None,
        value => value,
    })
}

/// An unread construction input for a resolved element whose defining
/// constraints the Creo decoder does not read yet (§5.9).
pub(super) fn unread_input() -> ConstructionInput {
    ConstructionInput {
        role: None,
        reference: Some(InputReference::Picks(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        })),
        value: None,
        reversed: None,
    }
}

/// A drafted side states a taper whose sign the decoder does not set against
/// §6.3's, so a drafted extent is not stated.
fn span(value: &ExtrudeExtent) -> Option<Span<Length>> {
    match value {
        ExtrudeExtent::OneSided { side } if side.draft.is_none() => Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(linear(&side.termination)?),
        }),
        ExtrudeExtent::TwoSided { first, second }
            if first.draft.is_none() && second.draft.is_none() =>
        {
            Some(Span::Between {
                start: Box::new(opposite(linear(&second.termination)?, |value| {
                    Length::new(-value.get())
                })?),
                end: Box::new(linear(&first.termination)?),
            })
        }
        ExtrudeExtent::Symmetric { side } if side.draft.is_none() => {
            match linear(&side.termination)? {
                Limit::Amount(amount) => Some(Span::Symmetric {
                    amount,
                    measure: Some(SymmetricMeasure::Total),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn revolve_span(value: &RevolveExtent) -> Option<Span<Angle>> {
    match value {
        RevolveExtent::OneSided { termination } => Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(angular(termination)?),
        }),
        RevolveExtent::TwoSided { first, second } => Some(Span::Between {
            start: Box::new(opposite(angular(second)?, |value| {
                Angle::new(-value.get())
            })?),
            end: Box::new(angular(first)?),
        }),
        RevolveExtent::Symmetric { termination } => match angular(termination)? {
            Limit::Amount(amount) => Some(Span::Symmetric {
                amount,
                measure: Some(SymmetricMeasure::Total),
            }),
            _ => None,
        },
    }
}

/// The travel's sense against the section's normal, where the travel lies
/// along it.
fn travel(
    value: &old::ExtrudeDirection,
    profile: &old::ProfileRef,
    stated: &Stated<'_>,
) -> Option<ExtrudeDirection> {
    let old::ExtrudeDirection::Explicit {
        vector,
        source: None,
    } = value
    else {
        return None;
    };
    let frame = stated.frame(selections::profile_sketch(profile, stated)?)?;
    let travel = UnitVector3::normalized_nonzero((*vector).into())?;
    let cosine = Vector3::from(travel).dot(Vector3::from(frame.z_axis));
    if (cosine.abs() - 1.0).abs() > PARALLEL {
        return None;
    }
    Some(ExtrudeDirection::ProfileNormal {
        reversed: Some(Dimension::Literal(cosine < 0.0)),
    })
}

fn unread(output: &SolidOutput) -> bool {
    output.body.is_none() && output.boolean.is_none()
}

fn unread_profile() -> cadmpeg_ir::design::selection::OrderedSelection {
    Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: None,
    }
}

pub(super) fn extrude(value: &LegacyOperation, stated: &Stated<'_>) -> Option<FeatureOperation> {
    let LegacyOperation::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    } = value
    else {
        return None;
    };
    let section = selections::profile(profile, stated);
    let span = span(extent);
    let direction = travel(direction, profile, stated);
    let origin = matches!(start, old::ExtrudeStart::ProfilePlane {})
        .then_some(ExtrudeOrigin::ProfilePlane)
        .filter(|_| span.is_some());
    let output = output(*op, *solid);
    if section.is_none() && span.is_none() && direction.is_none() && unread(&output) {
        return None;
    }
    Some(FeatureOperation::Extrude(Box::new(Extrude {
        profile: section.unwrap_or_else(unread_profile),
        loops: None,
        direction,
        origin,
        span,
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

pub(super) fn revolve(value: &LegacyOperation, stated: &Stated<'_>) -> Option<FeatureOperation> {
    let LegacyOperation::Revolve { construction, op } = value else {
        return None;
    };
    let (profile, axis, extent, solid) = match construction {
        old::RevolveConstruction::Resolved {
            profile,
            axis,
            extent,
            solid,
            face_maker: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        } => (Some(profile), Some(axis), Some(extent), *solid),
        old::RevolveConstruction::Unresolved(partial) => match partial {
            old::PartialRevolveConstruction::Profile {
                axis,
                extent,
                solid,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            } => (None, axis.as_ref(), extent.as_ref(), *solid),
            old::PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            } => (Some(profile), None, extent.as_ref(), *solid),
            old::PartialRevolveConstruction::Extent {
                profile,
                axis,
                solid,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            } => (Some(profile), Some(axis), None, *solid),
            _ => return None,
        },
        old::RevolveConstruction::Resolved { .. } => return None,
    };
    let section = profile.and_then(|profile| selections::planar_profile(profile, stated));
    let axis = axis
        .filter(|axis| axis.reference.is_none())
        .and_then(|axis| {
            Some(Construction::<kinds::Axis> {
                inputs: vec![unread_input()],
                resolved: Some(AxisValue {
                    origin: axis.origin,
                    direction: UnitVector3::normalized_nonzero(axis.direction.into())?,
                }),
                ..Construction::default()
            })
        });
    let span = extent.and_then(revolve_span);
    let output = output(*op, solid);
    if section.is_none() && axis.is_none() && span.is_none() && unread(&output) {
        return None;
    }
    Some(FeatureOperation::Revolve(Box::new(Revolve {
        profile: section.unwrap_or_else(unread_profile),
        loops: None,
        axis,
        span,
        fit: None,
        output,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}
