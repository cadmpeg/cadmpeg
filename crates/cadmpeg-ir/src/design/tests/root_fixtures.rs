// SPDX-License-Identifier: Apache-2.0
//! Hand-built values of the design root's types, one or more per variant.

use std::collections::{BTreeMap, BTreeSet};

use super::fixtures::{
    self, container, entity_refs, feature, id, name_entries, name_tables, native_name,
    occurrence_path, parameter_ref, picked, positive_length, real, record_refs, selections, text,
};
use crate::design::configuration::{
    Arrangement, ArrangementAppearance, ArrangementKind, ArrangementMove, ArrangementRef,
    DefinitionOverride, DesignConfiguration, FeatureStateOverride, MaterialOverride, MaterialRef,
    NestedArrangement, OccurrenceOverride, Overrides, ParameterOverride, PartNumber, Publication,
};
use crate::design::construction::kinds;
use crate::design::container::{Container, Folder, OrderedKind, Rollback, UnorderedKind};
use crate::design::dimension::Dimension;
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::expr::{ExpressionDialect, ExpressionToken, Neutral, Node};
use crate::design::feature::{
    Driver, Feature, FeatureDefinition, FeatureOperation, FuzzyTolerance, Link, LinkState,
    OperationFamily, StoredOrigin,
};
use crate::design::ops::body::TransformStep;
use crate::design::ops::sheet_metal::{BendAllowance, BendSpec, RadiusSide, SheetMetalRule};
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::parameter::{
    DesignParameter, Deviations, DimensionDisplay, FitDeviations, LimitSide, ParameterDefinition,
    ParameterOwner, ParameterValue, Tolerance,
};
use crate::design::program::{ParameterProgram, ProgramBody, ProgramStage, Statement};
use crate::design::refs::{ConfigurationRef, Member, Position};
use crate::design::scope::{DesignScope, OccurrenceDesign};
use crate::design::spreadsheet::Spreadsheet;
use crate::design::units::{Quantity, ValueKind};
use crate::document::{Design, Topology};
use crate::pmi::{FitStandard, FitZone, LimitsAndFits};
use crate::products::NestedOverride;
use crate::scalar::Fraction;
use crate::spreadsheets::{CellAddress, SpreadsheetCell, SpreadsheetDimension, SpreadsheetRange};
use crate::transform::Transform;

fn frame() -> Box<crate::design::construction::Construction<kinds::Frame>> {
    Box::new(
        fixtures::constructions::<kinds::Frame>(fixtures::frame_value(), fixtures::frame_value())
            .remove(0),
    )
}

pub(super) fn folders() -> Vec<Folder> {
    vec![
        Folder {
            name: Some("Holes".to_owned()),
            first: 1,
            last: 3,
        },
        Folder {
            name: None,
            first: 0,
            last: 0,
        },
    ]
}

pub(super) fn rollbacks() -> Vec<Rollback> {
    vec![Rollback::End, Rollback::Before(Member::Feature(feature(4)))]
}

pub(super) fn containers() -> Vec<Container> {
    vec![
        Container::Ordered {
            id: container(1),
            kind: OrderedKind::Timeline,
            members: vec![Member::Feature(feature(1)), Member::Container(container(2))],
            folders: folders(),
            rollback: Some(Rollback::Before(Member::Container(container(2)))),
            placement: None,
        },
        Container::Ordered {
            id: container(2),
            kind: OrderedKind::Body,
            members: Vec::new(),
            folders: Vec::new(),
            rollback: Some(Rollback::End),
            placement: Some(frame()),
        },
        Container::Unordered {
            id: container(3),
            kind: UnorderedKind::GeometricalSet,
            members: BTreeSet::from([Member::Feature(feature(5)), Member::Feature(feature(6))]),
        },
        Container::Unordered {
            id: container(4),
            kind: UnorderedKind::Group,
            members: BTreeSet::new(),
        },
        Container::Ordered {
            id: container(5),
            kind: OrderedKind::GeometricalSet,
            members: vec![Member::Feature(feature(7))],
            folders: Vec::new(),
            rollback: None,
            placement: None,
        },
    ]
}

