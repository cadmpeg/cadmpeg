// SPDX-License-Identifier: Apache-2.0
//! Datum planes and coordinate systems by their resolved frames.
//!
//! The decoder reads a datum's placed frame and not its constraints, so the
//! construction states the frame alone (interim: gap 20261119-CR2-1).

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{kinds, Construction, FrameValue, PlaneValue};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::units::UnitVector3;

pub(super) fn datum(value: &LegacyOperation) -> Option<FeatureOperation> {
    let element = match value {
        LegacyOperation::DatumPlane { frame } => {
            DatumElement::Plane(Construction::<kinds::Plane> {
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
