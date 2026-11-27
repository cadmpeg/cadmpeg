// SPDX-License-Identifier: Apache-2.0
//! The part helper's stub, retained bytes, and loss.

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeSet;

use cadmpeg_core::decode::{
    ArenaClass, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension,
};
use cadmpeg_core::CodecError;

use super::{link_committed, restate_loss, DecodeParts, GivenUpPart, Part};
use crate::document::CadIr;
use crate::ids::Identity;
use crate::report::Severity;
use crate::source_fidelity::{RetainedBytes, SourceFidelity};
use crate::NativeUnknownRecord;

const SOURCE: &[u8] = b"head[damaged part]tail";

fn identity(text: &str) -> Identity {
    Identity::new(text.to_owned()).expect("fixture identity")
}

/// Runs `decode` as one part and gives the part up when it fails.
fn decode_part<T>(
    ctx: &DecodeContext<'_>,
    part: Part<'_>,
    decode: impl FnOnce() -> Result<T, CodecError>,
) -> Result<Result<T, GivenUpPart>, CodecError> {
    match ctx.run_part(decode)? {
        Ok(value) => Ok(Ok(value)),
        Err(error) => ctx.give_up_part(part, error).map(Err),
    }
}

#[test]
fn a_failed_part_leaves_one_linked_stub_its_bytes_and_one_error_loss() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let extent = root.child(4, 18).expect("part extent");
    let part = Part::new(
        "synthetic",
        "record",
        extent,
        [ArenaClass::Design, ArenaClass::Model],
    )
    .named("Body/7")
    .in_stream(crate::stream_name!("Body.bin"))
    .contained_in(identity("synthetic:native:body#7"))
    .dropping([
        identity("synthetic:model:face#2"),
        identity("synthetic:model:edge#1"),
        identity("synthetic:model:face#2"),
    ]);
    let id = part.id().expect("part id");
    let Err(given_up) = decode_part(&ctx, part, || {
        Err::<(), _>(CodecError::malformed("record length runs past its stream"))
    })
    .expect("the failure belongs to the part") else {
        panic!("the part failed");
    };
    assert_eq!(given_up.id(), &id);

    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    let loss = given_up
        .commit(&ctx, &mut ir, &mut fidelity)
        .expect("commit");

    assert_eq!(loss.code.to_string(), "decode/record.unreadable");
    assert_eq!(loss.severity, Severity::Error);
    assert_eq!(loss.code.strict_floor(), Some(Severity::Error));
    assert_eq!(
        loss.message,
        format!(
            "part record \"Body/7\" stream=\"Body.bin\" offset=4 length=14 record={id}: malformed container: \
             record length runs past its stream; dropped=[synthetic:model:edge#1, synthetic:model:face#2]"
        )
    );
    assert_eq!(
        loss.provenance,
        Some(
            crate::SourceProvenance::in_stream("synthetic", crate::stream_name!("Body.bin"), 4)
                .with_tag("record")
        )
    );

    let stubs = ir
        .native_unknowns("synthetic")
        .expect("the stub is a native unknown record");
    assert_eq!(
        stubs,
        [NativeUnknownRecord {
            id: id.clone(),
            links: vec![identity("synthetic:native:body#7")],
        }]
    );
    let retained = fidelity
        .retained_record(id.as_str())
        .expect("retained bytes");
    assert_eq!(retained.data(), Some(b"[damaged part]".as_slice()));
    assert_eq!(retained.stream(), "Body.bin");
    assert_eq!(retained.offset(), 4);
    let holds = BTreeSet::from([ArenaClass::Design, ArenaClass::Model]);
    assert_eq!(retained.untyped(), &holds);
    assert_eq!(fidelity.unrepresented(), holds);
    ctx.finish_session()
        .expect("a given-up part refuses nothing");
}