pub(super) fn statuses() -> Vec<EvaluationStatus> {
    vec![
        EvaluationStatus::Current,
        EvaluationStatus::Stale,
        EvaluationStatus::Failed,
    ]
}

pub(super) fn state_bases() -> Vec<StateBase> {
    vec![
        StateBase::Empty,
        StateBase::Current(Position::After(Member::Feature(feature(2)))),
        StateBase::Evaluation(id("evaluation", 1)),
    ]
}

pub(super) fn state_deltas() -> Vec<StateDelta> {
    let mut path = occurrence_path();
    path.0.truncate(1);
    vec![
        StateDelta::default(),
        StateDelta {
            records: Topology::default(),
            removed: record_refs(),
            names: entity_refs()
                .into_iter()
                .zip([Some(name_entries().remove(0)), None].into_iter().cycle())
                .collect(),
            owners: BTreeMap::from([
                (id("body", 1), Some(id("product-definition", 1))),
                (id("body", 2), None),
            ]),
            members: BTreeMap::from([
                (id("occurrence", 1), Some(id("product-definition", 2))),
                (id("occurrence", 2), None),
            ]),
            instance_bodies: BTreeMap::from([
                (occurrence_path(), Some(vec![id("body", 3)])),
                (path.clone(), None),
            ]),
            poses: BTreeMap::from([
                (occurrence_path(), Some(Transform::identity())),
                (path, None),
            ]),
        },
    ]
}

pub(super) fn evaluations() -> Vec<Evaluation> {
    vec![
        Evaluation {
            id: id("evaluation", 1),
            scope: id("product-definition", 1),
            configuration: Some(id("configuration", 1)),
            of: crate::design::refs::Evaluated::State(Position::After(Member::Feature(feature(1)))),
            status: EvaluationStatus::Current,
            content: Some(StateContent {
                base: StateBase::Empty,
                delta: state_deltas().remove(1),
            }),
            native_ref: Some("sldprt:pmark#1".to_owned()),
        },
        Evaluation {
            id: id("evaluation", 2),
            scope: id("product-definition", 1),
            configuration: None,
            of: crate::design::refs::Evaluated::Tool(feature(2)),
            status: EvaluationStatus::Stale,
            content: None,
            native_ref: None,
        },
    ]
}

pub(super) fn current_states() -> Vec<CurrentState> {
    vec![
        CurrentState {
            at: Position::After(Member::Feature(feature(3))),
            bodies: vec![id("body", 1), id("body", 2)],
            poses: BTreeMap::from([(occurrence_path(), Transform::identity())]),
            status: EvaluationStatus::Current,
            native_ref: Some("f3d:state#3".to_owned()),
        },
        CurrentState {
            at: Position::Start(container(1)),
            bodies: Vec::new(),
            poses: BTreeMap::new(),
            status: EvaluationStatus::Failed,
            native_ref: None,
        },
    ]
}

pub(super) fn parameter_overrides() -> Vec<ParameterOverride> {
    vec![
        ParameterOverride {
            expression: fixtures::expressions().pop(),
            value: Some(ParameterValue::Number(real(10.0))),
        },
        ParameterOverride {
            expression: None,
            value: Some(ParameterValue::Text("A".to_owned())),
        },
    ]
}

pub(super) fn occurrence_designs() -> Vec<OccurrenceDesign> {
    vec![
        OccurrenceDesign {
            configuration: Some(ConfigurationRef::Named(text("Large"))),
            arrangements: arrangement_refs(),
            grounded: Some(true),
            flexible: Some(false),
            parameters: BTreeMap::from([(id("parameter", 1), parameter_overrides().remove(0))]),
        },
        OccurrenceDesign {
            configuration: None,
            arrangements: Vec::new(),
            grounded: None,
            flexible: None,
            parameters: BTreeMap::new(),
        },
    ]
}

