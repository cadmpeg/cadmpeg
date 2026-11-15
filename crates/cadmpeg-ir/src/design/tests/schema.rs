// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every design type.

use cadmpeg_core::text::NonBlankString;
use schemars::{schema_for, JsonSchema};

use crate::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, CurveValue, FrameAxis,
    FrameValue, InputReference, InputRole, PlaneValue, PostStep, Relation, ScalarDimension,
    SourceMode, Subject,
};
use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::expr::{
    BendVariable, BinaryOp, Constant, Expression, ExpressionDialect, ExpressionToken, Function,
    Measure, MeasureKind, MeasureTarget, Neutral, NeutralWire, Node, UnaryOp,
};
use crate::design::ids::{
    ArrangementId, ContainerId, EvaluationId, MachineId, MachiningFeatureId, OperationId,
    ParameterProgramId, PresetId, ProcessGroupId, ProcessParameterId, ProcessStateId, ProgramId,
    PublicationId, SetupId, SheetMetalRuleId, SpatialSketchConstraintId, ToolId, ToolpathId,
};
use crate::design::names::{
    Base64Bytes, CodecId, GenerationRole, Generator, HoleFaceRole, InstanceIndex, NameEntry,
    NameKind, NameTable, NativeName, NativeNameValue, TopoName,
};
use crate::design::options::{
    BodyForm, Boolean, BooleanOp, CornerRound, Hand, LimitOffset, LoopNesting, NativeOption,
    NativeOptions, ProjectionDirection, SectionCondition, SheetOutput, SolidOutput, Solidify,
    Support, SymmetricMeasure, Targets, Thickness, ThinEnds, ThinWall, WallSide,
};
use crate::design::refs::{
    ConfigurationRef, EntityRef, Evaluated, Member, Position, RecordRef, StateRef,
};
use crate::design::selection::{
    Anchor, BoundaryUse, Continuity, DatumPart, DefinitionRef, Fragment, Members, NativeOwner,
    OccurrencePath, Ordered, PathStep, PickContext, PickDomain, PickTarget, Picked, Picks,
    PrincipalEntity, Propagation, RuleTerm, Selection, SelectionRule, SketchRegion, StepRef,
    Unordered, Witness,
};
use crate::design::topology::Topology;
use crate::design::units::{
    AngularSpeed, Duration, Feed, PositiveArea, Quantity, Unit, UnitFactor, UnitSymbol, UnitSystem,
    ValueKind,
};
use crate::scalar::{Angle, Length};

fn generates<T: JsonSchema>() {
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    assert!(schema.is_object(), "{}", T::schema_name());
}

