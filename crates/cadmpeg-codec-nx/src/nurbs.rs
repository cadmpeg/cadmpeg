// SPDX-License-Identifier: Apache-2.0
//! Decode NURBS curves and surfaces from Parasolid neutral-binary records.
//!
//! The decoder joins descriptor, payload, knot, and multiplicity records by
//! their stream-scoped references. Control points are converted from metres to
//! millimetres. Invalid references, dimensions, knots, control points, and
//! weights cause the affected carrier to be omitted.

#![deny(clippy::disallowed_methods)]

use crate::framing::node_kind::NodeKind;
use std::collections::BTreeMap;

use crate::framing::read_xmt_width as read_xmt;
use cadmpeg_parasolid::xmt::NonNullXmt;
pub(crate) mod curve_references;
use crate::layout::nurbs_curve_descriptor_prefix as curve_desc;
use crate::layout::nurbs_surface_descriptor_prefix as surf_desc;
use crate::topology::Graph;
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use curve_references::CurveDescriptorReferences;

/// A decoded NURBS surface and its source descriptor offset.
#[derive(Debug, Clone)]
pub(crate) struct Surface {
    /// Byte offset of the tag-126 descriptor record within the input stream.
    pub(crate) pos: usize,
    /// Reconstructed surface geometry.
    pub(crate) geometry: SurfaceGeometry,
}

/// A decoded NURBS curve and its source descriptor offset.
#[derive(Debug, Clone)]
pub(crate) struct Curve {
    /// Byte offset of the tag-136 descriptor record within the input stream.
    pub(crate) pos: usize,
    /// Reconstructed curve geometry.
    pub(crate) geometry: CurveGeometry,
}

/// A decoded parameter-space NURBS curve and its source wrapper offset.
#[derive(Debug, Clone)]
pub(crate) struct Pcurve {
    /// Byte offset of the tag-134 wrapper record within the input stream.
    pub(crate) pos: usize,
    /// Reconstructed parameter-space geometry.
    pub(crate) geometry: PcurveGeometry,
}

/// Decode valid NURBS surface record families in source order.
///
/// The returned geometry uses millimetre control points. Malformed references,
/// knots, dimensions, control points, and weights are skipped.
#[cfg(test)]
pub(crate) fn surfaces(bytes: &[u8]) -> (Vec<Surface>, Vec<CarrierRefusal>) {
    let arrays = arrays(bytes);
    let payloads = surface_payloads(bytes);
    let descriptors = surface_descriptors(bytes);
    let graph = Graph::parse(bytes);
    let mut refusals = Vec::new();
    let surfaces = decode_surfaces(&graph, &arrays, &payloads, &descriptors, &mut refusals);
    (surfaces, refusals)
}

