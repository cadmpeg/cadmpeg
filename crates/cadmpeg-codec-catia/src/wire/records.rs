//! Generic CATIA record-walking and wire-shape record decoding.
//!
//! The family-independent scan layer: length-closed A/B record framing
//! (`consolidated_records` and its `a_family_frames`/`b_family_frames` views),
//! the `05 08 01` vertex-coordinate scanner (`scan_vertex_records`), and the
//! degree-5 UV jet decoder (`parse_consolidated_pcurve`). Nothing here depends
//! on a `families` module; the family decoders consume it downward.
//!
//! The `Consolidated*` names are retained from this code's original home in
//! `families::consolidated::records`; a rename cascades across ~40 call sites
//! and several `native` field paths, so the names carry naming debt here.

use std::ops::Range;

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::wire::le::u32_at as u32_le;

use super::bytes::{compact_int, f64_le};

/// Degree-5 UV jet stored in an A- or B-family class-`0x20` consolidated record.
#[derive(Debug, Clone)]
pub struct ConsolidatedPcurve {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support-surface identifier.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Number of leading extrapolation sites encoded by the array marker.
    pub extrapolation_sites: u32,
    /// Global parameters at the stored sites.
    pub knots: Vec<f64>,
    /// UV positions at the stored sites.
    pub points: Vec<[f64; 2]>,
    /// UV first derivatives at the stored sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// UV second derivatives at the stored sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native parameter range.
    pub range: [f64; 2],
    /// Bytes following the native range inside the framed record.
    pub tail: Vec<u8>,
}