pub(super) fn design_scopes() -> Vec<DesignScope> {
    vec![
        DesignScope {
            definition: id("product-definition", 1),
            roots: vec![container(1)],
            active_configuration: Some(id("configuration", 1)),
            active_arrangements: BTreeSet::from([id("arrangement", 1), id("arrangement", 2)]),
            occurrences: BTreeMap::from([(id("occurrence", 1), occurrence_designs().remove(0))]),
            current: current_states(),
            unproduced: BTreeSet::from([id("body", 8), id("body", 9)]),
        },
        DesignScope {
            definition: id("product-definition", 2),
            roots: Vec::new(),
            active_configuration: None,
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current: Vec::new(),
            unproduced: BTreeSet::default(),
        },
    ]
}

pub(super) fn drivers() -> Vec<Driver> {
    vec![Driver::Feature(feature(7)), Driver::Native(native_name())]
}

pub(super) fn fuzzy_tolerances() -> Vec<FuzzyTolerance> {
    vec![
        FuzzyTolerance::KernelDefault,
        FuzzyTolerance::Automatic,
        FuzzyTolerance::Explicit(Dimension::Literal(positive_length(0.001))),
    ]
}

pub(super) fn link_states() -> Vec<LinkState> {
    vec![
        LinkState::Current,
        LinkState::OutOfDate,
        LinkState::Broken,
        LinkState::Locked,
    ]
}

pub(super) fn links() -> Vec<Link> {
    vec![
        Link {
            source: Some(selections().remove(0)),
            document: Some(fixtures::definition_ref()),
            transform: Some(Transform::identity()),
            scale: Some(real(2.0)),
            associative: Some(true),
            state: Some(LinkState::OutOfDate),
            native_ref: Some("catia:link#1".to_owned()),
        },
        Link {
            source: None,
            document: None,
            transform: None,
            scale: None,
            associative: None,
            state: None,
            native_ref: None,
        },
    ]
}

pub(super) fn stored_origins() -> Vec<StoredOrigin> {
    vec![
        StoredOrigin::Base {},
        StoredOrigin::Imported {},
        StoredOrigin::Snapshot {},
        StoredOrigin::Linked(Box::new(links().remove(0))),
        StoredOrigin::Linked(Box::new(links().remove(1))),
    ]
}

pub(super) fn operation_families() -> Vec<OperationFamily> {
    use OperationFamily::*;
    vec![
        Sketch,
        SpatialSketch,
        Spreadsheet,
        Datum,
        Extrude,
        Revolve,
        Sweep,
        HelicalSweep,
        Rib,
        Primitive,
        Loft,
        Fillet,
        Chamfer,
        Shell,
        Draft,
        Thicken,
        Offset,
        Hole,
        Thread,
        Emboss,
        Dome,
        Decal,
        Pattern,
        TemplateInstance,
        Combine,
        Split,
        DeleteBodies,
        Extract,
        TransformBodies,
        Flex,
        MoveFaces,
        DeleteFaces,
        ReplaceFaces,
        Graft,
        Patch,
        Knit,
        Unsew,
        UnsewFaces,
        Extend,
        ImplicitSweep,
        BoundaryFill,
        Curve,
        Form,
        SheetMetal,
        Mesh,
        Place,
        PlaceSet,
        Joint,
        RigidGroup,
        ComponentPattern,
        MirrorComponents,
        AssemblyFeature,
        MaterialRemoval,
    ]
}

fn native_options() -> NativeOptions {
    NativeOptions::from([(
        crate::codec_name!("sldprt:auto_select"),
        NativeOption::Bool(Dimension::Literal(true)),
    )])
}

pub(super) fn feature_operations() -> Vec<FeatureOperation> {
    vec![
        FeatureOperation::Sketch(id("sketch", 1)),
        FeatureOperation::SpatialSketch(id("spatial-sketch", 1)),
        FeatureOperation::Spreadsheet(id("spreadsheet", 1)),
        FeatureOperation::MaterialRemoval,
        FeatureOperation::Stored {
            bodies: Some(vec![id("body", 1)]),
            tessellations: vec![id("tessellation", 1)],
            origin: StoredOrigin::Linked(Box::new(links().remove(0))),
        },
        FeatureOperation::Stored {
            bodies: None,
            tessellations: Vec::new(),
            origin: StoredOrigin::Imported {},
        },
        FeatureOperation::Unresolved {
            family: OperationFamily::Fillet,
        },
        FeatureOperation::Native {
            kind: crate::codec_name!("nx:untrim"),
            native_options: native_options(),
            source_settings: NativeOptions::new(),
        },
    ]
}

