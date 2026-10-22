// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;

use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartSpec, PartState};
use crate::{CodecFormat, NativeUnknownRecord};

use super::ParserPartOutput;

fn spec(level: &str, view: View<'_>) -> PartSpec {
    let start = u64::try_from(view.start()).expect("start");
    PartSpec::new(
        PartLevel::parse(level).expect("Appendix C"),
        vec![start..start + 1],
    )
}

#[test]
fn parser_scope_keeps_real_parentage_and_one_failed_child_home_with_its_sibling() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::default()).expect("root");
    let bad = root.child(0, 2).expect("first member");
    let good = root.child(2, 4).expect("second member");
    let mut output = ParserPartOutput::new(CodecFormat::Fcstd);
    let result = ctx
        .decode_parser_scope(&mut output, root, spec("zip.archive", root), |parent| {
            assert!(ctx
                .decode_parser_part(parent, bad, spec("zip.entry", bad), || {
                    assert_eq!(bad.read_at(0, 2), Some(b"AB".as_slice()));
                    Err::<(), _>(CodecError::malformed("unreadable first member"))
                })?
                .is_none());
            assert_eq!(
                ctx.decode_parser_part(parent, good, spec("zip.entry", good), || {
                    Ok(good.read_at(2, 2).expect("actual sibling").to_vec())
                })?,
                Some(b"CD".to_vec())
            );
            Ok(7)
        })
        .expect("parent result");
    assert_eq!(result, Some(7));
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    let parts = &fidelity.parts;
    assert_eq!(parts.len(), 3);
    let (parent_id, parent) = parts
        .iter()
        .find(|(_, p)| p.level.as_str() == "zip.archive")
        .expect("actual archive parent");
    assert_eq!(parent.state, PartState::Decoded {});
    assert!(parent.parent.is_none());
    let children = parts
        .values()
        .filter(|p| p.level.as_str() == "zip.entry")
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert!(children
        .iter()
        .all(|p| p.parent.as_ref() == Some(parent_id)));
    assert_eq!(
        children
            .iter()
            .filter(|p| p.state == PartState::GivenUp {})
            .count(),
        1
    );
    let unknowns = native
        .namespace("fcstd")
        .expect("codec namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed unknowns");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .expect("single home")
            .data(),
        Some(b"AB".as_slice())
    );
    assert_eq!(losses.len(), 1);
}

#[test]
fn parent_give_up_covers_nested_success_and_failure_without_extra_source_homes() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::default()).expect("root");
    let first = root.child(0, 2).expect("first member");
    let second = root.child(2, 4).expect("second member");
    let mut output = ParserPartOutput::new(CodecFormat::Fcstd);
    assert!(ctx
        .decode_parser_scope(&mut output, root, spec("zip.archive", root), |parent| {
            ctx.decode_parser_part(parent, first, spec("zip.entry", first), || {
                assert_eq!(first.read_at(0, 2), Some(b"AB".as_slice()));
                Ok(())
            })?;
            ctx.decode_parser_part(parent, second, spec("zip.entry", second), || {
                Err::<(), _>(CodecError::malformed("unreadable member"))
            })?;
            Err::<(), _>(CodecError::malformed("parent cannot finish"))
        })
        .expect("bounded parent failure")
        .is_none());
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    let (parent_id, _) = fidelity
        .parts
        .iter()
        .find(|(_, p)| p.state == PartState::GivenUp {})
        .expect("one parent give-up");
    assert_eq!(fidelity.parts.len(), 3);
    assert!(fidelity
        .parts
        .values()
        .filter(|p| p.level.as_str() == "zip.entry")
        .all(|p| p.state
            == PartState::Covered {
                by: parent_id.clone()
            }));
    let unknowns = native
        .namespace("fcstd")
        .expect("codec namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(fidelity.retained_records.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .expect("parent home")
            .data(),
        Some(root.window())
    );
    assert_eq!(losses.len(), 1);
    assert!(ctx
        .take_record_captures()
        .expect("final extraction")
        .is_empty());
}

