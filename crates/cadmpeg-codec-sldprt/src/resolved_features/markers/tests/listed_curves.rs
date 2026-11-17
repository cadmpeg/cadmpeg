//! Markers at the curve records of an `sgSketch` curve list
//! (`docs/formats/sldprt.md`, "Sketch objects").

use crate::records::{SketchInputKind, SketchRelationKind};
use crate::resolved_features::markers::sketch_input_entities;

const F32_MINUS_ONE: [u8; 4] = [0x00, 0x00, 0x80, 0xbf];
const F64_MINUS_ONE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xf0, 0xbf];
const I32_MINUS_TWO: [u8; 4] = [0xfe, 0xff, 0xff, 0xff];
/// A +17 code that reads as a distance relation under a layout without
/// coordinates.
const DISTANCE_CODE: u32 = 1;

/// A 64-byte record head with the +17 code `code`.
fn head(out: &mut Vec<u8>, code: u32) {
    out.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    out.extend_from_slice(&[0xff; 8]);
    out.extend_from_slice(&F32_MINUS_ONE);
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&F32_MINUS_ONE);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&1.0f64.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

fn point(out: &mut Vec<u8>, u: f64, v: f64) {
    head(out, 0);
    out.extend_from_slice(&0x1eu16.to_le_bytes());
    out.extend_from_slice(&u.to_le_bytes());
    out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&I32_MINUS_TWO);
    out.extend_from_slice(&[0; 46]);
}

fn curve(out: &mut Vec<u8>, start: u16, end: u16, center: Option<u16>) -> usize {
    let at = out.len();
    head(out, DISTANCE_CODE);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&end.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&F64_MINUS_ONE);
    out.extend_from_slice(&u16::from(center.is_some()).to_le_bytes());
    if let Some(center) = center {
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&center.to_le_bytes());
        for _ in 0..4 {
            out.extend_from_slice(&I32_MINUS_TWO);
        }
    }
    out.extend_from_slice(&[0; 10]);
    at
}

/// The sketch header, points 0 (the centre), 1 and 2, and the curve list;
/// with `listed` false, the header is absent and no list is framed.
fn payload(listed: bool) -> (Vec<u8>, [usize; 3]) {
    let mut out = vec![0; 7];
    if listed {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&1u32.to_le_bytes());
    } else {
        out.extend_from_slice(&[0; 14]);
    }
    point(&mut out, 0.0, 0.0);
    point(&mut out, 0.01, 0.0);
    point(&mut out, 0.0, 0.01);
    out.extend_from_slice(&0u32.to_le_bytes());
    let line = curve(&mut out, 1, 2, None);
    let arc = curve(&mut out, 1, 2, Some(0));
    let circle = curve(&mut out, 1, 1, Some(0));
    out.extend_from_slice(&[0; 16]);
    (out, [line, arc, circle])
}

fn kind_at(payload: &[u8], offset: usize) -> Option<SketchInputKind> {
    sketch_input_entities(payload, "sldprt:feature-input:resolved-features#1")
        .into_iter()
        .find(|marker| usize::try_from(marker.offset()).ok() == Some(offset))
        .map(|marker| marker.kind())
}

#[test]
fn a_listed_curve_record_is_a_curve_whatever_its_code() {
    let (payload, [line, arc, circle]) = payload(true);
    assert_eq!(kind_at(&payload, line), Some(SketchInputKind::LineOrCircle));
    assert_eq!(kind_at(&payload, arc), Some(SketchInputKind::Arc));
    assert_eq!(
        kind_at(&payload, circle),
        Some(SketchInputKind::LineOrCircle)
    );
}

#[test]
fn an_unlisted_arc_record_keeps_the_relation_code_of_its_layout() {
    let (payload, [_, arc, circle]) = payload(false);
    for offset in [arc, circle] {
        assert_eq!(
            kind_at(&payload, offset),
            Some(SketchInputKind::Relation(SketchRelationKind::Distance))
        );
    }
}
