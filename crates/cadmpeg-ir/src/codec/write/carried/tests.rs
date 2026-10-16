// SPDX-License-Identifier: Apache-2.0

use super::{EntityLocation, FindingLocator, FindingScope};
use crate::examples::unit_cube;
use crate::ids::Identity;
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::schema::{EntityKind, EntitySchema};
use crate::{CadIr, NativeRecord};

const NATIVE_ID: &str = "f3d:test:asm-history#0";

fn finding(entity: Option<&str>, related: &[&str], message: &str) -> Finding {
    Finding {
        related: related.iter().map(|id| (*id).to_owned()).collect(),
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        entity: entity.map(str::to_owned),
        message: message.into(),
    }
}

fn insert_native(ir: &mut CadIr, format: &str, ids: &[&str]) {
    ir.native.namespace_mut(format).arenas_mut().insert(
        "asm_histories".into(),
        ids.iter()
            .map(|id| {
                NativeRecord::new(
                    Identity::new(*id).expect("valid identity"),
                    serde_json::Map::default(),
                )
                .expect("valid native identity")
            })
            .collect(),
    );
    ir.finalize();
}

#[test]
fn no_entity_is_unlocated_even_with_a_resolving_related_id() {
    let ir = unit_cube().expect("valid cube");
    let finding = finding(None, &[ir.model.faces[0].identity()], "invalid reference");
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(locator.scope(&finding), FindingScope::Unlocated);
}

#[test]
fn absent_entity_is_unlocated_even_with_a_resolving_related_id() {
    let ir = unit_cube().expect("valid cube");
    let finding = finding(
        Some("test:model:face#missing"),
        &[ir.model.faces[0].identity()],
        "invalid reference",
    );
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(locator.scope(&finding), FindingScope::Unlocated);
}

#[test]
fn model_entity_resolves_to_its_arena() {
    let ir = unit_cube().expect("valid cube");
    let finding = finding(Some(ir.model.faces[0].identity()), &[], "invalid face");
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![EntityLocation::Model(EntityKind::Face)])
    );
}

#[test]
fn native_entity_resolves_to_its_namespace() {
    let mut ir = CadIr::empty();
    insert_native(&mut ir, "f3d", &[NATIVE_ID]);
    let finding = finding(Some(NATIVE_ID), &[], "invalid record");
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![EntityLocation::Native("f3d")])
    );
}

#[test]
fn native_related_id_resolves_a_face() {
    let mut ir = unit_cube().expect("valid cube");
    insert_native(&mut ir, "f3d", &[NATIVE_ID]);
    let finding = finding(
        Some(NATIVE_ID),
        &[ir.model.faces[0].identity()],
        &format!("invalid reference `{}`.", ir.model.faces[0].identity()),
    );
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![
            EntityLocation::Native("f3d"),
            EntityLocation::Model(EntityKind::Face),
        ])
    );
}

#[test]
fn unresolved_related_ids_are_ignored() {
    let ir = unit_cube().expect("valid cube");
    let finding = finding(
        Some(ir.model.faces[0].identity()),
        &["test:model:surface#missing"],
        "unresolved typed reference test:model:surface#missing ordinary:word #fragment",
    );
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![EntityLocation::Model(EntityKind::Face)])
    );
}

#[test]
fn duplicate_identities_keep_each_distinct_location_once() {
    let mut ir = unit_cube().expect("valid cube");
    let face = ir.model.faces[0].clone();
    let id = face.identity().to_owned();
    ir.model.faces.push(face);
    insert_native(&mut ir, "f3d", &[&id, &id]);
    insert_native(&mut ir, "step", &[&id]);
    let finding = finding(Some(&id), &[&id, &id], &format!("{id} `{id}`."));
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![
            EntityLocation::Model(EntityKind::Face),
            EntityLocation::Native("f3d"),
            EntityLocation::Native("step"),
        ])
    );
}

#[test]
fn distinct_related_ids_resolve_exactly() {
    let mut ir = CadIr::empty();
    insert_native(&mut ir, "f3d", &[NATIVE_ID, "test:native:record#target."]);
    insert_native(&mut ir, "step", &["test:native:record#target"]);
    let finding = finding(
        Some(NATIVE_ID),
        &["test:native:record#target.", "test:native:record#target"],
        "test:native:record#target.",
    );
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![
            EntityLocation::Native("f3d"),
            EntityLocation::Native("step"),
        ])
    );
}

#[test]
fn multiple_findings_share_the_index_and_keep_related_order() {
    let mut ir = unit_cube().expect("valid cube");
    insert_native(&mut ir, "f3d", &[NATIVE_ID]);
    let face = ir.model.faces[0].identity();
    let edge = ir.model.edges[0].identity();
    let findings = [
        finding(Some(face), &[], ""),
        finding(
            Some(NATIVE_ID),
            &[edge, face, edge],
            &format!("{edge} {face} {edge}"),
        ),
    ];
    let locator = FindingLocator::new(&ir, &findings);
    assert_eq!(
        locator.scope(&findings[0]),
        FindingScope::Located(vec![EntityLocation::Model(EntityKind::Face)])
    );
    assert_eq!(
        locator.scope(&findings[1]),
        FindingScope::Located(vec![
            EntityLocation::Native("f3d"),
            EntityLocation::Model(EntityKind::Edge),
            EntityLocation::Model(EntityKind::Face),
        ])
    );
}

#[test]
fn message_only_identity_is_not_located() {
    let mut ir = unit_cube().expect("valid cube");
    insert_native(&mut ir, "f3d", &[NATIVE_ID]);
    let finding = finding(Some(NATIVE_ID), &[], ir.model.faces[0].identity());
    let locator = FindingLocator::new(&ir, [&finding]);
    assert_eq!(
        locator.scope(&finding),
        FindingScope::Located(vec![EntityLocation::Native("f3d")])
    );
}
