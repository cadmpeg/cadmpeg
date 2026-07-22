//! A-family consolidated curve and surface record vocabulary.
//!
//! Decodes `a5`/`a8` NURBS surface carriers, common-form and consolidated
//! rolling-ball jets, guide-curve jets, and object-stream UV pcurves.

use cadmpeg_ir::geometry::{
    NurbsSurface, ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
    SurfaceGeometry,
};
use cadmpeg_ir::le::{u16_at as u16_le, u32_at as u32_le};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::HashSet;

use crate::nurbs::{expand_knots, pole_count};
use crate::wire::bytes::{compact_int, f64_le, f64_point, read_f64_array, u32_le_24};
use crate::wire::records::{
    a_family_frames, parse_consolidated_pcurve, ConsolidatedFrame, ConsolidatedPcurve,
};

/// A decoded common-form object-stream NURBS surface (`a8 03 34`).
#[derive(Debug, Clone)]
pub struct A8Surface {
    /// Source offset of the framed record.
    pub pos: usize,
    /// The inline persistent object id.
    pub object_id: u32,
    /// The decoded NURBS carrier.
    pub geometry: SurfaceGeometry,
}

/// Parameter lattice decoded from an `a8 03 34` surface record independently
/// of its pole representation.
#[derive(Debug, Clone, PartialEq)]
pub struct A8SurfaceHeader {
    /// Source offset of the framed record.
    pub pos: usize,
    /// Inline persistent object id.
    pub object_id: u32,
    /// U degree.
    pub u_degree: u32,
    /// V degree.
    pub v_degree: u32,
    /// Distinct U knots.
    pub u_distinct_knots: Vec<f64>,
    /// Distinct V knots.
    pub v_distinct_knots: Vec<f64>,
    /// U multiplicities corresponding to `u_distinct_knots`.
    pub u_multiplicities: Vec<u32>,
    /// V multiplicities corresponding to `v_distinct_knots`.
    pub v_multiplicities: Vec<u32>,
    /// Derived U pole count.
    pub u_count: u32,
    /// Derived V pole count.
    pub v_count: u32,
    /// Whether the record selects rational weights.
    pub rational: bool,
    /// The fixed 141-byte surface tail begins immediately after the mode byte,
    /// so no inline pole or weight grid is present.
    pub poles_elided: bool,
}

#[derive(Debug, Clone)]
/// Degree-5 UV jet stored in an `a8 03 20` object record.
pub struct A8Pcurve {
    /// Record byte offset.
    #[cfg(test)]
    pub pos: usize,
    /// Inline object identifier.
    pub object_id: u32,
    /// Referenced support-surface object identifier.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Stored UV-jet channel-mode byte.
    #[cfg(test)]
    pub mode: u8,
    /// UV positions at the knot sites.
    pub points: Vec<[f64; 2]>,
    /// UV first derivatives at the knot sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// UV second derivatives at the knot sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native parameter range.
    pub range: [f64; 2],
}

/// Decode framed `a5 03 20` consolidated UV jets.
#[must_use]
pub fn a5_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    a_family_frames(data, 0x20)
        .into_iter()
        .filter_map(|frame| parse_consolidated_pcurve(data, frame.pos, frame.payload, frame.end))
        .collect()
}

/// One knot-site value in an `a5 03 32` rolling-ball program.
#[derive(Debug, Clone, PartialEq)]
pub struct RollingBallSite {
    /// First limiting curve point.
    pub limit1: [f64; 3],
    /// Second limiting curve point.
    pub limit2: [f64; 3],
    /// Rolling-ball centre.
    pub center: [f64; 3],
    /// Stored opening angle.
    pub theta: f64,
    /// Radius derived from centre to either limit.
    pub radius: f64,
}

