// SPDX-License-Identifier: Apache-2.0
//! The original parent, not a calculated row width, bounds sequential recovery.

use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodePolicy, LimitScope};

use super::*;
use crate::features::FinitePoint3;
use crate::ids::PointId;
use crate::math::Point3;
use crate::source_fidelity::parts::{DecodeParts, PartState};
use crate::topology::Point;
use crate::{CodecFormat, NativeUnknownRecord};

fn level(name: &str) -> PartLevel {
    PartLevel::parse(name).unwrap()
}

fn spec() -> PartSpec {
    PartSpec::new(level("freecad.brep_text_polygon3d"), Vec::new())
}

fn point(
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
    row: View<'_>,
    id: &str,
) -> Result<(), CodecError> {
    ctx.record(id, row, || {
        let mut cursor = row;
        let value = cursor
            .u8()
            .ok_or_else(|| CodecError::malformed("missing actual point byte"))?;
        output.insert_model(
            ctx,
            Point::new(
                PointId::mint(id).unwrap(),
                FinitePoint3::new(Point3::new(f64::from(value), 0.0, 0.0)).unwrap(),
                None,
            ),
        )
    })
}

#[test]
fn a_failed_sequential_row_owns_the_parent_suffix_and_never_visits_the_next_row() {
    let source = b"prefix1!23peer";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::service()).unwrap();
    let member = ctx
        .register_named_slice(root, ByteRange { start: 2, end: 14 }, "actual-member")
        .unwrap();
    let parent = member.child(4, 8).unwrap();
    let first = parent.child(4, 5).unwrap();
    let failed = parent.child(5, 6).unwrap();
    let unvisited = parent.child(6, 8).unwrap();
    let peer = member.child(8, 12).unwrap();
    assert_eq!(parent.window(), b"1!23");
    assert_eq!(failed.window(), b"!");
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let calls = Cell::new(0);
    ctx.decode_part(
        &mut output,
        parent,
        PartSpec::new(level("freecad.brep_text_table"), Vec::new()),
        |stage| {
            let mut sequence = SequentialParts::new(
                &ctx,
                parent,
                level("freecad.brep_text_table"),
                level("freecad.brep_text_polygon3d"),
            )?;
            assert!(sequence
                .decode_part(&ctx, stage, first, spec(), |row| {
                    calls.set(calls.get() + 1);
                    point(&ctx, row, first, "test:model:point#first")
                })?
                .is_some());
            assert!(!sequence.stopped());
            assert!(sequence
                .decode_part::<()>(&ctx, stage, failed, spec(), |row| {
                    calls.set(calls.get() + 1);
                    point(&ctx, row, failed, "test:model:point#failed")?;
                    Err(CodecError::malformed("invalid point family"))
                })?
                .is_none());
            assert!(sequence.stopped());
            assert!(sequence
                .decode_part(&ctx, stage, unvisited, spec(), |row| {
                    calls.set(calls.get() + 1);
                    point(&ctx, row, unvisited, "test:model:point#unvisited")
                })
                .is_err());
            Ok(())
        },
    )
    .unwrap()
    .unwrap();
    ctx.decode_part(
        &mut output,
        peer,
        PartSpec::new(level("freecad.brep_text_table"), Vec::new()),
        |stage| point(&ctx, stage, peer, "test:model:point#peer"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(output.model().points.len(), 2);
    assert_eq!(
        output.model().points[0].id.as_str(),
        "test:model:point#first"
    );
    assert_eq!(output.model().points[0].position().get().x, f64::from(b'1'));
    assert_eq!(
        output.model().points[1].id.as_str(),
        "test:model:point#peer"
    );
    let namespace = output.native().namespace("fcstd").unwrap();
    let unknowns = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one sequential failure home")
    };
    let fidelity = output.fidelity();
    let failed_part = &fidelity.parts()[&unknown.id];
    assert_eq!(failed_part.level().as_str(), "freecad.brep_text_polygon3d");
    assert_eq!(failed_part.state(), &PartState::GivenUp {});
    assert_eq!(failed_part.stream(), "actual-member");
    assert_eq!((failed_part.offset(), failed_part.byte_len()), (5, 3));
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(b"!23".as_slice())
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .unwrap()
            .sha256(),
        crate::hash::sha256_hex(b"!23")
    );
    assert!(fidelity.parts().values().all(|part| {
        part.level().as_str() != "freecad.brep_text_polygon3d" || part.offset() != 6
    }));
    let layout = &fidelity.record_layout()[unknown.id.as_str()];
    assert!(layout.reads().is_empty());
    assert_eq!(output.losses().len(), 1);
    let loss = &output.losses()[0];
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert_eq!(loss.severity, crate::report::Severity::Error);
    assert!(loss
        .message
        .ends_with("dropped=[\"test:model:point#failed\"]"));
    let location = loss.provenance.as_ref().unwrap();
    assert_eq!(location.stream_name(), Some("actual-member"));
    assert_eq!(location.offset, 5);
    assert_eq!(location.byte_len(), Some(3));
    let first_part = fidelity
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.brep_text_polygon3d" && part.offset() == 4)
        .unwrap();
    assert_eq!(first_part.byte_len(), 1);
    assert_eq!(first_part.state(), &PartState::Decoded {});
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].id(), "test:model:point#first");
    assert_eq!(captures[0].reads()["actual-member"], [4..5]);
    assert_eq!(captures[1].id(), "test:model:point#peer");
    assert_eq!(captures[1].reads()["actual-member"], [8..9]);
}