fn decode_surfaces(
    graph: &Graph,
    arrays: &Arrays<'_>,
    payloads: &BTreeMap<u32, Option<Payload<'_>>>,
    descriptors: &BTreeMap<u32, Option<SurfaceDescriptor>>,
    refusals: &mut Vec<CarrierRefusal>,
) -> Vec<Surface> {
    use cadmpeg_parasolid::geometry::nurbs::{b_surface, Knots, SurfaceFields};
    let mut records = Vec::new();
    for node in graph.of_kind(NodeKind::BSurface) {
        let candidate = (|| {
            let refs = node.compact_tail_references::<2>()?;
            let descriptor = descriptors.get(&refs[0])?.as_ref()?;
            descriptor
                .payload
                .is_none_or(|payload| payload == refs[1])
                .then_some(())?;
            let payload = payloads.get(&refs[1])?.as_ref()?;
            let poles = descriptor.u_count.checked_mul(descriptor.v_count)?;
            let (vertex_dim, rational) = match descriptor.vertex_layout {
                Some((dimension, rational)) => (usize::from(dimension), rational),
                None => stride_layout(payload.value_count(), poles, 3)?,
            };
            let u_mult = arrays
                .u16s
                .get(&descriptor.u_mult)?
                .as_ref()?
                .u16_values()?;
            let v_mult = arrays
                .u16s
                .get(&descriptor.v_mult)?
                .as_ref()?
                .u16_values()?;
            let u_knots = arrays
                .f64s
                .get(&descriptor.u_knots)?
                .as_ref()?
                .f64_values()?;
            let v_knots = arrays
                .f64s
                .get(&descriptor.v_knots)?
                .as_ref()?
                .f64_values()?;
            let vertices = payload.values()?;
            let fields = SurfaceFields {
                degrees: [
                    u32::from(descriptor.u_degree),
                    u32::from(descriptor.v_degree),
                ],
                vertex_counts: [descriptor.u_count, descriptor.v_count],
                vertex_dim,
                rational,
                periodic: [descriptor.u_periodic, descriptor.v_periodic],
                vertices: &vertices,
                knots: [
                    Knots {
                        count: descriptor.u_distinct,
                        multiplicities: &u_mult,
                        values: &u_knots,
                    },
                    Knots {
                        count: descriptor.v_distinct,
                        multiplicities: &v_mult,
                        values: &v_knots,
                    },
                ],
            };
            let normal_reversed = node.common_header()?.0 == cadmpeg_ir::topology::Sense::Reversed;
            let surface = match b_surface(&fields, normal_reversed) {
                Ok(surface) => surface,
                Err(refusal) => {
                    refusals.push(CarrierRefusal {
                        pos: node.pos(),
                        family: "B_SURFACE",
                        refusal,
                    });
                    return None;
                }
            };
            Some(Surface {
                pos: node.pos(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            })
        })();
        if let Some(record) = candidate {
            records.push(record);
        }
    }
    records
}

/// The vertex dimension and rational flag of control values with `poles`
/// control points when the descriptor states neither: `base` values per
/// point are polynomial, `base + 1` are homogeneous.
fn stride_layout(value_count: usize, poles: usize, base: usize) -> Option<(usize, bool)> {
    let stride = value_count.checked_div(poles)?;
    (stride.checked_mul(poles)? == value_count).then_some(())?;
    if stride == base {
        Some((base, false))
    } else if stride == base + 1 {
        Some((base + 1, true))
    } else {
        None
    }
}

/// The shared B-curve geometry of `descriptor` over the lanes it names. A
/// compact descriptor states the space dimension without a rational flag;
/// the control values then add one weight per point when rational.
fn b_curve_geometry(
    arrays: &Arrays<'_>,
    controls: &BTreeMap<u32, Option<Payload<'_>>>,
    descriptor: &CurveDescriptor,
    control_ref: u32,
    planar: bool,
) -> Option<
    Result<cadmpeg_parasolid::geometry::nurbs::BCurve, cadmpeg_parasolid::geometry::nurbs::Refusal>,
> {
    use cadmpeg_parasolid::geometry::nurbs::{b_curve, CurveFields, Knots};
    let control = controls.get(&control_ref)?.as_ref()?;
    let (vertex_dim, rational) = match descriptor.basis.rational {
        Some(rational) => (usize::from(descriptor.basis.dimension), rational),
        None => {
            let space = if planar { 2 } else { 3 };
            let layout = stride_layout(control.value_count(), descriptor.basis.poles, space)?;
            (usize::from(descriptor.basis.dimension) == space || layout.0 == 4).then_some(layout)?
        }
    };
    let multiplicities = arrays
        .u16s
        .get(&descriptor.references.multiplicities())?
        .as_ref()?
        .u16_values()?;
    let values = arrays
        .f64s
        .get(&descriptor.references.knots())?
        .as_ref()?
        .f64_values()?;
    let vertices = control.values()?;
    Some(b_curve(&CurveFields {
        degree: u32::from(descriptor.basis.degree),
        vertex_count: descriptor.basis.poles,
        vertex_dim,
        rational,
        periodic: descriptor.basis.periodic,
        vertices: &vertices,
        knots: Knots {
            count: descriptor.basis.distinct,
            multiplicities: &multiplicities,
            values: &values,
        },
    }))
}

/// Decode dimension-2 `B_CURVE` families as surface parameter-space curves.
#[cfg(test)]
pub(crate) fn pcurves(bytes: &[u8]) -> (Vec<Pcurve>, Vec<CarrierRefusal>) {
    let arrays = arrays(bytes);
    let controls = curve_payloads(bytes);
    let descriptors = curve_descriptors(bytes);
    let graph = Graph::parse(bytes);
    let mut refusals = Vec::new();
    let pcurves = decode_pcurves(&graph, &arrays, &controls, &descriptors, &mut refusals);
    (pcurves, refusals)
}

fn decode_pcurves(
    graph: &Graph,
    arrays: &Arrays<'_>,
    controls: &BTreeMap<u32, Option<Payload<'_>>>,
    descriptors: &BTreeMap<u32, Option<CurveDescriptor>>,
    refusals: &mut Vec<CarrierRefusal>,
) -> Vec<Pcurve> {
    use cadmpeg_parasolid::geometry::nurbs::BCurve;
    let mut records = Vec::new();
    for node in graph.of_kind(NodeKind::BCurve) {
        let candidate = (|| {
            let refs = node.compact_tail_references::<2>()?;
            let descriptor = descriptors.get(&refs[0])?.as_ref()?;
            matches!(
                (descriptor.basis.dimension, descriptor.basis.rational),
                (2, _) | (3, Some(true))
            )
            .then_some(())?;
            match b_curve_geometry(arrays, controls, descriptor, refs[1], true)? {
                Ok(BCurve::Planar(nurbs)) => Some(Pcurve {
                    pos: node.pos(),
                    geometry: PcurveGeometry::Nurbs { nurbs },
                }),
                Ok(BCurve::Spatial(_)) => None,
                Err(refusal) => {
                    refusals.push(CarrierRefusal {
                        pos: node.pos(),
                        family: "B_CURVE pcurve",
                        refusal,
                    });
                    None
                }
            }
        })();
        if let Some(record) = candidate {
            records.push(record);
        }
    }
    records
}

/// Decode valid NURBS curve record families in source order.
///
/// The returned geometry uses millimetre control points. Malformed references,
/// knots, dimensions, control points, and weights are skipped.
#[cfg(test)]
pub(crate) fn curves(bytes: &[u8]) -> (Vec<Curve>, Vec<CarrierRefusal>) {
    let arrays = arrays(bytes);
    let controls = curve_payloads(bytes);
    let descriptors = curve_descriptors(bytes);
    let graph = Graph::parse(bytes);
    let mut refusals = Vec::new();
    let curves = decode_curves(&graph, &arrays, &controls, &descriptors, &mut refusals);
    (curves, refusals)
}

fn decode_curves(
    graph: &Graph,
    arrays: &Arrays<'_>,
    controls: &BTreeMap<u32, Option<Payload<'_>>>,
    descriptors: &BTreeMap<u32, Option<CurveDescriptor>>,
    refusals: &mut Vec<CarrierRefusal>,
) -> Vec<Curve> {
    use cadmpeg_parasolid::geometry::nurbs::BCurve;
    let mut records = Vec::new();
    for node in graph.of_kind(NodeKind::BCurve) {
        let candidate = (|| {
            let refs = node.compact_tail_references::<2>()?;
            let descriptor = descriptors.get(&refs[0])?.as_ref()?;
            matches!(
                (descriptor.basis.dimension, descriptor.basis.rational),
                (3, None | Some(false)) | (4, _)
            )
            .then_some(())?;
            match b_curve_geometry(arrays, controls, descriptor, refs[1], false)? {
                Ok(BCurve::Spatial(curve)) => Some(Curve {
                    pos: node.pos(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                }),
                Ok(BCurve::Planar(_)) => None,
                Err(refusal) => {
                    refusals.push(CarrierRefusal {
                        pos: node.pos(),
                        family: "B_CURVE",
                        refusal,
                    });
                    None
                }
            }
        })();
        if let Some(record) = candidate {
            records.push(record);
        }
    }
    records
}

/// All NURBS geometry families decoded from one graph and one byte view.
///
/// The descriptor, payload, and array lanes are shared across the three
/// family decoders. Callers that already own the parsed topology graph use
/// this entry point to avoid rescanning the same byte view for each family.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) curves: Vec<Curve>,
    pub(crate) pcurves: Vec<Pcurve>,
    /// Records of the right family that state no carrier.
    pub(crate) refusals: Vec<CarrierRefusal>,
}

/// A carrier record of the right family that states no carrier.
#[derive(Debug, Clone)]
pub(crate) struct CarrierRefusal {
    /// Byte offset of the descriptor record within the stream.
    pub(crate) pos: usize,
    /// Carrier family the record states.
    pub(crate) family: &'static str,
    /// The carrier's refusal.
    pub(crate) refusal: cadmpeg_parasolid::geometry::nurbs::Refusal,
}

impl CarrierRefusal {
    /// The loss of this record in Parasolid stream `si`.
    pub(crate) fn loss(&self, si: usize) -> cadmpeg_ir::report::loss::LossNote {
        use crate::loss::NxLossCode;
        use cadmpeg_parasolid::geometry::nurbs::Refusal;
        let record = format!("parasolid#{si} {} at byte {}", self.family, self.pos);
        let (code, cause) = match &self.refusal {
            Refusal::Lanes(error) => {
                return NxLossCode::CarrierLanesUnpaired
                    .note(format!("{record} states no carrier: {error}"))
            }
            Refusal::NonpositiveWeight => (
                NxLossCode::CarrierSplineWeightNonpositive,
                "stores a control-point weight that is zero or negative",
            ),
            Refusal::Fields => (
                NxLossCode::CarrierSplineFieldsInconsistent,
                "stores a dimension, count, or multiplicity that contradicts its fields or arrays",
            ),
        };
        code.note(format!("{record} {cause}; the carrier is not emitted."))
    }
}

pub(crate) fn parse_with_graph(bytes: &[u8], graph: &Graph) -> Parsed {
    let arrays = arrays(bytes);
    let surface_payloads = surface_payloads(bytes);
    let curve_payloads = curve_payloads(bytes);
    let surface_descriptors = surface_descriptors(bytes);
    let curve_descriptors = curve_descriptors(bytes);
    let mut refusals = Vec::new();
    Parsed {
        surfaces: decode_surfaces(
            graph,
            &arrays,
            &surface_payloads,
            &surface_descriptors,
            &mut refusals,
        ),
        curves: decode_curves(
            graph,
            &arrays,
            &curve_payloads,
            &curve_descriptors,
            &mut refusals,
        ),
        pcurves: decode_pcurves(
            graph,
            &arrays,
            &curve_payloads,
            &curve_descriptors,
            &mut refusals,
        ),
        refusals,
    }
}

type RecordIndex<T> = BTreeMap<u32, Option<T>>;

/// Insert a record when its reference is new; report whether it was inserted.
fn insert_if_vacant<T>(records: &mut RecordIndex<T>, reference: u32, value: Option<T>) -> bool {
    match records.entry(reference) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            true
        }
        std::collections::btree_map::Entry::Occupied(_) => false,
    }
}

