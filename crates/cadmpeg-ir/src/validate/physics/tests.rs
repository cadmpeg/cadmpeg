// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

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
    let body = ir.model.bodies[0].id.clone();
    ir.model.colliders.push(Collider {
        id: ColliderId::mint("test:model:collider#0").unwrap(),
        target: PhysicsTarget::Body { body },
        shape: ColliderShape::ConvexHull {},
        enabled: true,
        material: None,
        native_ref: None,
    });
    assert!(referential_findings(&ir).is_empty());
    ir.model.colliders[0].material =
        Some(PhysicalMaterialId::mint("test:model:material#0").unwrap());
    ir.model.colliders[0].target = PhysicsTarget::Body {
        body: crate::ids::BodyId::mint("test:model:body#missing").unwrap(),
    };
    let findings = referential_findings(&ir);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings
        .iter()
        .any(|message| message.contains("physical material")));
    assert!(findings.iter().any(|message| message.contains("body")));
}
