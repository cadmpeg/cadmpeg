// SPDX-License-Identifier: Apache-2.0
//! The design features of CSG items, filed under the scope of the product
//! definition whose shape representation lists them.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    kinds, AxisValue, Construction, FrameAxis, FrameValue, PostStep,
};
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, StoredOrigin};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::{CodecId, NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::body::Combine;
use cadmpeg_ir::design::ops::shared::{Limit, RadialDimension, Span, ToolDisposition};
use cadmpeg_ir::design::ops::swept::{
    Extrude, ExtrudeDirection, ExtrudeOrigin, Primitive, PrimitiveShape, Revolve,
};
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Length, NonNegativeLength, PositiveLength};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;

use super::frame::Frame;
use super::{CsgData, Item, Operator, Shape};
use crate::ids::{self, kind};
use crate::loss::StepLossCode;
use crate::parse::Exchange;

/// File the features of `csg` under design scopes. `definitions_by_shape`
/// maps each `PRODUCT_DEFINITION_SHAPE` to its product definition.
pub(in crate::reader) fn attach(
    csg: &CsgData,
    exchange: &Exchange,
    definitions_by_shape: &BTreeMap<u64, ProductDefinitionId>,
    ir: &mut CadIr,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut scoped = BTreeMap::<u64, (ProductDefinitionId, Vec<u64>)>::new();
    let mut placed = BTreeSet::new();
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let reference = |index: usize| {
            super::super::named_parameter(record, "SHAPE_DEFINITION_REPRESENTATION", index)
                .and_then(super::super::ValueExt::reference)
        };
        let (Some(shape), Some(representation)) = (reference(0), reference(1)) else {
            continue;
        };
        let (Some(definition), Some(members)) = (
            definitions_by_shape.get(&shape),
            csg.representations.get(&representation),
        ) else {
            continue;
        };
        let Some(definition_step) = super::super::step_instance_id(definition.as_str()) else {
            continue;
        };
        let entry = scoped
            .entry(definition_step)
            .or_insert_with(|| (definition.clone(), Vec::new()));
        for &member in members {
            if placed.insert(member) {
                entry.1.push(member);
            }
        }
    }
    let unscoped = csg
        .items
        .keys()
        .filter(|item| !placed.contains(item))
        .count();
    if unscoped > 0 {
        losses.push(StepLossCode::CsgDesignUnscoped.note(format!(
            "{unscoped} CSG item(s) belong to no product definition's shape representation; \
             their design features were not filed"
        )));
    }
    for (definition_step, (definition, members)) in scoped {
        if members.is_empty() {
            continue;
        }
        let container = ContainerId::from(ids::design(kind!("container"), definition_step));
        let mut container_members = BTreeSet::new();
        let mut current = Vec::new();
        let mut csg_bodies = BTreeSet::new();
        for &member in &members {
            let item = &csg.items[&member];
            let feature = feature_id(member);
            ir.design.features.push(Feature {
                id: feature.clone(),
                name: item.name.clone(),
                suppressed: None,
                definition: FeatureDefinition {
                    operation: operation(item, csg),
                    refine: None,
                    fuzzy_tolerance: None,
                },
                placement: swept_placement(item),
                component: None,
                driven_by: None,
                native_ref: Some(format!("#{member}")),
            });
            container_members.insert(Member::Feature(feature.clone()));
            if let Some(body) = csg.bodies.get(&member) {
                csg_bodies.insert(body.clone());
                current.push(state(Member::Feature(feature), vec![body.clone()]));
            }
        }
        let others: Vec<_> = ir
            .model
            .product_definitions
            .iter()
            .find(|candidate| candidate.id == definition)
            .map(|candidate| {
                candidate
                    .bodies
                    .iter()
                    .filter(|body| !csg_bodies.contains(*body))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if !others.is_empty() {
            let stored = FeatureId::from(ids::design(
                kind!("feature"),
                crate::ids::key_word!("stored").dash(definition_step),
            ));
            ir.design.features.push(Feature {
                id: stored.clone(),
                name: None,
                suppressed: None,
                definition: FeatureDefinition {
                    operation: FeatureOperation::Stored {
                        bodies: Some(others.clone()),
                        tessellations: Vec::new(),
                        origin: StoredOrigin::Base {},
                    },
                    refine: None,
                    fuzzy_tolerance: None,
                },
                placement: None,
                component: None,
                driven_by: None,
                native_ref: None,
            });
            container_members.insert(Member::Feature(stored.clone()));
            current.push(state(Member::Feature(stored), others));
        }
        ir.design.containers.push(Container::Unordered {
            id: container.clone(),
            kind: UnorderedKind::Group,
            members: container_members,
        });
        ir.design.scopes.push(DesignScope {
            definition,
            roots: vec![container],
            active_configuration: None,
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current,
        });
    }
    losses
}

/// The plant item orientation of a swept-area solid, which places the
/// result of its sweep. A primitive states the orientation in its own frame.
fn swept_placement(item: &Item) -> Option<Box<Construction<kinds::Frame>>> {
    match (&item.shape, &item.orientation) {
        (Shape::Extruded { .. } | Shape::Revolved { .. }, Some(frame)) => {
            fixed_frame(frame, None).and_then(stated).map(Box::new)
        }
        _ => None,
    }
}

fn feature_id(item: u64) -> FeatureId {
    FeatureId::from(ids::design(kind!("feature"), item))
}

fn state(at: Member, bodies: Vec<cadmpeg_ir::ids::BodyId>) -> CurrentState {
    CurrentState {
        at: Position::After(at),
        bodies,
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    }
}

fn literal<T>(value: Option<T>) -> Option<Dimension<T>> {
    value.map(Dimension::Literal)
}

fn new_solid() -> SolidOutput {
    SolidOutput {
        body: Some(BodyForm::Solid),
        boolean: Some(Boolean::NewBody),
    }
}

/// The operation an item states, or a native operation where a stated
/// value falls outside the IR's admitted range.
fn operation(item: &Item, csg: &CsgData) -> FeatureOperation {
    typed_operation(item, csg).unwrap_or_else(|| FeatureOperation::Native {
        kind: native_kind(item.entity),
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}

fn native_kind(entity: &str) -> NonBlankString {
    let text = format!("step:{}", entity.to_ascii_lowercase());
    NonBlankString::new(&text).unwrap_or_else(|| cadmpeg_core::nonblank_literal!("step:csg"))
}

fn typed_operation(item: &Item, csg: &CsgData) -> Option<FeatureOperation> {
    let positive = |value: f64| PositiveLength::new(value);
    let primitive = |shape: PrimitiveShape, placement: Construction<kinds::Frame>| {
        FeatureOperation::Primitive(Box::new(Primitive {
            shape,
            placement: stated(placement),
            output: new_solid(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        }))
    };
    let placed = match &item.shape {
        Shape::Extruded { .. } | Shape::Revolved { .. } | Shape::Boolean { .. } => {
            item.shape.clone()
        }
        _ => item.placed_shape(),
    };
    Some(match &placed {
        Shape::Block { frame, size } => primitive(
            PrimitiveShape::Box {
                size: [
                    literal(positive(size[0]))?,
                    literal(positive(size[1]))?,
                    literal(positive(size[2]))?,
                ],
            },
            fixed_frame(frame, None)?,
        ),
        Shape::Wedge { frame, size, ltx } => {
            let length = |value: f64| literal(Length::new(value));
            primitive(
                PrimitiveShape::Wedge {
                    y: [length(0.0)?, length(size[1])?],
                    base: [
                        length(0.0)?,
                        length(size[0])?,
                        length(0.0)?,
                        length(size[2])?,
                    ],
                    top: [length(0.0)?, length(*ltx)?, length(0.0)?, length(size[2])?],
                },
                fixed_frame(frame, None)?,
            )
        }
        Shape::Cylinder {
            frame,
            radius,
            height,
        } => primitive(
            PrimitiveShape::Cylinder {
                radius: RadialDimension::Radius(literal(positive(*radius))?),
                height: literal(positive(*height))?,
                angle: None,
            },
            fixed_frame(frame, None)?,
        ),
        Shape::Cone {
            radii,
            height,
            source,
            ..
        } => primitive(
            PrimitiveShape::Cone {
                radii: [
                    RadialDimension::Radius(literal(NonNegativeLength::new(radii[0]))?),
                    RadialDimension::Radius(literal(NonNegativeLength::new(radii[1]))?),
                ],
                height: literal(positive(*height))?,
                angle: None,
            },
            fixed_frame(source, Some(-*height))?,
        ),
        Shape::Sphere { frame, radius } => primitive(
            PrimitiveShape::Sphere {
                radius: RadialDimension::Radius(literal(positive(*radius))?),
                latitudes: None,
                longitude: None,
            },
            fixed_frame(frame, None)?,
        ),
        Shape::Torus {
            frame,
            major,
            minor,
        } => primitive(
            PrimitiveShape::Torus {
                radii: [
                    RadialDimension::Radius(literal(positive(*major))?),
                    RadialDimension::Radius(literal(positive(*minor))?),
                ],
                latitudes: None,
                longitude: None,
            },
            fixed_frame(frame, None)?,
        ),
        Shape::Ellipsoid { frame, semi_axes } => primitive(
            PrimitiveShape::Ellipsoid {
                radii: [
                    literal(positive(semi_axes[0]))?,
                    literal(positive(semi_axes[1]))?,
                    literal(positive(semi_axes[2]))?,
                ],
                latitudes: None,
                longitude: None,
            },
            fixed_frame(frame, None)?,
        ),
        Shape::Extruded {
            area,
            direction,
            depth,
            ..
        } => FeatureOperation::Extrude(Box::new(Extrude {
            profile: profile_pick(*area),
            loops: None,
            direction: Some(ExtrudeDirection::Along {
                direction: Box::new(fixed_direction(*direction)?),
                lengths_along_normal: Some(Dimension::Literal(false)),
            }),
            origin: Some(ExtrudeOrigin::ProfilePlane),
            span: Some(Span::Between {
                start: Box::new(Limit::AtOrigin),
                end: Box::new(Limit::Amount(literal(Length::new(*depth))?)),
            }),
            draft: None,
            fit: None,
            outside: None,
            sheet_cut: None,
            output: new_solid(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
        Shape::Revolved {
            area, axis, angle, ..
        } => FeatureOperation::Revolve(Box::new(Revolve {
            profile: profile_pick(*area),
            loops: None,
            axis: Some(fixed_axis(axis)?),
            span: Some(Span::Between {
                start: Box::new(Limit::AtOrigin),
                end: Box::new(Limit::Amount(literal(Angle::new(*angle))?)),
            }),
            fit: None,
            output: new_solid(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
        Shape::Boolean { operator, operands } => FeatureOperation::Combine(Box::new(Combine {
            target: Some(operand_pick(operands[0], csg)),
            tools: operand_pick(operands[1], csg),
            op: Some(match operator {
                Operator::Union => BooleanOp::Join,
                Operator::Intersection => BooleanOp::Intersect,
                Operator::Difference => BooleanOp::Cut,
            }),
            tools_after: Some(ToolDisposition::Consumed),
            new_component: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
    })
}

/// The selection of one pick.
fn single<O: MemberOrder>(at: Anchor, pick: Picked) -> Selection<O> {
    let picks = Members::new(vec![pick]).map_or(Picks::Unread, Picks::Named);
    Selection {
        at,
        picks,
        native: None,
    }
}

fn step_name(id: u64) -> NativeName {
    NativeName {
        codec: CodecId::from(cadmpeg_core::nonblank_literal!("step")),
        value: NativeNameValue::Text(cadmpeg_core::nonblank_literal!("#{id}")),
    }
}

/// The swept area, picked by its `CURVE_BOUNDED_SURFACE` record. No body
/// of the current geometry reaches the surface decoded from it, so the pick
/// states no resolved entity.
fn profile_pick<O: MemberOrder>(area: u64) -> Selection<O> {
    single(
        Anchor::Current,
        Picked {
            target: PickTarget::Native { within: None },
            context: None,
            native: Some(step_name(area)),
            domain: None,
            witness: None,
            resolved: None,
        },
    )
}

/// A Boolean operand: the result of its feature, or the source record where
/// the operand is not a CSG item.
fn operand_pick<O: MemberOrder>(operand: u64, csg: &CsgData) -> Selection<O> {
    if csg.items.contains_key(&operand) {
        let feature = feature_id(operand);
        return single(
            Anchor::Pinned(Position::After(Member::Feature(feature.clone()))),
            Picked {
                target: PickTarget::FeatureResult(feature),
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            },
        );
    }
    single(
        Anchor::Current,
        Picked {
            target: PickTarget::Native { within: None },
            context: None,
            native: Some(step_name(operand)),
            domain: None,
            witness: None,
            resolved: None,
        },
    )
}

fn unit(vector: Vector3) -> Option<UnitVector3> {
    UnitVector3::new(vector)
}

fn point(frame: &Frame) -> Option<FinitePoint3> {
    FinitePoint3::new(frame.origin)
}

fn transform(frame: &Frame) -> Option<Transform> {
    let o = frame.origin;
    let (x, y, z) = (frame.x, frame.y, frame.z);
    Transform::affine([
        [x.x, y.x, z.x, o.x],
        [x.y, y.y, z.y, o.y],
        [x.z, y.z, z.z, o.z],
    ])
}

/// The source's placement, then a move by `shift` along its z axis where
/// the CADIR frame lies there.
fn fixed_frame(frame: &Frame, shift: Option<f64>) -> Option<Construction<kinds::Frame>> {
    let mut after = vec![PostStep::Fixed(transform(frame)?)];
    let mut resolved = *frame;
    if let Some(distance) = shift {
        after.push(PostStep::Translate {
            axis: FrameAxis::Z,
            distance: Dimension::Literal(Length::new(distance)?),
        });
        resolved = frame.shifted(distance);
    }
    Some(Construction {
        inputs: Vec::new(),
        after,
        solution: None,
        mode: None,
        resolved: Some(FrameValue {
            origin: point(&resolved)?,
            x_axis: unit(resolved.x)?,
            y_axis: unit(resolved.y)?,
            z_axis: unit(resolved.z)?,
        }),
        resolved_in: BTreeMap::new(),
    })
}

/// The placement, or `None` where it resolves to the identity, which a
/// placement with no binding does not state.
fn stated(placement: Construction<kinds::Frame>) -> Option<Construction<kinds::Frame>> {
    let identity = placement.resolved.as_ref().is_some_and(|frame| {
        let origin = frame.origin.get();
        origin.x == 0.0
            && origin.y == 0.0
            && origin.z == 0.0
            && frame.x_axis == UnitVector3::X_AXIS
            && frame.y_axis == UnitVector3::Y_AXIS
            && frame.z_axis == UnitVector3::Z_AXIS
    });
    (!identity).then_some(placement)
}

fn fixed_axis(frame: &Frame) -> Option<Construction<kinds::Axis>> {
    Some(Construction {
        inputs: Vec::new(),
        after: vec![PostStep::Fixed(transform(frame)?)],
        solution: None,
        mode: None,
        resolved: Some(AxisValue {
            origin: point(frame)?,
            direction: unit(frame.z)?,
        }),
        resolved_in: BTreeMap::new(),
    })
}

fn fixed_direction(direction: Vector3) -> Option<Construction<kinds::Direction>> {
    let frame = Frame::about(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), direction)?;
    Some(Construction {
        inputs: Vec::new(),
        after: vec![PostStep::Fixed(transform(&frame)?)],
        solution: None,
        mode: None,
        resolved: Some(unit(frame.z)?),
        resolved_in: BTreeMap::new(),
    })
}
