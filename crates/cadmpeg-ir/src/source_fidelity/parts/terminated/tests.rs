// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodePolicy, LimitScope};

use super::*;
use crate::features::FinitePoint3;
use crate::ids::PointId;
use crate::math::Point3;
use crate::source_fidelity::parts::PartState;
use crate::topology::Point;
use crate::{CodecFormat, NativeUnknownRecord};

fn level(name: &str) -> PartLevel {
    PartLevel::parse(name).unwrap()
}
fn parent<'a>(ctx: &DecodeContext<'_>, source: View<'a>, count: usize) -> TerminatedParent<'a> {
    TerminatedParent::new(
        ctx,
        source.child(0, 2).unwrap(),
        source,
        PartSpec::new(level("freecad.brep_text_table"), vec![0..2]),
        level("freecad.brep_text_tshape"),
        count,
    )
    .unwrap()
}
fn row_spec(row: View<'_>) -> PartSpec {
    PartSpec::new(
        level("freecad.brep_text_tshape"),
        vec![
            cadmpeg_core::decode::u64_from_index(row.end() - 1)
                ..cadmpeg_core::decode::u64_from_index(row.end()),
        ],
    )
}
fn point(
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
    row: View<'_>,
    name: &str,
) -> Result<(), CodecError> {
    ctx.record(name, row, || {
        let mut cursor = row;
        let value = cursor
            .u8()
            .ok_or_else(|| CodecError::truncated(row.location(), "point byte"))?;
        output.insert_model(
            ctx,
            Point::new(
                PointId::mint(name).unwrap(),
                FinitePoint3::new(Point3::new(f64::from(value), 0.0, 0.0)).unwrap(),
                None,
            ),
        )
    })
}

#[test]
fn missing_nth_terminator_keeps_prior_rows_and_gives_up_only_actual_eof_prefix() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"2 A*B", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let first = source.child(2, 4).unwrap();
    let prefix = source.child(4, 5).unwrap();
    let result = parent(&ctx, source, 2)
        .decode(&ctx, &mut output, |stage, children| {
            children.decode_part(&ctx, stage, first, row_spec(first), |row| {
                point(&ctx, row, first, "test:model:point#first")
            })?;
            children.fail_at_source_end(
                &ctx,
                stage,
                prefix,
                PartSpec::new(level("freecad.brep_text_tshape"), vec![]),
                |row| {
                    point(&ctx, row, prefix, "test:model:point#discarded")?;
                    Err(CodecError::truncated(
                        prefix.location(),
                        "missing TShape delimiter",
                    ))
                },
            )?;
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert!(result.1.closed_extent().is_none());
    assert_eq!(output.model().points.len(), 1);
    assert_eq!(
        output.model().points[0].id.as_str(),
        "test:model:point#first"
    );
    assert_eq!(output.model().points[0].position().get().x, f64::from(b'A'));
    let unknowns = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    let failed = &output.fidelity().parts()[&unknowns[0].id];
    assert_eq!(failed.level().as_str(), "freecad.brep_text_tshape");
    assert_eq!((failed.offset(), failed.byte_len()), (4, 1));
    assert_eq!(failed.state(), &PartState::GivenUp {});
    assert!(failed.framing_bytes().is_empty());
    assert_eq!(
        output
            .fidelity()
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(b"B".as_slice())
    );
    assert_eq!(output.losses().len(), 1);
    assert_eq!(output.losses()[0].code.local_code(), "record.unreadable");
    assert_eq!(output.losses()[0].severity, crate::report::Severity::Error);
    assert!(output.losses()[0]
        .message
        .ends_with("dropped=[\"test:model:point#discarded\"]"));
    let table = output
        .fidelity()
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.brep_text_table")
        .unwrap();
    assert_eq!((table.offset(), table.byte_len()), (0, 5));
    assert_eq!(table.state(), &PartState::Decoded {});
    assert_eq!(
        failed.parent(),
        output
            .fidelity()
            .parts()
            .iter()
            .find_map(
                |(id, part)| (part.level().as_str() == "freecad.brep_text_table").then_some(id)
            )
    );
    assert!(output
        .fidelity()
        .parts()
        .values()
        .all(|part| part.level().as_str() != "freecad.brep_text_root"));
    let retained = output
        .fidelity()
        .retained_record(unknowns[0].id.as_str())
        .unwrap();
    assert_eq!(retained.sha256(), crate::hash::sha256_hex(b"B"));
    assert!(output.fidelity().record_layout()[unknowns[0].id.as_str()]
        .reads()
        .is_empty());
    let location = output.losses()[0].provenance.as_ref().unwrap();
    assert_eq!(location.stream_name(), Some("file"));
    assert_eq!((location.offset, location.byte_len()), (4, Some(1)));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "test:model:point#first");
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (2, 2));
    assert_eq!(captures[0].reads()["file"], [2..3]);
}

