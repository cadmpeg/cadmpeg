// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::design::construction::AxisValue;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::body::{PatternInstances, PatternSeeds};
use cadmpeg_ir::design::ops::shared::{PatternLayout, Spacing};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveAngle;
use cadmpeg_ir::units::UnitVector3;

use super::{assert_no_design_findings, part_with_history, timeline_feature, typed_timeline};
use crate::native::SldprtNative;

/// A part whose history holds `Sketch1` and `CirPattern1`, whose decoded
/// record repeats `Sketch1` six times about the axis through (0, 5, 0) mm
/// along +Z over `angle`.
fn part(angle: f64) -> (CadIr, SldprtNative, FeatureId) {
    let keywords = br#"<Keywords Name="Spin">
        <Feature Name="Front" id="2"/>
        <Feature Name="Top" id="3"/>
        <Feature Name="Right" id="4"/>
        <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
        <Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>
        <Feature Name="CirPattern1" Type="CirPattern" id="30"/>
    </Keywords>"#;
    let mut ir = part_with_history(
        keywords,
        &[
            ("moRefPlane_c", "Front", 2),
            ("moRefPlane_c", "Top", 3),
            ("moRefPlane_c", "Right", 4),
            ("moOriginProfileFeature_c", "Origin", 106),
            ("moProfileFeature_c", "Sketch1", 8),
            ("moCirPattern_c", "CirPattern1", 30),
        ],
    );
    let native = crate::test_support::native::load_under(
        ir.native.namespace("sldprt").unwrap(),
        cadmpeg_core::decode::ResourceLimits::default(),
        ir.tolerances.linear,
    )
    .unwrap();
    let id = |name: &str| {
        ir.model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .id
            .clone()
    };
    let (seed, pattern) = (id("Sketch1"), id("CirPattern1"));
    let definition =
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Feature(seed.clone())],
            pattern: PatternKind::new(PatternTransform::Circular {
                axis_origin: FinitePoint3::new(Point3::new(0.0, 5.0, 0.0)).unwrap(),
                axis_dir: FeatureDirection3::new(Vector3::new(0.0, 0.0, 2.0)).unwrap(),
                angle: PositiveAngle::try_from(angle).unwrap(),
                count: 6,
            })
            .unwrap(),
        });
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == pattern)
        .unwrap();
    feature
        .evaluation
        .edit(|current, _| *current = definition.clone());
    (ir, native, seed)
}

#[test]
fn a_full_turn_circular_pattern_of_features_fits_its_count_in_the_turn() {
    let (mut ir, native, seed) = part(TAU);
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let FeatureOperation::Pattern(pattern) = &timeline_feature(&timeline, "CirPattern1")
        .definition
        .operation
    else {
        panic!("the pattern is typed");
    };
    let PatternSeeds::Features { features, method } = &pattern.seeds else {
        panic!("the pattern repeats features");
    };
    assert_eq!(*method, None);
    let Picks::Named(picks) = &features.picks else {
        panic!("the seeds are named");
    };
    assert!(matches!(picks.as_slice(),
        [pick] if pick.target == PickTarget::FeatureResult(seed.clone())));
    let PatternLayout::Circular(layout) = &pattern.layout else {
        panic!("the layout is circular");
    };
    assert_eq!(
        layout.axis.resolved,
        Some(AxisValue {
            origin: FinitePoint3::new(Point3::new(0.0, 5.0, 0.0)).unwrap(),
            direction: UnitVector3::normalized(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        })
    );
    assert_eq!(
        layout.angular.spacing,
        Spacing::Fit {
            count: Dimension::Literal(6)
        }
    );
    assert_eq!(layout.radial, None);
    assert_eq!(pattern.instances, PatternInstances::Implicit(Vec::new()));
    let definition = ir.design.scopes[0].definition.clone();
    let (parameters, _, _) = entries.parameters(
        &definition,
        &super::super::parameters::Readings {
            decoded: super::super::parameters::Decoded::new(
                &ir.model.parameters,
                &[],
                &crate::resolved_features::assembly::ConfigurationMaps::new(),
            ),
            length_unit: Some(cadmpeg_ir::design::units::UnitSymbol::Mm),
            length_places: None,
            bound: typed.bound,
            measured: typed.measured,
            driven: typed.references,
            manager: std::collections::HashMap::new(),
            confirmed: std::collections::HashSet::new(),
        },
    );
    ir.design.features = timeline.features;
    ir.design.parameters = parameters;
    ir.design.sketches = typed.sketches.records;
    ir.design.sketch_entities = typed.sketches.entities;
    ir.design.sketch_constraints = typed.sketches.constraints;
    assert_no_design_findings(&ir);
}

#[test]
fn a_partial_turn_circular_pattern_stays_unresolved() {
    let (ir, native, _) = part(PI);
    let (timeline, _, _) = typed_timeline(&ir, &native);
    assert_eq!(
        timeline_feature(&timeline, "CirPattern1")
            .definition
            .operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Pattern
        }
    );
}
