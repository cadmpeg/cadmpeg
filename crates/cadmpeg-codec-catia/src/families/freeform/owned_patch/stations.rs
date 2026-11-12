// SPDX-License-Identifier: Apache-2.0
//! Edge endpoint stations bound by vertex incidence records.

use cadmpeg_core::decode::View;

use crate::families::consolidated::records::class25::{self, Class25SourceRelation};
use crate::nurbs::LaneRefusals;
use crate::wire::bytes::{allocation_reference, compact_int, AllocationReference};
use crate::wire::records::{
    owned_pcurve_from_record, ConsolidatedFamily as Family, ConsolidatedRecord,
};

use super::payload;

const STATION_TOLERANCE: f64 = 1.0e-9;

#[derive(Clone, Copy)]
pub(super) struct Domain<'a> {
    pub(super) range: [f64; 2],
    pub(super) knots: Option<(&'a [f64], usize, usize)>,
    pub(super) line: bool,
}

pub(super) fn parameter(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    vertex: usize,
    curve: usize,
    domain: Domain<'_>,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
    refusal: &mut LaneRefusals,
) -> Option<f64> {
    let typed = |index: usize, class: u8| {
        let record = records.get(index)?;
        (record.family() == Family::B && record.class() == class).then_some(record)
    };
    let vertex_bytes = payload(bytes, typed(vertex, 0x5d)?)?;
    let mut at = 0;
    let roster = resolve(vertex, allocation_reference(vertex_bytes, &mut at)?)?;
    if vertex_bytes.get(at..)? != [0] {
        return None;
    }
    let roster_bytes = payload(bytes, typed(roster, 0x05)?)?;
    let count = roster_bytes.first()?.checked_sub(0x80)?;
    let mut at = 1;
    let mut found: Option<f64> = None;
    for _ in 0..count {
        let incidence = resolve(roster, allocation_reference(roster_bytes, &mut at)?)?;
        let Some(incidence_bytes) = typed(incidence, 0x06).and_then(|r| payload(bytes, r)) else {
            continue;
        };
        if let Some((t, _)) =
            incidence_parameter(incidence_bytes, incidence, curve, domain, resolve, refusal)
        {
            if found.is_some_and(|prior| (prior - t).abs() > STATION_TOLERANCE) {
                return None;
            }
            found = Some(t);
        }
    }
    (at == roster_bytes.len()).then_some(found).flatten()
}

pub(super) fn edge_station(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    record: usize,
    curve: usize,
    domain: Domain<'_>,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
    refusal: &mut LaneRefusals,
) -> Option<(f64, bool)> {
    let incidence = records.get(record)?;
    if incidence.family() != Family::B || incidence.class() != 0x06 {
        return None;
    }
    incidence_parameter(
        payload(bytes, incidence)?,
        record,
        curve,
        domain,
        resolve,
        refusal,
    )
}

fn map_record(bytes: &[u8], record: &ConsolidatedRecord) -> Option<class25::Class25ParameterMap> {
    let bytes = payload(bytes, record)?;
    match (record.family(), record.class()) {
        (Family::B, 0x23..=0x25) => class25::parse(bytes),
        (Family::A, 0x25) => class25::parse_a(bytes),
        _ => None,
    }
}

fn operands(
    mapping: &class25::Class25ParameterMap,
    index: usize,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
) -> Option<[usize; 2]> {
    let bind = |operand: class25::Class25Operand| {
        resolve(
            index,
            AllocationReference {
                value: operand.value,
                encoding: operand.encoding,
            },
        )
    };
    Some([bind(mapping.source_operand)?, bind(mapping.target_operand)?])
}

pub(super) fn mapped_range(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    map: usize,
    curve: usize,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
) -> Option<[f64; 2]> {
    let mapping = map_record(bytes, records.get(map)?)?;
    let source = match mapping.source {
        Class25SourceRelation::Sampled { range, .. }
        | Class25SourceRelation::Affine { range, .. } => range,
        Class25SourceRelation::Absent => return None,
    };
    let [first, second] = operands(&mapping, map, resolve)?;
    match (first == curve, second == curve) {
        (true, false) => Some(mapping.target_range.endpoints()),
        (false, true) => Some(source.endpoints()),
        _ => None,
    }
}