#[test]
fn a_committed_stub_is_linked_to_the_record_defined_after_it() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    let given_up = ctx
        .give_up_part(
            Part::new("synthetic", "record", root, [ArenaClass::Model])
                .contained_in(identity("synthetic:native:document#0")),
            CodecError::malformed("damaged"),
        )
        .expect("given up");
    let id = given_up.id().clone();
    given_up
        .commit(&ctx, &mut ir, &mut fidelity)
        .expect("commit");
    let member = identity("synthetic:model:product-definition#member");
    assert!(link_committed(&mut ir, "synthetic", id.as_str(), member.clone()).expect("link"));
    assert_eq!(
        ir.native_unknowns("synthetic").expect("stub"),
        [NativeUnknownRecord {
            id: id.clone(),
            links: vec![member.clone()],
        }]
    );
    let before = ir.clone();
    assert!(!link_committed(&mut ir, "other", id.as_str(), member.clone()).expect("link"));
    assert!(
        !link_committed(&mut ir, "synthetic", "synthetic:source:part#absent", member)
            .expect("link")
    );
    assert_eq!(ir, before);
}

#[test]
fn repeated_source_parts_commit_once_with_every_containing_record() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).unwrap();
    let give_up = |owner| {
        ctx.give_up_part(
            Part::new("synthetic", "record", root, [ArenaClass::Design])
                .contained_in(identity(owner)),
            CodecError::malformed("damaged"),
        )
        .unwrap()
    };
    let first = give_up("synthetic:native:scope#one");
    let second = give_up("synthetic:native:scope#two");
    let id = first.id().clone();
    let merged = GivenUpPart::coalesce(vec![first.clone(), second, first.clone()]).unwrap();
    assert_eq!(merged.len(), 1);
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    let loss = merged
        .into_iter()
        .next()
        .unwrap()
        .commit(&ctx, &mut ir, &mut fidelity)
        .unwrap();
    assert_eq!(loss.code, first.loss().code);
    let stubs = ir.native_unknowns("synthetic").unwrap();
    assert_eq!(
        stubs[0].links,
        vec![
            identity("synthetic:native:scope#one"),
            identity("synthetic:native:scope#two")
        ]
    );
    assert_eq!(
        fidelity.retained_record(id.as_str()).unwrap().data(),
        Some(SOURCE)
    );
    let mut different = first.clone();
    different.retained = crate::source_fidelity::RetainedSourceRecord::from_bytes(
        crate::provenance::SourceOwner::Root,
        0,
        RetainedBytes::Inline {
            data: b"different".to_vec(),
        },
    )
    .unwrap()
    .with_untyped([ArenaClass::Design]);
    assert!(GivenUpPart::coalesce(vec![first, different]).is_err());
    ctx.finish_session().unwrap();
}

#[test]
fn the_loss_states_lost_entries_and_what_else_the_part_cost() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let part = Part::new(
        "synthetic",
        "record",
        root.child(4, 18).expect("extent"),
        [ArenaClass::Model],
    )
    .costing("none of its 2 members was placed")
    .losing_entries(["#7 entry #3".to_owned(), "#9 entry #3".to_owned()]);
    let id = part.id().expect("part id");
    let given_up = ctx
        .give_up_part(part, CodecError::malformed("damaged"))
        .expect("given up");
    assert_eq!(
        given_up.loss().message,
        format!(
            "part record stream=\"\" offset=4 length=14 record={id}: malformed container: damaged; \
             none of its 2 members was placed; dropped=[]; lost=[#7 entry #3, #9 entry #3]"
        )
    );
}

