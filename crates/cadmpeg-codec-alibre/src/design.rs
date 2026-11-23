// SPDX-License-Identifier: Apache-2.0
//! Projection of the Alibre feature history into the design layer: one scope,
//! one ordered timeline, one feature for each state, sketches, and parameters.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    kinds, AxisValue, Construction, ConstructionInput, FrameValue, InputReference, InputRole,
    PlaneValue, Relation, ScalarDimension, Subject,
};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::expr::{Expression, Neutral, Node};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::ops::dressup::{
    BlendControls, Bore, Fillet, FilletSection, FilletSet, FilletSize, Hole, HolePositions,
    HoleShape,
};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityRole, Operand, Planar, PlanarCurve, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::ops::swept::{
    Extrude, ExtrudeDirection, ExtrudeOrigin, Loft, LoftShape, Revolve, Section, SectionForm,
};
use cadmpeg_ir::design::options::SolidOutput;
use cadmpeg_ir::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, PositiveLength};
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};

use crate::history::{
    Constraint, Curve, End, Feature as HistoryFeature, Frame, History, Operation, Parameter,
    ParameterKind, PointRole, Sketch as HistorySketch, SketchAxis, Support, Vec3,
};
use crate::loss::AlibreLossCode;

fn invalid(what: &str) -> CodecError {
    CodecError::malformed(format_args!("STEP_DATA feature model: {what}"))
}

fn mint<I: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    kind: &str,
    record: impl std::fmt::Display,
) -> Result<I, CodecError> {
    I::try_from(format!("alibre:step:{kind}#{record}"))
        .map_err(|_| invalid("a generated identity is invalid"))
}

fn point3(value: Vec3) -> Result<FinitePoint3, CodecError> {
    FinitePoint3::new(Point3 {
        x: value[0],
        y: value[1],
        z: value[2],
    })
    .ok_or_else(|| invalid("a coordinate is not finite"))
}

fn unit3(value: Vec3) -> Result<UnitVector3, CodecError> {
    UnitVector3::normalized(Vector3::new(value[0], value[1], value[2]))
        .ok_or_else(|| invalid("a direction has no length"))
}

fn point2(value: [f64; 2]) -> Result<FinitePoint2, CodecError> {
    FinitePoint2::new(Point2 {
        u: value[0],
        v: value[1],
    })
    .ok_or_else(|| invalid("a sketch coordinate is not finite"))
}

fn frame_value(frame: &Frame) -> Result<FrameValue, CodecError> {
    Ok(FrameValue {
        origin: point3(frame.origin)?,
        x_axis: unit3(frame.x)?,
        y_axis: unit3(frame.y)?,
        z_axis: unit3(frame.z)?,
    })
}

fn picked(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn named<O: MemberOrder>(target: PickTarget) -> Result<Selection<O>, CodecError> {
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![picked(target)])
                .map_err(|_| invalid("a selection holds a repeated member"))?,
        ),
        native: None,
    })
}

fn unread<O: MemberOrder>(native: Option<String>) -> Result<Selection<O>, CodecError> {
    let native = native
        .map(|text| {
            vec![text]
                .try_into()
                .map_err(|_| invalid("a native selection text is not admitted"))
        })
        .transpose()?;
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native,
    })
}

fn signature_text(signature: &str) -> String {
    format!("alibre:topology-signature:{signature}")
}

type SketchRecords = (Vec<SketchEntity<Planar>>, Vec<SketchConstraint<Planar>>);

struct Projection<'a> {
    history: &'a History,
    features: Vec<FeatureId>,
    parameters: Vec<ParameterId>,
    sketches: BTreeMap<usize, SketchId>,
    scope: ProductDefinitionId,
}