struct Arrays<'bytes> {
    u16s: RecordIndex<ArrayValues<'bytes>>,
    f64s: RecordIndex<ArrayValues<'bytes>>,
}

#[derive(Clone, Copy)]
enum ArrayValues<'a> {
    U16(&'a [u8]),
    F64(&'a [u8]),
}

struct ArrayRecord<'a> {
    reference: u32,
    values: ArrayValues<'a>,
}

impl ArrayValues<'_> {
    fn u16_values(&self) -> Option<Vec<u16>> {
        let ArrayValues::U16(raw) = self else {
            return None;
        };
        self.u16_prefix(raw.len() / 2)
    }
    fn f64_values(&self) -> Option<Vec<f64>> {
        let ArrayValues::F64(raw) = self else {
            return None;
        };
        self.f64_prefix(raw.len() / 8)
    }
    fn u16_prefix(&self, count: usize) -> Option<Vec<u16>> {
        let ArrayValues::U16(raw) = self else {
            return None;
        };
        let raw = count.checked_mul(2).and_then(|end| raw.get(..end))?;

        let mut values = Vec::new();
        for bytes in raw.chunks_exact(2) {
            values.push(View::u16_be_at(bytes, 0)?);
        }
        Some(values)
    }
    fn f64_prefix(&self, count: usize) -> Option<Vec<f64>> {
        let ArrayValues::F64(raw) = self else {
            return None;
        };
        let raw = count.checked_mul(8).and_then(|end| raw.get(..end))?;

        let mut values = Vec::new();
        for bytes in raw.chunks_exact(8) {
            values.push(View::f64_be_at(bytes, 0)?);
        }
        Some(values)
    }
}