pub(super) fn feature_definitions() -> Vec<FeatureDefinition> {
    vec![
        FeatureDefinition {
            operation: FeatureOperation::Sketch(id("sketch", 1)),
            refine: None,
            fuzzy_tolerance: None,
        },
        FeatureDefinition {
            operation: FeatureOperation::Unresolved {
                family: OperationFamily::Extrude,
            },
            refine: Some(false),
            fuzzy_tolerance: Some(FuzzyTolerance::Automatic),
        },
    ]
}

pub(super) fn features() -> Vec<Feature> {
    vec![
        Feature {
            id: feature(1),
            name: Some("Boss-Extrude1".to_owned()),
            suppressed: Some(Dimension::Parameter(id("parameter", 1))),
            definition: feature_definitions().remove(1),
            placement: Some(frame()),
            component: Some(id("product-definition", 1)),
            driven_by: Some(Driver::Feature(feature(9))),
            native_ref: Some("sldprt:feature#1".to_owned()),
        },
        Feature {
            id: feature(2),
            name: None,
            suppressed: None,
            definition: feature_definitions().remove(0),
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        },
    ]
}

pub(super) fn part_numbers() -> Vec<PartNumber> {
    vec![
        PartNumber::Text("PN-100".to_owned()),
        PartNumber::ConfigurationName,
        PartNumber::ParentConfigurationName,
        PartNumber::DocumentName,
    ]
}

pub(super) fn feature_state_overrides() -> Vec<FeatureStateOverride> {
    vec![
        FeatureStateOverride {
            suppressed: Some(true),
            definition: None,
        },
        FeatureStateOverride {
            suppressed: None,
            definition: Some(feature_definitions().remove(0)),
        },
    ]
}

pub(super) fn nested_overrides() -> Vec<NestedOverride> {
    vec![
        NestedOverride {
            path: occurrence_path(),
            transform: Some(Transform::identity()),
            suppressed: Some(false),
            visible: Some(false),
            representation: Some(text("Simplified")),
        },
        NestedOverride {
            path: occurrence_path(),
            transform: None,
            suppressed: None,
            visible: None,
            representation: None,
        },
    ]
}

pub(super) fn occurrence_overrides() -> Vec<OccurrenceOverride> {
    vec![
        OccurrenceOverride {
            suppressed: Some(true),
            visible: Some(false),
            transform: Some(Transform::identity()),
            configuration: Some(ConfigurationRef::Local(id("configuration", 3))),
            prototype: Some(id("product-definition", 3)),
            grounded: Some(true),
            flexible: Some(true),
            representation: Some(text("Master")),
            nested: nested_overrides(),
        },
        OccurrenceOverride::default(),
    ]
}

pub(super) fn definition_overrides() -> Vec<DefinitionOverride> {
    vec![
        DefinitionOverride {
            source_name: Some("bracket-large".to_owned()),
            label: Some("Bracket".to_owned()),
            description: Some("Large bracket".to_owned()),
            part_number: Some(PartNumber::ConfigurationName),
            bom_properties: BTreeMap::from([(text("Title"), "Bracket".to_owned())]),
        },
        DefinitionOverride::default(),
    ]
}

pub(super) fn overrides() -> Vec<Overrides> {
    vec![
        Overrides {
            parameters: BTreeMap::from([(id("parameter", 1), parameter_overrides().remove(1))]),
            features: BTreeMap::from([(feature(1), feature_state_overrides().remove(0))]),
            occurrences: BTreeMap::from([(id("occurrence", 1), occurrence_overrides().remove(0))]),
            definitions: BTreeMap::from([(
                id("product-definition", 1),
                definition_overrides().remove(0),
            )]),
        },
        Overrides::default(),
    ]
}

