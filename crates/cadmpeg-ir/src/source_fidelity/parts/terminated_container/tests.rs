// SPDX-License-Identifier: Apache-2.0
use std::cell::Cell;
use std::convert::Infallible;

use cadmpeg_core::decode::{DecodeArena, DecodePolicy, LimitScope};

use super::*;
use crate::features::FinitePoint3;
use crate::ids::PointId;
use crate::math::Point3;
use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartState, SequentialParts};
use crate::topology::Point;
use crate::{CodecFormat, NativeUnknownRecord};

fn spec(name: &str) -> PartSpec {
    PartSpec::new(PartLevel::parse(name).unwrap(), Vec::new())
}

fn location(
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
    source: View<'_>,
) -> Result<(), CodecError> {
    ctx.record("test:model:point#location", source, || {
        let mut source = source;
        let bytes = source.read_remaining().unwrap();
        let values = std::str::from_utf8(bytes)
            .unwrap()
            .split_ascii_whitespace()
            .map(|value| value.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 13);
        output.insert_model(
            ctx,
            Point::new(
                PointId::mint("test:model:point#location").unwrap(),
                FinitePoint3::new(Point3::new(values[4], values[8], values[12])).unwrap(),
                None,
            ),
        )
    })
}

#[test]
fn complete_declared_rows_survive_a_failed_next_table_header_attempt_at_partial_or_empty_eof() {
    for bytes in [
        b"Locations 1\n1 1 0 0 7 0 1 0 8 0 0 1 9\nCurve2".as_slice(),
        b"Locations 1\n1 1 0 0 7 0 1 0 8 0 0 1 9".as_slice(),
    ] {
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        let opening = source.child(0, b"Locations 1".len()).unwrap();
        let end = bytes
            .windows(b"\nCurve2".len())
            .position(|value| value == b"\nCurve2")
            .unwrap_or(bytes.len());
        let row = source.child(opening.end(), end).unwrap();
        let mut output = PartOutput::new(CodecFormat::Fcstd);
        let calls = Cell::new(0);
        ctx.decode_part(&mut output, source, spec("freecad.brep_text"), |member| {
            let container =
                TerminatedContainer::new(&ctx, opening, source, spec("freecad.brep_text_table"))?;
            let (_, consumed) = container
                .decode(&ctx, member, |table| {
                    calls.set(calls.get() + 1);
                    let mut children = SequentialParts::new(
                        &ctx,
                        source,
                        PartLevel::parse("freecad.brep_text_table").unwrap(),
                        PartLevel::parse("freecad.brep_text_location").unwrap(),
                    )?;
                    children
                        .decode_part(
                            &ctx,
                            table,
                            row,
                            spec("freecad.brep_text_location"),
                            |stage| location(&ctx, stage, row),
                        )?
                        .unwrap();
                    Ok(((), source.child(source.start(), row.end()).unwrap()))
                })?
                .unwrap();
            assert_eq!(consumed.end(), row.end());
            let next = source.child(consumed.end(), source.end()).unwrap();
            ctx.require_source_end(next)?;
            let attempted = ctx.decode_part(
                member,
                next,
                spec("freecad.brep_text_table"),
                |_| -> Result<Infallible, CodecError> {
                    ctx.record("test:header#next", next, || {
                        let mut cursor = next;
                        let prefix = cursor.read_remaining().unwrap();
                        assert!(
                            b"Curve2ds".starts_with(prefix.strip_prefix(b"\n").unwrap_or(prefix))
                        );
                        Err(CodecError::truncated(
                            cursor.location(),
                            "next B-rep table header",
                        ))
                    })
                },
            )?;
            assert!(attempted.is_none());
            Ok(())
        })
        .unwrap()
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(output.model().points.len(), 1);
        assert_eq!(
            output.model().points[0].position().get(),
            Point3::new(7.0, 8.0, 9.0)
        );
        let unknowns = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<NativeUnknownRecord>("unknowns")
            .unwrap();
        assert_eq!(unknowns.len(), 1);
        let fidelity = output.fidelity();
        let failed = &fidelity.parts()[&unknowns[0].id];
        assert_eq!(failed.level().as_str(), "freecad.brep_text_table");
        assert_eq!(failed.state(), &PartState::GivenUp {});
        assert_eq!(
            (failed.offset(), failed.byte_len()),
            (
                cadmpeg_core::decode::u64_from_index(end),
                cadmpeg_core::decode::u64_from_index(bytes.len() - end)
            )
        );
        assert_eq!(
            fidelity
                .retained_record(unknowns[0].id.as_str())
                .unwrap()
                .data(),
            Some(&bytes[end..])
        );
        let decoded_table = fidelity
            .parts()
            .values()
            .find(|part| {
                part.level().as_str() == "freecad.brep_text_table"
                    && part.state() == &PartState::Decoded {}
            })
            .unwrap();
        assert_eq!(failed.parent(), decoded_table.parent());
        assert_eq!(
            decoded_table.byte_len(),
            cadmpeg_core::decode::u64_from_index(end)
        );
        assert_eq!(
            fidelity
                .parts()
                .values()
                .filter(|part| part.level().as_str() == "freecad.brep_text_location")
                .count(),
            1
        );
        assert_eq!(output.losses().len(), 1);
        assert!(output.losses()[0]
            .message
            .ends_with("dropped=[\"test:header#next\"]"));
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].id(), "test:model:point#location");
        assert_eq!(
            captures[0].reads()["file"],
            [cadmpeg_core::decode::u64_from_index(opening.end())
                ..cadmpeg_core::decode::u64_from_index(end)]
        );
    }
}