fn arrays(bytes: &[u8]) -> Arrays<'_> {
    let mut u16s = RecordIndex::new();
    let mut f64s = RecordIndex::new();
    for pos in bytes
        .len()
        .checked_sub(7)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        if let Some(record) = array_record_at(bytes, pos) {
            let index = match record.values {
                ArrayValues::U16(_) => &mut u16s,
                ArrayValues::F64(_) => &mut f64s,
            };
            if !insert_if_vacant(index, record.reference, Some(record.values)) {
                if let Some(value) = index.get_mut(&record.reference) {
                    *value = None;
                }
            }
        }
    }
    Arrays { u16s, f64s }
}

fn array_record_at(bytes: &[u8], pos: usize) -> Option<ArrayRecord<'_>> {
    {
        let tag = *bytes.get(pos + 1)?;
        let width = match bytes.get(pos..pos + 2)? {
            [0, 127] => 2,
            [0, 128] => 8,
            _ => return None,
        };
        let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
        let count = usize::try_from(View::u32_be_at(bytes, pos + 2 + escape)?).ok()?;
        (count > 0).then_some(())?;
        let (reference, reference_len) = read_xmt(bytes, pos + 6 + escape)?;
        NonNullXmt::try_from(reference).ok()?;
        let data = pos + 6 + escape + reference_len;
        let end = data.checked_add(count.checked_mul(width)?)?;
        let raw = bytes.get(data..end)?;
        let values = if tag == 127 {
            ArrayValues::U16(raw)
        } else {
            finite_f64_bytes(raw)?;
            ArrayValues::F64(raw)
        };
        Some(ArrayRecord { reference, values })
    }
}