pub(super) fn material_refs() -> Vec<MaterialRef> {
    vec![
        MaterialRef::Material(id("physical-material", 1)),
        MaterialRef::Named(text("Steel")),
    ]
}

pub(super) fn material_overrides() -> Vec<MaterialOverride> {
    vec![
        MaterialOverride {
            target: Some(selections().remove(0)),
            material: material_refs().remove(0),
        },
        MaterialOverride {
            target: None,
            material: material_refs().remove(1),
        },
    ]
}

pub(super) fn design_configurations() -> Vec<DesignConfiguration> {
    vec![
        DesignConfiguration {
            id: id("configuration", 1),
            scope: id("product-definition", 1),
            name: "Large".to_owned(),
            parent: Some(id("configuration", 2)),
            comment: Some("The large size".to_owned()),
            overrides: overrides().remove(0),
            active_arrangements: BTreeSet::from([id("arrangement", 1)]),
            materials: material_overrides(),
            native_ref: Some("sldprt:configuration#1".to_owned()),
        },
        DesignConfiguration {
            id: id("configuration", 2),
            scope: id("product-definition", 1),
            name: "Default".to_owned(),
            parent: None,
            comment: None,
            overrides: Overrides::default(),
            active_arrangements: BTreeSet::new(),
            materials: Vec::new(),
            native_ref: None,
        },
    ]
}

pub(super) fn publications() -> Vec<Publication> {
    vec![Publication {
        id: id("publication", 1),
        scope: id("product-definition", 1),
        name: text("TopFace"),
        target: picked(1),
    }]
}

pub(super) fn arrangement_kinds() -> Vec<ArrangementKind> {
    vec![
        ArrangementKind::Positional,
        ArrangementKind::Display,
        ArrangementKind::Explode,
        ArrangementKind::Native(text("creo:style_state")),
    ]
}

pub(super) fn arrangement_appearances() -> Vec<ArrangementAppearance> {
    vec![
        ArrangementAppearance {
            path: occurrence_path(),
            appearance: Some(id("appearance", 1)),
            transparency: Some(Fraction::new(0.25).unwrap()),
        },
        ArrangementAppearance {
            path: occurrence_path(),
            appearance: None,
            transparency: None,
        },
    ]
}

pub(super) fn arrangement_moves() -> Vec<ArrangementMove> {
    vec![
        ArrangementMove {
            occurrences: vec![occurrence_path()],
            step: TransformStep::Translate {
                direction: fixtures::direction_constructions().remove(0),
                distance: Dimension::Literal(fixtures::length(25.0)),
            },
        },
        ArrangementMove {
            occurrences: Vec::new(),
            step: TransformStep::Rotate {
                axis: fixtures::constructions::<kinds::Axis>(
                    fixtures::axis_value(),
                    fixtures::axis_value(),
                )
                .remove(0),
                angle: Dimension::Parameter(id("parameter", 1)),
            },
        },
    ]
}

pub(super) fn arrangement_refs() -> Vec<ArrangementRef> {
    vec![
        ArrangementRef::Local(id("arrangement", 1)),
        ArrangementRef::Named(text("Exploded")),
    ]
}

pub(super) fn nested_arrangements() -> Vec<NestedArrangement> {
    arrangement_refs()
        .into_iter()
        .map(|arrangement| NestedArrangement {
            path: occurrence_path(),
            arrangement,
        })
        .collect()
}

pub(super) fn arrangements() -> Vec<Arrangement> {
    vec![
        Arrangement {
            id: id("arrangement", 1),
            scope: id("product-definition", 1),
            name: text("Display State-1"),
            kind: ArrangementKind::Display,
            configuration: Some(id("configuration", 1)),
            overrides: overrides().remove(0),
            appearances: arrangement_appearances(),
            moves: arrangement_moves(),
            nested: nested_arrangements(),
            native_ref: Some("sldprt:display-state#1".to_owned()),
        },
        Arrangement {
            id: id("arrangement", 2),
            scope: id("product-definition", 1),
            name: text("Explode1"),
            kind: ArrangementKind::Explode,
            configuration: None,
            overrides: Overrides::default(),
            appearances: Vec::new(),
            moves: Vec::new(),
            nested: Vec::new(),
            native_ref: None,
        },
    ]
}