/// Consolidated degree-5 rolling-ball jet.
#[derive(Debug, Clone)]
pub struct A5FreeformCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Schema token immediately before the payload.
    pub header_token: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct knots.
    pub knots: Vec<f64>,
    /// Position channels at each knot.
    pub sites: Vec<RollingBallSite>,
    /// Ten first-derivative channels per knot.
    pub first_derivatives: Vec<[f64; 10]>,
    /// Ten second-derivative channels per knot.
    pub second_derivatives: Vec<[f64; 10]>,
}

/// One position and unit reference direction in an `a5/a6/a7 03 39` jet.
#[derive(Debug, Clone, PartialEq)]
pub struct GuideCurveSite {
    /// Guide-curve point.
    pub point: [f64; 3],
    /// Unit direction from the first stored triple to the second.
    pub direction: [f64; 3],
}

/// Width-coded guide-curve and reference-direction jet.
#[derive(Debug, Clone)]
pub struct A5GuideCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Position and unit-direction values at the knot sites.
    pub sites: Vec<GuideCurveSite>,
    /// Six first-derivative channels per site.
    pub first_derivatives: Vec<[f64; 6]>,
    /// Six second-derivative channels per site.
    pub second_derivatives: Vec<[f64; 6]>,
}

/// Decode `a5/a6/a7 03 39` guide-curve and unit-direction jets.
#[must_use]
pub fn a5_guide_curves(data: &[u8]) -> Vec<A5GuideCurve> {
    a_family_frames(data, 0x39)
        .into_iter()
        .filter_map(|frame| parse_a5_guide_curve(data, frame))
        .collect()
}

fn parse_a5_guide_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5GuideCurve> {
    let mut at = frame.payload;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || !(2..=4096).contains(&count)
        || !(1..=9).contains(&degree)
    {
        return None;
    }
    at = consume_array_marker(data, at)?;
    let knots = f64_values(data, &mut at, count, frame.end)?;
    if !monotonic(&knots) {
        return None;
    }
    let block_bytes = count.checked_mul(48)?;
    if at.checked_add(3 * block_bytes)?.checked_add(48)? != frame.end {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 6]>> {
        (0..count)
            .map(|site| read_f64_array::<6>(data, start + site * 48))
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites: Option<Vec<_>> = positions
        .into_iter()
        .map(|value| {
            let point = [value[0], value[1], value[2]];
            let direction = [
                value[3] - value[0],
                value[4] - value[1],
                value[5] - value[2],
            ];
            let length =
                (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
            ((length - 1.0).abs() < 1e-9).then_some(GuideCurveSite { point, direction })
        })
        .collect();
    Some(A5GuideCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        degree,
        knots,
        sites: sites?,
        first_derivatives,
        second_derivatives,
    })
}

/// Common-form degree-5 rolling-ball jet stored in an `a8 03 32` object record.
#[derive(Debug, Clone, PartialEq)]
pub struct A8FreeformCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Inline persistent object identifier.
    pub object_id: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Multiplicity for each distinct knot.
    pub multiplicities: Vec<u32>,
    /// Position channels at each knot.
    pub sites: Vec<RollingBallSite>,
    /// Ten first-derivative channels per knot.
    pub first_derivatives: Vec<[f64; 10]>,
    /// Ten second-derivative channels per knot.
    pub second_derivatives: Vec<[f64; 10]>,
    /// Bytes following the three jet blocks inside the payload.
    pub tail_len: usize,
}

