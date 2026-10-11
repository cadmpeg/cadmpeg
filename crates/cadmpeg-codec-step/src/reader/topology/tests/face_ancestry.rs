// SPDX-License-Identifier: Apache-2.0
//! Completed face ancestry and stage claims.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::reader::ValueExt;

use super::super::{claim_face_ancestors, face_attributes, FaceAttributeCache, FaceResolution};

fn shared_face_chain_work(count: u64) -> u64 {
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE('base',(#2));#2=FACE_BOUND('',#3,.T.);#3=EDGE_LOOP('',());");
    for id in 4..count + 4 {
        let parent = if id == 4 { 1 } else { id - 1 };
        write!(source, "#{id}=ORIENTED_FACE('',*,#{parent},.F.);").expect("append face use");
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid face ancestry");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut cache = FaceAttributeCache::new(&ctx).expect("empty cache");
    let mut typed = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut typed_storage = ctx
        .reserve_scoped(0, "test face claims")
        .expect("empty claim storage");
    let mut seen_storage = ctx
        .reserve_scoped(0, "test face seen")
        .expect("empty ancestry storage");
    for id in (4..count + 4).rev() {
        let FaceResolution::Resolved(info) = face_attributes(
            id,
            exchange.records().get(&id).expect("oriented face"),
            &exchange,
            &mut BTreeSet::new(),
            &mut cache,
        )
        .expect("face ancestry fits") else {
            panic!("resolved face");
        };
        assert_eq!(info.same_sense, (id - 3) % 2 == 0);
        assert_eq!(info.reverse_bound_orientation, (id - 3) % 2 != 0);
        assert_eq!(info.bounds.len(), 1);
        assert_eq!(info.bounds[0].reference(), Some(2));
        assert!(
            matches!(info.name, Some(crate::parse::Value::String(bytes)) if bytes.as_slice() == b"base")
        );
        claim_face_ancestors(
            info.parent,
            &cache,
            (&mut typed, &mut typed_storage),
            (&mut seen, &mut seen_storage),
        )
        .expect("claim ancestry");
    }
    assert_eq!(
        cache.completed.len(),
        usize::try_from(count + 1).expect("fixture size")
    );
    let expected = std::iter::once(1)
        .chain(4..count + 3)
        .collect::<BTreeSet<_>>();
    assert_eq!(typed, expected);
    assert_eq!(seen, expected);
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "measure shared face ancestry work")
        .expect_err("work counter probe")
    else {
        panic!("work probe resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn shared_face_ancestry_and_claims_do_not_rescan_completed_prefixes() {
    let small = shared_face_chain_work(64);
    let large = shared_face_chain_work(128);
    // Ordered lookups grow with n log n; resolving every prefix grows with n squared.
    assert!(large < 3 * small, "work grew from {small} to {large}");
}

#[test]
fn cyclic_face_ancestry_caches_failure_and_clears_the_active_set() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ORIENTED_FACE('',*,#2,.T.);#2=ORIENTED_FACE('',*,#1,.F.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid cyclic references");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = FaceAttributeCache::new(&ctx).expect("empty cache");
    let mut active = BTreeSet::new();
    for id in [1, 2, 1] {
        assert!(matches!(
            face_attributes(
                id,
                exchange.records().get(&id).expect("face"),
                &exchange,
                &mut active,
                &mut cache
            )
            .expect("cycle rejection fits"),
            FaceResolution::Unresolved
        ));
        assert!(active.is_empty());
    }
    assert_eq!(cache.completed.len(), 2);
    assert!(cache
        .completed
        .values()
        .all(|resolution| matches!(resolution, FaceResolution::Unresolved)));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn face_attribute_cache_releases_its_scratch_after_use() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE('',(#2));#2=FACE_BOUND('',#3,.T.);#3=EDGE_LOOP('',());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid face references");
    let ctx = cadmpeg_test_support::service_decode_context();
    {
        let mut cache = FaceAttributeCache::new(&ctx).expect("empty cache");
        assert!(matches!(
            face_attributes(
                1,
                exchange.records().get(&1).expect("face"),
                &exchange,
                &mut BTreeSet::new(),
                &mut cache
            )
            .expect("resolve face"),
            FaceResolution::Resolved(_)
        ));
    }
    let CodecError::ResourceLimit(limit) = ctx
        .reserve_scoped(u64::MAX, "measure released face scratch")
        .expect_err("scratch counter probe")
    else {
        panic!("scratch probe resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, 0);
}

fn shared_complex_face_work(count: usize) -> u64 {
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(REPRESENTATION_ITEM('base') ");
    for index in 0..64 * count {
        write!(source, "UNRELATED{index}() ").expect("append unrelated partial");
    }
    source.push_str("FACE((#10)));#10=FACE_BOUND('',#11,.T.);#11=EDGE_LOOP('',());");
    let mut shells = Vec::new();
    for index in 0..count {
        let id = 1000 + index;
        write!(source, "#{id}=OPEN_SHELL('',(#1));").expect("append shell use");
        shells.push(format!("#{id}"));
    }
    write!(
        source,
        "#2=SHELL_BASED_SURFACE_MODEL('',({}));ENDSEC;END-ISO-10303-21;",
        shells.join(",")
    )
    .expect("append topology root");
    // Parser setup is outside the topology work comparison; retain the full complex input.
    let mut setup_policy = DecodePolicy::service();
    setup_policy.limits.max_work_units = u64::MAX;
    let (exchange, _) = crate::test_support::with_policy_context(
        source.as_bytes(),
        &setup_policy,
        crate::parse::parse_inner,
    )
    .expect("valid complex face uses");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).expect("source fits");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("empty carriers");
    let outcome =
        super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("topology rejection fits");
    assert!(ir.model.bodies.is_empty());
    assert!(outcome
        .losses
        .iter()
        .any(|loss| loss.message.contains("implicit face plane #1")));
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "measure complex face reuse work")
        .expect_err("work counter probe")
    else {
        panic!("work probe resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn shared_complex_face_uses_do_not_rescan_type_partials() {
    let small = shared_complex_face_work(256);
    let large = shared_complex_face_work(512);
    // Both use count and unrelated partial count double; each record is classified once.
    assert!(large < 3 * small, "work grew from {small} to {large}");
}

#[test]
fn cached_face_failures_keep_carrier_and_attribute_diagnostics_distinct() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHELL_BASED_SURFACE_MODEL('',(#2));#2=OPEN_SHELL('',(#3));#3=DUMMY();#4=SHELL_BASED_SURFACE_MODEL('',(#5));#5=OPEN_SHELL('',(#6));#6=ORIENTED_FACE('',*,#3,.T.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid failed face uses");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("empty carriers");
    let outcome =
        super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("face rejection fits");
    assert!(ir.model.bodies.is_empty());
    assert!(outcome
        .losses
        .iter()
        .any(|loss| loss.message.contains("face carrier #3")));
    assert!(outcome
        .losses
        .iter()
        .any(|loss| loss.message.contains("face attributes #6")));
    assert_eq!(ctx.resource_refusal(), None);
}

mod original_context;

fn committed_shared_ancestry_work(count: u64, rejected_first: bool, shared_shell: bool) -> u64 {
    // Admit the configured depth without exhausting a debug test thread's stack.
    std::thread::Builder::new().stack_size(16 * 1024 * 1024)
        .spawn(move || committed_shared_ancestry_work_inner(count, rejected_first, shared_shell))
        .expect("ancestry thread starts").join().expect("ancestry thread completes")
}

fn committed_shared_ancestry_work_inner(count: u64, rejected_first: bool, shared_shell: bool) -> u64 {
    let fixture = include_str!("../../../../tests/fixtures/ap214_sheet.p21");
    let (header, _) = fixture.split_once("DATA;\n").expect("fixture DATA section");
    let mut source = format!("{header}DATA;\n");
    for record in fixture.lines().filter(|line| line.starts_with('#')) {
        if ["#30=", "#31=", "#32=", "#33="].iter().any(|prefix| record.starts_with(prefix)) {
            continue;
        }
        writeln!(source, "{}", record.replace("#19=EDGE_CURVE('',#6,#7,#57,.T.);", "#19=EDGE_CURVE('',#6,#7,#16,.T.);"))
            .expect("append carrier record");
    }
    let mut parent = 29;
    for index in 0..count {
        let id = 10_000 + index;
        writeln!(source, "#{id}=ORIENTED_FACE('',*,#{parent},.T.);").expect("append ancestor");
        parent = id;
    }
    let mut shell_parent = 30;
    if shared_shell {
        writeln!(source, "#30=OPEN_SHELL('',(#{parent}));").expect("append shared base shell");
        for index in 0..count {
            let id = 20_000 + index;
            writeln!(source, "#{id}=ORIENTED_OPEN_SHELL('',*,#{shell_parent},.T.);").expect("append shell ancestor");
            shell_parent = id;
        }
    }
    if rejected_first {
        writeln!(source, "#90=SHELL_BASED_SURFACE_MODEL('',(#91));\n#91=OPEN_SHELL('',(#{parent},#92));\n#92=DUMMY();")
            .expect("append rejected root");
    }
    for index in 0..count {
        let root = 100_000 + index;
        let shell = 200_000 + index;
        let shell_references = if shared_shell { format!("#{shell_parent},#{shell}") } else { format!("#{shell}") };
        writeln!(source, "#{root}=SHELL_BASED_SURFACE_MODEL('',({shell_references}));\n#{shell}=OPEN_SHELL('',(#{parent}));")
            .expect("append independent root");
    }
    source.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("shared ancestor roots parse");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("shared face geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("predecoded carriers");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_recursion_depth = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).expect("source fits");
    let outcome = super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("independent roots commit");
    let count_usize = usize::try_from(count).expect("root count fits");
    let bodies = count_usize * if shared_shell { 2 } else { 1 };
    assert_eq!(ir.model.bodies.len(), bodies);
    assert_eq!(ir.model.faces.len(), bodies);
    assert!(outcome.claims.contains(&29));
    for id in 10_000..10_000 + count {
        assert!(outcome.claims.contains(&id), "ancestor #{id} is claimed");
    }
    if shared_shell {
        for id in 20_000..20_000 + count {
            assert!(outcome.claims.contains(&id), "shell ancestor #{id} is claimed");
        }
    }
    assert!(!outcome.claims.contains(&90));
    assert!(!outcome.claims.contains(&91));
    assert!(!outcome.claims.contains(&92));
    if rejected_first {
        assert!(outcome.losses.iter().any(|loss| loss.message.contains("face carrier #92")));
    } else {
        assert!(outcome.losses.is_empty());
    }
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "committed face ancestry work probe")
        .expect_err("probe work counter") else { panic!("work refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn independent_roots_claim_shared_face_ancestors_once_per_stage() {
    let small = committed_shared_ancestry_work(128, false, false);
    let large = committed_shared_ancestry_work(256, false, false);
    assert!(large < 3 * small, "committed ancestry work grew from {small} to {large}");
}

#[test]
fn independent_roots_claim_shared_shell_ancestors_once_per_stage() {
    let small = committed_shared_ancestry_work(128, false, true);
    let large = committed_shared_ancestry_work(256, false, true);
    assert!(large < 3 * small, "committed shell ancestry work grew from {small} to {large}");
}

#[test]
fn rejected_root_does_not_suppress_shared_face_ancestor_claims() {
    assert!(committed_shared_ancestry_work(8, true, false) > 0);
}

fn shared_complex_root_work(count: usize) -> u64 {
    let fixture = include_str!("../../../../tests/fixtures/ap214_sheet.p21");
    let (header, _) = fixture.split_once("DATA;\n").expect("fixture DATA section");
    let mut source = format!("{header}DATA;\n");
    for record in fixture.lines().filter(|line| line.starts_with('#')) {
        if ["#30=", "#31=", "#32=", "#33="].iter().any(|prefix| record.starts_with(prefix)) {
            continue;
        }
        writeln!(source, "{}", record.replace("#19=EDGE_CURVE('',#6,#7,#57,.T.);", "#19=EDGE_CURVE('',#6,#7,#16,.T.);"))
            .expect("append carrier record");
    }
    let mut shells = Vec::new();
    for index in 0..count {
        let shell = 1000 + index;
        writeln!(source, "#{shell}=OPEN_SHELL('',(#29));").expect("append shell");
        shells.push(format!("#{shell}"));
    }
    source.push_str("#31=(REPRESENTATION_ITEM('') ");
    for index in 0..64 * count {
        write!(source, "UNRELATED{index}() ").expect("append unrelated partial");
    }
    writeln!(source, "SHELL_BASED_SURFACE_MODEL(({})));\nENDSEC;\nEND-ISO-10303-21;", shells.join(","))
        .expect("append complex root");
    let mut parse_policy = DecodePolicy::desktop();
    // Parsing diagnostics are setup; topology work is measured below.
    parse_policy.limits.max_work_units = 16_000_000_000;
    let (exchange, _) = crate::test_support::with_policy_context(source.as_bytes(), &parse_policy, crate::parse::parse_inner)
        .expect("complex root parses");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("sheet geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("predecoded carriers");
    let ctx = cadmpeg_test_support::service_decode_context();
    let outcome = super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("root shells commit");
    assert_eq!(ir.model.bodies.len(), count);
    assert_eq!(ir.model.faces.len(), count);
    assert!(outcome.losses.is_empty());
    assert!(outcome.claims.contains(&31));
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "complex root work probe")
        .expect_err("probe work counter") else { panic!("work refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn shared_complex_root_is_classified_once_before_body_builds() {
    let small = shared_complex_root_work(128);
    let large = shared_complex_root_work(256);
    assert!(large < 3 * small, "complex root work grew from {small} to {large}");
}