pub(super) fn parameter_owners() -> Vec<ParameterOwner> {
    vec![
        ParameterOwner::Feature(feature(1)),
        ParameterOwner::Container(container(1)),
        ParameterOwner::Attribute(id("attribute", 1)),
    ]
}

pub(super) fn parameter_definitions() -> Vec<ParameterDefinition> {
    vec![
        ParameterDefinition::Expression(fixtures::expressions().remove(0)),
        ParameterDefinition::Driven,
        ParameterDefinition::Assigned(id("parameter-program", 1)),
    ]
}

pub(super) fn parameter_values() -> Vec<ParameterValue> {
    vec![
        ParameterValue::Number(real(-2.5)),
        ParameterValue::Integer(-3),
        ParameterValue::Boolean(true),
        ParameterValue::Text("blind".to_owned()),
    ]
}

pub(super) fn dimension_displays() -> Vec<DimensionDisplay> {
    vec![DimensionDisplay::Diameter, DimensionDisplay::Radius]
}

pub(super) fn deviations() -> Deviations {
    Deviations {
        lower: real(-0.1),
        upper: real(0.2),
    }
}

pub(super) fn fit_deviations() -> Vec<FitDeviations> {
    vec![
        FitDeviations::Cell(deviations()),
        FitDeviations::Own(deviations()),
        FitDeviations::Unchecked(deviations()),
    ]
}

pub(super) fn limit_sides() -> Vec<LimitSide> {
    vec![LimitSide::Minimum, LimitSide::Maximum]
}

fn fit(zone: FitZone) -> LimitsAndFits {
    LimitsAndFits {
        form_variance: "H".to_owned(),
        zone,
        grade: "7".to_owned(),
        source: Some(FitStandard::Iso286),
    }
}

pub(super) fn tolerances() -> Vec<Tolerance> {
    vec![
        Tolerance::Deviations(deviations()),
        Tolerance::Fit {
            fit: fit(FitZone::Hole),
            mate: Some(fit(FitZone::Shaft)),
            deviations: Some(FitDeviations::Cell(deviations())),
        },
        Tolerance::Fit {
            fit: fit(FitZone::Hole),
            mate: None,
            deviations: None,
        },
        Tolerance::General,
        Tolerance::Basic,
        Tolerance::SingleLimit(LimitSide::Maximum),
    ]
}

pub(super) fn design_parameters() -> Vec<DesignParameter> {
    vec![
        DesignParameter {
            id: id("parameter", 1),
            scope: id("product-definition", 1),
            owner: Some(ParameterOwner::Feature(feature(1))),
            name: Some(text("D1")),
            kind: ValueKind::Quantity(Quantity::LENGTH),
            unit: Some(fixtures::mm()),
            definition: parameter_definitions().remove(0),
            value: Some(ParameterValue::Number(real(10.0))),
            display: Some(DimensionDisplay::Radius),
            tolerance: Some(Tolerance::Deviations(deviations())),
            decimals: Some(2),
            comment: Some("Depth".to_owned()),
            native_options: native_options(),
            source_settings: NativeOptions::from([(
                crate::codec_name!("f3d:favorite"),
                NativeOption::Bool(Dimension::Literal(true)),
            )]),
            native_ref: Some("sldprt:dimension#1".to_owned()),
        },
        DesignParameter {
            id: id("parameter", 2),
            scope: id("product-definition", 1),
            owner: None,
            name: None,
            kind: ValueKind::Text,
            unit: None,
            definition: ParameterDefinition::Driven,
            value: None,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        },
    ]
}

pub(super) fn program_stages() -> Vec<ProgramStage> {
    vec![
        ProgramStage::Initial,
        ProgramStage::PostRegeneration,
        ProgramStage::Native(text("creo:after_feature")),
    ]
}