#[test]
fn damaged_marker_bounded_middle_row_keeps_later_peer_and_complete_root_locator() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"3 A*!*B*R", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let result = parent(&ctx, source, 3)
        .decode(&ctx, &mut output, |stage, children| {
            for (start, end, name) in [
                (2, 4, "test:model:point#first"),
                (4, 6, "test:model:point#bad"),
                (6, 8, "test:model:point#last"),
            ] {
                let row = source.child(start, end).unwrap();
                let accepted = children.decode_part(&ctx, stage, row, row_spec(row), |stage| {
                    point(&ctx, stage, row, name)?;
                    if start == 4 {
                        Err(CodecError::malformed("damaged actual row"))
                    } else {
                        Ok(())
                    }
                })?;
                assert_eq!(accepted.is_some(), start != 4);
            }
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(result.1.closed_extent().unwrap().window(), b"3 A*!*B*");
    let root = source.child(8, 9).unwrap();
    if let Some(closed) = result.1.closed_extent() {
        assert_eq!(closed.end(), root.start());
        ctx.decode_part(
            &mut output,
            root,
            PartSpec::new(level("freecad.brep_text_root"), vec![]),
            |stage| point(&ctx, stage, root, "test:model:point#root"),
        )
        .unwrap()
        .unwrap();
    }
    assert_eq!(
        output
            .model()
            .points
            .iter()
            .map(|point| point.position().get().x)
            .collect::<Vec<_>>(),
        vec![f64::from(b'A'), f64::from(b'B'), f64::from(b'R')]
    );
    let table = output
        .fidelity()
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.brep_text_table")
        .unwrap();
    assert_eq!(table.byte_len(), 8);
    assert_eq!(output.losses().len(), 1);
    let retained = output
        .fidelity()
        .retained_records()
        .values()
        .next()
        .unwrap();
    assert_eq!(retained.data(), Some(b"!*".as_slice()));
}

#[test]
fn zero_count_ends_at_actual_header_and_does_not_need_a_child_delimiter() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"0 R", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let result = parent(&ctx, source, 0)
        .decode(&ctx, &mut output, |_stage, _children| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(result.1.closed_extent().unwrap().window(), b"0 ");
    assert_eq!(
        output
            .fidelity()
            .parts()
            .values()
            .next()
            .unwrap()
            .byte_len(),
        2
    );
    assert!(output.losses().is_empty());
}

#[test]
fn clipped_or_foreign_navigation_cannot_establish_actual_eof() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"2 A*Bmore", &arena, &DecodePolicy::service()).unwrap();
    let other_arena = DecodeArena::new();
    let (_, foreign) =
        DecodeContext::from_root_bytes(b"2 A*B", &other_arena, &DecodePolicy::service()).unwrap();
    assert!(ctx.require_source_end(foreign).is_err());
    assert!(ctx.require_source_end(source.child(0, 5).unwrap()).is_err());
    assert!(ctx
        .require_source_end(source.child(4, source.end()).unwrap())
        .is_ok());
    assert!(TerminatedParent::new(
        &ctx,
        foreign.child(0, 2).unwrap(),
        source,
        PartSpec::new(level("freecad.brep_text_table"), vec![]),
        level("freecad.brep_text_tshape"),
        2
    )
    .is_err());
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let clipped = source.child(0, 5).unwrap();
    parent(&ctx, clipped, 2)
        .decode(&ctx, &mut output, |stage, children| {
            let first = source.child(2, 4).unwrap();
            children.decode_part(&ctx, stage, first, row_spec(first), |stage| {
                point(&ctx, stage, first, "test:model:point#rolled_back")
            })?;
            children.fail_at_source_end(
                &ctx,
                stage,
                source.child(4, 5).unwrap(),
                PartSpec::new(level("freecad.brep_text_tshape"), vec![]),
                |_stage| {
                    Err(CodecError::truncated(
                        clipped.location(),
                        "missing TShape delimiter",
                    ))
                },
            )
        })
        .unwrap();
    assert!(output.model().points.is_empty());
    assert!(output
        .fidelity()
        .parts()
        .values()
        .all(|part| part.level().as_str() != "freecad.brep_text_root"));
}