#[derive(Clone, Copy)]
struct Payload<'a> {
    raw: &'a [u8],
}

impl Payload<'_> {
    fn value_count(self) -> usize {
        self.raw.len() / 8
    }

    fn value_at(self, index: usize) -> Option<f64> {
        View::f64_be_at(self.raw, index.checked_mul(8)?)
    }

    fn values(self) -> Option<Vec<f64>> {
        (0..self.value_count())
            .map(|index| self.value_at(index))
            .collect()
    }
}

fn surface_payloads(bytes: &[u8]) -> RecordIndex<Payload<'_>> {
    let records = bytes
        .len()
        .checked_sub(96)
        .into_iter()
        .flat_map(|last| 0..last)
        .map(|pos| surface_payload_at(bytes, pos).map(|(xmt, payload, _)| (xmt, payload)));
    unique_records(records)
}

fn surface_payload_at(bytes: &[u8], pos: usize) -> Option<(u32, Payload<'_>, usize)> {
    {
        (bytes.get(pos..pos + 2) == Some(&[0, 125])).then_some(())?;
        let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
        let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
        NonNullXmt::try_from(xmt).ok()?;
        let shift = escape + xmt_len - 2;
        let count_escape = usize::from(bytes.get(pos + 91 + shift) == Some(&0xff));
        let count_at = pos + 91 + shift + count_escape;
        let nested_same_record = (pos + 2..count_at).any(|candidate| {
            if bytes.get(candidate..candidate + 2) != Some(&[0, 125]) {
                return false;
            }
            let escape = usize::from(bytes.get(candidate + 2) == Some(&0xff));
            read_xmt(bytes, candidate + 2 + escape)
                .is_some_and(|(candidate_xmt, _)| candidate_xmt == xmt)
        });
        (!nested_same_record).then_some(())?;
        let count = usize::try_from(View::u32_be_at(bytes, count_at)?).ok()?;
        (count > 0).then_some(())?;
        let (_, first_len) = read_xmt(bytes, count_at + 4)?;
        let data = count_at + 4 + first_len;
        let end = data.checked_add(count.checked_mul(8)?)?;
        let raw = bytes.get(data..end)?;
        finite_f64_bytes(raw)?;
        Some((xmt, Payload { raw }, end))
    }
}

