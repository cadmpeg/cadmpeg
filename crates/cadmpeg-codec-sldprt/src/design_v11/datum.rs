// SPDX-License-Identifier: Apache-2.0
//! Typed user datums: a reference plane, axis, point or coordinate system
//! whose decoded record states its geometry is a datum whose construction
//! resolves to that geometry.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{
    kinds, AxisValue, Construction, ConstructionKind, FrameValue, PlaneValue,
};
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::geometry::{FeatureDatumPlaneFrame, FeatureDirection3};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::units::UnitVector3;

use super::swept::{self, Context};

/// Type each unresolved datum of `features` whose decoded record states its
/// plane, axis, point or frame, and return the id and name of each one whose
/// record states none.
pub(super) fn project(
    features: &mut [Feature],
    context: &Context<'_>,
) -> Vec<(FeatureId, Option<String>)> {
    let mut unresolved = Vec::new();
    swept::walk(features, OperationFamily::Datum, |feature, _| {
        let element = context
            .model
            .features
            .iter()
            .find(|candidate| candidate.id == feature.id)
            .and_then(|source| element(source.evaluation.definition()));
        match element {
            Some(element) => {
                feature.definition.operation = FeatureOperation::Datum(Box::new(Datum {
                    element,
                    link: None,
                    native_options: NativeOptions::new(),
                    source_settings: NativeOptions::new(),
                }));
            }
            None => unresolved.push((feature.id.clone(), feature.name.clone())),
        }
    });
    unresolved
}

/// The datum element of a decoded definition that states its geometry.
fn element(definition: &FeatureDefinition) -> Option<DatumElement> {
    let FeatureDefinition::Operation(operation) = definition else {
        return None;
    };
    Some(match operation {
        v10::FeatureOperation::DatumPlane { frame }
        | v10::FeatureOperation::DatumThreePointPlane { frame, .. } => {
            DatumElement::Plane(resolved(plane(*frame)?))
        }
        v10::FeatureOperation::DatumAxis { axis } => {
            let (origin, direction) = axis.geometry()?;
            DatumElement::Axis(resolved(AxisValue {
                origin: *origin,
                direction: unit(*direction)?,
            }))
        }
        v10::FeatureOperation::DatumPoint { position, .. } => {
            DatumElement::Points(vec![resolved::<kinds::Point>(*position)])
        }
        v10::FeatureOperation::DatumCoordinateSystem { frame } => {
            DatumElement::Frame(resolved(FrameValue {
                origin: frame.origin(),
                x_axis: frame.x_axis(),
                y_axis: frame.y_axis(),
                z_axis: frame.z_axis(),
            }))
        }
        _ => return None,
    })
}

/// The plane of `frame`: its origin, its unit normal and its unit u axis.
fn plane(frame: FeatureDatumPlaneFrame) -> Option<PlaneValue> {
    Some(PlaneValue {
        origin: frame.origin(),
        normal: unit(frame.normal())?,
        x_axis: unit(frame.u_axis())?,
    })
}

fn unit(direction: FeatureDirection3) -> Option<UnitVector3> {
    UnitVector3::normalized_nonzero(direction.into())
}

/// A construction with no inputs that resolves to `value`.
fn resolved<K: ConstructionKind>(value: K::Value) -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: Some(value),
        resolved_in: BTreeMap::new(),
    }
}