#[test]
fn every_design_type_has_a_schema() {
    generates::<ContainerId>();
    generates::<EvaluationId>();
    generates::<PublicationId>();
    generates::<ArrangementId>();
    generates::<ParameterProgramId>();
    generates::<SheetMetalRuleId>();
    generates::<SpatialSketchConstraintId>();
    generates::<SetupId>();
    generates::<OperationId>();
    generates::<MachiningFeatureId>();
    generates::<ToolId>();
    generates::<PresetId>();
    generates::<MachineId>();
    generates::<ProgramId>();
    generates::<ProcessGroupId>();
    generates::<ProcessParameterId>();
    generates::<ToolpathId>();
    generates::<ProcessStateId>();
    generates::<EntityRef>();
    generates::<RecordRef>();
    generates::<ConfigurationRef>();
    generates::<Member>();
    generates::<Position>();
    generates::<Evaluated>();
    generates::<StateRef>();
    generates::<CodecId>();
    generates::<Base64Bytes>();
    generates::<NameTable>();
    generates::<NameEntry>();
    generates::<NativeName>();
    generates::<NativeNameValue>();
    generates::<InstanceIndex>();
    generates::<TopoName>();
    generates::<NameKind>();
    generates::<Generator>();
    generates::<GenerationRole>();
    generates::<HoleFaceRole>();
    generates::<Topology>();
    generates::<Ordered>();
    generates::<Unordered>();
    generates::<Members<Picked, Ordered>>();
    generates::<Selection<Ordered>>();
    generates::<Selection<Unordered>>();
    generates::<Anchor>();
    generates::<Picks<Ordered>>();
    generates::<Picks<Unordered>>();
    generates::<Picked>();
    generates::<PickTarget>();
    generates::<NativeOwner>();
    generates::<PickContext>();
    generates::<DefinitionRef>();
    generates::<OccurrencePath>();
    generates::<PathStep>();
    generates::<StepRef>();
    generates::<PrincipalEntity>();
    generates::<DatumPart>();
    generates::<Witness>();
    generates::<PickDomain>();
    generates::<RuleTerm<Ordered>>();
    generates::<RuleTerm<Unordered>>();
    generates::<SelectionRule>();
    generates::<Propagation>();
    generates::<Continuity>();
    generates::<SketchRegion>();
    generates::<BoundaryUse>();
    generates::<Fragment>();
    generates::<Construction<kinds::Point>>();
    generates::<Construction<kinds::Direction>>();
    generates::<Construction<kinds::Axis>>();
    generates::<Construction<kinds::Plane>>();
    generates::<Construction<kinds::Frame>>();
    generates::<Construction<kinds::Curve>>();
    generates::<ConstructionInput>();
    generates::<InputReference>();
    generates::<AnyConstruction>();
    generates::<InputRole>();
    generates::<Subject>();
    generates::<Relation>();
    generates::<ScalarDimension>();
    generates::<PostStep>();
    generates::<FrameAxis>();
    generates::<SourceMode>();
    generates::<AxisValue>();
    generates::<PlaneValue>();
    generates::<FrameValue>();
    generates::<CurveValue>();
    generates::<Expression<ParameterRef>>();
    generates::<Expression<ParamRef>>();
    generates::<Neutral<ParameterRef>>();
    generates::<NeutralWire<ParamRef>>();
    generates::<ExpressionToken<ParameterRef>>();
    generates::<ExpressionDialect>();
    generates::<Node<ParameterRef>>();
    generates::<Constant>();
    generates::<BendVariable>();
    generates::<UnaryOp>();
    generates::<BinaryOp>();
    generates::<Function>();
    generates::<Measure>();
    generates::<MeasureKind>();
    generates::<MeasureTarget>();
    generates::<Dimension<Length>>();
    generates::<Dimension<bool>>();
    generates::<Dimension<Angle, ParamRef>>();
    generates::<Dimension<Hand>>();
    generates::<Dimension<NonBlankString>>();
    generates::<Dimension<u32>>();
    generates::<ParameterRef>();
    generates::<RefTarget>();
    generates::<ParamRef>();
    generates::<Unit>();
    generates::<UnitFactor>();
    generates::<UnitSymbol>();
    generates::<UnitSystem>();
    generates::<ValueKind>();
    generates::<Quantity>();
    generates::<Duration>();
    generates::<Feed>();
    generates::<AngularSpeed>();
    generates::<PositiveArea>();
    generates::<NativeOptions>();
    generates::<NativeOption>();
    generates::<Hand>();
    generates::<LoopNesting>();
    generates::<SolidOutput>();
    generates::<BodyForm>();
    generates::<ThinWall>();
    generates::<CornerRound>();
    generates::<Thickness>();
    generates::<WallSide>();
    generates::<ThinEnds>();
    generates::<Boolean>();
    generates::<BooleanOp>();
    generates::<Targets>();
    generates::<SheetOutput>();
    generates::<Solidify>();
    generates::<SymmetricMeasure>();
    generates::<LimitOffset>();
    generates::<SectionCondition>();
    generates::<Support>();
    generates::<ProjectionDirection>();
}

#[test]
fn a_neutral_schema_is_its_wire_form() {
    let schema = serde_json::to_value(schema_for!(Neutral<ParameterRef>)).unwrap();
    let properties = &schema["properties"];
    for field in ["text", "refs", "measures"] {
        assert!(properties.get(field).is_some(), "{field}: {schema}");
    }
}
