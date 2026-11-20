// SPDX-License-Identifier: Apache-2.0
//! Datum planes and coordinate systems by their resolved frames.
//!
//! The decoder reads a datum's placed frame while its defining constraints
//! remain unread, so each construction carries an unread input beside its
//! resolved value (§5.9).

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{Construction, FrameValue, PlaneValue, kinds};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::units::UnitVector3;

use super::swept::unread_input;

pub(super) fn datum(value: &LegacyOperation) -> Option<FeatureOperation> {
    let element = match value {
        LegacyOperation::DatumPlane { frame } => {
            DatumElement::Plane(Construction::<kinds::Plane> {
                inputs: vec![unread_input()],
                resolved: Some(PlaneValue {
                    origin: frame.origin(),
                    normal: UnitVector3::normalized_nonzero(frame.normal().into())?,
                    x_axis: UnitVector3::normalized_nonzero(frame.u_axis().into())?,
                }),
                ..Construction::default()
            })
        }
        LegacyOperation::DatumCoordinateSystem { frame } => {
            DatumElement::Frame(Construction::<kinds::Frame> {
                inputs: vec![unread_input()],
                resolved: Some(FrameValue {
                    origin: frame.origin(),
                    x_axis: frame.x_axis(),
                    y_axis: frame.y_axis(),
                    z_axis: frame.z_axis(),
                }),
                ..Construction::default()
            })
        }
        _ => return None,
    };
    Some(FeatureOperation::Datum(Box::new(Datum {
        element,
        link: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}