impl Projection<'_> {
    fn dimension<T>(&self, parameter: usize) -> Dimension<T> {
        Dimension::Parameter(self.parameters[parameter].clone())
    }

    fn sketch_pick<O: MemberOrder>(&self, feature: usize) -> Result<Selection<O>, CodecError> {
        let sketch = self
            .sketches
            .get(&feature)
            .ok_or_else(|| invalid("an operation names a state that holds no sketch"))?;
        named(PickTarget::Sketch(sketch.clone()))
    }

    fn datum_pick(
        &self,
        feature: usize,
    ) -> Result<Selection<cadmpeg_ir::design::selection::Ordered>, CodecError> {
        named(PickTarget::Datum {
            feature: self.features[feature].clone(),
            part: None,
        })
    }

    fn input(
        relation: Relation,
        reference: Selection<cadmpeg_ir::design::selection::Ordered>,
        value: Option<ScalarDimension>,
    ) -> ConstructionInput {
        ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation,
            }),
            reference: Some(InputReference::Picks(reference)),
            value,
            reversed: None,
        }
    }

    fn sketch(
        &self,
        sketch: &HistorySketch,
        id: &SketchId,
        owner: &FeatureId,
    ) -> Result<Sketch, CodecError> {
        let reference = match &sketch.support {
            Support::Plane(feature) => self.datum_pick(*feature)?,
            Support::Face(signature) => unread(Some(signature_text(signature)))?,
        };
        Ok(Sketch {
            id: id.clone(),
            owner: owner.clone(),
            name: Some(sketch.name.clone()),
            plane: SketchPlane::Placed(Box::new(Construction::<kinds::Frame> {
                inputs: vec![Self::input(Relation::Coincident, reference, None)],
                resolved: Some(frame_value(&sketch.frame)?),
                ..Construction::default()
            })),
            solve: SketchSolve {
                state: None,
                status: EvaluationStatus::Current,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: None,
        })
    }

    fn entities(sketch: &HistorySketch, id: &SketchId) -> Result<SketchRecords, CodecError> {
        let entity = |record: u32, definition, roles| -> Result<SketchEntity<Planar>, CodecError> {
            Ok(SketchEntity {
                id: mint::<SketchEntityId>("sketch-entity", record)?,
                sketch: id.clone(),
                definition,
                roles,
                construction: None,
                centerline: None,
                native: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: None,
            })
        };
        let mut entities = Vec::new();
        for primitive in &sketch.primitives {
            let curve = match primitive.curve {
                Curve::Point(at) => PlanarCurve::Point(point2(at)?),
                Curve::Line { start, end } => PlanarCurve::Line {
                    start: point2(start)?,
                    end: point2(end)?,
                },
                Curve::Circle { center, radius } => PlanarCurve::Circle {
                    center: point2(center)?,
                    radius: PositiveLength::new(radius)
                        .ok_or_else(|| invalid("a circle radius is not positive"))?,
                },
            };
            entities.push(entity(
                primitive.record,
                EntityDefinition::Free(curve),
                Vec::new(),
            )?);
        }
        for point in &sketch.points {
            let roles = match point.owner {
                Some((owner, role)) => vec![EntityRole {
                    of: mint("sketch-entity", owner)?,
                    role: match role {
                        PointRole::Start => DefiningRole::Start,
                        PointRole::End => DefiningRole::End,
                        PointRole::Center => DefiningRole::Center,
                    },
                }],
                None => Vec::new(),
            };
            entities.push(entity(
                point.record,
                EntityDefinition::Free(PlanarCurve::Point(point2(point.position)?)),
                roles,
            )?);
        }
        let mut constraints = Vec::new();
        for constraint in &sketch.constraints {
            let (record, relation) = match *constraint {
                Constraint::Coincident { record, a, b } => (
                    record,
                    SketchRelation::Coincident {
                        a: Operand::Entity(mint("sketch-entity", a)?),
                        b: Operand::Entity(mint("sketch-entity", b)?),
                    },
                ),
                Constraint::Parallel { record, line, axis } => (
                    record,
                    SketchRelation::Parallel {
                        a: Operand::Entity(mint("sketch-entity", line)?),
                        b: Operand::Frame(match axis {
                            SketchAxis::X => PrincipalEntity::XAxis,
                            SketchAxis::Y => PrincipalEntity::YAxis,
                        }),
                    },
                ),
            };
            constraints.push(SketchConstraint {
                id: mint::<SketchConstraintId>("sketch-constraint", record)?,
                sketch: id.clone(),
                relation,
                name: None,
                active: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: None,
            });
        }
        Ok((entities, constraints))
    }

    fn operation(
        &self,
        at: usize,
        feature: &HistoryFeature,
    ) -> Result<FeatureOperation, CodecError> {
        let output = || SolidOutput {
            body: None,
            boolean: None,
        };
        Ok(match &feature.operation {
            Operation::Origin(position) => {
                datum(DatumElement::Points(vec![Construction::<kinds::Point> {
                    resolved: Some(point3(*position)?),
                    ..Construction::default()
                }]))
            }
            Operation::Axis { origin, direction } => {
                datum(DatumElement::Axis(Construction::<kinds::Axis> {
                    resolved: Some(AxisValue {
                        origin: point3(*origin)?,
                        direction: unit3(*direction)?,
                    }),
                    ..Construction::default()
                }))
            }
            Operation::Plane { frame } => {
                datum(DatumElement::Plane(Construction::<kinds::Plane> {
                    resolved: Some(plane_value(frame)?),
                    ..Construction::default()
                }))
            }
            Operation::OffsetPlane {
                base,
                offset,
                frame,
            } => datum(DatumElement::Plane(Construction::<kinds::Plane> {
                inputs: vec![Self::input(
                    Relation::Offset,
                    self.datum_pick(*base)?,
                    Some(ScalarDimension::Length(self.dimension(*offset))),
                )],
                resolved: Some(plane_value(frame)?),
                ..Construction::default()
            })),
            Operation::Sketch(_) => FeatureOperation::Sketch(
                self.sketches
                    .get(&at)
                    .cloned()
                    .ok_or_else(|| invalid("a sketch state has no sketch identity"))?,
            ),
            Operation::Extrude {
                sketch,
                end,
                reversed,
            } => FeatureOperation::Extrude(Box::new(Extrude {
                profile: self.sketch_pick(*sketch)?,
                loops: None,
                direction: Some(ExtrudeDirection::ProfileNormal {
                    reversed: Some(Dimension::Literal(*reversed)),
                }),
                origin: Some(ExtrudeOrigin::ProfilePlane),
                span: Some(Span::Between {
                    start: Box::new(Limit::AtOrigin),
                    end: Box::new(self.limit(*end)),
                }),
                draft: None,
                fit: None,
                outside: None,
                sheet_cut: None,
                output: output(),
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })),
            Operation::Revolve {
                sketch,
                axis_origin,
                axis_direction,
                axis_feature,
                angle,
            } => {
                let inputs = match axis_feature {
                    Some(axis) => vec![Self::input(
                        Relation::Coincident,
                        self.datum_pick(*axis)?,
                        None,
                    )],
                    None => Vec::new(),
                };
                FeatureOperation::Revolve(Box::new(Revolve {
                    profile: self.sketch_pick(*sketch)?,
                    loops: None,
                    axis: Some(Construction::<kinds::Axis> {
                        inputs,
                        resolved: Some(AxisValue {
                            origin: point3(*axis_origin)?,
                            direction: unit3(*axis_direction)?,
                        }),
                        ..Construction::default()
                    }),
                    span: Some(Span::Between {
                        start: Box::new(Limit::AtOrigin),
                        end: Box::new(Limit::Amount(self.dimension(*angle))),
                    }),
                    fit: None,
                    output: output(),
                    native_options: BTreeMap::new(),
                    source_settings: BTreeMap::new(),
                }))
            }
            Operation::Fillet {
                radius,
                edge_signature,
            } => FeatureOperation::Fillet(Box::new(Fillet {
                sets: vec![FilletSet::Edges {
                    edges: unread(edge_signature.as_deref().map(signature_text))?,
                    section: FilletSection {
                        size: Some(FilletSize::Radius(self.dimension(*radius))),
                        profile: None,
                        weight: None,
                    },
                    spine: None,
                    hold: [None, None],
                }],
                controls: BlendControls {
                    corner_style: None,
                    corners: Vec::new(),
                    limits: Vec::new(),
                    keep: None,
                    transitions: Vec::new(),
                    overflow: None,
                    keep_features: None,
                    round_corners: None,
                },
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })),
            Operation::Hole {
                sketch,
                diameter,
                end,
            } => {
                let Operation::Sketch(hole_sketch) = &self.history.features[*sketch].operation
                else {
                    return Err(invalid("a hole names a state that holds no sketch"));
                };
                let center = hole_sketch
                    .primitives
                    .iter()
                    .find(|primitive| matches!(primitive.curve, Curve::Point(_)));
                let points = match center {
                    Some(primitive) => named(PickTarget::SketchEntity(mint(
                        "sketch-entity",
                        primitive.record,
                    )?))?,
                    None => unread(None)?,
                };
                let start = match &hole_sketch.support {
                    Support::Face(signature) => Some(unread(Some(signature_text(signature)))?),
                    Support::Plane(_) => None,
                };
                FeatureOperation::Hole(Box::new(Hole {
                    positions: HolePositions::Points {
                        points,
                        centers: None,
                    },
                    start,
                    direction: None,
                    shape: HoleShape::Bore(Bore {
                        diameter: Some(self.dimension(*diameter)),
                        head: None,
                        taper: None,
                        bottom: None,
                        near_chamfer: None,
                        neck_chamfer: None,
                        far_chamfer: None,
                    }),
                    extent: Some(self.limit(*end)),
                    standard: None,
                    thread: None,
                    targets: None,
                    series: Vec::new(),
                    native_options: BTreeMap::new(),
                    source_settings: BTreeMap::new(),
                }))
            }
            Operation::Loft { sections } => FeatureOperation::Loft(Box::new(Loft {
                start: None,
                sections: sections
                    .iter()
                    .map(|sketch| {
                        Ok(Section {
                            form: SectionForm::Profile(self.sketch_pick(*sketch)?),
                            flip: None,
                            seam: None,
                            condition: None,
                            trim_support: None,
                            spine_point: None,
                        })
                    })
                    .collect::<Result<Vec<_>, CodecError>>()?,
                end: None,
                cross: Vec::new(),
                spine: None,
                guides: Vec::new(),
                correspondence: None,
                shape: LoftShape {
                    style: None,
                    max_degree: None,
                    rebuild: None,
                    ruled: None,
                    closed: None,
                    relimit: None,
                    merge_tangent_faces: None,
                    smoothing: None,
                    section_law: None,
                },
                output: output(),
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })),
            Operation::Unmapped(name) => FeatureOperation::Native {
                kind: NonBlankString::new(format!("alibre:{}", name.to_ascii_lowercase()))
                    .ok_or_else(|| invalid("an operation record has no name"))?,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            },
        })
    }

    fn limit<Q>(&self, end: End) -> Limit<Q> {
        match end {
            End::Blind(parameter) => Limit::Amount(self.dimension(parameter)),
            End::ThroughAll => Limit::ThroughAll,
        }
    }

    fn parameter(&self, at: usize, parameter: &Parameter) -> Result<DesignParameter, CodecError> {
        let (quantity, symbol) = match parameter.kind {
            ParameterKind::Length => (Quantity::LENGTH, UnitSymbol::Mm),
            ParameterKind::Angle => (Quantity::ANGLE, UnitSymbol::Rad),
        };
        let unit = Unit(vec![UnitFactor {
            symbol,
            power: std::num::NonZeroI8::new(1).ok_or_else(|| invalid("unit power"))?,
        }]);
        let value = FiniteReal::new(parameter.value)
            .ok_or_else(|| invalid("a parameter value is not finite"))?;
        let definition = match NonNegativeReal::new(parameter.value) {
            Some(number) => Expression::Neutral(Neutral(Node::Number {
                value: number,
                unit: unit.clone(),
            })),
            None => Expression::Source {
                dialect: cadmpeg_ir::design::expr::ExpressionDialect::Native(
                    NonBlankString::new("alibre")
                        .ok_or_else(|| invalid("the dialect name is blank"))?,
                ),
                tokens: vec![cadmpeg_ir::design::expr::ExpressionToken::Text(
                    parameter.value.to_string(),
                )],
            },
        };
        let owner = self
            .history
            .features
            .iter()
            .position(|feature| uses(&feature.operation, at))
            .map(|feature| ParameterOwner::Feature(self.features[feature].clone()));
        let diameter = self.history.features.iter().any(|feature| {
            matches!(feature.operation, Operation::Hole { diameter, .. } if diameter == at)
        });
        Ok(DesignParameter {
            id: self.parameters[at].clone(),
            scope: self.scope.clone(),
            owner,
            name: NonBlankString::new(parameter.name.clone()),
            kind: ValueKind::Quantity(quantity),
            unit: Some(unit),
            definition: ParameterDefinition::Expression(definition),
            value: Some(ParameterValue::Number(value)),
            display: diameter.then_some(DimensionDisplay::Diameter),
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: None,
        })
    }
}