fn curve_payloads(bytes: &[u8]) -> RecordIndex<Payload<'_>> {
    let records = bytes
        .len()
        .checked_sub(14)
        .into_iter()
        .flat_map(|last| 0..last)
        .map(|pos| curve_payload_at(bytes, pos).map(|(xmt, payload, _)| (xmt, payload)));
    unique_records(records)
}

fn curve_payload_at(bytes: &[u8], pos: usize) -> Option<(u32, Payload<'_>, usize)> {
    {
        (bytes.get(pos..pos + 2) == Some(&[0, 135])).then_some(())?;
        let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
        let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
        NonNullXmt::try_from(xmt).ok()?;
        let shift = escape + xmt_len - 2;
        let count_escape = usize::from(bytes.get(pos + 9 + shift) == Some(&0xff));
        let count_at = pos + 9 + shift + count_escape;
        let count = usize::try_from(View::u32_be_at(bytes, count_at)?).ok()?;
        (count > 0).then_some(())?;
        let (_, control_ref_len) = read_xmt(bytes, count_at + 4)?;
        let data = count_at + 4 + control_ref_len;
        let end = data.checked_add(count.checked_mul(8)?)?;
        let raw = bytes.get(data..end)?;
        finite_f64_bytes(raw)?;
        Some((xmt, Payload { raw }, end))
    }
}

fn finite_f64_bytes(raw: &[u8]) -> Option<()> {
    if !raw.len().is_multiple_of(8) {
        return None;
    }
    for index in 0..raw.len() / 8 {
        if View::f64_be_at(raw, index * 8).is_none_or(|value| !value.is_finite()) {
            return None;
        }
    }
    Some(())
}

#[derive(Clone, PartialEq, Eq)]
struct SurfaceDescriptor {
    u_degree: u16,
    v_degree: u16,
    u_count: usize,
    v_count: usize,
    u_periodic: bool,
    v_periodic: bool,
    u_knot_type: u8,
    v_knot_type: u8,
    u_distinct: usize,
    v_distinct: usize,
    u_mult: u32,
    v_mult: u32,
    u_knots: u32,
    v_knots: u32,
    payload: Option<u32>,
    vertex_layout: Option<(u16, bool)>,
}

fn surface_descriptors(bytes: &[u8]) -> RecordIndex<SurfaceDescriptor> {
    let mut index: RecordIndex<SurfaceDescriptor> = RecordIndex::new();
    for pos in bytes
        .len()
        .checked_sub(47)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        let Some((xmt, descriptor, _)) = surface_descriptor_at(bytes, pos) else {
            continue;
        };

        if let Some(state) = index.get_mut(&xmt) {
            let Some(current) = state.take() else {
                continue;
            };

            *state = if current == descriptor {
                None
            } else {
                let mut left = current.clone();
                let mut right = descriptor.clone();
                left.payload = None;
                right.payload = None;
                if left == right {
                    match (current.payload, descriptor.payload) {
                        (Some(left), Some(right)) if left != right => None,
                        (Some(payload), _) | (_, Some(payload)) => Some(SurfaceDescriptor {
                            payload: Some(payload),
                            ..current
                        }),
                        (None, None) => Some(current),
                    }
                } else {
                    None
                }
            };
        } else {
            insert_if_vacant(&mut index, xmt, Some(descriptor));
        }
    }
    index
}

