// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every design type.

use cadmpeg_core::serde_erase::json;
use cadmpeg_core::text::NonBlankString;
use schemars::{schema_for, SchemaGenerator};

use cadmpeg_ir::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, CurveValue, FrameAxis,
    FrameValue, InputReference, InputRole, PlaneValue, PostStep, Relation, ScalarDimension,
    SourceMode, Subject,
};
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    BendVariable, BinaryOp, Constant, Expression, ExpressionDialect, ExpressionToken, Function,
    Measure, MeasureKind, MeasureTarget, Neutral, NeutralWire, Node, UnaryOp,
};
use cadmpeg_ir::design::ids::{
    ArrangementId, ContainerId, EvaluationId, SheetMetalRuleId, SpatialSketchConstraintId,
};
use cadmpeg_ir::design::names::{
    Base64Bytes, CodecId, GenerationRole, Generator, HoleFaceRole, InstanceIndex, NameEntry,
    NameKind, NameTable, NativeName, NativeNameValue, TopoName,
};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, CornerRound, Hand, LimitOffset, LoopNesting, NativeOption,
    NativeOptions, ProjectionDirection, SectionCondition, SheetOutput, SolidOutput, Solidify,
    Support, SymmetricMeasure, Targets, Thickness, ThinEnds, ThinWall, WallSide,
};
use cadmpeg_ir::design::refs::{
    ConfigurationRef, EntityRef, Evaluated, Member, Position, RecordRef, StateRef,
};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, Continuity, DatumPart, DefinitionRef, Fragment, Members, NativeOwner,
    OccurrencePath, Ordered, PathStep, PickContext, PickDomain, PickTarget, Picked, Picks,
    PrincipalEntity, Propagation, RuleTerm, Selection, SelectionRule, SketchRegion, StepRef,
    Unordered, Witness,
};
use cadmpeg_ir::design::units::{
    AngularSpeed, Duration, Feed, PositiveArea, Quantity, Unit, UnitFactor, UnitSymbol, UnitSystem,
    ValueKind,
};
use cadmpeg_ir::scalar::{Angle, Length};

use crate::generates;