pub(crate) fn parse_consolidated_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
) -> Option<ConsolidatedPcurve> {
    let mut at = payload;
    let support_id = compact_int(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    if degree != 5 || !(2..=4096).contains(&count) {
        return None;
    }
    let extrapolation_sites = match *data.get(at)? {
        0x0c => {
            at += 1;
            0
        }
        0x08 => {
            let encoded = *data.get(at + 1)?;
            if encoded % 4 != 1 {
                return None;
            }
            at += 2;
            u32::from((encoded - 1) / 4)
        }
        _ => return None,
    };
    let read = |at: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read(&mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    at += 1;
    data.get(..at)?;
    let u = read(&mut at)?;
    let v = read(&mut at)?;
    let du = read(&mut at)?;
    let dv = read(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = read(&mut at)?;
    let ddv = read(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if at > end
        || !matches!(&data[at..end], [0x07] | [0x07, 0x00])
        || knots.windows(2).any(|v| v[0] >= v[1])
        || range[0] >= range[1]
        || knots
            .iter()
            .chain(&u)
            .chain(&v)
            .chain(&du)
            .chain(&dv)
            .chain(&ddu)
            .chain(&ddv)
            .chain(&range)
            .any(|x| !x.is_finite())
    {
        return None;
    }
    Some(ConsolidatedPcurve {
        pos,
        support_id,
        degree,
        extrapolation_sites,
        knots,
        points: u.into_iter().zip(v).map(|p| [p.0, p.1]).collect(),
        first_derivatives: du.into_iter().zip(dv).map(|p| [p.0, p.1]).collect(),
        second_derivatives: ddu.into_iter().zip(ddv).map(|p| [p.0, p.1]).collect(),
        range,
        tail: data[at..end].to_vec(),
    })
}

#[derive(Clone, Copy)]
pub(crate) struct ConsolidatedFrame {
    pub(crate) pos: usize,
    pub(crate) payload: usize,
    pub(crate) end: usize,
    pub(crate) header_token: u32,
}

/// Width-coded consolidated record family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidatedFamily {
    /// U32-length A family (`a5/a6/a7`).
    A,
    /// U8-length B family (`b2/b3/b4`).
    B,
}

/// One length-closed record in a consolidated A/B cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedRecord {
    /// Record family.
    pub family: ConsolidatedFamily,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent flag byte (`0x03`, `0x13`, or `0x83`).
    pub flag: u8,
    /// Record class byte.
    pub class: u8,
    /// Little-endian width-coded header token.
    pub header_token: u32,
    /// Complete record byte range.
    pub range: Range<usize>,
    /// Payload byte range.
    pub payload: Range<usize>,
}

/// Inventory length-closed consolidated A/B records while suppressing candidates
/// nested inside the payload of an already accepted frame.
#[must_use]
pub fn consolidated_records(data: &[u8]) -> Vec<ConsolidatedRecord> {
    let flags = [0x03, 0x13, 0x83];
    let mut candidates = Vec::new();
    for pos in 0..data.len().saturating_sub(4) {
        let (family, width, token_at, length) = if let Some(width) = data[pos]
            .checked_sub(0xa4)
            .filter(|width| (1..=3).contains(width))
        {
            let Some(length) = u32_le(data, pos + 3).and_then(|v| usize::try_from(v).ok()) else {
                continue;
            };
            (ConsolidatedFamily::A, width, pos + 7, length)
        } else if let Some(width) = data[pos]
            .checked_sub(0xb1)
            .filter(|width| (1..=3).contains(width))
        {
            (
                ConsolidatedFamily::B,
                width,
                pos + 4,
                usize::from(data[pos + 3]),
            )
        } else {
            continue;
        };
        let Some(&flag) = data.get(pos + 1) else {
            continue;
        };
        let Some(&class) = data.get(pos + 2) else {
            continue;
        };
        if !flags.contains(&flag) {
            continue;
        }
        let width_usize = usize::from(width);
        let Some(payload_start) = token_at.checked_add(width_usize) else {
            continue;
        };
        let Some(end) = payload_start.checked_add(length) else {
            continue;
        };
        if end > data.len() {
            continue;
        }
        let header_token = data[token_at..payload_start]
            .iter()
            .enumerate()
            .fold(0u32, |value, (shift, byte)| {
                value | (u32::from(*byte) << (8 * shift))
            });
        candidates.push(ConsolidatedRecord {
            family,
            width,
            flag,
            class,
            header_token,
            range: pos..end,
            payload: payload_start..end,
        });
    }
    let mut records: Vec<ConsolidatedRecord> = Vec::new();
    let mut active_payload: Option<Range<usize>> = None;
    for candidate in candidates {
        if active_payload
            .as_ref()
            .is_some_and(|payload| payload.contains(&candidate.range.start))
        {
            continue;
        }
        active_payload = Some(candidate.payload.clone());
        records.push(candidate);
    }
    records
}

pub(crate) fn a_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    consolidated_records(data)
        .into_iter()
        .filter(|record| record.family == ConsolidatedFamily::A && record.class == class)
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

pub(crate) fn b_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    consolidated_records(data)
        .into_iter()
        .filter(|record| record.family == ConsolidatedFamily::B && record.class == class)
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

/// Scan every `05 08 01` coordinate row in `bytes`, returning the decoded
/// vertex points in stream order.
pub fn scan_vertex_records(bytes: &[u8]) -> Vec<Point3> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 15 <= bytes.len() {
        if bytes[p] == 0x05 && bytes[p + 1] == 0x08 && bytes[p + 2] == 0x01 {
            let x = f32_le(bytes, p + 3);
            let y = f32_le(bytes, p + 7);
            let z = f32_le(bytes, p + 11);
            if finite_in_range(x) && finite_in_range(y) && finite_in_range(z) {
                out.push(Point3::new(x as f64, y as f64, z as f64));
            }
            p += 15;
        } else {
            p += 1;
        }
    }
    out
}

fn f32_le(bytes: &[u8], at: usize) -> f32 {
    cadmpeg_ir::wire::le::f32_at(bytes, at).unwrap_or(f32::NAN)
}

fn finite_in_range(v: f32) -> bool {
    v.is_finite() && v.abs() < 1e4
}