#[test]
fn session_refusal_escapes_without_committing_prior_container_rows() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 1;
    let (ctx, source) = DecodeContext::from_root_bytes(b"2 A*B*", &arena, &policy).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let error = parent(&ctx, source, 2)
        .decode(&ctx, &mut output, |stage, children| {
            for (start, end, name) in [
                (2, 4, "test:model:point#first"),
                (4, 6, "test:model:point#last"),
            ] {
                let row = source.child(start, end).unwrap();
                children.decode_part(&ctx, stage, row, row_spec(row), |stage| {
                    point(&ctx, stage, row, name)
                })?;
            }
            Ok(())
        })
        .unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session)
    );
    assert!(output.model().points.is_empty());
    assert!(output.fidelity().parts().is_empty());
    assert!(output.losses().is_empty());
}

#[test]
fn eof_after_complete_prior_row_has_only_empty_failed_prefix_and_no_successful_last_row() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"2 A*", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let result = parent(&ctx, source, 2)
        .decode(&ctx, &mut output, |stage, children| {
            let first = source.child(2, 4).unwrap();
            children.decode_part(&ctx, stage, first, row_spec(first), |row| {
                point(&ctx, row, first, "test:model:point#first")
            })?;
            let eof = source.child(4, 4).unwrap();
            children.fail_at_source_end(
                &ctx,
                stage,
                eof,
                PartSpec::new(level("freecad.brep_text_tshape"), vec![]),
                |_stage| Err(CodecError::truncated(eof.location(), "missing next TShape")),
            )?;
            assert!(children
                .decode_part::<()>(&ctx, stage, first, row_spec(first), |_row| panic!(
                    "no row after EOF"
                ))
                .is_err());
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert!(result.1.closed_extent().is_none());
    assert_eq!(output.model().points.len(), 1);
    let failed = output
        .fidelity()
        .parts()
        .values()
        .find(|part| part.state() == &PartState::GivenUp {})
        .unwrap();
    assert_eq!((failed.offset(), failed.byte_len()), (4, 0));
    assert!(failed.framing_bytes().is_empty());
    assert_eq!(
        output
            .fidelity()
            .retained_records()
            .values()
            .next()
            .unwrap()
            .data(),
        Some(b"".as_slice())
    );
}

#[test]
fn absent_final_framing_interval_never_runs_a_successful_row_callback() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"1 A", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let result = parent(&ctx, source, 1)
        .decode(&ctx, &mut output, |stage, children| {
            children.decode_part(
                &ctx,
                stage,
                source.child(2, 3).unwrap(),
                PartSpec::new(level("freecad.brep_text_tshape"), vec![]),
                |_row| -> Result<(), CodecError> {
                    panic!("EOF is not a successful TShape delimiter")
                },
            )
        })
        .unwrap();
    assert!(result.is_none());
    assert!(output.model().points.is_empty());
    assert!(!output
        .fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.brep_text_tshape"
            && part.state() == &PartState::Decoded {}));
    assert_eq!(
        output
            .fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .count(),
        1
    );
}

#[test]
fn authenticated_source_end_query_adds_no_read_capture_or_retained_charge() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"1 A", &arena, &DecodePolicy::service()).unwrap();
    let before = ctx.retained_bytes_available();
    ctx.require_source_end(source).unwrap();
    ctx.require_source_end(source.child(2, 3).unwrap()).unwrap();
    assert!(ctx.require_source_end(source.child(0, 2).unwrap()).is_err());
    assert!(ctx.require_source_end(View::over_retained(b"1 A")).is_err());
    assert_eq!(ctx.retained_bytes_available(), before);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