#[test]
fn every_design_type_has_a_schema() {
    let mut generator = SchemaGenerator::default();
    generates::<ContainerId>(&mut generator);
    generates::<EvaluationId>(&mut generator);
    generates::<ArrangementId>(&mut generator);
    generates::<SheetMetalRuleId>(&mut generator);
    generates::<SpatialSketchConstraintId>(&mut generator);
    generates::<EntityRef>(&mut generator);
    generates::<RecordRef>(&mut generator);
    generates::<ConfigurationRef>(&mut generator);
    generates::<Member>(&mut generator);
    generates::<Position>(&mut generator);
    generates::<Evaluated>(&mut generator);
    generates::<StateRef>(&mut generator);
    generates::<CodecId>(&mut generator);
    generates::<Base64Bytes>(&mut generator);
    generates::<NameTable>(&mut generator);
    generates::<NameEntry>(&mut generator);
    generates::<NativeName>(&mut generator);
    generates::<NativeNameValue>(&mut generator);
    generates::<InstanceIndex>(&mut generator);
    generates::<TopoName>(&mut generator);
    generates::<NameKind>(&mut generator);
    generates::<Generator>(&mut generator);
    generates::<GenerationRole>(&mut generator);
    generates::<HoleFaceRole>(&mut generator);
    generates::<Ordered>(&mut generator);
    generates::<Unordered>(&mut generator);
    generates::<Members<Picked, Ordered>>(&mut generator);
    generates::<Selection<Ordered>>(&mut generator);
    generates::<Selection<Unordered>>(&mut generator);
    generates::<Anchor>(&mut generator);
    generates::<Picks<Ordered>>(&mut generator);
    generates::<Picks<Unordered>>(&mut generator);
    generates::<Picked>(&mut generator);
    generates::<PickTarget>(&mut generator);
    generates::<NativeOwner>(&mut generator);
    generates::<PickContext>(&mut generator);
    generates::<DefinitionRef>(&mut generator);
    generates::<OccurrencePath>(&mut generator);
    generates::<PathStep>(&mut generator);
    generates::<StepRef>(&mut generator);
    generates::<PrincipalEntity>(&mut generator);
    generates::<DatumPart>(&mut generator);
    generates::<Witness>(&mut generator);
    generates::<PickDomain>(&mut generator);
    generates::<RuleTerm<Ordered>>(&mut generator);
    generates::<RuleTerm<Unordered>>(&mut generator);
    generates::<SelectionRule>(&mut generator);
    generates::<Propagation>(&mut generator);
    generates::<Continuity>(&mut generator);
    generates::<SketchRegion>(&mut generator);
    generates::<BoundaryUse>(&mut generator);
    generates::<Fragment>(&mut generator);
    generates::<Construction<kinds::Point>>(&mut generator);
    generates::<Construction<kinds::Direction>>(&mut generator);
    generates::<Construction<kinds::Axis>>(&mut generator);
    generates::<Construction<kinds::Plane>>(&mut generator);
    generates::<Construction<kinds::Frame>>(&mut generator);
    generates::<Construction<kinds::Curve>>(&mut generator);
    generates::<ConstructionInput>(&mut generator);
    generates::<InputReference>(&mut generator);
    generates::<AnyConstruction>(&mut generator);
    generates::<InputRole>(&mut generator);
    generates::<Subject>(&mut generator);
    generates::<Relation>(&mut generator);
    generates::<ScalarDimension>(&mut generator);
    generates::<PostStep>(&mut generator);
    generates::<FrameAxis>(&mut generator);
    generates::<SourceMode>(&mut generator);
    generates::<AxisValue>(&mut generator);
    generates::<PlaneValue>(&mut generator);
    generates::<FrameValue>(&mut generator);
    generates::<CurveValue>(&mut generator);
    generates::<Expression<ParameterRef>>(&mut generator);
    generates::<Neutral<ParameterRef>>(&mut generator);
    generates::<ExpressionToken<ParameterRef>>(&mut generator);
    generates::<ExpressionDialect>(&mut generator);
    generates::<Node<ParameterRef>>(&mut generator);
    generates::<Constant>(&mut generator);
    generates::<BendVariable>(&mut generator);
    generates::<UnaryOp>(&mut generator);
    generates::<BinaryOp>(&mut generator);
    generates::<Function>(&mut generator);
    generates::<Measure>(&mut generator);
    generates::<MeasureKind>(&mut generator);
    generates::<MeasureTarget>(&mut generator);
    generates::<Dimension<Length>>(&mut generator);
    generates::<Dimension<bool>>(&mut generator);
    generates::<Dimension<Hand>>(&mut generator);
    generates::<Dimension<NonBlankString>>(&mut generator);
    generates::<Dimension<u32>>(&mut generator);
    generates::<ParameterRef>(&mut generator);
    generates::<RefTarget>(&mut generator);
    generates::<Unit>(&mut generator);
    generates::<UnitFactor>(&mut generator);
    generates::<UnitSymbol>(&mut generator);
    generates::<UnitSystem>(&mut generator);
    generates::<ValueKind>(&mut generator);
    generates::<Quantity>(&mut generator);
    generates::<Duration>(&mut generator);
    generates::<Feed>(&mut generator);
    generates::<AngularSpeed>(&mut generator);
    generates::<PositiveArea>(&mut generator);
    generates::<NativeOptions>(&mut generator);
    generates::<NativeOption>(&mut generator);
    generates::<Hand>(&mut generator);
    generates::<LoopNesting>(&mut generator);
    generates::<SolidOutput>(&mut generator);
    generates::<BodyForm>(&mut generator);
    generates::<ThinWall>(&mut generator);
    generates::<CornerRound>(&mut generator);
    generates::<Thickness>(&mut generator);
    generates::<WallSide>(&mut generator);
    generates::<ThinEnds>(&mut generator);
    generates::<Boolean>(&mut generator);
    generates::<BooleanOp>(&mut generator);
    generates::<Targets>(&mut generator);
    generates::<SheetOutput>(&mut generator);
    generates::<Solidify>(&mut generator);
    generates::<SymmetricMeasure>(&mut generator);
    generates::<LimitOffset>(&mut generator);
    generates::<SectionCondition>(&mut generator);
    generates::<Support>(&mut generator);
    generates::<ProjectionDirection>(&mut generator);
}