pub(super) fn mapped_station(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    [vertex, map, curve, incidence]: [usize; 4],
    allow_spline_companion: bool,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
    refusal: &mut LaneRefusals,
) -> Option<(f64, bool)> {
    let mapping = map_record(bytes, records.get(map)?)?;
    let range = match &mapping.source {
        Class25SourceRelation::Sampled { range, .. }
        | Class25SourceRelation::Affine { range, .. } => *range,
        Class25SourceRelation::Absent => return None,
    };
    let [first, second] = operands(&mapping, map, resolve)?;
    if first != second && (first == curve || second == curve) {
        if let Some(t) = map_station(bytes, records, incidence, map, first, resolve)
            .filter(|&t| mapping.target_range.lower() <= t && t <= mapping.target_range.upper())
        {
            if first == curve {
                return Some((t, true));
            }
            if let Some(t) = map_parameter(&mapping.source, 0, t) {
                return Some((t, true));
            }
        }
    }
    let (axis, companion, limits) = match (first == curve, second == curve) {
        (true, false) => (1, second, range.endpoints()),
        (false, true) => (0, first, mapping.target_range.endpoints()),
        _ => return None,
    };
    let record = records.get(companion)?;
    let knots;
    let domain = if (record.family(), record.class()) == (Family::B, 0x18) {
        Domain {
            range: limits,
            knots: None,
            line: true,
        }
    } else if allow_spline_companion
        && matches!(record.family(), Family::A | Family::B)
        && record.class() == 0x20
    {
        let spline = owned_pcurve_from_record(bytes, record)?;
        knots = spline
            .sites
            .iter()
            .map(|site| site.knot.get())
            .collect::<Vec<_>>();
        let first_arc = spline_first_arc(payload(bytes, record)?, &knots)?;
        let extrapolation_sites = leading_extrapolation_sites(&knots, spline.extrapolation_sites);
        Domain {
            range: spline.range.endpoints(),
            knots: Some((&knots, first_arc, extrapolation_sites)),
            line: false,
        }
    } else {
        return None;
    };
    let t = edge_station(
        bytes, records, incidence, companion, domain, resolve, refusal,
    )
    .map(|(parameter, _)| parameter)
    .or_else(|| parameter(bytes, records, vertex, companion, domain, resolve, refusal))?;
    if t < limits[0] - STATION_TOLERANCE || t > limits[1] + STATION_TOLERANCE {
        return None;
    }
    map_parameter(&mapping.source, axis, t).map(|t| (t, false))
}

fn map_parameter(relation: &Class25SourceRelation, axis: usize, t: f64) -> Option<f64> {
    match relation {
        Class25SourceRelation::Affine { scale, offset, .. } => Some(if axis == 0 {
            (t - offset.get()) / scale.get()
        } else {
            scale.get().mul_add(t, offset.get())
        }),
        Class25SourceRelation::Sampled { samples, .. } => samples
            .pairs()
            .find(|sample| (sample[axis] - t).abs() <= STATION_TOLERANCE)
            .map(|sample| sample[1 - axis])
            .or_else(|| {
                samples
                    .pairs()
                    .all(|[a, b]| (a - b).abs() <= STATION_TOLERANCE)
                    .then_some(t)
            }),
        Class25SourceRelation::Absent => None,
    }
}

fn map_station(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    incidence: usize,
    map: usize,
    chart: usize,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
) -> Option<f64> {
    let record = records.get(incidence)?;
    if record.family() != Family::B || record.class() != 0x06 {
        return None;
    }
    let raw = payload(bytes, record)?;
    if raw.first() != Some(&0x82) {
        return None;
    }
    let mut at = 1;
    for _ in 0..2 {
        if resolve(incidence, allocation_reference(raw, &mut at)?)? != map {
            return None;
        }
    }
    let tail = raw.get(at..)?;
    if tail.len() != 10 || tail[0] != 0x80 || tail[9] % 4 != 1 {
        return None;
    }
    let local = View::f64_le_at(tail, 1)?;
    let record = records.get(chart)?;
    let spline = owned_pcurve_from_record(bytes, record)?;
    let knots = spline
        .sites
        .iter()
        .map(|site| site.knot.get())
        .collect::<Vec<_>>();
    let first_arc = spline_first_arc(payload(bytes, record)?, &knots)?;
    let extrapolation_sites = leading_extrapolation_sites(&knots, spline.extrapolation_sites);
    arc_parameter(local, tail[9], &knots, first_arc, extrapolation_sites)
}

