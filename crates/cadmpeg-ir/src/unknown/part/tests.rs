// SPDX-License-Identifier: Apache-2.0
//! The part helper's stub, retained bytes, and loss.

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension,
};
use cadmpeg_core::CodecError;

use super::{restate_loss, DecodeParts, GivenUpPart, Part};
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
    let part = Part::new("synthetic", "record", extent)
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
    let mut fidelity = SourceFidelity::default();
    let loss = given_up
        .commit(&ctx, &mut ir, &mut fidelity)
        .expect("commit");

    assert_eq!(loss.code.to_string(), "decode/record.unreadable");
    assert_eq!(loss.severity, Severity::Error);
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
    ctx.finish_session()
        .expect("a given-up part refuses nothing");
}

#[test]
fn the_loss_states_lost_entries_and_what_else_the_part_cost() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let part = Part::new("synthetic", "record", root.child(4, 18).expect("extent"))
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
            Part::new("synthetic", "record", root.child(4, 18).expect("extent")).digest_only(),
            CodecError::malformed("damaged"),
        )
        .expect("given up")
        .contained_in(identity("synthetic:native:document#0"));
    let id = given_up.id().clone();
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
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
            Part::new("synthetic", "record", root.child(4, 18).expect("extent")),
            CodecError::malformed("damaged"),
        )
        .expect("given up");
    let from = given_up.id().clone();
    let to = format!(
        "synthetic:member/a/{}",
        &from.as_str()["synthetic:".len()..]
    );
    let mut loss = given_up.loss();
    assert!(restate_loss(&mut loss, &from, &to));
    assert!(loss.message.contains(&format!(" record={to}: ")));
    assert!(!loss.message.contains(&format!(" record={from}: ")));
    assert!(!restate_loss(&mut loss, &from, &to));
}

#[test]
fn the_stub_id_is_a_function_of_codec_kind_stream_and_offset() {
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let at = |start: usize, end: usize| root.child(start, end).expect("extent");
    let id = |part: Part<'_>| part.id().expect("part id");
    let base = id(Part::new("synthetic", "record", at(4, 18)));
    assert!(base.as_str().starts_with("synthetic:source:part#"));
    assert_eq!(base, id(Part::new("synthetic", "record", at(4, 10))));
    for other in [
        id(Part::new("other", "record", at(4, 18))),
        id(Part::new("synthetic", "section", at(4, 18))),
        id(Part::new("synthetic", "record", at(4, 18)).in_stream(crate::stream_name!("A"))),
        id(Part::new("synthetic", "record", at(5, 18))),
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
    let outcome = decode_part(&ctx, Part::new("synthetic", "stream", root), || {
        ctx.begin_expand(ExpandSpec::Exact(3)).map(drop)
    })
    .expect("a Part refusal gives up the part");
    let Err(given_up) = outcome else {
        panic!("the expansion was refused");
    };
    assert!(given_up.loss().message.contains("scope Part"));
    assert_eq!(ctx.resource_refusal(), None);
    assert!(matches!(
        decode_part(&ctx, Part::new("synthetic", "stream", root), || Ok(())),
        Ok(Ok(()))
    ));
}

#[test]
fn a_session_refusal_or_io_error_is_not_given_up() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root");
    let error = decode_part(&ctx, Part::new("synthetic", "record", root), || {
        ctx.charge_entities(1, "entity")
    })
    .expect_err("the session is refused");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities)
    );

    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::default()).expect("root");
    let error = ctx
        .give_up_part(
            Part::new("synthetic", "record", root),
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
            Part::new("synthetic", "record", root),
            CodecError::malformed("damaged"),
        )
        .expect("given up")
    };
    let mut ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
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
    let mut fidelity = SourceFidelity::default();
    ctx.give_up_part(
        Part::new("synthetic", "record", root.child(4, 18).expect("extent")),
        CodecError::malformed("damaged"),
    )
    .expect("given up")
    .commit(&ctx, &mut ir, &mut fidelity)
    .expect("commit");
    let parsed = CadIr::from_json(&ir.to_canonical_json().expect("serialize")).expect("parse");
    assert_eq!(parsed, ir);
    let wire = serde_json::to_string(&fidelity).expect("serialize fidelity");
    assert_eq!(
        serde_json::from_str::<SourceFidelity>(&wire).expect("parse fidelity"),
        fidelity
    );
}