#[test]
fn nested_parser_resource_refusal_stays_fatal_and_never_becomes_a_give_up() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("root");
    let mut output = ParserPartOutput::new(CodecFormat::Fcstd);
    let error = ctx
        .decode_parser_scope(&mut output, root, spec("zip.archive", root), |parent| {
            ctx.decode_parser_part(parent, root, spec("zip.entry", root), || {
                ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")?;
                ctx.retain_bytes(1, "over caller allowance")
            })
        })
        .expect_err("fused resource refusal");
    assert!(matches!(error, CodecError::ResourceLimit(_)));
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    assert!(native.namespace("fcstd").is_none());
    assert!(fidelity.parts.is_empty());
    assert!(losses.is_empty());
}

#[test]
fn parser_handoff_keeps_actual_failed_record_attempts_in_the_normal_closure() {
    let bytes = b"#7=UNKNOWN();";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("source");
    let mut output = ParserPartOutput::new(CodecFormat::Step);
    let owner = "step:native:record#7";
    assert!(ctx
        .decode_parser_part(
            &mut output,
            root,
            PartSpec::new(
                PartLevel::parse("step.simple_entity_instance").expect("Appendix C"),
                Vec::new(),
            ),
            || ctx.record(owner, root, || {
                assert_eq!(root.read_at(0, bytes.len()), Some(bytes.as_slice()));
                Err::<(), _>(CodecError::malformed("unsupported source record"))
            }),
        )
        .expect("bounded failure")
        .is_none());
    let (model, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("normal dependency closure")
        .into_parts();
    assert_eq!(model.entity_count(), 0);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(owner));
    let unknowns = native
        .namespace("step")
        .expect("source namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed unknown stub");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(bytes.as_slice())
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn parser_handoff_cannot_bypass_the_typed_dependency_visitor_budget() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"UNKNOWN", &arena, &DecodePolicy::service())
        .expect("source");
    let mut output = ParserPartOutput::new(CodecFormat::Step);
    ctx.decode_parser_part(
        &mut output,
        root,
        PartSpec::new(
            PartLevel::parse("step.simple_entity_instance").expect("Appendix C"),
            Vec::new(),
        ),
        || Err::<(), _>(CodecError::malformed("unsupported source record")),
    )
    .expect("bounded failure");
    ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")
        .expect("exact remaining allowance");
    assert!(matches!(
        output.into_parts(&ctx),
        Err(CodecError::ResourceLimit(_))
    ));
}

/// Retained bytes charged to run one child part, given up or decoded, at the
/// root output or inside a parser scope.
fn child_part_cost(scoped: bool, give_up: bool) -> u64 {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::default()).expect("root");
    let child = root.child(0, 2).expect("member");
    let before = ctx.retained_bytes_available();
    let mut output = ParserPartOutput::new(CodecFormat::Fcstd);
    let run = |parent: &mut ParserPartOutput| {
        ctx.decode_parser_part(parent, child, spec("zip.entry", child), || {
            if give_up {
                Err(CodecError::malformed("unreadable member"))
            } else {
                Ok(())
            }
        })
        .map(|_| ())
    };
    if scoped {
        ctx.decode_parser_scope(&mut output, root, spec("zip.archive", root), run)
            .expect("scope")
            .expect("scope decoded");
    } else {
        run(&mut output).expect("child part");
    }
    let charged = before - ctx.retained_bytes_available();
    drop(output);
    charged
}

#[test]
fn a_child_given_up_inside_a_parser_scope_is_charged_once_at_the_root() {
    let at_root = child_part_cost(false, true) - child_part_cost(false, false);
    assert!(at_root > 0);
    assert_eq!(
        child_part_cost(true, true) - child_part_cost(true, false),
        at_root
    );
}