/// Resolve a circle endpoint through the edge selector's affine chart map.
pub(super) fn mapped_circle_endpoint(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    [station, map, circle]: [usize; 3],
    endpoint: usize,
    range: [f64; 2],
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
) -> Option<f64> {
    let typed = |index: usize, class: u8| {
        let record = records.get(index)?;
        (record.family() == Family::B && record.class() == class).then_some(record)
    };
    let mapping = class25::parse(payload(bytes, typed(map, 0x25)?)?)?;
    let Class25SourceRelation::Affine {
        range: from,
        scale,
        offset,
    } = mapping.source
    else {
        return None;
    };
    let bind = |operand: class25::Class25Operand| {
        resolve(
            map,
            AllocationReference {
                value: operand.value,
                encoding: operand.encoding,
            },
        )
    };
    if bind(mapping.source_operand)? != circle || mapping.target_range.endpoints() != range {
        return None;
    }
    let station_bytes = payload(bytes, typed(station, 0x06)?)?;
    let mut at = 1;
    if station_bytes.first() != Some(&0x83)
        || resolve(station, allocation_reference(station_bytes, &mut at)?)? != map
        || resolve(station, allocation_reference(station_bytes, &mut at)?)?
            != bind(mapping.target_operand)?
        || resolve(station, allocation_reference(station_bytes, &mut at)?)? != circle
    {
        return None;
    }
    let tail = station_bytes.get(at..)?;
    if tail.len() != 9 || !matches!((endpoint, tail[0]), (0, 0x22) | (1, 0x11)) {
        return None;
    }
    let t = View::f64_le_at(tail, 1)?;
    if t < from.lower() - STATION_TOLERANCE || t > from.upper() + STATION_TOLERANCE {
        return None;
    }
    let uv = scale.get().mul_add(t, offset.get());
    (uv.is_finite() && (uv - range[endpoint]).abs() <= STATION_TOLERANCE).then_some(range[endpoint])
}

pub(super) fn mapped_spline_parameter(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    [vertex, map, curve]: [usize; 3],
    range: [f64; 2],
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
    refusal: &mut LaneRefusals,
) -> Option<f64> {
    let record = records.get(map)?;
    if record.family() != Family::B || !matches!(record.class(), 0x23 | 0x25) {
        return None;
    }
    let mapping = class25::parse(payload(bytes, record)?)?;
    let Class25SourceRelation::Affine {
        range: from,
        scale,
        offset,
    } = mapping.source
    else {
        return None;
    };
    let bind = |operand: class25::Class25Operand| {
        resolve(
            map,
            AllocationReference {
                value: operand.value,
                encoding: operand.encoding,
            },
        )
    };
    if bind(mapping.source_operand)? != curve || mapping.target_range.endpoints() != range {
        return None;
    }
    let basis = bind(mapping.target_operand)?;
    let companion_record = records.get(basis)?;
    let companion = owned_pcurve_from_record(bytes, companion_record)?;
    let knots = companion
        .sites
        .iter()
        .map(|site| site.knot.get())
        .collect::<Vec<_>>();
    let first_arc = if record.class() == 0x23 && scale.get() == 1.0 && offset.get() == 0.0 {
        spline_first_arc(payload(bytes, companion_record)?, &knots)?
    } else {
        1
    };
    let extrapolation_sites = leading_extrapolation_sites(&knots, companion.extrapolation_sites);
    let t = parameter(
        bytes,
        records,
        vertex,
        basis,
        Domain {
            range: from.endpoints(),
            knots: Some((&knots, first_arc, extrapolation_sites)),
            line: false,
        },
        resolve,
        refusal,
    )?;
    let uv = scale.get().mul_add(t, offset.get());
    (uv.is_finite() && uv >= range[0] - STATION_TOLERANCE && uv <= range[1] + STATION_TOLERANCE)
        .then(|| uv.clamp(range[0], range[1]))
}

pub(super) fn spline_first_arc(bytes: &[u8], knots: &[f64]) -> Option<usize> {
    if !knots.first().is_some_and(|&knot| knot > 0.0) {
        return Some(1);
    }
    let mut at = 0;
    for _ in 0..3 {
        compact_int(bytes, &mut at)?;
    }
    Some(match bytes.get(at..at + 2)? {
        [0x08, code] if code % 4 == 3 => usize::from(code / 4) + 2,
        _ => 1,
    })
}