#[test]
fn a_digest_only_part_keeps_its_length_and_digest_without_a_copy() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let bytes = &SOURCE[4..18];
    let given_up = ctx
        .give_up_part(
            Part::new(
                "synthetic",
                "record",
                root.child(4, 18).expect("extent"),
                [ArenaClass::Model],
            )
            .digest_only(),
            CodecError::malformed("damaged"),
        )
        .expect("given up")
        .contained_in(identity("synthetic:native:document#0"));
    let id = given_up.id().clone();
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    given_up
        .commit(&ctx, &mut ir, &mut fidelity)
        .expect("commit");
    let retained = fidelity.retained_record(id.as_str()).expect("retained");
    assert_eq!(retained.data(), None);
    assert_eq!(retained.offset(), 4);
    assert_eq!(retained.byte_len(), 14);
    assert_eq!(
        retained.sha256(),
        RetainedBytes::Inline {
            data: bytes.to_vec()
        }
        .sha256()
    );
    assert_eq!(
        ir.native_unknowns("synthetic").expect("stub")[0].links,
        [identity("synthetic:native:document#0")]
    );
}

#[test]
fn a_rescoped_stub_is_restated_in_its_loss() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let given_up = ctx
        .give_up_part(
            Part::new(
                "synthetic",
                "record",
                root.child(4, 18).expect("extent"),
                [ArenaClass::Model],
            ),
            CodecError::malformed("damaged"),
        )
        .expect("given up");
    let from = given_up.id().clone();
    let to = format!(
        "synthetic:member/a/{}",
        &from.as_str()["synthetic:".len()..]
    );
    let mut loss = given_up.loss();
    assert!(restate_loss(&mut loss, from.as_str(), &to));
    assert!(loss.message.contains(&format!(" record={to}: ")));
    assert!(!loss.message.contains(&format!(" record={from}: ")));
    assert!(!restate_loss(&mut loss, from.as_str(), &to));
}

#[test]
fn the_stub_id_is_a_function_of_codec_kind_stream_and_offset() {
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let at = |start: usize, end: usize| root.child(start, end).expect("extent");
    let id = |part: Part<'_>| part.id().expect("part id");
    let part = |codec, kind, extent| Part::new(codec, kind, extent, [ArenaClass::Model]);
    let base = id(part("synthetic", "record", at(4, 18)));
    assert!(base.as_str().starts_with("synthetic:source:part#"));
    assert_eq!(base, id(part("synthetic", "record", at(4, 10))));
    assert_eq!(
        base,
        id(Part::new(
            "synthetic",
            "record",
            at(4, 18),
            [ArenaClass::Design]
        ))
    );
    for other in [
        id(part("other", "record", at(4, 18))),
        id(part("synthetic", "section", at(4, 18))),
        id(part("synthetic", "record", at(4, 18)).in_stream(crate::stream_name!("A"))),
        id(part("synthetic", "record", at(5, 18))),
    ] {
        assert_ne!(other, base);
    }
}

#[test]
fn a_part_resource_refusal_gives_up_the_part_and_the_next_part_decodes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 2;
    let (ctx, root) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root");
    let outcome = decode_part(
        &ctx,
        Part::new("synthetic", "stream", root, [ArenaClass::Model]),
        || ctx.begin_expand(ExpandSpec::Exact(3)).map(drop),
    )
    .expect("a Part refusal gives up the part");
    let Err(given_up) = outcome else {
        panic!("the expansion was refused");
    };
    assert!(given_up.loss().message.contains("scope Part"));
    assert_eq!(ctx.resource_refusal(), None);
    assert!(matches!(
        decode_part(
            &ctx,
            Part::new("synthetic", "stream", root, [ArenaClass::Model]),
            || Ok(())
        ),
        Ok(Ok(()))
    ));
}

#[test]
fn a_session_refusal_or_io_error_is_not_given_up() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root");
    let error = decode_part(
        &ctx,
        Part::new("synthetic", "record", root, [ArenaClass::Model]),
        || ctx.charge_entities(1, "entity"),
    )
    .expect_err("the session is refused");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities)
    );

    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let error = ctx
        .give_up_part(
            Part::new("synthetic", "record", root, [ArenaClass::Model]),
            CodecError::Io(std::io::Error::other("source unavailable")),
        )
        .expect_err("I/O is fatal");
    assert!(matches!(error, CodecError::Io(_)));
}

