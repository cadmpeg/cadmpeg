// SPDX-License-Identifier: Apache-2.0
//! The change between two stored states of one blob, and the evaluations
//! and losses of the timeline items of a scope.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::design::configuration::{DesignConfiguration, Overrides};
use cadmpeg_ir::design::evaluation::{EvaluationStatus, StateDelta};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::{PointId, ProductDefinitionId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Point;

use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignScopeTailFlags;
use crate::test_support::zip_test::f3d_with_smbh;

use super::{project, step, Scope, Tail, Tails};

fn point(name: &str, x: f64) -> Point {
    Point::new(
        PointId::mint(format!("f3d:brep:entity#{name}")).expect("identity grammar"),
        FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("a finite point"),
        None,
    )
}

fn points(points: Vec<Point>) -> Topology {
    Topology {
        points,
        ..Topology::default()
    }
}

#[test]
fn a_step_adds_the_changed_and_new_records_and_removes_the_absent_ones() {
    let before = points(vec![point("1", 0.0), point("2", 1.0), point("4", 4.0)]);
    let after = points(vec![point("1", 0.0), point("2", 2.0), point("3", 3.0)]);
    let head = points(vec![point("3", 3.0)]);
    let current = points(vec![point("3", 5.0)]);
    let mut delta = StateDelta::default();
    step(&before, &after, &head, &current, &mut delta).expect("the step");
    assert_eq!(delta.records.points, [point("2", 2.0), point("3", 5.0)]);
    assert_eq!(delta.removed, [RecordRef::Point(point("4", 0.0).id)]);
}

#[test]
fn a_step_between_equal_states_is_empty() {
    let state = points(vec![point("1", 0.0), point("2", 1.0)]);
    let mut delta = StateDelta::default();
    step(&state, &state, &state, &Topology::default(), &mut delta).expect("the step");
    assert_eq!(delta, StateDelta::default());
}

/// The Design stream of the timeline items of [`timeline`].
const STREAM: &str = "f3d:timeline";

fn feature_id(name: &str) -> FeatureId {
    FeatureId::mint(format!("test:design:feature#{name}")).expect("identity grammar")
}

/// A root scope whose timeline holds the unresolved extrudes `first` (record
/// 10) and `second` (record 11), with its current state after `at`.
fn timeline(at: &str) -> (CadIr, Scope) {
    let mut ir = CadIr::empty();
    let mut members = Vec::new();
    let mut items = BTreeMap::new();
    for (record, name) in [(10, "first"), (11, "second")] {
        let id = feature_id(name);
        ir.design.features.push(Feature {
            id: id.clone(),
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: FeatureOperation::Unresolved {
                    family: OperationFamily::Extrude,
                },
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
        members.push(Member::Feature(id.clone()));
        items.insert(id, (STREAM.to_owned(), record));
    }
    let scope = Scope {
        definition: ProductDefinitionId::mint("test:design:definition#root")
            .expect("identity grammar"),
        at: Position::After(Member::Feature(feature_id(at))),
        bodies: BTreeSet::new(),
        members,
        items,
    };
    (ir, scope)
}

/// The tail of the item `record` that stores `state` and the no-result flag
/// `s0` of a rolled-back item.
fn rolled_back(record: u64, state: i64) -> Tails<'static> {
    let flags = DesignScopeTailFlags {
        suppression: [false; 2],
        unevaluated: false,
        wide_state: None,
        state_offset: 0,
        no_result: true,
        rolled_back: true,
        no_result_offset: 0,
        envelope_offset: None,
    };
    Tails::from([(
        (STREAM, record),
        Tail {
            state: Some(state),
            flags: Some(flags),
        },
    )])
}

/// The losses of the states of `scope` in an archive with no model blob,
/// after the evaluations go to `ir`.
fn project_without_blobs(ir: &mut CadIr, scope: &Scope, tails: &Tails<'_>) -> Vec<LossNote> {
    let bytes = f3d_with_smbh(&[]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("the archive opens");
    let scan = crate::container::scan(&ctx, root).expect("the archive scans");
    let mut losses = Vec::new();
    project(&ctx, ir, &scan, scope, tails, &mut losses).expect("the states project");
    losses
}

#[test]
fn a_built_item_with_no_state_id_charges_a_loss_and_has_no_evaluation() {
    let (mut ir, scope) = timeline("second");
    let losses = project_without_blobs(&mut ir, &scope, &Tails::new());
    assert!(ir.design.evaluations.is_empty());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        losses[0].message.contains(feature_id("first").as_str()),
        "{}",
        losses[0].message
    );
}

#[test]
fn a_rolled_back_item_whose_state_is_not_stored_has_a_stale_evaluation_with_no_content() {
    let (mut ir, scope) = timeline("first");
    let losses = project_without_blobs(&mut ir, &scope, &rolled_back(11, 7));
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", ir.design.evaluations);
    };
    assert_eq!(evaluation.status, EvaluationStatus::Stale);
    assert_eq!(
        evaluation.of,
        Evaluated::State(Position::After(Member::Feature(feature_id("second"))))
    );
    assert!(evaluation.content.is_none());
    assert!(evaluation.native_ref.is_none());
    assert_eq!(evaluation.scope, scope.definition);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        losses[0].message.contains(feature_id("second").as_str()),
        "{}",
        losses[0].message
    );
}

#[test]
fn a_scope_with_configurations_and_no_active_one_states_no_evaluation() {
    let (mut ir, scope) = timeline("first");
    ir.design.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("test:design:configuration#large").expect("identity grammar"),
        scope: scope.definition.clone(),
        name: "Large".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    let losses = project_without_blobs(&mut ir, &scope, &rolled_back(11, 7));
    assert!(ir.design.evaluations.is_empty());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].code, F3dLossCode::HistoryStatesUnread.kind());
    assert!(
        losses[0].message.contains(scope.definition.as_str()),
        "{}",
        losses[0].message
    );
}