fn surface_descriptor_at(bytes: &[u8], pos: usize) -> Option<(u32, SurfaceDescriptor, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 126])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    NonNullXmt::try_from(xmt).ok()?;
    let shift = escape + xmt_len - 2;
    let u_periodic = logical_at(bytes, pos + surf_desc::U_PERIODIC + shift)?;
    let v_periodic = logical_at(bytes, pos + surf_desc::V_PERIODIC + shift)?;
    let u_degree = View::u16_be_at(bytes, pos + surf_desc::U_DEGREE + shift)?;
    let v_degree = View::u16_be_at(bytes, pos + surf_desc::V_DEGREE + shift)?;
    let u_count = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::U_POLE_COUNT + shift,
    )?)
    .ok()?;
    let v_count = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::V_POLE_COUNT + shift,
    )?)
    .ok()?;
    let u_knot_type = *bytes.get(pos + surf_desc::U_KNOT_TYPE + shift)?;
    let v_knot_type = *bytes.get(pos + surf_desc::V_KNOT_TYPE + shift)?;
    let u_distinct = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::U_DISTINCT_KNOT_COUNT + shift,
    )?)
    .ok()?;
    let v_distinct = usize::try_from(View::u32_be_at(
        bytes,
        pos + surf_desc::V_DISTINCT_KNOT_COUNT + shift,
    )?)
    .ok()?;
    ((u_count > 0)
        && (v_count > 0)
        && valid_knot_type(u_knot_type)
        && valid_knot_type(v_knot_type)
        && (u_distinct > 0)
        && (v_distinct > 0))
        .then_some(())?;
    let mut vertex_layout = None;
    let short = View::u16_be_at(bytes, pos + 44 + shift) == Some(125);
    let (u_mult, v_mult, u_knots, v_knots, payload, end) = if short {
        let payload_at = pos + 46 + shift;
        let (payload, payload_len) = read_enveloped_xmt(bytes, payload_at)?;
        (payload > 1).then_some(())?;
        (
            u32::from(View::u16_be_at(bytes, pos + 36 + shift)?),
            u32::from(View::u16_be_at(bytes, pos + 38 + shift)?),
            u32::from(View::u16_be_at(bytes, pos + 40 + shift)?),
            u32::from(View::u16_be_at(bytes, pos + 42 + shift)?),
            Some(payload),
            payload_at + payload_len,
        )
    } else {
        let mut at = pos + 34 + shift;
        let mut refs = [0u32; 5];
        for reference in &mut refs {
            let (value, len) = read_xmt(bytes, at)?;
            *reference = value;
            at += len;
        }
        if at == pos + 54 + shift && View::u16_be_at(bytes, at) == Some(125) {
            let payload_at = at + 2;
            let (payload, payload_len) = read_enveloped_xmt(bytes, payload_at)?;
            (payload > 1).then_some(())?;
            (
                refs[1],
                refs[2],
                refs[3],
                refs[4],
                Some(payload),
                payload_at + payload_len,
            )
        } else {
            at = pos + 34 + shift;
            for reference in &mut refs {
                let (value, len) = read_xmt(bytes, at)?;
                (value > 1).then_some(())?;
                *reference = value;
                at += len;
                (bytes.get(at) == Some(&0)).then_some(())?;
                at += 1;
            }
            vertex_layout = Some((
                View::u16_be_at(bytes, pos + surf_desc::DIMENSION + shift)?,
                logical_at(bytes, pos + surf_desc::RATIONAL + shift)?,
            ));
            (refs[1], refs[2], refs[3], refs[4], None, at)
        }
    };
    Some((
        xmt,
        SurfaceDescriptor {
            u_degree,
            v_degree,
            u_count,
            v_count,
            u_periodic,
            v_periodic,
            u_knot_type,
            v_knot_type,
            u_distinct,
            v_distinct,
            u_mult,
            v_mult,
            u_knots,
            v_knots,
            payload,
            vertex_layout,
        },
        end,
    ))
}

#[derive(Clone, PartialEq, Eq)]
struct CurveDescriptor {
    basis: CurveBasis,
    references: CurveDescriptorReferences,
}

#[derive(Clone, PartialEq, Eq)]
struct CurveBasis {
    degree: u16,
    poles: usize,
    dimension: u16,
    rational: Option<bool>,
    distinct: usize,
    knot_type: u8,
    periodic: bool,
}

fn curve_descriptors(bytes: &[u8]) -> RecordIndex<CurveDescriptor> {
    let mut index: RecordIndex<CurveDescriptor> = RecordIndex::new();
    for pos in bytes
        .len()
        .checked_sub(26)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        let Some((xmt, descriptor, _)) = curve_descriptor_at(bytes, pos, true) else {
            continue;
        };

        if let Some(state) = index.get_mut(&xmt) {
            let Some(current) = state.take() else {
                continue;
            };

            *state = if current == descriptor {
                None
            } else if current.basis == descriptor.basis
                && current.references.multiplicities() == descriptor.references.multiplicities()
                && current.references.knots() == descriptor.references.knots()
            {
                Some(current)
            } else {
                None
            };
        } else {
            insert_if_vacant(&mut index, xmt, Some(descriptor));
        }
    }
    index
}