#[test]
fn extent_only_rows_still_stop_at_the_actual_parent_end() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        b"012345678901234567890123",
        &arena,
        &DecodePolicy::service(),
    )
    .unwrap();
    let parent = root.child(2, 22).unwrap();
    let row = root.child(8, 12).unwrap();
    let mut output = PartOutput::new(CodecFormat::Nx);
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("nx.om_offset_store"),
        level("nx.om_store_column_block"),
    )
    .unwrap();
    let calls = Cell::new(0);
    assert!(sequence
        .decode_part::<()>(
            &ctx,
            &mut output,
            row,
            PartSpec::new(level("nx.om_store_column_block"), Vec::new()),
            |_| {
                calls.set(calls.get() + 1);
                let mut actual = row;
                assert_eq!(actual.u8(), Some(b'8'));
                Err(CodecError::malformed("actual column family is unavailable"))
            }
        )
        .unwrap()
        .is_none());
    assert_eq!(calls.get(), 1);
    assert!(sequence.stopped());
    let part = output.fidelity().parts().values().next().unwrap();
    assert_eq!((part.offset(), part.byte_len()), (8, 14));
    let retained = output
        .fidelity()
        .retained_records()
        .values()
        .next()
        .unwrap();
    assert_eq!(retained.data(), Some(b"89012345678901".as_slice()));
}

#[test]
fn an_unlocated_row_uses_its_actual_prefix_without_inventing_a_successful_extent() {
    let arena = DecodeArena::new();
    let (ctx, parent) =
        DecodeContext::from_root_bytes(b"1?234", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap();
    let first = parent.child(0, 1).unwrap();
    sequence
        .decode_part(&ctx, &mut output, first, spec(), |stage| {
            point(&ctx, stage, first, "test:model:point#first")
        })
        .unwrap()
        .unwrap();
    let prefix = parent.child(1, 2).unwrap();
    let calls = Cell::new(0);
    sequence
        .fail_from(&ctx, &mut output, prefix, spec(), |_| {
            calls.set(calls.get() + 1);
            ctx.record("test:model:point#unlocated", prefix, || {
                let mut actual = prefix;
                let byte = actual
                    .u8()
                    .ok_or_else(|| CodecError::malformed("absent row count"))?;
                if !byte.is_ascii_digit() {
                    return Err(CodecError::malformed("invalid actual row count"));
                }
                panic!("the original count token is malformed")
            })
        })
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(sequence.stopped());
    assert_eq!(output.model().points.len(), 1);
    let part = output
        .fidelity()
        .parts()
        .values()
        .find(|part| matches!(part.state(), PartState::GivenUp {}))
        .unwrap();
    assert_eq!((part.offset(), part.byte_len()), (1, 4));
    assert_eq!(
        output
            .fidelity()
            .retained_record(part_id(&output))
            .unwrap()
            .data(),
        Some(b"?234".as_slice())
    );
    assert!(output.losses()[0]
        .message
        .ends_with("dropped=[\"test:model:point#unlocated\"]"));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "test:model:point#first");
}

fn part_id(output: &PartOutput) -> &str {
    output
        .fidelity()
        .parts()
        .iter()
        .find(|(_, part)| matches!(part.state(), PartState::GivenUp {}))
        .unwrap()
        .0
        .as_str()
}