/// Convert a complete common-form rolling-ball jet to its exact neutral
/// procedural carrier.
pub(crate) fn rolling_ball_jet_definition(
    jet: &A8FreeformCurve,
) -> Option<ProceduralSurfaceDefinition> {
    if jet.degree != 5
        || jet.sites.len() != jet.knots.len()
        || jet.first_derivatives.len() != jet.knots.len()
        || jet.second_derivatives.len() != jet.knots.len()
        || jet.multiplicities.len() != jet.knots.len()
    {
        return None;
    }
    let derivative = |values: [f64; 10]| RollingBallJetDerivative {
        first_limit: Vector3::new(values[0], values[1], values[2]),
        second_limit: Vector3::new(values[3], values[4], values[5]),
        center: Vector3::new(values[6], values[7], values[8]),
        angle: values[9],
    };
    let sites = jet
        .sites
        .iter()
        .zip(&jet.first_derivatives)
        .zip(&jet.second_derivatives)
        .map(|((site, first), second)| RollingBallJetSite {
            first_limit: Point3::new(site.limit1[0], site.limit1[1], site.limit1[2]),
            second_limit: Point3::new(site.limit2[0], site.limit2[1], site.limit2[2]),
            center: Point3::new(site.center[0], site.center[1], site.center[2]),
            angle: site.theta,
            first_derivative: derivative(*first),
            second_derivative: derivative(*second),
        })
        .collect();
    Some(ProceduralSurfaceDefinition::RollingBallJet {
        degree: jet.degree,
        multiplicities: jet.multiplicities.clone(),
        knots: jet.knots.clone(),
        sites,
    })
}

/// Decode framed `a8 03 32` common-form rolling-ball jet records.
#[must_use]
pub fn a8_freeform_curves(data: &[u8]) -> Vec<A8FreeformCurve> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(relative) = data[search..].windows(3).position(|v| v == [0xa8, 3, 0x32]) {
        let pos = search + relative;
        search = pos + 3;
        let Some(length) = u32_le(data, pos + 3).and_then(|v| usize::try_from(v).ok()) else {
            continue;
        };
        let Some(end) = pos.checked_add(11).and_then(|v| v.checked_add(length)) else {
            continue;
        };
        if end <= data.len() {
            if let Some(value) = parse_a8_curve(data, pos, end) {
                out.push(value);
            }
        }
    }
    out
}