#[test]
fn a_part_already_given_up_is_not_committed_twice() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let give_up = || {
        ctx.give_up_part(
            Part::new("synthetic", "record", root, [ArenaClass::Model]),
            CodecError::malformed("damaged"),
        )
        .expect("given up")
    };
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    give_up()
        .commit(&ctx, &mut ir, &mut fidelity)
        .expect("first commit");
    let before = (ir.clone(), fidelity.clone());
    assert!(give_up().commit(&ctx, &mut ir, &mut fidelity).is_err());
    assert_eq!((ir, fidelity), before);
}

#[test]
fn a_given_up_part_survives_the_wire() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    ctx.give_up_part(
        Part::new(
            "synthetic",
            "record",
            root.child(4, 18).expect("extent"),
            [ArenaClass::Model],
        ),
        CodecError::malformed("damaged"),
    )
    .expect("given up")
    .commit(&ctx, &mut ir, &mut fidelity)
    .expect("commit");
    let parsed = CadIr::from_json(&ir.to_canonical_json().expect("serialize")).expect("parse");
    assert_eq!(parsed, ir);
    let wire = json::to_string(&fidelity).expect("serialize fidelity");
    assert_eq!(
        serde_json::from_str::<SourceFidelity>(&wire).expect("parse fidelity"),
        fidelity
    );
}

#[test]
fn a_part_of_a_nested_document_is_restated_in_its_container_stream() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let part = || {
        Part::new(
            "synthetic",
            "record",
            root.child(4, 18).expect("extent"),
            [ArenaClass::Model],
        )
        .in_stream(crate::stream_name!("Body.bin"))
    };
    let outer = part().id().expect("part id");
    let container = crate::stream_name!("Embedded");
    let nested = ctx
        .give_up_part(part(), CodecError::malformed("damaged"))
        .expect("given up")
        .nested_in(&ctx, &container)
        .expect("restated");
    assert_eq!(nested.kind(), "record");
    assert_ne!(nested.id(), &outer);
    let literal = part()
        .in_stream(crate::stream_name!("Embedded/Body.bin"))
        .id()
        .expect("part id");
    assert_ne!(nested.id(), &literal);
    let id = nested.id().clone();
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::for_root(root);
    let loss = nested.commit(&ctx, &mut ir, &mut fidelity).expect("commit");
    assert_eq!(
        loss.message,
        format!(
            "part record stream=\"Embedded/Body.bin\" offset=4 length=14 record={id}: \
             malformed container: damaged; dropped=[]"
        )
    );
    assert_eq!(
        loss.provenance,
        Some(
            crate::SourceProvenance::in_stream(
                "synthetic",
                crate::stream_name!("Embedded/Body.bin"),
                4
            )
            .with_tag("record")
        )
    );
    let retained = fidelity.retained_record(id.as_str()).expect("retained");
    assert_eq!(retained.stream(), "Embedded/Body.bin");
    assert_eq!(retained.data(), Some(b"[damaged part]".as_slice()));
}

#[test]
fn a_record_given_up_whole_is_refused_by_a_pinned_decode_through_record_unreadable() {
    use crate::provenance::SourceProvenance;
    use crate::report::loss::{LossKind, LossNote, LossTaxonomy};
    use crate::report::strictness::Strictness;

    let why = LossNote::new(
        LossKind::namespaced(
            crate::loss_namespace!("step"),
            "topology.root-rejected",
            LossTaxonomy::SourceTopologyInvalid,
        ),
        "STEP topology root #12 rejected",
    )
    .with_severity(Severity::Warning)
    .with_provenance(SourceProvenance::root("step", 40));
    let [codec, unreadable] = super::record_given_up(
        Identity::new("step:data:shell#12").expect("an id"),
        why.clone(),
    );
    assert_eq!(codec, why);
    assert_eq!(unreadable.code, super::record_unreadable_kind());
    assert_eq!(unreadable.severity, Severity::Error);
    assert_eq!(unreadable.provenance, why.provenance);
    assert!(unreadable
        .message
        .split_whitespace()
        .any(|token| token == "step:data:shell#12"));
    assert_eq!(
        unreadable.message,
        "record step:data:shell#12 stream=\"\" offset=40 given up: step/topology.root-rejected: \
         STEP topology root #12 rejected; dropped=[]"
    );
    let pinned = Strictness {
        pinned: true,
        ..Strictness::default()
    };
    assert!(!pinned.refuses(&codec, false));
    assert!(pinned.refuses(&unreadable, false));
    assert!(!Strictness::default().refuses(&unreadable, false));
}

