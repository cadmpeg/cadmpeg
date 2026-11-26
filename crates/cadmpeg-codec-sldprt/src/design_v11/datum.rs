// SPDX-License-Identifier: Apache-2.0
//! Typed user datums: a reference plane, axis, point or coordinate system
//! whose decoded record states its geometry is a datum whose construction
//! resolves to that geometry, and an offset plane is a datum offset from its
//! reference plane by its distance.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, ConstructionKind,
    FrameValue, InputReference, InputRole, PlaneValue, Relation, ScalarDimension, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::datums::DatumPlaneReference;
use cadmpeg_ir::features::geometry::{FeatureDatumPlaneFrame, FeatureDirection3};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::units::UnitVector3;

use super::swept::{self, Context};
use crate::history::literals::parse_angle_rad;
use crate::records::{CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE, REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY};

/// A datum's id and its name where it has one.
pub(super) type Named = (FeatureId, Option<String>);

/// The datums [`project`] leaves unresolved, and the typed datums that state
/// an assumed value.
#[derive(Default)]
pub(super) struct Projected {
    /// The datums whose decoded definition has no typed form.
    pub(super) unresolved: Vec<Named>,
    /// The typed planes whose origin and x axis the decoder constructs.
    pub(super) constructed_axis: Vec<Named>,
    /// The typed planes with an angular `D1` whose x axis is their angular
    /// frame's u-axis.
    pub(super) angled_axis: Vec<Named>,
    /// The typed datums whose geometry decodes differently in a
    /// configuration.
    pub(super) configured: Vec<Named>,
}

/// Type each unresolved datum of `features` whose decoded definition states
/// its geometry or its offset from a reference plane.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) -> Projected {
    let mut projected = Projected::default();
    swept::walk(features, OperationFamily::Datum, |feature, _| {
        let named = (feature.id.clone(), feature.name.clone());
        let Some(source) = context
            .model
            .features
            .iter()
            .find(|candidate| candidate.id == feature.id)
        else {
            projected.unresolved.push(named);
            return;
        };
        let definition = source.evaluation.definition();
        let Some(element) = element(definition) else {
            projected.unresolved.push(named);
            return;
        };
        if let FeatureDefinition::Operation(v10::FeatureOperation::DatumPlane { .. }) = definition {
            if source
                .source_properties
                .get(REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY)
                .map(String::as_str)
                == Some(CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE)
            {
                projected.constructed_axis.push(named.clone());
            } else if angular_d1(source, context) {
                projected.angled_axis.push(named.clone());
            }
        }
        if context.model.configurations.iter().any(|configuration| {
            configuration
                .feature_states
                .get(&feature.id)
                .and_then(|state| self::element(&state.definition))
                .is_some_and(|other| other != element)
        }) {
            projected.configured.push(named);
        }
        feature.definition.operation = FeatureOperation::Datum(Box::new(Datum {
            element,
            link: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        }));
    });
    projected
}

/// Whether the history record of `source` states its `D1` as an angle.
fn angular_d1(source: &v10::Feature, context: &Context<'_>) -> bool {
    let Some(native) = source.native_ref.as_deref() else {
        return false;
    };
    context
        .histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|record| record.id == native)
        .and_then(|record| record.parameters.get("D1"))
        .is_some_and(|text| parse_angle_rad(text).is_some())
}

/// The datum element of a decoded definition that states its geometry or its
/// offset from a reference plane.
fn element(definition: &FeatureDefinition) -> Option<DatumElement> {
    let FeatureDefinition::Operation(operation) = definition else {
        return None;
    };
    Some(match operation {
        v10::FeatureOperation::DatumPlane { frame }
        | v10::FeatureOperation::DatumThreePointPlane { frame, .. } => {
            DatumElement::Plane(resolved(plane(*frame)?))
        }
        v10::FeatureOperation::DatumOffsetPlane {
            reference: Some(reference),
            distance,
        } => DatumElement::Plane(Construction {
            inputs: vec![ConstructionInput {
                role: Some(InputRole {
                    subject: Subject::Whole,
                    relation: Relation::Offset,
                }),
                reference: Some(plane_reference(reference)?),
                value: Some(ScalarDimension::Length(Dimension::Literal(*distance))),
                reversed: None,
            }],
            ..resolved_none()
        }),
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

/// The input reference of an offset plane's reference: the datum feature it
/// names, or the plane the decoder resolved for it.
fn plane_reference(reference: &DatumPlaneReference) -> Option<InputReference> {
    Some(match reference {
        DatumPlaneReference::Feature { feature } => {
            let pick = Picked {
                target: PickTarget::Datum {
                    feature: feature.clone(),
                    part: None,
                },
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            };
            InputReference::Picks(Selection {
                at: Anchor::Input,
                picks: Picks::Named(Members::new(vec![pick]).ok()?),
                native: None,
            })
        }
        DatumPlaneReference::ResolvedPlane { frame } => {
            InputReference::Built(Box::new(AnyConstruction::Plane(resolved(PlaneValue {
                origin: frame.origin(),
                normal: unit(frame.normal())?,
                x_axis: unit(frame.u_axis())?,
            }))))
        }
        DatumPlaneReference::Face { .. } => return None,
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
        resolved: Some(value),
        ..resolved_none()
    }
}

/// A construction with no inputs and no resolved value.
fn resolved_none<K: ConstructionKind>() -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    }
}
