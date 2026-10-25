// SPDX-License-Identifier: Apache-2.0
//! Variable-width rows use their actual parser cursor, never a framing preparse.

use super::super::*;
use crate::features::FinitePoint3;
use crate::ids::PointId;
use crate::math::Point3;
use crate::source_fidelity::parts::PartState;
use crate::topology::Point;
use crate::{CodecFormat, NativeUnknownRecord};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use std::cell::Cell;

fn level(name: &str) -> PartLevel {
    PartLevel::parse(name).unwrap()
}
fn spec() -> PartSpec {
    PartSpec::new(level("freecad.brep_text_polygon3d"), Vec::new())
}
fn sequence<'a>(ctx: &DecodeContext<'_>, parent: View<'a>) -> SequentialParts<'a> {
    SequentialParts::new(
        ctx,
        parent,
        level("freecad.brep_text_table"),
        level("freecad.brep_text_polygon3d"),
    )
    .unwrap()
}
fn variable_row<'a>(
    ctx: &DecodeContext<'_>,
    stage: &mut PartOutput,
    remaining: View<'a>,
    prefix: View<'a>,
    id: &str,
) -> Result<(u64, View<'a>), CodecError> {
    ctx.record(id, prefix, || {
        let mut cursor = remaining;
        let count = usize::from(cursor.req_u8()?);
        let mut value = 0u64;
        for _ in 0..count {
            value += u64::from(cursor.req_u8()?);
        }
        let consumed = remaining
            .child(remaining.start(), cursor.position())
            .unwrap();
        stage.insert_model(
            ctx,
            Point::new(
                PointId::mint(id).unwrap(),
                FinitePoint3::new(Point3::new(
                    f64::from(u32::try_from(value).unwrap()),
                    0.0,
                    0.0,
                ))
                .unwrap(),
                None,
            ),
        )?;
        Ok((value, consumed))
    })
}

#[test]
fn one_actual_variable_width_parse_preserves_the_first_row_and_stops_at_bad_next_or_eof() {
    for source in [&b"\x02\x03\x04\x03\x09"[..], &b"\x02\x03\x04"[..]] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::service()).unwrap();
        let mut rows = sequence(&ctx, root);
        let mut output = PartOutput::new(CodecFormat::Fcstd);
        let calls = Cell::new(0);
        let first = root.child(0, 1).unwrap();
        assert_eq!(
            rows.decode_discovered(&ctx, &mut output, first, spec(), |stage| {
                calls.set(calls.get() + 1);
                variable_row(&ctx, stage, root, first, "test:model:point#first")
            })
            .unwrap(),
            Some(7)
        );
        let remaining = root.child(3, root.end()).unwrap();
        let prefix = remaining.child(3, 4.min(root.end())).unwrap();
        assert_eq!(
            rows.decode_discovered(&ctx, &mut output, prefix, spec(), |stage| {
                calls.set(calls.get() + 1);
                variable_row(&ctx, stage, remaining, prefix, "test:model:point#missing")
            })
            .unwrap(),
            None
        );
        assert!(rows.stopped());
        assert!(rows
            .decode_discovered(&ctx, &mut output, prefix, spec(), |_| {
                calls.set(calls.get() + 1);
                Ok((0, prefix))
            })
            .is_err());
        assert_eq!(calls.get(), 2);
        assert_eq!(output.model().points.len(), 1);
        assert_eq!(
            output
                .model()
                .points
                .iter()
                .next()
                .unwrap()
                .position()
                .get()
                .x,
            7.0
        );
        let parts = &output.fidelity().parts();
        assert_eq!(parts.len(), 2);
        let first_part = parts
            .values()
            .find(|part| matches!(part.state(), &PartState::Decoded {}))
            .unwrap();
        assert_eq!((first_part.offset(), first_part.byte_len()), (0, 3));
        let failed = parts
            .values()
            .find(|part| matches!(part.state(), &PartState::GivenUp {}))
            .unwrap();
        assert_eq!(
            (failed.offset(), failed.byte_len()),
            (3, u64::try_from(source.len() - 3).unwrap())
        );
        let unknowns = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<NativeUnknownRecord>("unknowns")
            .unwrap();
        assert_eq!(unknowns.len(), 1);
        assert_eq!(
            output
                .fidelity()
                .retained_record(unknowns[0].id.as_str())
                .unwrap()
                .data(),
            Some(&source[3..])
        );
        assert_eq!(output.losses().len(), 1);
        assert_eq!(output.losses()[0].code.local_code(), "record.unreadable");
        assert!(output.losses()[0]
            .message
            .contains("test:model:point#missing"));
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].id(), "test:model:point#first");
        assert_eq!(captures[0].reads()["file"], [0..3]);
    }
}

#[test]
fn a_claimed_shorter_prefix_cannot_keep_a_semantic_read_from_the_following_row() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"\x02\x03\x04", &arena, &DecodePolicy::service()).unwrap();
    let mut rows = sequence(&ctx, root);
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let prefix = root.child(0, 1).unwrap();
    assert!(rows
        .decode_discovered(&ctx, &mut output, prefix, spec(), |stage| {
            let (value, _) = variable_row(&ctx, stage, root, prefix, "test:model:point#rejected")?;
            Ok((value, prefix))
        })
        .unwrap()
        .is_none());
    assert!(rows.stopped());
    assert!(output.model().points.is_empty());
    assert_eq!(
        output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), &PartState::GivenUp {}))
            .count(),
        1
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_recordless_staged_model_cannot_keep_a_value_read_beyond_its_claimed_prefix() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    let mut rows = sequence(&ctx, root);
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    let prefix = root.child(0, 1).unwrap();
    assert!(rows
        .decode_discovered(&ctx, &mut output, prefix, spec(), |stage| {
            let value = root.read_at(3, 1).unwrap()[0];
            stage.insert_model(
                &ctx,
                Point::new(
                    PointId::mint("test:model:point#recordless").unwrap(),
                    FinitePoint3::new(Point3::new(f64::from(value), 0.0, 0.0)).unwrap(),
                    None,
                ),
            )?;
            Ok((value, prefix))
        })
        .unwrap()
        .is_none());
    assert!(rows.stopped());
    assert!(output.model().points.is_empty());
    assert_eq!(output.losses().len(), 1);
    assert!(output.losses()[0]
        .message
        .contains("test:model:point#recordless"));
    let namespace = output.native().namespace("fcstd").unwrap();
    let unknowns = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        output
            .fidelity()
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&b"1234"[..])
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
