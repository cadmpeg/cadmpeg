// SPDX-License-Identifier: Apache-2.0
//! Ownership edits update both sides and invalidate evaluated feature outputs.

use crate::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation,
};
use crate::ids::{BodyId, ShellId};
use crate::topology::Shell;

fn feature(id: &str, ordinal: u64, dependencies: Vec<FeatureId>, outputs: Vec<BodyId>) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: DistinctMembers::try_from(dependencies).unwrap(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            DistinctMembers::try_from(outputs).unwrap(),
        ),
        native_ref: None,
    }
}

#[test]
fn moving_a_region_updates_both_lists_and_invalidates_producers_and_consumers() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let original = ir.model.bodies[0].id.clone();
    let mut destination = ir.model.bodies[0].clone();
    destination.id = BodyId::mint("test:model:body#destination").unwrap();
    destination.regions.clear();
    let destination_id = destination.id.clone();
    ir.model.bodies.push(destination);
    let producer = feature("test:model:feature#producer", 0, vec![], vec![original]);
    let consumer = feature(
        "test:model:feature#consumer",
        1,
        vec![producer.id.clone()],
        vec![],
    );
    ir.model.features.extend([producer, consumer]);
    let region = ir.model.regions[0].id.clone();
    ir.reparent_region(&region, &destination_id).unwrap();
    assert!(ir.model.bodies[0].regions.is_empty());
    assert_eq!(ir.model.bodies[1].regions, [region]);
    assert_eq!(ir.model.regions[0].body, destination_id);
    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.needs_recompute));
    let mut findings = Vec::new();
    crate::validate::check_topology_reciprocity(&ir.model, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn an_edit_that_would_empty_a_shell_is_atomic() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let first = ir.model.faces[0].id.clone();
    let remaining = ir.model.faces[1..]
        .iter()
        .map(|face| face.id.clone())
        .collect();
    ir.model.shells[0]
        .replace_faces(vec![first.clone()])
        .unwrap();
    let destination = ShellId::mint("test:model:shell#destination").unwrap();
    ir.model.shells.push(
        Shell::with_faces(
            destination.clone(),
            ir.model.regions[0].id.clone(),
            remaining,
        )
        .unwrap(),
    );
    ir.model.regions[0].shells.push(destination.clone());
    for face in &mut ir.model.faces[1..] {
        face.shell = destination.clone();
    }
    let before = ir.clone();
    let error = ir.reparent_face(&first, &destination).unwrap_err();
    assert!(matches!(error, super::TopologyEditError::Invalid(_)));
    assert_eq!(ir, before);
}
