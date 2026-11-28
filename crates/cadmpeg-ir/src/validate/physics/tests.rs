// SPDX-License-Identifier: Apache-2.0

use crate::examples::unit_cube;
use crate::ids::{ColliderId, PhysicalMaterialId};
use crate::physics::{Collider, ColliderShape, PhysicsTarget};
use crate::report::check::Check;
use crate::validate::validate_neutral;

fn referential_findings(ir: &crate::CadIr) -> Vec<String> {
    validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::ReferentialIntegrity)
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn a_collider_names_an_existing_body_and_material() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let body = ir.geometry.bodies[0].id.clone();
    ir.model.colliders.push(Collider {
        id: ColliderId::mint("test:model:collider#0").unwrap(),
        target: PhysicsTarget::Body { body }.into(),
        shape: ColliderShape::ConvexHull,
        enabled: true,
        material: None,
        native_ref: None,
    });
    assert!(referential_findings(&ir).is_empty());
    ir.model.colliders[0].material =
        Some(PhysicalMaterialId::mint("test:model:material#0").unwrap());
    ir.model.colliders[0].target = PhysicsTarget::Body {
        body: crate::ids::BodyId::mint("test:model:body#missing").unwrap(),
    }
    .into();
    let findings = referential_findings(&ir);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings
        .iter()
        .any(|message| message.contains("physical material")));
    assert!(findings.iter().any(|message| message.contains("body")));
}

#[test]
fn a_solver_setting_names_a_joint_feature() {
    use crate::design::feature::OperationFamily;
    use crate::physics::solver::{SolverSettings, SolverTarget};
    use crate::validate::design::fixture::{feature, unresolved};

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.design.features = vec![
        unresolved("hinge", OperationFamily::Joint),
        unresolved("sketch", OperationFamily::Sketch),
    ];
    let settings = |key: &str| SolverSettings {
        id: crate::ids::SolverSettingsId::mint(format!("test:model:solver#{key}")).unwrap(),
        solver: "physx".to_owned(),
        schema: "PhysxJointAPI".to_owned(),
        target: SolverTarget::Joint {
            joint: feature(key),
        }
        .into(),
        values: std::collections::BTreeMap::new(),
        native_ref: None,
    };
    ir.model.solver_settings = vec![settings("hinge"), settings("missing"), settings("sketch")];
    let findings: Vec<String> = referential_findings(&ir)
        .into_iter()
        .filter(|message| message.contains("joint feature"))
        .collect();
    assert_eq!(
        findings,
        [
            "references missing joint feature `test:design:feature#missing`",
            "references missing joint feature `test:design:feature#sketch`",
        ],
    );
}
