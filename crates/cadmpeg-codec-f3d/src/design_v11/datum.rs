// SPDX-License-Identifier: Apache-2.0
//! Coordinate frames and the source construction data that defines them.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, FrameValue, InputReference,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::features::{Feature, FeatureOperation as LegacyOperation};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

#[cfg(test)]
mod tests;

pub(super) fn project(feature: &Feature, losses: &mut Vec<LossNote>) -> Option<FeatureOperation> {
    let operation = feature.evaluation.definition().operation();
    let LegacyOperation::DatumCoordinateSystem { frame } = operation else {
        if matches!(operation, LegacyOperation::Native { kind, .. }
            if matches!(kind.as_str(), "JointOrigin" | "Coordinate System"))
        {
            losses.push(F3dLossCode::CoordinateSystemFrameUnresolved.note(format!(
                "feature {} retains its coordinate frame and construction in the native scope",
                feature.id
            )));
        }
        return None;
    };
    let native_options = feature
        .source_properties
        .iter()
        .filter_map(|(key, value)| {
            Some((
                NonBlankString::new(format!("f3d:{key}"))?,
                NativeOption::Text(Dimension::Literal(NonBlankString::new(value.clone())?)),
            ))
        })
        .collect::<NativeOptions>();
    losses.push(if native_options.is_empty() {
        F3dLossCode::CoordinateSystemConstructionDropped.note(format!(
            "feature {} transfers its solved coordinate frame without its construction references and parameter expressions",
            feature.id
        ))
    } else {
        F3dLossCode::CoordinateSystemConstructionNative.note(format!(
            "feature {} transfers its solved coordinate frame; construction references and parameter expressions retain their source meanings",
            feature.id
        ))
    });
    Some(FeatureOperation::Datum(Box::new(Datum {
        element: DatumElement::Frame(Construction {
            inputs: vec![ConstructionInput {
                role: None,
                reference: Some(InputReference::Picks(Selection {
                    at: Anchor::Input,
                    picks: Picks::Unread,
                    native: None,
                })),
                value: None,
                reversed: None,
            }],
            resolved: Some(FrameValue {
                origin: frame.origin(),
                x_axis: frame.x_axis(),
                y_axis: frame.y_axis(),
                z_axis: frame.z_axis(),
            }),
            ..Default::default()
        }),
        link: None,
        native_options,
        source_settings: BTreeMap::default(),
    })))
}