pub(super) fn leading_extrapolation_sites(knots: &[f64], encoded: u32) -> usize {
    let count = knots
        .iter()
        .take_while(|&&k| k < -STATION_TOLERANCE)
        .count();
    if u32::try_from(count).ok() == Some(encoded)
        && knots
            .get(count)
            .is_some_and(|k| k.abs() <= STATION_TOLERANCE)
    {
        count
    } else {
        0
    }
}

fn arc_parameter(
    t: f64,
    code: u8,
    knots: &[f64],
    first_arc: usize,
    extrapolation_sites: usize,
) -> Option<f64> {
    let arc = usize::from(code.checked_sub(1)? / 4)
        .checked_sub(first_arc)?
        .checked_add(extrapolation_sites)?;
    parameter_on_arc(t, knots, arc)
}

fn parameter_on_arc(t: f64, knots: &[f64], arc: usize) -> Option<f64> {
    let a = *knots.get(arc)?;
    let b = *knots.get(arc + 1)?;
    if !t.is_finite() || t < -STATION_TOLERANCE || t > (b - a) + STATION_TOLERANCE {
        return None;
    }
    Some(a + t.clamp(0.0, b - a))
}

fn incidence_parameter(
    bytes: &[u8],
    record: usize,
    curve: usize,
    Domain {
        range: [lo, hi],
        knots,
        line,
    }: Domain<'_>,
    resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
    refusal: &mut LaneRefusals,
) -> Option<(f64, bool)> {
    let count = match bytes.first()? {
        0x82 => 2,
        0x83 => 3,
        0x84 => 4,
        _ => return None,
    };
    let mut at = 1;
    let mut lane = None;
    for index in 0..count {
        if resolve(record, allocation_reference(bytes, &mut at)?)? == curve
            && lane.replace(index).is_some()
        {
            return None;
        }
    }
    let lane = lane?;
    let tail = bytes.get(at..)?;
    let scalar = |at: usize, tagged: bool| {
        let t = View::f64_le_at(tail, at)?;
        let tag = tagged.then(|| tail.get(at + 8..)).flatten();
        match (knots, tag) {
            (Some((knots, _, _)), Some([0x04, code, ..])) => {
                parameter_on_arc(t, knots, usize::from(code.checked_sub(0x44)?))
            }
            (Some((knots, first_arc, extrapolation_sites)), Some([code, ..])) if *code > 1 => {
                arc_parameter(t, *code, knots, first_arc, extrapolation_sites)
            }
            _ => Some(t),
        }
    };
    let [min, max] = if let Some((knots, _, _)) = knots {
        [*knots.first()?, *knots.last()?]
    } else {
        [lo.min(hi), lo.max(hi)]
    };
    let valid = |t: f64| {
        t.is_finite() && (line || (t >= min - STATION_TOLERANCE && t <= max + STATION_TOLERANCE))
    };
    let mut inferred = None;
    let t = match (count, lane, tail) {
        (2, 1, [0x84]) => hi,
        (2, 1, [0x88]) => lo,
        (3, 1 | 2, [code @ (0x85 | 0x86 | 0x89 | 0x8a)]) => match (code >> (2 * (lane - 1))) & 3 {
            1 => hi,
            2 => lo,
            _ => return None,
        },
        (2, 1, [0x80, ..]) if tail.len() == 9 => scalar(1, false)?,
        (2, 1, [0x80, ..]) if tail.len() == 10 && tail[9] % 4 == 1 => scalar(1, true)?,
        (2, 1, [0x80, ..]) if tail.len() == 11 && tail[9] == 0x04 => scalar(1, true)?,
        (2, _, [0, ..]) if tail.len() == 19 && tail[9] % 4 == 1 && tail[18] % 4 == 1 => {
            scalar(1 + 9 * lane, true)?
        }
        (2, _, [0, ..]) if tail.len() == 18 && tail[9] % 4 == 1 => scalar(1 + 9 * lane, lane == 0)?,
        (4, 2 | 3, [0x80, ..]) if tail.len() == 27 && tail[9] % 4 == 1 && tail[18] == 0 => {
            scalar(1 + 9 * (lane - 1), false)?
        }
        (3, _, [0, ..])
            if tail.len() == 28 && [9, 18, 27].into_iter().all(|i| tail[i] % 4 == 1) =>
        {
            scalar(1 + 9 * lane, true)?
        }
        (3, _, [0, ..]) if tail.len() == 27 && tail[9] % 4 == 1 && tail[26] % 4 == 1 => {
            scalar([1, 10, 18][lane], lane != 1)?
        }
        (3, _, [0, ..]) if tail.len() == 29 && tail[9] == 0x04 && tail[27] == 0x04 => {
            scalar([1, 11, 19][lane], lane != 1)?
        }
        (3, 1 | 2, [0x10, ..]) if tail.len() == 18 && tail[9] % 4 == 1 => {
            scalar(if lane == 1 { 1 } else { 10 }, lane == 1)?
        }
        (3, 1 | 2, [0x80, ..]) if tail.len() == 18 => {
            let first_tag = tail[9] % 4 == 1;
            let final_tag = tail[17] % 4 == 1;
            let legacy_layout = first_tag || (line && lane == 2 && tail[9] % 2 == 1);
            let legacy = if legacy_layout {
                scalar(if lane == 1 { 1 } else { 10 }, lane == 1).filter(|&t| valid(t))
            } else {
                None
            };
            let extended = final_tag
                .then(|| scalar(if lane == 1 { 1 } else { 9 }, lane == 2))
                .flatten()
                .filter(|&t| valid(t));
            let t = legacy.or(extended)?;
            if legacy_layout && final_tag {
                inferred = Some(legacy.is_some());
            }
            t
        }
        (3, 1, [0x11 | 0x22, ..]) | (3, 2, [0x05, ..]) if tail.len() == 10 && tail[9] % 4 == 1 => {
            scalar(1, true)?
        }
        (3, 1 | 2, [0x10 | 0x20 | 0x80, ..]) if tail.len() == 17 => {
            scalar(1 + 8 * (lane - 1), false)?
        }
        (3, 1 | 2, [0x80, ..]) if tail.len() == 19 && tail[9] % 4 == 1 && tail[18] % 4 == 1 => {
            scalar(1 + 9 * (lane - 1), true)?
        }
        (3, 1, [0x80, ..]) if tail.len() == 19 && tail[9] == 0x04 => scalar(1, true)?,
        (3, 2, [0x80, ..]) if tail.len() == 19 && tail[9] == 0x04 => scalar(11, false)?,
        (3, 2, [0x88, ..]) if line && tail.len() == 9 => scalar(1, false)?,
        (3, 1, [0x81, ..]) | (3, 2, [0x82, ..]) if tail.len() == 9 => scalar(1, false)?,
        (3, 1, [0x81, ..]) | (3, 2, [0x82, ..]) if tail.len() == 10 && tail[9] % 4 == 1 => {
            scalar(1, true)?
        }
        (3, 1, [0x81, ..]) if tail.len() == 11 && tail[9] == 0x04 => scalar(1, true)?,
        _ => return None,
    };
    if !valid(t) {
        return None;
    }
    if let Some(first_tag) = inferred {
        refusal.push_endpoint_station_reading_inferred(record, curve, t, first_tag);
    }
    let arc_station = count == 2
        && knots.is_some()
        && (matches!(tail, [0x80, _, _, _, _, _, _, _, _, code] if *code > 1)
            || matches!(tail, [0x80, _, _, _, _, _, _, _, _, 0x04, _]));
    Some((if line { t } else { t.clamp(min, max) }, arc_station))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures;

    fn station_value(
        bytes: &[u8],
        record: usize,
        curve: usize,
        domain: Domain<'_>,
        resolve: &impl Fn(usize, AllocationReference) -> Option<usize>,
    ) -> Option<f64> {
        incidence_parameter(
            bytes,
            record,
            curve,
            domain,
            resolve,
            &mut LaneRefusals::new(),
        )
        .map(|(t, _)| t)
    }

    #[test]
    fn arc_local_stations_skip_encoded_leading_extrapolation() {
        let knots = [-2.0, -0.5, 0.0, 1.5];
        let count = leading_extrapolation_sites(&knots, 2);
        assert_eq!(arc_parameter(0.25, 0x05, &knots, 1, count), Some(0.25));
        assert_eq!(leading_extrapolation_sites(&knots, 0), 0);
        assert_eq!(leading_extrapolation_sites(&[-2.0, -0.5, 1.0, 1.5], 2), 0);
    }

    #[test]
    fn mixed_station_lanes_keep_their_own_parameter_gauges() {
        let resolve =
            |_: usize, reference: AllocationReference| usize::try_from(reference.value).ok();
        let mut bytes = vec![0x83, 3, 7, 11, 0x10];
        bytes.extend(0.5_f64.to_le_bytes());
        bytes.push(0x21);
        bytes.extend(12.25_f64.to_le_bytes());
        let knots = [2.0, 4.0, 7.0];
        let first_arc =
            spline_first_arc(&[5, 0x15, 0x0d, 8, 0x17], &knots).expect("valid fixture station");
        assert_eq!(
            station_value(
                &bytes,
                0,
                1,
                Domain {
                    range: [2.0, 7.0],
                    knots: Some((&knots, first_arc, 0)),
                    line: false
                },
                &resolve
            ),
            Some(4.5)
        );
        assert_eq!(
            station_value(
                &bytes,
                0,
                2,
                Domain {
                    range: [10.0, 15.0],
                    knots: None,
                    line: true
                },
                &resolve
            ),
            Some(12.25)
        );
    }

    #[test]
    fn endpoint_parameters_select_the_resolved_occurrence_lane() {
        let resolve =
            |_: usize, reference: AllocationReference| usize::try_from(reference.value).ok();
        let mut bytes = vec![0x83, 3, 7, 11, 0x80];
        bytes.extend(2.0_f64.to_le_bytes());
        bytes.push(0x05);
        bytes.extend((-3.0_f64).to_le_bytes());
        bytes.push(0x09);
        assert_eq!(
            station_value(
                &bytes,
                0,
                1,
                Domain {
                    range: [0.0, 10.0],
                    knots: None,
                    line: false
                },
                &resolve
            ),
            Some(2.0)
        );
        assert_eq!(
            station_value(
                &bytes,
                0,
                2,
                Domain {
                    range: [-5.0, 0.0],
                    knots: None,
                    line: false
                },
                &resolve
            ),
            Some(-3.0)
        );
        assert_eq!(
            station_value(
                &bytes,
                0,
                2,
                Domain {
                    range: [0.0, 10.0],
                    knots: None,
                    line: false
                },
                &resolve
            ),
            None
        );
        assert_eq!(
            station_value(
                &bytes,
                0,
                0,
                Domain {
                    range: [0.0, 10.0],
                    knots: None,
                    line: false
                },
                &resolve
            ),
            None
        );
    }

    #[test]
    fn tagged_endpoint_parameters_use_the_distinct_knot_span() {
        let resolve =
            |_: usize, reference: AllocationReference| usize::try_from(reference.value).ok();
        let mut bytes = vec![0x82, 3, 7, 0x80];
        bytes.extend(0.5_f64.to_le_bytes());
        bytes.push(0x09);
        let knots = [-1.0, 2.0, 4.0];
        assert_eq!(
            incidence_parameter(
                &bytes,
                0,
                1,
                Domain {
                    range: [0.0, 4.0],
                    knots: Some((&knots, 1, 0)),
                    line: false
                },
                &resolve,
                &mut LaneRefusals::new(),
            ),
            Some((2.5, true))
        );
        bytes[12] = 0x0d;
        assert_eq!(
            incidence_parameter(
                &bytes,
                0,
                1,
                Domain {
                    range: [0.0, 4.0],
                    knots: Some((&knots, 1, 0)),
                    line: false
                },
                &resolve,
                &mut LaneRefusals::new(),
            ),
            None
        );
    }

    #[test]
    fn extended_endpoint_tag_selects_an_absolute_knot_span() {
        let resolve =
            |_: usize, reference: AllocationReference| usize::try_from(reference.value).ok();
        let mut bytes = vec![0x82, 3, 7, 0x80];
        bytes.extend(0.5_f64.to_le_bytes());
        bytes.extend([0x04, 0x45]);
        let knots = [-1.0, 2.0, 4.0];
        let domain = Domain {
            range: [-1.0, 4.0],
            knots: Some((&knots, 1, 1)),
            line: false,
        };
        assert_eq!(
            incidence_parameter(&bytes, 0, 1, domain, &resolve, &mut LaneRefusals::new(),),
            Some((2.5, true))
        );
        bytes[13] = 0x46;
        assert_eq!(
            incidence_parameter(&bytes, 0, 1, domain, &resolve, &mut LaneRefusals::new(),),
            None
        );
    }
}