fn parse_a8_curve(data: &[u8], pos: usize, end: usize) -> Option<A8FreeformCurve> {
    let object_id = u32_le(data, pos + 7)?;
    let mut at = pos + 12;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || !(2..=8192).contains(&count)
        || degree != 5
    {
        return None;
    }
    at += if data.get(at) == Some(&0x08) { 2 } else { 1 };
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        knots.push(f64_le(data, at)?);
        at += 8;
    }
    let mut multiplicities = Vec::with_capacity(count);
    for _ in 0..count {
        multiplicities.push(compact_int(data, &mut at)?);
    }
    if knots.iter().any(|v| !v.is_finite()) || knots.windows(2).any(|v| v[0] >= v[1]) {
        return None;
    }
    let block_bytes = count.checked_mul(80)?;
    let blocks_end = at.checked_add(3 * block_bytes)?;
    if multiplicities.first() != Some(&6)
        || multiplicities.last() != Some(&6)
        || multiplicities[1..multiplicities.len() - 1]
            .iter()
            .any(|value| !matches!(value, 1 | 3))
        || blocks_end > end
        || end - blocks_end != 59
    {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 10]>> {
        (0..count)
            .map(|site| {
                let mut values = [0.0; 10];
                for (channel, value) in values.iter_mut().enumerate() {
                    *value = f64_le(data, start + site * 80 + channel * 8)?;
                }
                values.iter().all(|v| v.is_finite()).then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A8FreeformCurve {
        pos,
        object_id,
        degree,
        knots,
        multiplicities,
        sites,
        first_derivatives,
        second_derivatives,
        tail_len: end - blocks_end,
    })
}

/// Decode framed `a5 03 32` rolling-ball jet records.
#[must_use]
pub fn a5_freeform_curves(data: &[u8]) -> Vec<A5FreeformCurve> {
    a_family_frames(data, 0x32)
        .into_iter()
        .filter_map(|frame| parse_a5_curve(data, frame))
        .collect()
}

fn parse_a5_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5FreeformCurve> {
    let ConsolidatedFrame {
        pos,
        payload,
        end,
        header_token,
    } = frame;
    let mut at = payload;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || !(2..=4096).contains(&count)
        || degree != 5
    {
        return None;
    }
    match data.get(at..at + 2) {
        Some([0x0c, _]) => at += 1,
        Some([0x08, 0x09 | 0x05]) => at += 2,
        _ => return None,
    }
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        knots.push(f64_le(data, at)?);
        at += 8;
    }
    if knots.iter().any(|v| !v.is_finite()) || knots.windows(2).any(|v| v[0] >= v[1]) {
        return None;
    }
    let block_bytes = count.checked_mul(80)?;
    if at.checked_add(3 * block_bytes)? > end || end - (at + 3 * block_bytes) > 4096 {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 10]>> {
        (0..count)
            .map(|site| {
                let mut values = [0.0; 10];
                for (channel, value) in values.iter_mut().enumerate() {
                    *value = f64_le(data, start + site * 80 + channel * 8)?;
                }
                values.iter().all(|v| v.is_finite()).then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A5FreeformCurve {
        pos,
        header_token,
        degree,
        knots,
        sites,
        first_derivatives,
        second_derivatives,
    })
}

fn rolling_ball_sites(positions: Vec<[f64; 10]>) -> Option<Vec<RollingBallSite>> {
    let mut sites = Vec::with_capacity(positions.len());
    for v in positions {
        let limit1 = [v[0], v[1], v[2]];
        let limit2 = [v[3], v[4], v[5]];
        let center = [v[6], v[7], v[8]];
        let radius = distance3(center, limit1);
        let other = distance3(center, limit2);
        let chord = distance3(limit1, limit2);
        if radius <= f64::EPSILON
            || (radius - other).abs() > 1e-9 * radius.max(1.0)
            || (v[9] - 2.0 * (chord / (2.0 * radius)).clamp(-1.0, 1.0).asin()).abs() > 1e-9
        {
            return None;
        }
        sites.push(RollingBallSite {
            limit1,
            limit2,
            center,
            theta: v[9],
            radius,
        });
    }
    Some(sites)
}

fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Decode framed `a8 03 20` UV jet records.
#[must_use]
#[cfg(test)]
pub fn a8_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    object_stream_pcurves(data)
        .into_iter()
        .filter(|pcurve| data.get(pcurve.pos) == Some(&0xa8))
        .collect()
}

/// Decode framed `a8 03 20` and `b5 03 20` object-stream UV jet records.
#[must_use]
pub fn object_stream_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    let mut out = Vec::new();
    for pos in 0..data.len().saturating_sub(11) {
        if data.get(pos + 1..pos + 3) != Some(&[0x03, 0x20]) {
            continue;
        }
        let Some((payload, length, object_id)) = (match data[pos] {
            0xa8 => u32_le(data, pos + 3)
                .and_then(|length| usize::try_from(length).ok())
                .zip(u32_le(data, pos + 7))
                .map(|(length, object_id)| (pos + 11, length, object_id)),
            0xb5 => data
                .get(pos + 3)
                .zip(u32_le(data, pos + 4))
                .map(|(length, object_id)| (pos + 8, usize::from(*length), object_id)),
            _ => None,
        }) else {
            continue;
        };
        let Some(end) = payload.checked_add(length) else {
            continue;
        };
        if end > data.len() {
            continue;
        }
        if let Some(value) = parse_object_stream_pcurve(data, pos, payload, end, object_id) {
            out.push(value);
        }
    }
    out
}

