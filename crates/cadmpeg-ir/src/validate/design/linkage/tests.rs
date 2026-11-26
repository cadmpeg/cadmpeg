// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use serde_json::Map;

use crate::design::construction::{AxisValue, Construction};
use crate::design::container::Container;
use crate::design::feature::{Feature, FeatureOperation, Link, OperationFamily};
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::refs::Member;
use crate::document::{ArenaName, CadIr, ScopeViolation};
use crate::features::geometry::FinitePoint3;
use crate::ids::Identity;
use crate::math::Point3;
use crate::report::Severity;
use crate::units::UnitVector3;
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, configuration, design_findings, document, feature,
    id, unresolved,
};
use crate::NativeRecord;

#[test]
fn the_baseline_document_breaks_no_rule() {
    let ir = document();
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

/// The rules of the scope findings.
const SCOPE_RULES: [&str; 3] = [
    "design.scope-holds-class-without-required-class",
    "design.scope-excluded-arena-holds-records",
    "design.scope-excluded-class-referenced-another-arena",
];

/// The scope findings of `ir`, as rule and message.
fn scope_findings(ir: &CadIr) -> Vec<(String, String)> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| SCOPE_RULES.contains(&finding.rule.as_str()))
        .inspect(|finding| assert_eq!(finding.severity, Severity::Error))
        .map(|finding| (finding.rule.as_str().to_owned(), finding.message))
        .collect()
}

/// One finding per violation, under the rule of its variant.
fn reported(violations: &[ScopeViolation]) -> Vec<(String, String)> {
    violations
        .iter()
        .map(|violation| {
            let rule = match violation {
                ScopeViolation::Unmet { .. } => SCOPE_RULES[0],
                ScopeViolation::Populated { .. } => SCOPE_RULES[1],
                ScopeViolation::Referenced { .. } => SCOPE_RULES[2],
            };
            (rule.to_owned(), violation.to_string())
        })
        .collect()
}

#[test]
fn a_scope_that_holds_a_class_without_its_requirement_is_one_finding() {
    let mut ir = CadIr::empty();
    ir.set_scope(ContentScope::container().with(ArenaClass::Design, true));
    let violations = ir.scope_violations();
    assert_eq!(
        violations,
        [ScopeViolation::Unmet {
            held: ArenaClass::Design,
            required: ArenaClass::Model,
        }]
    );
    assert_eq!(scope_findings(&ir), reported(&violations));
}

#[test]
fn each_breach_of_the_scope_is_its_own_finding() {
    let mut ir = document();
    ir.model
        .solver_settings
        .push(crate::physics::solver::SolverSettings {
            id: crate::ids::SolverSettingsId::mint("test:model:solver#hinge").unwrap(),
            solver: "physx".to_owned(),
            schema: "PhysxJointAPI".to_owned(),
            target: crate::physics::solver::SolverTarget::Joint {
                joint: id("feature", "hinge"),
            }
            .into(),
            values: BTreeMap::new(),
            native_ref: None,
        });
    ir.set_scope(ContentScope::geometry());
    let violations = ir.scope_violations();
    let mut expected = populated(&[
        "design.scopes",
        "design.containers",
        "design.features",
        "design.configurations",
        "design.evaluations",
    ]);
    expected.extend(referenced(ArenaClass::Design, &["solver_settings"]));
    assert_eq!(violations, expected);
    assert_eq!(scope_findings(&ir), reported(&violations));

    ir.set_scope(ContentScope::container().with(ArenaClass::Design, true));
    let violations = ir.scope_violations();
    let mut expected = vec![ScopeViolation::Unmet {
        held: ArenaClass::Design,
        required: ArenaClass::Model,
    }];
    expected.extend(populated(&[
        "bodies",
        "regions",
        "shells",
        "faces",
        "loops",
        "coedges",
        "edges",
        "vertices",
        "points",
        "surfaces",
        "curves",
        "product_definitions",
        "solver_settings",
    ]));
    expected.extend(referenced(
        ArenaClass::Model,
        &[
            "design.scopes",
            "design.configurations",
            "design.evaluations",
        ],
    ));
    assert_eq!(violations, expected);
    assert_eq!(scope_findings(&ir), reported(&violations));
}

/// A `Populated` violation for each of `arenas`.
fn populated(arenas: &[&str]) -> Vec<ScopeViolation> {
    arenas
        .iter()
        .map(|arena| ScopeViolation::Populated {
            arena: ArenaName::parse(arena).unwrap(),
        })
        .collect()
}

/// A `Referenced` violation of `class` for each of `arenas`.
fn referenced(class: ArenaClass, arenas: &[&str]) -> Vec<ScopeViolation> {
    arenas
        .iter()
        .map(|arena| ScopeViolation::Referenced {
            class,
            arena: ArenaName::parse(arena).unwrap(),
        })
        .collect()
}

#[test]
fn design_identities_are_unique() {
    let mut ir = document();
    ir.design
        .features
        .push(unresolved("fillet", OperationFamily::Fillet));
    assert_finding(&ir, "is not unique among the design records");
}

#[test]
fn a_design_record_shares_no_id_with_a_model_record() {
    let mut ir = document();
    let mut shared = configuration("shared", None);
    shared.id = id("definition", "part");
    ir.design.configurations.push(shared);
    assert_finding(
        &ir,
        "design.configurations record id is also the id of a product_definitions record",
    );
}

/// A datum axis feature whose record and link name native records.
fn linked_datum(record: &str, link: &str) -> Feature {
    let mut datum = unresolved("datum", OperationFamily::Datum);
    datum.native_ref = Some(record.to_owned());
    datum.definition.operation = FeatureOperation::Datum(Box::new(Datum {
        element: DatumElement::Axis(Construction {
            inputs: Vec::new(),
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: Some(AxisValue {
                origin: FinitePoint3::new(Point3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                })
                .unwrap(),
                direction: UnitVector3::Z_AXIS,
            }),
            resolved_in: BTreeMap::new(),
        }),
        link: Some(Link {
            source: None,
            document: None,
            transform: None,
            scale: None,
            associative: None,
            state: None,
            native_ref: Some(link.to_owned()),
        }),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    datum
}

#[test]
fn a_datum_link_native_ref_resolves() {
    let mut ir = document();
    ir.native.namespace_mut(crate::codec_id!("sldprt"));
    ir.design.features.push(linked_datum(
        "sldprt:feature#missing",
        "sldprt:link#missing",
    ));
    assert_finding(&ir, "native_ref `sldprt:link#missing` does not resolve");

    let (record, link) = ("sldprt:native:feature#0", "sldprt:native:link#0");
    ir.native
        .namespace_mut(crate::codec_id!("sldprt"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("records"),
            [record, link]
                .into_iter()
                .map(|id| NativeRecord::new(Identity::new(id).unwrap(), Map::new()).unwrap())
                .collect(),
        );
    ir.native.finalize();
    ir.design.features.pop();
    ir.design.features.push(linked_datum(record, link));
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.insert(0, Member::Feature(feature("datum")));
    assert_no_design_finding(&ir);
}
