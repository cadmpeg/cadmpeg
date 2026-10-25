// SPDX-License-Identifier: Apache-2.0
//! Independent Appendix C exact-shape member boundaries.

use crate::framing::{FrameParent, FramedPart};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use std::ops::Range;
mod binary;
mod grammar;
mod text;

const TABLES: [&[u8]; 8] = [
    b"Locations",
    b"Curve2ds",
    b"Curves",
    b"Polygon3D",
    b"PolygonOnTriangulations",
    b"Surfaces",
    b"Triangulations",
    b"TShapes",
];
const TEXT_ROWS: [&str; 8] = [
    "freecad.brep_text_location",
    "freecad.brep_text_curve2d",
    "freecad.brep_text_curve",
    "freecad.brep_text_polygon3d",
    "freecad.brep_text_polygon_on_triangulation",
    "freecad.brep_text_surface",
    "freecad.brep_text_triangulation",
    "freecad.brep_text_tshape",
];

/// Frame an opened exact-shape side member named by its genuine ZIP entry.
/// The grammar walk adds no semantic read ranges or decoder record captures.
pub fn frame_brep_member<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    entry_name: View<'a>,
    zip_parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let binary = entry_name
        .window()
        .rsplit(|byte| *byte == b'.')
        .next()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(b"bin"));
    let root = out.len();
    let level = if binary {
        "freecad.brep_binary"
    } else {
        "freecad.brep_text"
    };
    let frame = FramedPart::new(
        ctx,
        super::level(level)?,
        source,
        FrameParent::Part(zip_parent),
        vec![],
        vec![],
    )?
    .with_external_dispatch(ctx, vec![entry_name])?;
    super::append(ctx, out, frame)?;
    if source.window().is_empty() {
        return Ok(());
    }
    if binary {
        binary::frame(ctx, source, root, out)
    } else {
        text::frame(ctx, source, root, out)
    }
}
fn add<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    start: usize,
    end: usize,
    level: &str,
    parent: usize,
    framing: Vec<Range<u64>>,
    dispatch: Vec<Range<u64>>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<usize, String> {
    let view = source
        .child(start, end)
        .ok_or("B-rep boundary escapes its source")?;
    let index = out.len();
    super::append(
        ctx,
        out,
        FramedPart::new(
            ctx,
            super::level(level)?,
            view,
            FrameParent::Part(parent),
            framing,
            dispatch,
        )?,
    )?;
    Ok(index)
}
fn range(start: usize, end: usize) -> Range<u64> {
    u64_from_index(start)..u64_from_index(end)
}
fn header(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut at = 0;
    while at < bytes.len() {
        let end = bytes[at..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|length| at + length + 1)
            .unwrap_or(bytes.len());
        if !bytes[at..end].iter().all(u8::is_ascii_whitespace) {
            return Some((at, end));
        }
        at = end;
    }
    None
}
#[cfg(test)]
mod tests;