fn parse_object_stream_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
    object_id: u32,
) -> Option<A8Pcurve> {
    #[cfg(not(test))]
    let _ = pos;
    let mut at = payload + 1;
    let support_id = object_stream_reference(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    at += 2;
    data.get(..at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    at += if data.get(at) == Some(&0x08) { 2 } else { 1 };
    if !(2..=8192).contains(&count) || degree != 5 {
        return None;
    }
    let read = |at: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read(&mut at)?;
    let mut multiplicities = Vec::with_capacity(count);
    for _ in 0..count {
        multiplicities.push(compact_int(data, &mut at)?);
    }
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    let mode = *data.get(at)?;
    at += 1;
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
    if data.get(at) != Some(&0x07)
        || mode % 4 != 1
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.first() != Some(&6)
        || multiplicities.last() != Some(&6)
        || multiplicities[1..multiplicities.len() - 1]
            .iter()
            .any(|multiplicity| *multiplicity != 3)
        || range[0] >= range[1]
        || end != at + 1
        || knots
            .iter()
            .chain(&u)
            .chain(&v)
            .chain(&du)
            .chain(&dv)
            .chain(&ddu)
            .chain(&ddv)
            .chain(&range)
            .any(|x| !x.is_finite() || x.abs() >= 1e12)
    {
        return None;
    }
    Some(A8Pcurve {
        #[cfg(test)]
        pos,
        object_id,
        support_id,
        degree,
        knots,
        #[cfg(test)]
        mode,
        points: u.into_iter().zip(v).map(|p| [p.0, p.1]).collect(),
        first_derivatives: du.into_iter().zip(dv).map(|p| [p.0, p.1]).collect(),
        second_derivatives: ddu.into_iter().zip(ddv).map(|p| [p.0, p.1]).collect(),
        range,
    })
}

/// Decode common-form object-stream NURBS surfaces.  Every variable-length
/// field is bounded by the record's `payload_len`, so signature collisions do
/// not become carriers.
pub fn a8_surfaces(data: &[u8]) -> Vec<A8Surface> {
    scan_surfaces(data, *b"\xa8\x03\x34", 3, a8_surface)
}

/// Decode every complete common-form object-stream NURBS surface, including
/// parameter records whose pole grids occupy a uniquely bounded external
/// allocation.
#[must_use]
pub fn resolved_a8_surfaces(data: &[u8]) -> Vec<A8Surface> {
    let mut surfaces = a8_surfaces(data);
    let inline_positions = surfaces
        .iter()
        .map(|surface| surface.pos)
        .collect::<HashSet<_>>();
    surfaces.extend(
        a8_surface_headers(data)
            .into_iter()
            .filter(|header| !inline_positions.contains(&header.pos))
            .filter_map(|header| a8_surface_from_external_grid(data, &header)),
    );
    surfaces.sort_by_key(|surface| surface.pos);
    surfaces
}

/// Decode every structurally complete `a8 03 34` parameter lattice, including
/// records whose pole representation is not inline.
#[must_use]
pub fn a8_surface_headers(data: &[u8]) -> Vec<A8SurfaceHeader> {
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = data[search..].windows(3).position(|v| v == [0xa8, 3, 0x34]) {
        let pos = search + relative;
        search = pos + 3;
        if let Some(parsed) = parse_a8_surface_header(data, pos) {
            out.push(parsed.header);
        }
    }
    out
}

/// Resolve an elided-pole `a8 03 34` carrier from its uniquely sized external
/// grid allocation. The allocation occupies the complete unframed gap between
/// a length-closed `b5 03 21` pcurve and the following A/B-family frame.
#[must_use]
pub fn a8_surface_from_external_grid(data: &[u8], header: &A8SurfaceHeader) -> Option<A8Surface> {
    if !header.poles_elided {
        return None;
    }
    let poles = usize::try_from(header.u_count)
        .ok()?
        .checked_mul(usize::try_from(header.v_count).ok()?)?;
    let weight_bytes = if header.rational {
        poles.checked_mul(8)?
    } else {
        0
    };
    let grid_bytes = poles.checked_mul(24)?.checked_add(weight_bytes)?;
    let mut candidates = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = data[search..]
        .windows(3)
        .position(|bytes| bytes == [0xb5, 0x03, 0x21])
    {
        let frame = search + relative;
        search = frame + 3;
        let payload_len = usize::from(*data.get(frame + 3)?);
        let start = frame.checked_add(8)?.checked_add(payload_len)?;
        let end = start.checked_add(grid_bytes)?;
        if !matches!(
            data.get(end..end + 3),
            Some([
                0xa5 | 0xa8 | 0xa9 | 0xb2 | 0xb3 | 0xb4 | 0xb5 | 0xb6,
                0x03,
                _
            ])
        ) {
            continue;
        }
        let mut at = start;
        let mut control_points = Vec::with_capacity(poles);
        let mut complete = true;
        for _ in 0..poles {
            let Some(point) = f64_point(data, at) else {
                complete = false;
                break;
            };
            control_points.push(point);
            at += 24;
        }
        if !complete
            || control_points
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .any(|coordinate| !coordinate.is_finite() || coordinate.abs() >= 1e9)
        {
            continue;
        }
        let weights = if header.rational {
            let Some(values) = f64_values(data, &mut at, poles, end) else {
                continue;
            };
            if values
                .iter()
                .any(|weight| !weight.is_finite() || *weight == 0.0)
            {
                continue;
            }
            Some(values)
        } else {
            None
        };
        if at == end {
            candidates.push((control_points, weights));
        }
    }
    let [(control_points, weights)] = candidates.as_slice() else {
        return None;
    };
    Some(A8Surface {
        pos: header.pos,
        object_id: header.object_id,
        geometry: SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: header.u_degree,
            v_degree: header.v_degree,
            u_knots: expand_knots(&header.u_distinct_knots, &header.u_multiplicities)?,
            v_knots: expand_knots(&header.v_distinct_knots, &header.v_multiplicities)?,
            u_count: header.u_count,
            v_count: header.v_count,
            control_points: control_points.clone(),
            weights: weights.clone(),
            u_periodic: false,
            v_periodic: false,
        }),
    })
}