fn curve_descriptor_at(
    bytes: &[u8],
    pos: usize,
    allow_compact_fallback: bool,
) -> Option<(u32, CurveDescriptor, usize)> {
    (bytes.get(pos..pos + 2) == Some(&[0, 136])).then_some(())?;
    let escape = usize::from(bytes.get(pos + 2) == Some(&0xff));
    let (xmt, xmt_len) = read_xmt(bytes, pos + 2 + escape)?;
    NonNullXmt::try_from(xmt).ok()?;
    let shift = escape + xmt_len - 2;
    let degree = View::u16_be_at(bytes, pos + curve_desc::DEGREE + shift)?;
    let poles = usize::try_from(View::u32_be_at(
        bytes,
        pos + curve_desc::POLE_COUNT + shift,
    )?)
    .ok()?;
    let dimension = View::u16_be_at(bytes, pos + curve_desc::DIMENSION + shift)?;
    let distinct = usize::try_from(View::u32_be_at(
        bytes,
        pos + curve_desc::DISTINCT_KNOT_COUNT + shift,
    )?)
    .ok()?;
    let knot_type = *bytes.get(pos + curve_desc::KNOT_TYPE + shift)?;
    let periodic = logical_at(bytes, pos + curve_desc::PERIODIC + shift)?;
    ((poles > 0) && matches!(dimension, 2..=4) && (distinct > 0) && valid_knot_type(knot_type))
        .then_some(())?;
    let flags = bytes.get(pos + curve_desc::PERIODIC + shift..pos + curve_desc::LEN + shift)?;
    // The primary array declares three logical flags and one byte. The form
    // byte does not restrict the following complete status-reference array.
    if matches!(flags, [0 | 1, 0 | 1, 0 | 1, _]) {
        let status_references = (|| {
            let mut at = pos + curve_desc::LEN + shift;
            let mut read_reference = || {
                let (value, consumed) = read_xmt(bytes, at)?;
                let reference = NonNullXmt::try_from(value).ok()?;
                at = at.checked_add(consumed)?;
                (bytes.get(at) == Some(&0)).then_some(())?;
                at = at.checked_add(1)?;
                Some(reference)
            };
            let references = [read_reference()?, read_reference()?, read_reference()?];
            Some((references, at))
        })();
        if let Some((references, at)) = status_references {
            return Some((
                xmt,
                CurveDescriptor {
                    basis: CurveBasis {
                        degree,
                        poles,
                        dimension,
                        rational: Some(flags[2] != 0),
                        distinct,
                        knot_type,
                        periodic,
                    },
                    references: CurveDescriptorReferences::Status(references),
                },
                at,
            ));
        }
        if matches!(flags, [0, 0, 0, 1] | [0, 0, 1, 4]) {
            allow_compact_fallback.then_some(())?;
        }
    }
    let (mult, mult_len) = read_xmt(bytes, pos + 23 + shift)?;
    let (knots, knots_len) = read_xmt(bytes, pos + 23 + shift + mult_len)?;
    Some((
        xmt,
        CurveDescriptor {
            basis: CurveBasis {
                degree,
                poles,
                dimension,
                rational: None,
                distinct,
                knot_type,
                periodic,
            },
            references: CurveDescriptorReferences::Compact([mult, knots]),
        },
        pos + 23 + shift + mult_len + knots_len,
    ))
}

fn unique_records<T>(records: impl IntoIterator<Item = Option<(u32, T)>>) -> RecordIndex<T> {
    let mut index = RecordIndex::new();
    for (xmt, record) in records.into_iter().flatten() {
        if !insert_if_vacant(&mut index, xmt, Some(record)) {
            if let Some(value) = index.get_mut(&xmt) {
                *value = None;
            }
        }
    }
    index
}

fn read_enveloped_xmt(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let escape = usize::from(bytes.get(at) == Some(&0xff));
    let (value, len) = read_xmt(bytes, at + escape)?;
    Some((value, escape + len))
}

fn logical_at(bytes: &[u8], at: usize) -> Option<bool> {
    match bytes.get(at)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn valid_knot_type(value: u8) -> bool {
    (1..=6).contains(&value)
}

#[cfg(test)]
mod tests;
