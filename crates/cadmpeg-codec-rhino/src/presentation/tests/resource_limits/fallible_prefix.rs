// SPDX-License-Identifier: Apache-2.0
//! Presentation installation admits each next object, table, record and layer.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::container::{Record, Table};
use crate::objects::{AttributeState, ObjectRecord};
use crate::wire::Uuid;

fn empty_scan() -> crate::container::Scan<'static> {
    let mut scan = super::presentation_install_scan(super::InstallFixture::Full);
    scan.tables.clear();
    scan.objects.clear();
    scan.metadata.layers.clear();
    scan
}

fn table(records: Vec<Record>) -> Table {
    let count = records.len();
    Table::new(0x1000_0999, 0..8, 0..0, records, count,
        std::collections::BTreeMap::new()).unwrap()
}

fn assert_visit_refusal(scan: crate::container::Scan<'_>, preceding_visits: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = preceding_visits;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let error = crate::presentation::install(&ctx, &scan, &mut ir).unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else { panic!("installation visit refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino install traversal");
    assert_eq!((refusal.used, refusal.additional), (preceding_visits, 1));
    assert!(ir.native.namespace("rhino").is_none());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn presentation_object_identity_pass_refuses_only_the_first_visit() {
    let mut scan = empty_scan();
    scan.objects = vec![ObjectRecord::Degraded { range: 0..0, warning: String::new() }; 1024];
    assert_visit_refusal(scan, 0);
}

#[test]
fn presentation_object_projection_pass_refuses_only_the_first_visit() {
    let mut scan = empty_scan();
    scan.objects = vec![ObjectRecord::Degraded { range: 0..0, warning: String::new() }; 1024];
    // The identity pass visits all degraded objects; none has an identity to index.
    assert_visit_refusal(scan, 1024);
}

#[test]
fn presentation_table_pass_refuses_only_the_first_visit() {
    let mut scan = empty_scan();
    scan.tables = vec![table(Vec::new()); 1024];
    assert_visit_refusal(scan, 0);
}

#[test]
fn presentation_record_pass_refuses_only_the_first_visit() {
    let mut scan = empty_scan();
    scan.tables = vec![table(vec![Record::short(0x1234, 0..0, 0); 1024])];
    assert_visit_refusal(scan, 1);
}

fn scan_with_unidentified_layers() -> crate::container::Scan<'static> {
    let mut scan = super::presentation_install_scan(super::InstallFixture::LayersOnly);
    scan.tables.clear();
    let mut layer = scan.metadata.layers.remove(0);
    layer.id = None;
    scan.metadata.layers = vec![layer; 1024];
    scan
}

#[test]
fn presentation_layer_identity_pass_refuses_only_the_first_visit() {
    assert_visit_refusal(scan_with_unidentified_layers(), 0);
}

#[test]
fn presentation_layer_projection_pass_refuses_only_the_first_visit() {
    // No source UUID is present, so the first layer pass performs no hash work.
    assert_visit_refusal(scan_with_unidentified_layers(), 1024);
}

#[test]
fn presentation_group_membership_pass_refuses_only_the_first_visit() {
    let mut scan = super::presentation_install_scan(super::InstallFixture::Full);
    scan.tables.clear();
    scan.metadata.layers.clear();
    assert_eq!(scan.objects.len(), 1);
    let ObjectRecord::Framed(object) = &mut scan.objects[0] else { panic!("framed fixture object"); };
    let AttributeState::Parsed(attributes) = &mut object.attributes else { panic!("parsed fixture attributes"); };
    attributes.groups = vec![7; 1024];
    // One identity visit, the initial hash storage bound, two UUID hashes,
    // then one object projection visit. The empty group index adds no work.
    let first_growth = 4 * std::mem::size_of::<(Uuid, usize)>()
        + std::mem::align_of::<(Uuid, usize)>().max(16) - 1 + 4 + 16;
    let preceding = u64::try_from(1 + first_growth + 2 * std::mem::size_of::<Uuid>() + 1).unwrap();
    assert_visit_refusal(scan, preceding);
}