/// Decode consolidated `a5 03 34` NURBS surface carriers.  This family uses
/// implicit clamped multiplicities instead of the explicit `a8` vectors.
pub fn a5_surfaces(data: &[u8]) -> Vec<A8Surface> {
    a_family_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| a5_surface(data, frame))
        .collect()
}

fn scan_surfaces(
    data: &[u8],
    marker: [u8; 3],
    advance: usize,
    decode: fn(&[u8], usize) -> Option<A8Surface>,
) -> Vec<A8Surface> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some(relative) = data[start..]
        .windows(marker.len())
        .position(|bytes| bytes == marker)
    {
        let pos = start + relative;
        start = pos + advance;
        let Some(surface) = decode(data, pos) else {
            continue;
        };
        out.push(surface);
    }
    out
}

fn a5_surface(data: &[u8], frame: ConsolidatedFrame) -> Option<A8Surface> {
    let ConsolidatedFrame {
        pos, payload, end, ..
    } = frame;
    let mut at = payload;
    let u_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let u_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?;
    let v_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let v_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?;
    let mode = *data.get(at)?;
    at += 1;
    let (u_knots, u_count) = a5_knots(&u_distinct, u_degree)?;
    let (v_knots, v_count) = a5_knots(&v_distinct, v_degree)?;
    if !monotonic(&u_distinct) || !monotonic(&v_distinct) {
        return None;
    }
    let poles = (u_count as usize).checked_mul(v_count as usize)?;
    if at.checked_add(poles.checked_mul(24)?)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, at)?);
        at += 24;
    }
    if control_points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .any(|coordinate| !coordinate.is_finite() || coordinate.abs() >= 1e12)
    {
        return None;
    }
    let weights = match mode {
        0x01 => None,
        0x05 => Some(a5_weights(
            data,
            &mut at,
            u_count as usize,
            v_count as usize,
        )?),
        _ => return None,
    };
    // The structured tail is the false-positive gate for this unlength-framed family.
    if at + 4 > end
        || !matches!(data.get(at..at + 4), Some([0x05, a, 0x05, b]) if (*a - 1) % 4 == 0 && (*b - 1) % 4 == 0)
    {
        return None;
    }
    Some(A8Surface {
        pos,
        object_id: 0,
        geometry: SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            u_count,
            v_count,
            control_points,
            weights,
            u_periodic: false,
            v_periodic: false,
        }),
    })
}

