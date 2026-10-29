// SPDX-License-Identifier: Apache-2.0
//! Part boundary: stage commit, give-up and resource scope.

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ExpandSpec, LimitScope,
    ResourceDimension,
};
use cadmpeg_core::CodecError;

use super::{DecodeParts, PartOutput, PartSpec};
use crate::codec::DecodeLossCode;
use crate::document::{ArenaClass, ArenaClasses};
use crate::math::Point3;
use crate::report::Severity;
use crate::topology::Point;
use crate::CodecFormat;

fn point(id: &str) -> Point {
    Point::new(
        id.try_into().expect("point identity"),
        crate::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite point"),
        None,
    )
}

#[test]
fn a_successful_part_commits_its_stage() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let value = ctx
        .decode_part(&mut output, root, PartSpec::new("step.data"), |stage| {
            stage.insert_model(&ctx, point("step:model:point#1"))?;
            stage.push_loss(&ctx, DecodeLossCode::SidecarStale.note("staged"))?;
            Ok(7)
        })
        .unwrap();
    assert_eq!(value, Some(7));
    let (model, _, fidelity, losses) = output.into_parts();
    assert_eq!(model.points.len(), 1);
    assert!(fidelity.retained_records().is_empty());
    assert_eq!(losses.len(), 1);
}

#[test]
fn a_failed_part_drops_its_stage_and_retains_its_bytes_once() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDEF", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let extent = root.child(2, 5).unwrap();
    let value = ctx
        .decode_part(&mut output, extent, PartSpec::new("step.entity"), |stage| {
            stage.insert_model(&ctx, point("step:model:point#dropped"))?;
            Err::<(), _>(CodecError::malformed("damaged entity"))
        })
        .unwrap();
    assert_eq!(value, None);
    let (model, _, fidelity, losses) = output.into_parts();
    assert!(model.points.is_empty());
    let [(id, record)] = fidelity.retained_records().iter().collect::<Vec<_>>()[..] else {
        panic!("one retained record");
    };
    assert!(id.as_str().starts_with("step:source:part#"));
    assert_eq!(record.stream(), "");
    assert_eq!(record.offset(), 2);
    assert_eq!(record.data(), Some(b"CDE".as_slice()));
    assert_eq!(
        record.untyped(),
        &ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    assert_eq!(fidelity.unrepresented(), *record.untyped());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, Severity::Error);
    for text in ["step.entity", "offset=2", "length=3", "damaged entity"] {
        assert!(losses[0].message.contains(text), "{}", losses[0].message);
    }
    let provenance = losses[0].provenance.as_ref().expect("loss provenance");
    assert_eq!((provenance.offset, provenance.byte_len()), (2, Some(3)));
}

#[test]
fn the_part_identity_derives_from_codec_label_stream_and_offset() {
    let ids = |label: &'static str, stream: Option<&'static str>, start: usize| {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"ABCDEF", &arena, &DecodePolicy::service()).unwrap();
        let mut output = PartOutput::new(CodecFormat::Step);
        let mut spec = PartSpec::new(label);
        if let Some(stream) = stream {
            spec = spec.in_stream(crate::StreamName::try_from(stream.to_owned()).unwrap());
        }
        ctx.decode_part(&mut output, root.child(start, 6).unwrap(), spec, |_| {
            Err::<(), _>(CodecError::malformed("damaged"))
        })
        .unwrap();
        output
            .fidelity()
            .retained_records()
            .keys()
            .next()
            .unwrap()
            .clone()
    };
    let base = ids("a", None, 1);
    assert_eq!(ids("a", None, 1), base);
    assert_ne!(ids("b", None, 1), base);
    assert_ne!(ids("a", Some("member"), 1), base);
    assert_ne!(ids("a", None, 2), base);
}

#[test]
fn classes_default_to_the_request_and_can_be_narrowed() {
    for (depth, narrowed, expected) in [
        (
            DecodeDepth::Geometry,
            None,
            ArenaClasses::from([ArenaClass::Model]),
        ),
        (DecodeDepth::Container, None, ArenaClasses::new()),
        (
            DecodeDepth::Full,
            Some(ArenaClass::Design),
            ArenaClasses::from([ArenaClass::Design]),
        ),
    ] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes_at(b"AB", &arena, &DecodePolicy::service(), depth)
                .unwrap();
        let mut output = PartOutput::new(CodecFormat::Step);
        let spec = narrowed.map_or(PartSpec::new("step.data"), |class| {
            PartSpec::new("step.data").holding([class])
        });
        ctx.decode_part(&mut output, root, spec, |_| {
            Err::<(), _>(CodecError::malformed("damaged"))
        })
        .unwrap();
        assert_eq!(output.fidelity().unrepresented(), expected);
    }
}

#[test]
fn a_nested_failure_is_kept_by_a_successful_parent_and_dropped_with_a_failed_one() {
    for parent_fails in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::service()).unwrap();
        let mut output = PartOutput::new(CodecFormat::Step);
        ctx.decode_part(&mut output, root, PartSpec::new("parent"), |stage| {
            ctx.decode_part(
                stage,
                root.child(1, 2).unwrap(),
                PartSpec::new("child"),
                |_| Err::<(), _>(CodecError::malformed("child")),
            )?;
            if parent_fails {
                return Err(CodecError::malformed("parent"));
            }
            Ok(())
        })
        .unwrap();
        let (_, _, fidelity, losses) = output.into_parts();
        let records = fidelity.retained_records().values().collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(losses.len(), 1);
        let (data, label) = if parent_fails {
            (b"ABCD".as_slice(), "parent")
        } else {
            (b"B".as_slice(), "child")
        };
        assert_eq!(records[0].data(), Some(data));
        assert!(losses[0].message.contains(label));
    }
}

#[test]
fn a_part_scoped_limit_gives_up_only_the_part() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &policy).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let value = ctx
        .decode_part(&mut output, root, PartSpec::new("step.data"), |_| {
            assert!(ctx.begin_expand(ExpandSpec::Exact(2)).is_err());
            Ok(1)
        })
        .unwrap();
    assert_eq!(value, None);
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(output.fidelity().retained_records().len(), 1);
    assert_eq!(
        ctx.decode_part(&mut output, root, PartSpec::new("step.next"), |_| Ok(2))
            .unwrap(),
        Some(2)
    );
}

#[test]
fn a_session_scoped_limit_escapes_the_part() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &policy).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let error = ctx
        .decode_part(&mut output, root, PartSpec::new("step.data"), |stage| {
            stage.insert_model(&ctx, point("step:model:point#1"))
        })
        .expect_err("session refusal escapes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities
    ));
    assert!(output.fidelity().retained_records().is_empty());
}

#[test]
fn a_part_outside_the_root_stream_names_its_stream() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).unwrap();
    let mut writer = ctx.begin_expand(ExpandSpec::Exact(2)).unwrap();
    writer.write(b"XY").unwrap();
    let expanded = writer.finalize().unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    assert!(matches!(
        ctx.decode_part(&mut output, expanded, PartSpec::new("member"), |_| Ok(())),
        Err(CodecError::Malformed(_))
    ));
    let named = PartSpec::new("member").in_stream(crate::stream_name!("member.bin"));
    ctx.decode_part(&mut output, expanded, named, |_| {
        Err::<(), _>(CodecError::malformed("damaged member"))
    })
    .unwrap();
    let record = output
        .fidelity()
        .retained_records()
        .values()
        .next()
        .unwrap();
    assert_eq!(record.stream(), "member.bin");
    assert_eq!(record.data(), Some(b"XY".as_slice()));
}
