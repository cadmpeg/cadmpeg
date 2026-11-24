// SPDX-License-Identifier: Apache-2.0
//! The root definition a decoder states for what its file holds at its top.

use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::document::CadIr;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::products::{ExternalDocument, InstanceBodies, ProductDefinition, Storage};
use crate::topology::{Body, BodyKind};

fn body(id: &str) -> Body {
    Body {
        unstated: None,
        id: id.try_into().unwrap(),
        kind: BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    }
}

fn mesh(id: &str, body: Option<BodyId>) -> crate::tessellation::Tessellation {
    use crate::math::Point3;
    use crate::tessellation::{Tessellation, TessellationMesh};

    Tessellation::new(
        id.try_into().unwrap(),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap()
    .with_body(body)
}

fn root_id() -> ProductDefinitionId {
    "test:model:product-definition#root".try_into().unwrap()
}

#[test]
fn a_geometry_document_gains_a_root_that_owns_its_top() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = CadIr::empty();
    ir.geometry
        .bodies
        .extend([body("test:model:body#a"), body("test:model:body#b")]);
    ir.model.tessellations.extend([
        mesh("test:model:tessellation#mesh", None),
        mesh(
            "test:model:tessellation#shaded",
            Some("test:model:body#a".try_into().unwrap()),
        ),
    ]);
    let root = ir
        .state_root_definition(&ctx, root_id(), ExternalDocument::document_id("uid"))
        .unwrap()
        .unwrap();
    assert!(root.is_root());
    assert_eq!(
        root.bodies.iter().map(BodyId::as_str).collect::<Vec<_>>(),
        ["test:model:body#a", "test:model:body#b"]
    );
    assert_eq!(
        root.tessellations
            .iter()
            .map(crate::tessellation::TessellationId::as_str)
            .collect::<Vec<_>>(),
        ["test:model:tessellation#mesh"]
    );
    assert_eq!(
        root.storage,
        Storage::File {
            document: ExternalDocument::document_id("uid"),
            source: None,
        }
    );

    // A second call adds nothing.
    let again = ir.clone();
    ir.state_root_definition(&ctx, root_id(), ExternalDocument::default())
        .unwrap();
    assert_eq!(ir, again);
}

#[test]
fn a_stated_root_is_kept_and_owns_only_what_no_definition_holds() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = CadIr::empty();
    ir.geometry.bodies.extend([
        body("test:model:body#part"),
        body("test:model:body#cut"),
        body("test:model:body#top"),
    ]);
    let mut part = ProductDefinition::new(
        "test:model:product-definition#part".try_into().unwrap(),
        Storage::Embedded,
    );
    part.bodies.push("test:model:body#part".try_into().unwrap());
    let mut assembly = ProductDefinition::root(
        "test:model:product-definition#assembly".try_into().unwrap(),
        ExternalDocument::default(),
    );
    assembly.instance_bodies.push(InstanceBodies {
        occurrence: OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence("test:model:occurrence#part".try_into().unwrap()),
            configuration: None,
        }]),
        bodies: vec!["test:model:body#cut".try_into().unwrap()],
    });
    ir.model.product_definitions.extend([part, assembly]);
    let root = ir
        .state_root_definition(&ctx, root_id(), ExternalDocument::default())
        .unwrap()
        .unwrap();
    assert_eq!(root.id.as_str(), "test:model:product-definition#assembly");
    assert_eq!(
        root.bodies.iter().map(BodyId::as_str).collect::<Vec<_>>(),
        ["test:model:body#top"]
    );
    assert_eq!(ir.model.product_definitions.len(), 2);
}

#[test]
fn a_new_root_is_charged_as_one_entity() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let refused = ir
        .state_root_definition(&ctx, root_id(), ExternalDocument::default())
        .unwrap_err();
    assert!(
        matches!(&refused, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities),
        "{refused:?}"
    );
    assert!(ir.model.product_definitions.is_empty());

    ir.model.product_definitions.push(ProductDefinition::root(
        root_id(),
        ExternalDocument::default(),
    ));
    ir.state_root_definition(&ctx, root_id(), ExternalDocument::default())
        .unwrap();
    assert_eq!(ir.model.product_definitions.len(), 1);
}

#[test]
fn only_a_bare_root_is_the_document_itself() {
    let mut root = ProductDefinition::root(root_id(), ExternalDocument::default());
    root.kind = Some(crate::products::ProductDefinitionKind::Part);
    root.bodies.push("test:model:body#a".try_into().unwrap());
    assert!(root.is_document_root());
    let definitions = [root.clone()];
    assert_eq!(crate::products::stated_definitions(&definitions).count(), 0);

    let mut named = root.clone();
    named.part_number = Some("P-1".to_owned());
    let mut identified = root.clone();
    identified.storage = Storage::File {
        document: ExternalDocument::document_id("uid"),
        source: None,
    };
    let mut assembly = root;
    assembly
        .occurrences
        .push("test:model:occurrence#a".try_into().unwrap());
    for definition in [named, identified, assembly] {
        assert!(!definition.is_document_root(), "{definition:?}");
    }
}