struct ParsedA8SurfaceHeader {
    header: A8SurfaceHeader,
    pole_start: usize,
    end: usize,
}

fn parse_a8_surface_header(data: &[u8], pos: usize) -> Option<ParsedA8SurfaceHeader> {
    let payload_len = u32_le(data, pos + 3)? as usize;
    let object_id = u32_le(data, pos + 7)?;
    let end = pos.checked_add(11)?.checked_add(payload_len)?;
    if payload_len < 20 || end > data.len() {
        return None;
    }
    let mut at = pos + 12; // framing + lead byte
    let u_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?; // flags
    let u_distinct_count = compact_int(data, &mut at)? as usize;
    at = consume_array_marker(data, at)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?;
    let u_mults = compact_values(data, &mut at, u_distinct_count)?;
    let v_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    let v_distinct_count = compact_int(data, &mut at)? as usize;
    at = consume_array_marker(data, at)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?;
    let v_mults = compact_values(data, &mut at, v_distinct_count)?;
    let mode = *data.get(at)?;
    at += 1;
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || !(2..=8192).contains(&u_distinct_count)
        || !(2..=8192).contains(&v_distinct_count)
        || !matches!(mode, 0x01 | 0x05)
        || !monotonic(&u_distinct)
        || !monotonic(&v_distinct)
    {
        return None;
    }
    let u_count = pole_count(&u_mults, u_degree)?;
    let v_count = pole_count(&v_mults, v_degree)?;
    if u_count == 0 || v_count == 0 || u_count > 20_000 || v_count > 20_000 {
        return None;
    }
    let tail_end = at.checked_add(141)?;
    let poles_elided = matches!(data.get(at..at + 4), Some([0x05, a, 0x05, b]) if *a % 4 == 1 && *b % 4 == 1)
        && tail_end <= end
        && (tail_end == end
            || matches!(
                data.get(tail_end..tail_end + 3),
                Some([
                    0xa5 | 0xa8 | 0xa9 | 0xb2 | 0xb3 | 0xb4 | 0xb5 | 0xb6,
                    0x03,
                    _
                ])
            ));
    Some(ParsedA8SurfaceHeader {
        header: A8SurfaceHeader {
            pos,
            object_id,
            u_degree,
            v_degree,
            u_distinct_knots: u_distinct,
            v_distinct_knots: v_distinct,
            u_multiplicities: u_mults,
            v_multiplicities: v_mults,
            u_count,
            v_count,
            rational: mode == 0x05,
            poles_elided,
        },
        pole_start: at,
        end,
    })
}

fn a8_surface(data: &[u8], pos: usize) -> Option<A8Surface> {
    let ParsedA8SurfaceHeader {
        header,
        mut pole_start,
        end,
    } = parse_a8_surface_header(data, pos)?;
    let A8SurfaceHeader {
        object_id,
        u_degree,
        v_degree,
        u_distinct_knots,
        v_distinct_knots,
        u_multiplicities,
        v_multiplicities,
        u_count,
        v_count,
        rational,
        poles_elided,
        ..
    } = header;
    if poles_elided {
        return None;
    }
    let poles = (u_count as usize).checked_mul(v_count as usize)?;
    let pole_bytes = poles.checked_mul(24)?;
    if pole_start.checked_add(pole_bytes)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, pole_start)?);
        pole_start += 24;
    }
    if control_points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .any(|coordinate| !coordinate.is_finite() || coordinate.abs() >= 1e12)
    {
        return None;
    }
    let weights = if rational {
        let values = f64_values(data, &mut pole_start, poles, end)?;
        values
            .iter()
            .all(|weight| *weight != 0.0)
            .then_some(values)?
    } else {
        Vec::new()
    };
    if pole_start > end {
        return None;
    }
    Some(A8Surface {
        pos,
        object_id,
        geometry: SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree,
            v_degree,
            u_knots: expand_knots(&u_distinct_knots, &u_multiplicities)?,
            v_knots: expand_knots(&v_distinct_knots, &v_multiplicities)?,
            u_count,
            v_count,
            control_points,
            weights: rational.then_some(weights),
            u_periodic: false,
            v_periodic: false,
        }),
    })
}