fn uses(operation: &Operation, parameter: usize) -> bool {
    let end = |end: &End| matches!(end, End::Blind(at) if *at == parameter);
    match operation {
        Operation::OffsetPlane { offset, .. } => *offset == parameter,
        Operation::Extrude { end: e, .. } => end(e),
        Operation::Revolve { angle, .. } => *angle == parameter,
        Operation::Fillet { radius, .. } => *radius == parameter,
        Operation::Hole {
            diameter, end: e, ..
        } => *diameter == parameter || end(e),
        _ => false,
    }
}

fn plane_value(frame: &Frame) -> Result<PlaneValue, CodecError> {
    Ok(PlaneValue {
        origin: point3(frame.origin)?,
        normal: unit3(frame.z)?,
        x_axis: unit3(frame.x)?,
    })
}

fn datum(element: DatumElement) -> FeatureOperation {
    FeatureOperation::Datum(Box::new(Datum {
        element,
        link: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

/// States the design scope of the root definition: its timeline in state
/// order, its features, its sketches, and its parameters.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    history: &History,
    definition: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    losses.extend(history.losses.iter().cloned());
    if history
        .features
        .iter()
        .any(|feature| matches!(feature.operation, Operation::Extrude { .. }))
    {
        losses.push(AlibreLossCode::HistoryDirectionFlagAssumed.note(
            "the logical of a blind end condition is read as the reversal of the sketch normal",
        ));
    }
    losses.push(AlibreLossCode::HistoryPresentationUnmapped.note(
        "document profile, measurement format, font, hatch, configuration, and version records are not mapped",
    ));
    let mut projection = Projection {
        history,
        features: Vec::new(),
        parameters: Vec::new(),
        sketches: BTreeMap::new(),
        scope: definition.clone(),
    };
    for feature in &history.features {
        projection.features.push(mint("feature", feature.record)?);
    }
    for parameter in &history.parameters {
        projection
            .parameters
            .push(mint("parameter", parameter.record)?);
    }
    for (at, feature) in history.features.iter().enumerate() {
        if let Operation::Sketch(sketch) = &feature.operation {
            projection
                .sketches
                .insert(at, mint("sketch", sketch.record)?);
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(history.features.len() + history.parameters.len() + 2),
        "project Alibre design records",
    )?;
    let mut features = Vec::new();
    for (at, feature) in history.features.iter().enumerate() {
        if let Operation::Sketch(sketch) = &feature.operation {
            let id = &projection.sketches[&at];
            ir.design
                .sketches
                .push(projection.sketch(sketch, id, &projection.features[at])?);
            let (entities, constraints) = Projection::entities(sketch, id)?;
            ctx.charge_entities(
                cadmpeg_core::decode::u64_from_index(entities.len() + constraints.len()),
                "project Alibre sketch records",
            )?;
            ir.design.sketch_entities.extend(entities);
            ir.design.sketch_constraints.extend(constraints);
        }
        features.push(Feature {
            id: projection.features[at].clone(),
            name: Some(feature.name.clone()),
            suppressed: None,
            definition: FeatureDefinition {
                operation: projection.operation(at, feature)?,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
    }
    for (at, parameter) in history.parameters.iter().enumerate() {
        ir.design
            .parameters
            .push(projection.parameter(at, parameter)?);
    }
    let timeline: ContainerId = mint("container", "timeline")?;
    let members = features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<Vec<_>>();
    let at = members.last().map_or_else(
        || Position::Start(timeline.clone()),
        |last| Position::After(last.clone()),
    );
    let bodies = ir
        .model
        .product_definitions
        .iter()
        .filter(|candidate| candidate.id == *definition)
        .flat_map(|candidate| candidate.bodies.iter().cloned())
        .collect();
    ir.design.features.extend(features);
    ir.design.containers.push(Container::Ordered {
        id: timeline.clone(),
        kind: OrderedKind::Timeline,
        members,
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    ir.design.scopes.push(DesignScope {
        definition: definition.clone(),
        roots: vec![timeline],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at,
            bodies,
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
        unproduced: BTreeSet::default(),
    });
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketches.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_entities.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_constraints.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}
