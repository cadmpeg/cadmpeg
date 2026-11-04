// SPDX-License-Identifier: Apache-2.0
//! Declared quantities compare only with their identified bodies.

use cadmpeg_ir::declared_measures::{
    DeclaredMeasure, DeclaredMeasureProvenance, DeclaredQuantity, DeclaredTarget,
};
use cadmpeg_ir::ids::{BodyId, DeclaredMeasureId, ProductDefinitionId};
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::check::Finding;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;

use crate::test_support::builders::{merged, renamed, service_context};
use crate::{declared_findings, MeasureOptions};

fn declare(ir: &mut CadIr, target: DeclaredTarget, value: f64) {
    ir.model.declared_measures.push(DeclaredMeasure::new(
        DeclaredMeasureId::mint("test:model:declared_measure#volume").expect("identity"),
        target,
        DeclaredQuantity::Volume {
            value: FiniteReal::new(value).expect("finite"),
        },
    ));
}

fn definition(ir: &mut CadIr, bodies: Vec<BodyId>) -> ProductDefinitionId {
    let id = ProductDefinitionId::mint("test:model:product-definition#part").expect("identity");
    ir.model.product_definitions.push(ProductDefinition {
        id: id.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies,
        sketches: Vec::new(),
        native_ref: None,
    });
    id
}

fn run(ir: &CadIr) -> Vec<Finding> {
    let ctx = service_context();
    let found = declared_findings(&ctx, ir, MeasureOptions::default()).expect("findings");
    ctx.finish_session().expect("session");
    found
}

#[test]
fn a_body_target_counts_each_identity_once() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("cube");
    let body = cube.model.bodies[0].id.clone();
    declare(
        &mut cube,
        DeclaredTarget::Bodies {
            bodies: vec![body.clone(), body],
        },
        1000.0,
    );
    assert!(run(&cube).is_empty());
}

#[test]
fn saved_cache_disagreements_warn_while_declarations_and_validation_properties_error() {
    for (provenance, severity) in [
        (
            DeclaredMeasureProvenance::SourceDeclaration,
            Severity::Error,
        ),
        (
            DeclaredMeasureProvenance::ValidationProperty,
            Severity::Error,
        ),
        (DeclaredMeasureProvenance::SavedCache, Severity::Warning),
    ] {
        let mut cube = cadmpeg_ir::examples::unit_cube().expect("cube");
        let body = cube.model.bodies[0].id.clone();
        cube.model.declared_measures.push(
            DeclaredMeasure::new(
                DeclaredMeasureId::mint("test:model:declared_measure#mismatch").expect("identity"),
                DeclaredTarget::Bodies {
                    bodies: vec![body.clone()],
                },
                DeclaredQuantity::Volume {
                    value: FiniteReal::new(900.0).expect("finite"),
                },
            )
            .with_provenance(provenance),
        );

        let found = run(&cube);
        assert_eq!(found.len(), 1, "{provenance:?}");
        assert_eq!(
            found[0].check,
            cadmpeg_ir::report::check::Check::DeclaredMeasure
        );
        assert_eq!(found[0].severity, severity, "{provenance:?}");
        assert_eq!(
            found[0].entity.as_deref(),
            Some("test:model:declared_measure#mismatch")
        );
        assert_eq!(found[0].related, vec![body.to_string()]);
    }
}

#[test]
fn a_definition_without_members_warns_even_when_the_declared_value_is_zero() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("cube");
    let definition = definition(&mut cube, Vec::new());
    declare(&mut cube, DeclaredTarget::Definition { definition }, 0.0);
    let found = run(&cube);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert!(found[0].message.contains("declared.no-members"));
}

#[test]
fn a_part_property_excludes_hidden_bodies_but_a_body_property_can_address_them() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("cube");
    let other = renamed(&cube, "synthetic:cube:", "synthetic:second:");
    let mut both = merged(&cube, &other);
    let hidden = other.model.bodies[0].id.clone();
    both.model
        .bodies
        .iter_mut()
        .find(|body| body.id == hidden)
        .expect("other body")
        .visible = Some(false);
    let bodies = both
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    let definition = definition(&mut both, bodies);
    declare(&mut both, DeclaredTarget::Definition { definition }, 1000.0);
    assert!(run(&both).is_empty());
    both.model.declared_measures[0].target = DeclaredTarget::Bodies {
        bodies: vec![hidden],
    };
    assert!(run(&both).is_empty());
}

#[test]
fn a_part_target_counts_each_owned_body_once() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("cube");
    let body = cube.model.bodies[0].id.clone();
    let definition = definition(&mut cube, vec![body.clone(), body]);
    declare(&mut cube, DeclaredTarget::Definition { definition }, 1000.0);
    assert!(run(&cube).is_empty());
}