fn object_stream_reference(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let lead = *bytes.get(*at)?;
    let (value, width) = match lead {
        0x38 => (u32_le_24(bytes, *at + 1)?, 4),
        0x30 => (u32::from(u16_le(bytes, *at + 1)?) << 8, 3),
        0x28 => (
            u32::from(*bytes.get(*at + 1)?) | (u32::from(*bytes.get(*at + 2)?) << 16),
            3,
        ),
        0x20 => (u32::from(*bytes.get(*at + 1)?) << 16, 2),
        0x18 => (u32::from(u16_le(bytes, *at + 1)?), 3),
        0x10 => (u32::from(*bytes.get(*at + 1)?) << 8, 2),
        0x08 => (u32::from(*bytes.get(*at + 1)?), 2),
        0x80..=0xff => (u32::from(lead - 0x80), 1),
        _ => return None,
    };
    *at += width;
    Some(value)
}

fn consume_array_marker(bytes: &[u8], at: usize) -> Option<usize> {
    if *bytes.get(at)? == 0x08 {
        bytes.get(at + 1).map(|_| at + 2)
    } else {
        Some(at + 1)
    }
}

fn f64_values(bytes: &[u8], at: &mut usize, count: usize, end: usize) -> Option<Vec<f64>> {
    if at.checked_add(count.checked_mul(8)?)? > end {
        return None;
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(f64_le(bytes, *at)?);
        *at += 8;
    }
    Some(values)
}

fn compact_values(bytes: &[u8], at: &mut usize, count: usize) -> Option<Vec<u32>> {
    (0..count).map(|_| compact_int(bytes, at)).collect()
}

fn monotonic(values: &[f64]) -> bool {
    values.windows(2).all(|pair| pair[0] <= pair[1])
}

fn a5_int(byte: u8) -> Option<u32> {
    (byte % 4 == 1).then_some(((byte - 1) / 4) as u32)
}

fn a5_array_marker(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at..at + 2) {
        Some([0x0c, ..]) => Some(at + 1),
        Some([0x08, 0x09]) => Some(at + 2),
        _ => None,
    }
}

fn a5_knots(distinct: &[f64], degree: u32) -> Option<(Vec<f64>, u32)> {
    let multiplicities = match degree {
        5 if distinct.len() >= 2 => {
            let mut values = vec![6u32];
            values.extend(std::iter::repeat_n(3, distinct.len() - 2));
            values.push(6);
            values
        }
        1 if distinct.len() == 2 => vec![2, 2],
        _ => return None,
    };
    let count = pole_count(&multiplicities, degree)?;
    Some((expand_knots(distinct, &multiplicities)?, count))
}

fn a5_weights(bytes: &[u8], at: &mut usize, rows: usize, cols: usize) -> Option<Vec<f64>> {
    if bytes.get(*at..*at + 3)? != [0x01, 0x07, 0x00] {
        return None;
    }
    *at += 3;
    let mut seed = Vec::new();
    while bytes.get(*at) != Some(&0x02) {
        seed.push(f64_le(bytes, *at)?);
        *at += 8;
    }
    let row = if seed.len() * 2 == cols {
        seed.iter()
            .copied()
            .chain(seed.iter().rev().copied())
            .collect()
    } else if seed.len() == cols {
        seed
    } else {
        return None;
    };
    let mut copies = 0usize;
    while bytes.get(*at) == Some(&0x02) {
        copies += 1;
        *at += 1;
    }
    if copies + 1 != rows || row.contains(&0.0) {
        return None;
    }
    Some((0..rows).flat_map(|_| row.iter().copied()).collect())
}