#[test]
fn facade_rejects_nonterminated_levels_foreign_openings_and_empty_success_prefixes() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::service()).unwrap();
    assert!(
        TerminatedContainer::new(&ctx, source, source, spec("freecad.brep_binary_record")).is_err()
    );
    assert!(TerminatedContainer::new(&ctx, source, source, spec("freecad.brep_text")).is_err());
    assert!(TerminatedContainer::new(
        &ctx,
        source.child(3, 3).unwrap(),
        source.child(3, 3).unwrap(),
        spec("freecad.brep_text_table")
    )
    .is_err());
    let other = DecodeArena::new();
    let (_, foreign) =
        DecodeContext::from_root_bytes(b"abc", &other, &DecodePolicy::service()).unwrap();
    assert!(
        TerminatedContainer::new(&ctx, foreign, source, spec("freecad.brep_text_table")).is_err()
    );
    assert!(TerminatedContainer::new(
        &ctx,
        source,
        source.child(0, 2).unwrap(),
        spec("freecad.brep_text_table")
    )
    .is_err());
}

#[test]
fn facade_cannot_hide_new_tail_reads_behind_a_shorter_delivered_prefix() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"headTAIL", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let calls = Cell::new(0);
    let result = TerminatedContainer::new(
        &ctx,
        source.child(0, 4).unwrap(),
        source,
        spec("freecad.brep_text_table"),
    )
    .unwrap()
    .decode(&ctx, &mut output, |_| {
        calls.set(calls.get() + 1);
        assert_eq!(source.read_slice(4..8).unwrap(), b"TAIL");
        Ok(((), source.child(0, 4).unwrap()))
    })
    .unwrap();
    assert!(result.is_none());
    assert_eq!(calls.get(), 1);
    assert_eq!(output.losses().len(), 1);
    assert_eq!(
        output
            .fidelity()
            .retained_records()
            .values()
            .next()
            .unwrap()
            .data(),
        Some(b"headTAIL".as_slice())
    );
}

#[test]
fn actual_session_refusal_aborts_open_container_without_partial_delivery() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let bytes = b"Locations 1\n1 1 0 0 7 0 1 0 8 0 0 1 9";
    let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let opening = source.child(0, b"Locations 1".len()).unwrap();
    let row = source.child(opening.end(), source.end()).unwrap();
    let error = TerminatedContainer::new(&ctx, opening, source, spec("freecad.brep_text_table"))
        .unwrap()
        .decode(&ctx, &mut output, |stage| {
            location(&ctx, stage, row)?;
            Ok(((), source))
        })
        .unwrap_err();
    assert!(matches!(error,CodecError::ResourceLimit(limit)if limit.scope==LimitScope::Session));
    assert!(output.model().points.is_empty());
    assert!(output.fidelity().parts().is_empty());
    assert!(output.losses().is_empty());
}

#[test]
fn facade_preserves_primary_extent_and_foreign_result_guards() {
    for oversized_primary in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(b"headTAIL", &arena, &DecodePolicy::service()).unwrap();
        let other = DecodeArena::new();
        let (_, foreign) =
            DecodeContext::from_root_bytes(b"headTAIL", &other, &DecodePolicy::service()).unwrap();
        let prefix = source.child(0, 4).unwrap();
        let mut output = PartOutput::new(CodecFormat::Fcstd);
        let result =
            TerminatedContainer::new(&ctx, prefix, source, spec("freecad.brep_text_table"))
                .unwrap()
                .decode(&ctx, &mut output, |_| {
                    if oversized_primary {
                        ctx.record("test:primary#tail", source, || Ok(()))?;
                        Ok(((), prefix))
                    } else {
                        Ok(((), foreign))
                    }
                })
                .unwrap();
        assert!(result.is_none());
        assert_eq!(output.losses().len(), 1);
        assert_eq!(
            output
                .fidelity()
                .retained_records()
                .values()
                .next()
                .unwrap()
                .data(),
            Some(b"headTAIL".as_slice())
        );
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn actual_io_error_is_not_replaced_by_a_container_give_up() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"header", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let error = TerminatedContainer::new(&ctx, source, source, spec("freecad.brep_text_table"))
        .unwrap()
        .decode(
            &ctx,
            &mut output,
            |_| -> Result<((), View<'_>), CodecError> {
                Err(std::io::Error::other("actual parser I/O refusal").into())
            },
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::Io(_)));
    assert!(output.fidelity().parts().is_empty());
    assert!(output.losses().is_empty());
}