#[test]
fn a_record_given_up_names_its_span_and_the_sorted_records_dropped_with_it() {
    use crate::provenance::{SourceProvenance, StreamName};

    let stream = StreamName::try_from("Body".to_owned()).expect("stream");
    let id = |text: &str| Identity::new(text).expect("an id");
    let loss = super::GivenUpRecord::new(id("t:r:rec#7"), "carrier unsupported")
        .at(SourceProvenance::in_stream("nx", stream, 96))
        .spanning(24)
        .dropping([id("t:r:rec#9"), id("t:r:rec#8"), id("t:r:rec#9")])
        .loss();
    assert_eq!(loss.code, super::record_unreadable_kind());
    assert_eq!(loss.severity, Severity::Error);
    assert_eq!(
        loss.message,
        "record t:r:rec#7 stream=\"Body\" offset=96 length=24 given up: carrier unsupported; \
         dropped=[t:r:rec#8, t:r:rec#9]"
    );
    assert_eq!(
        super::GivenUpRecord::new(id("t:r:rec#7"), "carrier unsupported")
            .loss()
            .message,
        "record t:r:rec#7 given up: carrier unsupported; dropped=[]"
    );
}

#[test]
fn a_tail_gains_the_wrapper_s_dropped_and_marked_records_in_order() {
    let id = |text: &str| Identity::new(text).expect("an id");
    let mut loss = super::GivenUpRecord::new(id("t:r:rec#1"), "bad")
        .dropping([id("t:b:rec#2")])
        .loss();
    loss.message.insert_str(0, "prefix: ");
    super::name_released(
        &mut loss,
        ["t:b:rec#1".to_owned(), "t:b:rec#2".to_owned()],
        ["t:m:rec#0".to_owned()],
        ["delta of s: dropped=[t:d:rec#1]; marked=[]".to_owned()],
    );
    assert_eq!(
        loss.message,
        "prefix: record t:r:rec#1 given up: bad; dropped=[t:b:rec#1, t:b:rec#2]; \
         marked=[t:m:rec#0]; delta of s: dropped=[t:d:rec#1]; marked=[]"
    );
    let (named, dropped) = super::charged(&loss).expect("a charge");
    assert_eq!(named.as_str(), "t:r:rec#1");
    assert_eq!(dropped, ["t:b:rec#1", "t:b:rec#2"]);

    super::rescope_give_up(&mut loss, |text| Ok(text.replace(":rec#", ":rec#s/"))).unwrap();
    assert_eq!(
        loss.message,
        "prefix: record t:r:rec#s/1 given up: bad; dropped=[t:b:rec#s/1, t:b:rec#s/2]; \
         marked=[t:m:rec#s/0]; delta of s: dropped=[t:d:rec#1]; marked=[]"
    );

    let mut note = super::dropped_with(id("t:a:rec#1"), id("t:r:rec#1"));
    assert!(super::needs(&note).is_some());
    super::rescope_give_up(&mut note, |text| Ok(format!("{text}/s"))).unwrap();
    assert_eq!(note.message, "record t:a:rec#1/s needs t:r:rec#1/s");
    let (record, needs) = super::needs(&note).expect("a note");
    assert_eq!(
        (record.as_str(), needs.as_str()),
        ("t:a:rec#1/s", "t:r:rec#1/s")
    );
}