#[test]
fn a_neutral_schema_is_its_wire_form() {
    let schema = json::to_value(schema_for!(Neutral<ParameterRef>)).unwrap();
    let properties = &schema["properties"];
    for field in ["text", "refs", "measures"] {
        assert!(properties.get(field).is_some(), "{field}: {schema}");
    }
}

#[test]
fn every_design_root_type_has_a_schema() {
    use cadmpeg_ir::design::{configuration, container, evaluation, feature, parameter, scope};
    let mut generator = SchemaGenerator::default();
    generates::<cadmpeg_ir::document::Design>(&mut generator);
    generates::<scope::DesignScope>(&mut generator);
    generates::<scope::OccurrenceDesign>(&mut generator);
    generates::<container::Container>(&mut generator);
    generates::<container::OrderedKind>(&mut generator);
    generates::<container::UnorderedKind>(&mut generator);
    generates::<container::Folder>(&mut generator);
    generates::<container::Rollback>(&mut generator);
    generates::<evaluation::Evaluation>(&mut generator);
    generates::<evaluation::StateContent>(&mut generator);
    generates::<evaluation::EvaluationStatus>(&mut generator);
    generates::<evaluation::StateBase>(&mut generator);
    generates::<evaluation::StateDelta>(&mut generator);
    generates::<evaluation::CurrentState>(&mut generator);
    generates::<feature::Feature>(&mut generator);
    generates::<feature::FeatureDefinition>(&mut generator);
    generates::<feature::Driver>(&mut generator);
    generates::<feature::FuzzyTolerance>(&mut generator);
    generates::<feature::FeatureOperation>(&mut generator);
    generates::<feature::StoredOrigin>(&mut generator);
    generates::<feature::Link>(&mut generator);
    generates::<feature::LinkState>(&mut generator);
    generates::<feature::OperationFamily>(&mut generator);
    generates::<configuration::DesignConfiguration>(&mut generator);
    generates::<configuration::Overrides>(&mut generator);
    generates::<configuration::DefinitionOverride>(&mut generator);
    generates::<configuration::PartNumber>(&mut generator);
    generates::<configuration::ParameterOverride>(&mut generator);
    generates::<configuration::FeatureStateOverride>(&mut generator);
    generates::<configuration::OccurrenceOverride>(&mut generator);
    generates::<configuration::MaterialOverride>(&mut generator);
    generates::<configuration::MaterialRef>(&mut generator);
    generates::<configuration::Arrangement>(&mut generator);
    generates::<configuration::ArrangementKind>(&mut generator);
    generates::<configuration::ArrangementAppearance>(&mut generator);
    generates::<configuration::InstanceAppearance>(&mut generator);
    generates::<configuration::ArrangementMove>(&mut generator);
    generates::<configuration::NestedArrangement>(&mut generator);
    generates::<configuration::ArrangementRef>(&mut generator);
    generates::<cadmpeg_ir::products::NestedOverride>(&mut generator);
    generates::<parameter::DesignParameter>(&mut generator);
    generates::<parameter::ParameterOwner>(&mut generator);
    generates::<parameter::ParameterDefinition>(&mut generator);
    generates::<parameter::ParameterValue>(&mut generator);
    generates::<parameter::DimensionDisplay>(&mut generator);
    generates::<parameter::Tolerance>(&mut generator);
    generates::<parameter::Deviations>(&mut generator);
    generates::<parameter::FitDeviations>(&mut generator);
    generates::<parameter::LimitSide>(&mut generator);
    generates::<cadmpeg_ir::design::spreadsheet::Spreadsheet>(&mut generator);
}

#[test]
fn unproduced_schema_is_a_required_unique_array_of_body_ids() {
    let schema = json::to_value(schema_for!(DesignScope)).unwrap();
    let property = &schema["properties"]["unproduced"];
    assert_eq!(property["type"], "array");
    assert_eq!(property["uniqueItems"], true);
    assert!(property["items"]["$ref"]
        .as_str()
        .unwrap()
        .ends_with("/BodyId"));
    assert!(schema["required"]
        .as_array()
        .unwrap()
        .contains(&cadmpeg_core::json!("unproduced")));
    assert_eq!(schema["additionalProperties"], false);
}