#[test]
fn invalid_registry_policy_and_source_relationships_never_run_the_callback() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"12345", &arena, &DecodePolicy::service()).unwrap();
    for (parent, child) in [
        ("zip.archive", "zip.entry"),
        ("step.data_section", "step.simple_entity_instance"),
        ("zip.archive", "freecad.brep_text_polygon3d"),
    ] {
        assert!(SequentialParts::new(&ctx, root, level(parent), level(child)).is_err());
    }
    let parent = root.child(1, 4).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 4 }, "another-space")
        .unwrap();
    assert_ne!(alias.location().space, parent.location().space);
    let foreign_arena = DecodeArena::new();
    let (foreign, foreign_root) =
        DecodeContext::from_root_bytes(b"12345", &foreign_arena, &DecodePolicy::service()).unwrap();
    assert!(SequentialParts::new(
        &foreign,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d")
    )
    .is_err());
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let calls = Cell::new(0);
    for row in [
        alias,
        foreign_root.child(1, 2).unwrap(),
        root.child(0, 2).unwrap(),
        root.child(3, 5).unwrap(),
        root.child(2, 2).unwrap(),
    ] {
        assert!(sequence
            .decode_part(&ctx, &mut output, row, spec(), |_| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .is_err());
    }
    assert!(sequence
        .decode_part(
            &foreign,
            &mut output,
            root.child(2, 3).unwrap(),
            spec(),
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        )
        .is_err());
    assert!(sequence
        .decode_part(
            &ctx,
            &mut output,
            root.child(2, 3).unwrap(),
            PartSpec::new(level("freecad.brep_text_curve"), Vec::new()),
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        )
        .is_err());
    assert_eq!(calls.get(), 0);
    assert!(output.fidelity().parts().is_empty());
    let first = root.child(2, 3).unwrap();
    sequence
        .decode_part(&ctx, &mut output, first, spec(), |_| Ok(()))
        .unwrap()
        .unwrap();
    assert!(sequence
        .decode_part(&ctx, &mut output, root.child(1, 2).unwrap(), spec(), |_| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 0);
}

#[test]
fn a_session_refusal_escapes_without_a_suffix_home_and_stops_the_sequence() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, parent) = DecodeContext::from_root_bytes(b"123", &arena, &policy).unwrap();
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let row = parent.child(0, 1).unwrap();
    let error = sequence
        .decode_part(&ctx, &mut output, row, spec(), |stage| {
            point(&ctx, stage, row, "test:model:point#fatal")
        })
        .unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session)
    );
    assert!(sequence.stopped());
    assert!(output.fidelity().parts().is_empty());
    assert!(output.losses().is_empty());
    assert!(output.native().namespace("fcstd").is_none());
    assert!(output.model().points.is_empty());
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn an_unwind_rolls_back_actual_reads_and_prevents_later_sibling_calls() {
    let arena = DecodeArena::new();
    let (ctx, parent) =
        DecodeContext::from_root_bytes(b"123", &arena, &DecodePolicy::service()).unwrap();
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let row = parent.child(0, 1).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = sequence.decode_part::<()>(&ctx, &mut output, row, spec(), |stage| {
            point(&ctx, stage, row, "test:model:point#unwound")?;
            panic!("actual parser callback unwound")
        });
    }))
    .is_err());
    assert!(sequence.stopped());
    let calls = Cell::new(0);
    assert!(sequence
        .decode_part(
            &ctx,
            &mut output,
            parent.child(1, 2).unwrap(),
            spec(),
            |_| {
                calls.set(1);
                Ok(())
            }
        )
        .is_err());
    assert_eq!(calls.get(), 0);
    assert!(output.model().points.is_empty());
    assert!(output.fidelity().parts().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

fn counted_source(bytes: &[u8]) -> (PartOutput, Vec<cadmpeg_core::decode::RecordCapture>, usize) {
    let arena = DecodeArena::new();
    let (ctx, parent) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut header = parent.child(0, 1).unwrap();
    let declared = usize::from(header.u8().unwrap() - b'0');
    assert_eq!(declared, 2);
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap();
    let calls = Cell::new(0);
    for ordinal in 0..declared {
        let start = ordinal + 1;
        if start == parent.end() {
            let eof = parent.child(parent.end(), parent.end()).unwrap();
            sequence
                .fail_from(&ctx, &mut output, eof, spec(), |_| {
                    calls.set(calls.get() + 1);
                    ctx.record("test:model:point#missing", eof, || {
                        let mut actual = eof;
                        actual.u8().ok_or_else(|| {
                            CodecError::malformed("truncated actual sequential point")
                        })?;
                        panic!("an actual EOF cannot supply a row byte")
                    })
                })
                .unwrap();
            assert!(sequence.stopped());
            assert!(sequence
                .fail_from(&ctx, &mut output, eof, spec(), |_| {
                    calls.set(calls.get() + 1);
                    panic!("a stopped parent cannot run another parser")
                })
                .is_err());
            break;
        }
        let row = parent.child(start, start + 1).unwrap();
        sequence
            .decode_part(&ctx, &mut output, row, spec(), |stage| {
                calls.set(calls.get() + 1);
                point(&ctx, stage, row, &format!("test:model:point#{ordinal}"))
            })
            .unwrap()
            .unwrap();
    }
    let captures = ctx.take_record_captures().unwrap();
    (output, captures, calls.get())
}

#[test]
fn an_actual_eof_after_one_of_two_declared_rows_preserves_the_first_record_and_its_reads() {
    let (complete, complete_reads, complete_calls) = counted_source(b"212");
    assert_eq!(complete.model().points.len(), 2);
    assert_eq!(complete_reads.len(), 2);
    assert_eq!(complete_calls, 2);
    assert!(complete.losses().is_empty());
    let (cut, cut_reads, cut_calls) = counted_source(b"21");
    assert_eq!(cut.model().points, complete.model().points[..1]);
    assert_eq!(cut_reads, complete_reads[..1]);
    assert_eq!(cut_reads[0].reads()["file"], [1..2]);
    assert_eq!(cut_calls, 2);
    let namespace = cut.native().namespace("fcstd").unwrap();
    let unknowns = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one actual EOF failure")
    };
    let fidelity = cut.fidelity();
    let failed = &fidelity.parts()[&unknown.id];
    assert_eq!(failed.state(), &PartState::GivenUp {});
    assert_eq!(failed.level().as_str(), "freecad.brep_text_polygon3d");
    assert_eq!((failed.offset(), failed.byte_len()), (2, 0));
    let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
    assert_eq!(retained.data(), Some([].as_slice()));
    assert_eq!(retained.sha256(), crate::hash::sha256_hex(&[]));
    assert_eq!(fidelity.retained_records().len(), 1);
    assert!(fidelity.record_layout()[unknown.id.as_str()]
        .reads()
        .is_empty());
    let [loss] = cut.losses() else {
        panic!("one real truncation diagnostic")
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert_eq!(loss.severity, crate::report::Severity::Error);
    assert!(loss.message.contains("truncated actual sequential point"));
    assert!(loss
        .message
        .ends_with("dropped=[\"test:model:point#missing\"]"));
    let location = loss.provenance.as_ref().unwrap();
    assert_eq!(location.stream_name(), Some("file"));
    assert_eq!(location.offset, 2);
    assert_eq!(location.byte_len(), None);
}

#[test]
fn empty_prefixes_outside_actual_parent_eof_are_refused_before_parsing() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    let parent = root.child(1, 3).unwrap();
    let other_arena = DecodeArena::new();
    let (other, other_root) =
        DecodeContext::from_root_bytes(b"1234", &other_arena, &DecodePolicy::service()).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 3 }, "other-space")
        .unwrap();
    let mut sequence = SequentialParts::new(
        &ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let calls = Cell::new(0);
    for prefix in [
        parent.child(1, 1).unwrap(),
        parent.child(2, 2).unwrap(),
        root.child(4, 4).unwrap(),
        other_root.child(3, 3).unwrap(),
        alias.child(alias.end(), alias.end()).unwrap(),
    ] {
        assert!(sequence
            .fail_from(&ctx, &mut output, prefix, spec(), |_| {
                calls.set(calls.get() + 1);
                panic!("invalid empty source must not run a parser")
            })
            .is_err());
    }
    let eof = parent.child(parent.end(), parent.end()).unwrap();
    assert!(sequence
        .fail_from(&other, &mut output, eof, spec(), |_| {
            calls.set(calls.get() + 1);
            panic!("foreign parent must not run a parser")
        })
        .is_err());
    assert!(sequence
        .decode_part(&ctx, &mut output, eof, spec(), |_| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 0);
    assert!(!sequence.stopped());
    assert!(output.fidelity().parts().is_empty());
    assert!(output.losses().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

mod discovered;
