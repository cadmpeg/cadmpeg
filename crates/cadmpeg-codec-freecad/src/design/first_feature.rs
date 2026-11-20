// SPDX-License-Identifier: Apache-2.0
//! Boolean operation of the first solid feature of a `PartDesign` body.

use super::{
    body_membership_property, is_body, is_boolean, is_dress_up, is_pattern, is_primitive,
    is_stored_geometry_feature, native_definition, nonempty_link, property,
};
use crate::loss::FreecadLossCode;
use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BooleanOp, FeatureDefinition, FeatureOperation, SolidSweepOperation, SweepShape,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::HashMap;

fn is_solid_feature(kind: &str) -> bool {
    let Some(name) = kind.strip_prefix("PartDesign::") else {
        return false;
    };
    is_primitive(kind)
        || is_stored_geometry_feature(kind)
        || is_boolean(kind)
        || is_dress_up(kind)
        || is_pattern(kind)
        || name == "FeatureBase"
        || name.starts_with("Additive")
        || name.starts_with("Subtractive")
        || matches!(
            name,
            "Pad" | "Pocket" | "Revolution" | "Groove" | "Loft" | "Pipe" | "Hole"
        )
}

fn link_is_empty(properties: &[&PropertyRecord], name: &str) -> bool {
    property(properties, name).is_none_or(|property| {
        property.type_name == "App::PropertyLink"
            && property
                .links()
                .iter()
                .all(|link| !nonempty_link(link.as_ref()))
    })
}

fn boolean_of(operation: &FeatureOperation) -> Option<BooleanOp> {
    match operation {
        FeatureOperation::Extrude { op, .. }
        | FeatureOperation::Revolve { op, .. }
        | FeatureOperation::Primitive { op, .. }
        | FeatureOperation::Loft { op, .. }
        | FeatureOperation::HelicalSweep { op, .. } => Some(*op),
        FeatureOperation::Sweep {
            shape: SweepShape::Solid { op, .. },
            ..
        } => Some(BooleanOp::from(*op)),
        FeatureOperation::Hole { .. } => Some(BooleanOp::Cut),
        _ => None,
    }
}

fn set_additive_operation(operation: &mut FeatureOperation, starts_body: bool) {
    let additive_op = if starts_body {
        BooleanOp::NewBody
    } else {
        BooleanOp::Join
    };
    match operation {
        FeatureOperation::Extrude { op, .. }
        | FeatureOperation::Revolve { op, .. }
        | FeatureOperation::Primitive { op, .. }
        | FeatureOperation::Loft { op, .. }
        | FeatureOperation::HelicalSweep { op, .. } => *op = additive_op,
        FeatureOperation::Sweep {
            shape: SweepShape::Solid { op, .. },
            ..
        } => {
            *op = if starts_body {
                SolidSweepOperation::NewBody
            } else {
                SolidSweepOperation::Join
            };
        }
        _ => {}
    }
}

pub(super) fn bind_first_features(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let kinds = objects
        .iter()
        .map(|object| (object.id().as_str(), object.type_name.as_str()))
        .collect::<HashMap<_, _>>();
    let mut starts_body = HashMap::new();
    for body in objects.iter().filter(|object| is_body(&object.type_name)) {
        let owned = properties_by_owner
            .get(body.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let Some(group) = body_membership_property(owned) else {
            continue;
        };
        for (index, member) in group
            .links()
            .iter()
            .filter_map(|link| link.as_ref()?.object())
            .filter(|member| kinds.get(member).is_some_and(|kind| is_solid_feature(kind)))
            .enumerate()
        {
            let own = properties_by_owner
                .get(member)
                .map_or(&[][..], Vec::as_slice);
            starts_body.insert(
                member,
                index == 0
                    && link_is_empty(owned, "BaseFeature")
                    && link_is_empty(own, "BaseFeature"),
            );
        }
    }
    for feature in &mut ir.model.features {
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(&starts_body) = starts_body.get(native) else {
            continue;
        };
        let mut removal = false;
        feature.evaluation.edit(|definition, _| {
            let (FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. }) = definition;
            match boolean_of(operation) {
                Some(BooleanOp::Join | BooleanOp::NewBody) => {
                    set_additive_operation(operation, starts_body);
                }
                Some(BooleanOp::Cut | BooleanOp::Intersect) => removal = starts_body,
                _ => {}
            }
        });
        if removal {
            let kind = kinds.get(native).copied().unwrap_or_default();
            let owned = properties_by_owner
                .get(native)
                .map_or(&[][..], Vec::as_slice);
            feature
                .evaluation
                .set_definition(native_definition(ctx, kind, owned)?);
            losses.push(
                FreecadLossCode::FeatureRemovalWithoutBase
                    .note(format!("{native} starts a removal from an empty body")),
            );
        }
    }
    Ok(())
}