pub(super) fn statements() -> Vec<Statement> {
    let value = || Neutral(Node::Reference(parameter_ref(2)));
    vec![
        Statement::Assign {
            target: parameter_ref(1),
            value: value(),
        },
        Statement::If {
            condition: Neutral(Node::Boolean(true)),
            then: vec![Statement::Assign {
                target: parameter_ref(1),
                value: value(),
            }],
            otherwise: Vec::new(),
        },
    ]
}

pub(super) fn program_bodies() -> Vec<ProgramBody> {
    vec![
        ProgramBody::Neutral(statements()),
        ProgramBody::Source {
            dialect: ExpressionDialect::Creo,
            tokens: vec![
                ExpressionToken::Reference(parameter_ref(1)),
                ExpressionToken::Text(" = SOLVE".to_owned()),
            ],
        },
    ]
}

pub(super) fn programs() -> Vec<ParameterProgram> {
    vec![
        ParameterProgram {
            id: id("parameter-program", 1),
            scope: id("product-definition", 1),
            owner: None,
            stage: Some(ProgramStage::PostRegeneration),
            name: Some(text("Relations")),
            body: program_bodies().remove(0),
            native_ref: Some("creo:relations#1".to_owned()),
        },
        ParameterProgram {
            id: id("parameter-program", 2),
            scope: id("product-definition", 1),
            owner: Some(feature(1)),
            stage: None,
            name: None,
            body: program_bodies().remove(1),
            native_ref: None,
        },
    ]
}

pub(super) fn spreadsheets() -> Vec<Spreadsheet> {
    let address = |row, col| CellAddress::new(row, col).unwrap();
    vec![
        Spreadsheet::new(
            id("spreadsheet", 1),
            vec![
                SpreadsheetCell {
                    address: address(2, 2),
                    parameter: id("parameter", 1),
                },
                SpreadsheetCell {
                    address: address(2, 3),
                    parameter: id("parameter", 2),
                },
            ],
            vec![SpreadsheetDimension {
                index: std::num::NonZeroU32::new(28).unwrap(),
                pixels: 120,
            }],
            vec![SpreadsheetDimension {
                index: std::num::NonZeroU32::new(2).unwrap(),
                pixels: 30,
            }],
            vec![SpreadsheetRange::new(address(2, 2), address(3, 2)).unwrap()],
            Some("freecad:Spreadsheet".to_owned()),
        )
        .unwrap(),
        Spreadsheet::new(
            id("spreadsheet", 2),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap(),
    ]
}

pub(super) fn sheet_metal_rules() -> Vec<SheetMetalRule> {
    vec![
        SheetMetalRule {
            id: id("sheet-metal-rule", 1),
            scope: id("product-definition", 1),
            name: Some(text("Default")),
            thickness: Some(Dimension::Parameter(id("parameter", 1))),
            bend: BendSpec {
                radius: Some(Dimension::Literal(positive_length(1.5))),
                radius_side: Some(RadiusSide::Inside),
                relief: None,
            },
            allowance: Some(BendAllowance::KFactor(Dimension::Literal(real(0.44)))),
            native_options: native_options(),
            source_settings: NativeOptions::new(),
            native_ref: Some("f3d:rule#1".to_owned()),
        },
        SheetMetalRule {
            id: id("sheet-metal-rule", 2),
            scope: id("product-definition", 1),
            name: None,
            thickness: None,
            bend: BendSpec {
                radius: None,
                radius_side: None,
                relief: None,
            },
            allowance: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        },
    ]
}

pub(super) fn designs() -> Vec<Design> {
    vec![
        Design::default(),
        Design {
            scopes: design_scopes(),
            containers: containers(),
            features: features(),
            parameters: design_parameters(),
            spreadsheets: spreadsheets(),
            sheet_metal_rules: sheet_metal_rules(),
            programs: programs(),
            configurations: design_configurations(),
            arrangements: arrangements(),
            publications: publications(),
            evaluations: evaluations(),
            names: name_tables().remove(1),
            ..crate::design::ops::tests::sketch::sketch_arenas()
        },
    ]
}
