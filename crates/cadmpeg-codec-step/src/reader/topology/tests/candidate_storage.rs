// SPDX-License-Identifier: Apache-2.0
//! Candidate child storage follows rejection and model installation.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::super::{BuildSources, FaceAttributeCache, WireScope, WireSources};

fn exchange(records: &str) -> crate::parse::Exchange {
    let input = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(input.as_bytes(), crate::parse::parse_inner)
        .expect("candidate source parses").0
}

fn retained_bytes(ctx: &DecodeContext<'_>) -> u64 {
    let CodecError::ResourceLimit(limit) = ctx
        .charge_retained(u64::MAX, "candidate retained storage probe")
        .expect_err("probe retained bytes")
    else { panic!("retained refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    limit.used
}

#[test]
fn repeated_rejected_shell_candidates_release_identity_and_region_storage() {
    let exchange = exchange("#1=SHELL_BASED_SURFACE_MODEL('',(#2));#2=OPEN_SHELL('',(#3));#3=DUMMY();");
    let setup = cadmpeg_test_support::service_decode_context();
    let shells = super::super::shell_defs(&exchange, &setup).expect("shell definitions");
    let ir = CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("empty carriers");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = FaceAttributeCache::new(&ctx).expect("face cache");
    let mut storage = ctx.reserve_scoped(0, "test losses").expect("loss slots");
    let mut losses = Vec::new();
    for _ in 0..64 {
        let outcome = super::super::build(
            1, exchange.records().get(&1).expect("root"),
            BuildSources {
                exchange: &exchange, ir: &ir, vdefs: &BTreeMap::new(),
                edefs: &BTreeMap::new(), odefs: &BTreeMap::new(),
                shell_definitions: &shells, decoded_pcurves: &BTreeSet::new(),
                point_positions: &carriers, ctx: &ctx,
            }, &mut cache, false, (&mut losses, &mut storage),
        ).expect("recoverable shell rejection");
        let (built, failures, owner) = outcome.into_parts();
        assert!(built.is_empty());
        assert_eq!(failures.expect("rejection").first.expect("carrier failure").record_id, 3);
        drop((built, owner));
    }
    assert!(losses.is_empty());
    drop((losses, storage, cache));
    assert_eq!(retained_bytes(&ctx), 0);
}

fn rejected_wire_payload(shell: bool) {
    let records = if shell {
        "#1=WIRE_SHELL('',(#2));#2=EDGE_LOOP('',(#3));#3=ORIENTED_EDGE('',*,*,#4,.T.);#4=EDGE('',#5,#6);#5=DUMMY();#6=DUMMY();"
    } else {
        "#1=CONNECTED_EDGE_SET('',(#4));#4=EDGE('',#5,#6);#5=DUMMY();#6=DUMMY();"
    };
    let exchange = exchange(records);
    let setup = cadmpeg_test_support::service_decode_context();
    let edefs = super::super::edge_defs(&exchange, &setup).expect("edge definitions");
    let ir = CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("empty carriers");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut storage = ctx.reserve_scoped(0, "test losses").expect("loss slots");
    let mut losses = Vec::new();
    let sources = WireSources { vdefs: &BTreeMap::new(), edefs: &edefs, point_positions: &carriers };
    let built = if shell {
        super::super::build_shell_wire_set(10, 1, &exchange, sources,
            WireScope { scoped: false, root: false }, (&mut losses, &mut storage), &ctx)
    } else {
        super::super::build_wire_set(10, 1, &exchange, sources, false,
            (&mut losses, &mut storage), &ctx)
    }.expect("missing vertices reject the wire");
    assert!(built.is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].message,
        "STEP edge #4 has no 3D curve carrier; edge committed without a curve");
    let expected = u64::try_from(losses[0].message.len()).expect("warning bytes fit");
    assert_eq!(retained_bytes(&ctx), expected);
    drop((losses, storage));
}

#[test]
fn rejected_connected_wire_releases_edges_but_retains_warning_text() {
    rejected_wire_payload(false);
}

#[test]
fn rejected_shell_wire_releases_edges_but_retains_warning_text() {
    rejected_wire_payload(true);
}

fn sheet_source(name: &str, rejected: bool) -> String {
    let mut source = std::str::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
        .expect("sheet fixture UTF-8")
        .replace("#29=ADVANCED_FACE('',(#26),#28,.T.);",
            &format!("#29=ADVANCED_FACE('{name}',(#26),#28,.T.);"));
    if rejected {
        source = source.replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#29,#999));")
            .replace("ENDSEC;\nEND-ISO-10303-21;", "#999=DUMMY();\nENDSEC;\nEND-ISO-10303-21;");
    }
    source
}

fn rejected_named_face(name: &str, invalid: bool) {
    let source = sheet_source(name, true);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("named sheet parses");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("sheet geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("sheet carriers");
    let vdefs = super::super::vertex_defs(&exchange, &setup).expect("vertices");
    let edefs = super::super::edge_defs(&exchange, &setup).expect("edges");
    let odefs = super::super::oriented_defs(&exchange, &setup).expect("oriented edges");
    let shells = super::super::shell_defs(&exchange, &setup).expect("shell definitions");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = FaceAttributeCache::new(&ctx).expect("face cache");
    let mut storage = ctx.reserve_scoped(0, "test losses").expect("loss slots");
    let mut losses = Vec::new();
    let outcome = super::super::build(31, exchange.records().get(&31).expect("sheet root"),
        BuildSources { exchange: &exchange, ir: &ir, vdefs: &vdefs, edefs: &edefs,
            odefs: &odefs, shell_definitions: &shells, decoded_pcurves: &BTreeSet::new(),
            point_positions: &carriers, ctx: &ctx },
        &mut cache, false, (&mut losses, &mut storage),
    ).expect("late face rejection");
    let (built, failures, owner) = outcome.into_parts();
    assert!(built.is_empty());
    assert_eq!(failures.expect("rejection").first.expect("carrier failure").record_id, 999);
    assert_eq!(losses.len(), usize::from(invalid));
    if invalid {
        assert_eq!(losses[0].code, crate::loss::StepLossCode::MetadataStringInvalid.kind());
        assert!(losses[0].message.starts_with("STEP record #29 has an invalid face name string:"));
    }
    let expected: u64 = losses.iter().map(|loss| u64::try_from(loss.message.len()).expect("warning bytes fit")).sum();
    drop((built, owner, cache));
    assert_eq!(retained_bytes(&ctx), expected);
    drop((losses, storage));
}

#[test]
fn late_shell_rejection_releases_decoded_face_name_and_children() {
    rejected_named_face("candidate face", false);
}

#[test]
fn late_shell_rejection_retains_invalid_face_name_warning() {
    rejected_named_face(r"\X2\ZZZZ\X0\", true);
}

fn installed_name_storage(name: &str) -> u64 {
    let source = sheet_source(name, false);
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("named sheet parses");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("sheet geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("sheet carriers");
    let ctx = cadmpeg_test_support::service_decode_context();
    let outcome = super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("topology installed");
    assert_eq!(ir.model.faces.len(), 1);
    assert_eq!(ir.model.faces[0].name.as_deref(), (!name.is_empty()).then_some(name));
    drop(outcome);
    retained_bytes(&ctx)
}

#[test]
fn installed_topology_keeps_decoded_face_name_retained() {
    let baseline = installed_name_storage("");
    let name = "A".repeat(16_384);
    let named = installed_name_storage(&name);
    assert_eq!(named - baseline, u64::try_from(name.len()).expect("name bytes fit"));
}
