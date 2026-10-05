// SPDX-License-Identifier: Apache-2.0
//! Parasolid B-rep record decoding.
//!
//! This module resolves topology and geometry carriers by stream-local
//! attribute id. [`decode`] handles one stream; [`decode_bodies`] combines
//! related partition and deltas streams before building the graph.
//!
//! The decoded chain connects face bridges to support surfaces and loop heads,
//! coedges to edge uses and curves, and vertex uses to world points. Supported
//! carriers include lines, circles, ellipses, planes, cylinders, cones, spheres,
//! tori, NURBS curves and surfaces, and recursive offset surfaces. The decoder
//! converts model-space metres to millimetres and leaves dimensionless vectors
//! and ratios unchanged.
//!
//! [`Brep::stats`] counts carriers and grouping that could not be transferred
//! directly. Untyped carriers use opaque IR geometry while resolvable topology
//! remains available.

use self::index::scan_carriers;
use self::spline::patch_nurbs_curve;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::layout::compact_analytic_header as analytic;

const EPS_BREP_UNIT_LENGTH_E9: f64 = 1.0e-9;
const EPS_BREP_ORTHONORMAL_E9: f64 = 1.0e-9;
const EPS_BREP_VALID_CARRIER_SCALARS_E9: f64 = 1.0e-9;

mod attrib;
mod blend;
pub(crate) mod entity;
pub(crate) mod evaluation;
mod index;
mod intersection;
mod offset;
pub(crate) mod spline;
mod subset;
mod sweep;
pub(crate) mod topology;
mod typed;

/// Millimetres per Parasolid model-space length unit (metres), [spec §12](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#9-units).
const LEN_TO_MM: f64 = 1000.0;

pub(crate) mod feature_source;
pub(crate) mod graph;

/// The native persistent identity shared by B-rep face attributes and display
/// tessellation references.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PersistentFaceIdentity {
    /// Native history-feature object identifier.
    pub(crate) feature_source_id: feature_source::FeatureSourceId,
    /// Face identity local to the producing feature.
    pub(crate) local_id: u32,
    /// Optional signed path fields stored as their native u32 bit patterns.
    pub(crate) trailing_fields: Vec<u32>,
}

impl cadmpeg_core::decode::cost::DecodeCost for PersistentFaceIdentity {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(self.feature_source_id, self.local_id, &self.trailing_fields),
            ctx,
            operation,
        )
    }
}

fn scale_point(v: &[f64]) -> Point3 {
    Point3::new(v[0] * LEN_TO_MM, v[1] * LEN_TO_MM, v[2] * LEN_TO_MM)
}

fn norm3(v: &[f64]) -> f64 {
    Vector3::from([v[0], v[1], v[2]]).norm()
}

/// The unit direction of a stored vector, absent when the vector is not
/// finite or its length is within `f64::EPSILON` of zero.
fn direction(v: &[f64]) -> Option<UnitVector3> {
    UnitVector3::normalized(Vector3::from([v[0], v[1], v[2]]))
}

/// The frame of two stored directions, absent when either direction is
/// degenerate or the two are not perpendicular.
fn frame(axis: &[f64], reference: &[f64]) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::from_units(direction(axis)?, direction(reference)?)
}

// ---- compact analytic carriers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#71-compact-analytic-records)) -----------------------------------

/// Analytic surface/curve tags and the count of trailing f64 values each holds.
///
/// The partition record is `00 TT [ff]? attr:u16 ordinal:u32 refs:u16[5]
/// marker:u8(0x2b|0x2d) values:f64[n]`; deltas replace each reference with a
/// `[hi][lo][01]` triple ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#71-compact-analytic-records)). Offsets below are measured from the
/// tag byte; the optional `0xff` shifts everything after it by one.
mod tag {
    pub(super) const LINE: u8 = 0x1e;
    pub(super) const CIRCLE: u8 = 0x1f;
    pub(super) const ELLIPSE: u8 = 0x20;
    pub(super) const PLANE: u8 = 0x32;
    pub(super) const CYLINDER: u8 = 0x33;
    pub(super) const CONE: u8 = 0x34;
    pub(super) const SPHERE: u8 = 0x35;
    pub(super) const TORUS: u8 = 0x36;
}

const COMPACT_REF_COUNT: usize = 5;
const DELTAS_REF_STRIDE: usize = 3;
const DELTAS_MARKER_OFFSET: usize = analytic::REFS + COMPACT_REF_COUNT * DELTAS_REF_STRIDE;

/// f64 count for each analytic tag; `None` if the tag is not an analytic carrier.
fn analytic_value_count(tt: u8) -> Option<usize> {
    Some(match tt {
        tag::LINE => 6,
        tag::CIRCLE => 10,
        tag::ELLIPSE => 11,
        tag::PLANE => 9,
        tag::CYLINDER => 10,
        tag::CONE => 12,
        tag::SPHERE => 10,
        tag::TORUS => 11,
        _ => return None,
    })
}

fn unit_length(values: &[f64]) -> bool {
    (norm3(values) - 1.0).abs() <= EPS_BREP_UNIT_LENGTH_E9
}

fn orthonormal(left: &[f64], right: &[f64]) -> bool {
    unit_length(left)
        && unit_length(right)
        && (left[0] * right[0] + left[1] * right[1] + left[2] * right[2]).abs()
            <= EPS_BREP_ORTHONORMAL_E9
}

fn valid_carrier_frame(tt: u8, values: &[f64]) -> bool {
    match tt {
        tag::LINE => unit_length(&values[3..6]),
        tag::CIRCLE | tag::ELLIPSE | tag::PLANE => orthonormal(&values[3..6], &values[6..9]),
        tag::CYLINDER => orthonormal(&values[3..6], &values[7..10]),
        tag::CONE => orthonormal(&values[3..6], &values[9..12]),
        tag::SPHERE => orthonormal(&values[4..7], &values[7..10]),
        tag::TORUS => orthonormal(&values[3..6], &values[8..11]),
        _ => false,
    }
}

fn valid_carrier_scalars(tt: u8, values: &[f64]) -> bool {
    match tt {
        tag::LINE | tag::PLANE => true,
        tag::CIRCLE => values[9] > 0.0,
        tag::ELLIPSE => values[9] >= values[10] && values[10] > 0.0,
        tag::CYLINDER => values[6] > 0.0,
        tag::CONE => {
            values[6] >= 0.0
                && values[7] > f64::EPSILON
                && values[7] < 1.0
                && values[8] > 0.0
                && (values[7] * values[7] + values[8] * values[8] - 1.0).abs()
                    <= EPS_BREP_VALID_CARRIER_SCALARS_E9
        }
        tag::SPHERE => values[3] > 0.0,
        tag::TORUS => values[6].abs() > f64::EPSILON && values[7] > 0.0,
        _ => false,
    }
}

/// A parsed analytic carrier selected by its geometry family.
#[derive(Debug, Clone)]
pub(crate) enum Carrier {
    Curve(CurveCarrier),
    Surface(SurfaceCarrier),
}

#[derive(Debug, Clone)]
pub(crate) struct CurveCarrier {
    attr: u16,
    offset: usize,
    pub(crate) end: usize,
    pub(crate) geometry: CurveGeometry,
    parameter_range: Option<cadmpeg_ir::units::FiniteVector<2>>,
}

#[derive(Debug, Clone)]
pub(crate) struct SurfaceCarrier {
    attr: u16,
    offset: usize,
    pub(crate) end: usize,
    pub(crate) geometry: SurfaceGeometry,
    orientation_reversed: bool,
}

fn analytic_marker_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: &[u8],
    hdr: usize,
) -> Result<Option<Vec<usize>>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::with_capacity(2);

    let partition_marker = match hdr.checked_add(analytic::MARKER) {
        Some(value) => value,
        None => return Ok::<_, cadmpeg_core::CodecError>(None),
    };
    if matches!(body.get(partition_marker), Some(0x2b | 0x2d)) {
        ctx.push_vec(
            &mut (candidates),
            partition_marker,
            "collect SLDPRT decoded vector items",
        )?;
    }

    // Deltas records encode each of the five references as [hi][lo][01].
    // The marker follows that fixed-width roster, so its position is not a
    // search result. The terminators distinguish this framing from arbitrary
    // marker-like bytes in the reference and ordinal fields.
    let refs_at = match hdr.checked_add(analytic::REFS) {
        Some(value) => value,
        None => return Ok::<_, cadmpeg_core::CodecError>(None),
    };
    let tripled_marker = match hdr.checked_add(DELTAS_MARKER_OFFSET) {
        Some(value) => value,
        None => return Ok::<_, cadmpeg_core::CodecError>(None),
    };
    let tripled_refs = (0..COMPACT_REF_COUNT).all(|index| {
        refs_at
            .checked_add(index * DELTAS_REF_STRIDE + DELTAS_REF_STRIDE - 1)
            .and_then(|at| body.get(at))
            == Some(&1)
    });
    if tripled_refs && matches!(body.get(tripled_marker), Some(0x2b | 0x2d)) {
        ctx.push_vec(
            &mut (candidates),
            tripled_marker,
            "collect SLDPRT decoded vector items",
        )?;
    }

    Ok::<_, cadmpeg_core::CodecError>(Some(candidates))
}

fn parse_carrier_at_marker(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: &[u8],
    off: usize,
    tt: u8,
    attr: u16,
    n: usize,
    marker_at: usize,
) -> Result<Option<Carrier>, cadmpeg_core::CodecError> {
    let values_at = match marker_at.checked_add(1) {
        Some(value) => value,
        None => return Ok(None),
    };
    let end = match values_at.checked_add(match n.checked_mul(8) {
        Some(value) => value,
        None => return Ok(None),
    }) {
        Some(value) => value,
        None => return Ok(None),
    };
    let mut view = View::over_retained(body);
    match view.seek(values_at) {
        Some(value) => value,
        None => return Ok(None),
    };
    let mut storage = [0_f64; 12];
    let vals = match storage.get_mut(..n) {
        Some(value) => value,
        None => return Ok(None),
    };
    for value in vals.iter_mut() {
        *value = match view.f64_be() {
            Some(value) => value,
            None => return Ok(None),
        };
    }
    if ctx
        .admit_iter(&*vals, "scan SLDPRT analytic carrier scalar finiteness")?
        .any(|value| !value.is_finite())
    {
        return Ok(None);
    }
    if !valid_carrier_frame(tt, vals) || !valid_carrier_scalars(tt, vals) {
        return Ok(None);
    }

    Ok(decode_carrier_values(tt, vals, attr, off, end))
}

/// Try to parse a compact analytic carrier whose tag byte pair `00 TT` begins at
/// `off`. The partition and deltas framings are both considered, and a carrier
/// is returned only when exactly one framing passes all structural and geometry
/// invariants.
pub(crate) fn parse_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: &[u8],
    off: usize,
) -> Result<Option<Carrier>, cadmpeg_core::CodecError> {
    if body.get(off) != Some(&0x00) {
        return Ok(None);
    }
    let tt = *match body.get(match off.checked_add(1) {
        Some(value) => value,
        None => return Ok(None),
    }) {
        Some(value) => value,
        None => return Ok(None),
    };
    let n = match analytic_value_count(tt) {
        Some(value) => value,
        None => return Ok(None),
    };

    // The optional 0xff after the tag shifts the fixed header by one byte.
    let tag_end = match off.checked_add(2) {
        Some(value) => value,
        None => return Ok(None),
    };
    let has_ff = body.get(tag_end) == Some(&0xff);
    let hdr = match tag_end.checked_add(usize::from(has_ff)) {
        Some(value) => value,
        None => return Ok(None),
    };
    let attr = match View::u16_be_at(body, hdr) {
        Some(value) => value,
        None => return Ok(None),
    };
    let mut candidates = match analytic_marker_candidates(ctx, body, hdr)? {
        Some(value) => value,
        None => return Ok(None),
    }
    .into_iter()
    .filter_map(|marker_at| {
        parse_carrier_at_marker(ctx, body, off, tt, attr, n, marker_at).transpose()
    });
    let Some(carrier) = candidates.next().transpose()? else {
        return Ok(None);
    };
    Ok(candidates.next().transpose()?.is_none().then_some(carrier))
}

/// Map a tag's decoded f64 run to IR geometry, applying the ×1000 length rule to
/// coordinates and radii only.
fn decode_carrier_values(
    tt: u8,
    v: &[f64],
    attr: u16,
    offset: usize,
    end: usize,
) -> Option<Carrier> {
    let curve = |geometry| {
        Carrier::Curve(CurveCarrier {
            attr,
            offset,
            end,
            geometry,
            parameter_range: None,
        })
    };
    let surface = |geometry| {
        Carrier::Surface(SurfaceCarrier {
            attr,
            offset,
            end,
            geometry,
            orientation_reversed: tt == tag::TORUS && v[6].is_sign_negative(),
        })
    };
    let g = match tt {
        tag::LINE => curve(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::new(
                FinitePoint3::new(scale_point(&v[0..3]))?,
                direction(&v[3..6])?,
            ),
        ))),
        tag::CIRCLE => curve(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::new(
                FinitePoint3::new(scale_point(&v[0..3]))?,
                frame(&v[3..6], &v[6..9])?,
                PositiveLength::new(v[9] * LEN_TO_MM)?,
            ),
        ))),
        tag::ELLIPSE => curve(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
            EllipseCurve::try_from_parts(
                FinitePoint3::new(scale_point(&v[0..3]))?,
                frame(&v[3..6], &v[6..9])?,
                PositiveLength::new(v[9] * LEN_TO_MM)?,
                PositiveLength::new(v[10] * LEN_TO_MM)?,
            )
            .ok()?,
        ))),
        tag::PLANE => surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::new(
                FinitePoint3::new(scale_point(&v[0..3]))?,
                frame(&v[3..6], &v[6..9])?,
            ),
        ))),
        tag::CYLINDER => surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::new(
                FinitePoint3::new(scale_point(&v[0..3]))?,
                frame(&v[3..6], &v[7..10])?,
                PositiveLength::new(v[6] * LEN_TO_MM)?,
            ),
        ))),
        tag::CONE => {
            // origin(3) axis(3) radius sin cos refdir(3). Admission requires
            // a positive sine below one and a positive cosine.
            let half_angle = v[7].asin();
            return Some(surface(SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cone(ConeSurface::new(
                    FinitePoint3::new(scale_point(&v[0..3]))?,
                    frame(&v[3..6], &v[9..12])?,
                    NonNegativeLength::new(v[6] * LEN_TO_MM)?,
                    PositiveReal::ONE,
                    Angle::new(half_angle)?,
                )),
            )));
        }
        tag::SPHERE => surface(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::new(
                FinitePoint3::new(scale_point(&v[0..3]))?,
                frame(&v[4..7], &v[7..10])?,
                NonZeroLength::new(v[3] * LEN_TO_MM)?,
            ),
        ))),
        tag::TORUS => {
            return Some(surface(SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Torus(TorusSurface::new(
                    FinitePoint3::new(scale_point(&v[0..3]))?,
                    frame(&v[3..6], &v[8..11])?,
                    PositiveLength::new(v[6].abs() * LEN_TO_MM)?,
                    NonZeroLength::new(v[7] * LEN_TO_MM)?,
                )),
            )));
        }
        _ => return None,
    };
    Some(g)
}

/// Return the typed curve carried by one stream-local attribute.
pub(crate) fn curve_by_attr(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    attr: u16,
) -> Result<Option<CurveGeometry>, cadmpeg_core::CodecError> {
    let carriers = scan_carriers(ctx, body)?;
    carriers
        .curve(attr)
        .map(|indexed| {
            indexed
                .carrier()
                .geometry
                .try_clone_for_decode(ctx, "SLDPRT patch curve geometry copy")
        })
        .transpose()
}

/// Replace the scalar run of one compact analytic carrier.
pub(crate) fn patch_compact_values(
    ctx: &DecodeContext<'_>,
    body: &mut [u8],
    attr: u16,
    values: &[f64],
) -> Result<bool, cadmpeg_core::CodecError> {
    let carriers = scan_carriers(ctx, body)?;
    let Some(indexed) = carriers.curve(attr) else {
        return Ok(false);
    };
    let carrier = indexed.carrier();
    let Some(size) = values.len().checked_mul(8) else {
        return Ok(false);
    };
    let Some(start) = carrier.end.checked_sub(size) else {
        return Ok(false);
    };
    let Some(bytes) = body.get_mut(start..carrier.end) else {
        return Ok(false);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(size),
        "patch SLDPRT compact values",
    )?;
    for (slot, value) in bytes.chunks_exact_mut(8).zip(values) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    Ok(true)
}

/// Patch one stream-local NURBS curve without changing its storage shape.
pub(crate) fn patch_nurbs_by_attr(
    ctx: &DecodeContext<'_>,
    body: &mut [u8],
    attr: u16,
    new: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<bool, cadmpeg_core::CodecError> {
    let carriers = scan_carriers(ctx, body)?;
    let Some(indexed) = carriers.curve(attr) else {
        return Ok(false);
    };
    let carrier = indexed.carrier();
    let Some(SolvedCurveGeometry::Nurbs(old)) = carrier.geometry.solved() else {
        return Ok(false);
    };
    Ok(patch_nurbs_curve(ctx, body, carrier.offset, old, new, 0.001)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::index::scan_carriers;
    use super::{parse_carrier, tag, Carrier};
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::math::Vector3;

    fn compact_carrier(tag: u8, attr: u16, values: &[f64]) -> Vec<u8> {
        let mut bytes = vec![0, tag];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&[0; 14]);
        bytes.push(0x2b);
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn tripled_compact_carrier(
        tag: u8,
        attr: u16,
        refs: [u16; 5],
        values: &[f64],
        has_ff: bool,
    ) -> Vec<u8> {
        let mut bytes = vec![0, tag];
        if has_ff {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&[0; 4]);
        for reference in refs {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(0x2b);
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn analytic_surface_frames_follow_the_source_axis_order() {
        let reference = Vector3::new(0.0, 1.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        for (kind, values, expected_v) in [
            (
                tag::PLANE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
                Vector3::new(-1.0, 0.0, 0.0),
            ),
            (
                tag::CYLINDER,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.0, 1.0, 0.0],
                axis,
            ),
            (
                tag::SPHERE,
                vec![0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
                axis,
            ),
            (
                tag::TORUS,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, -0.002, 0.001, 0.0, 1.0, 0.0],
                axis,
            ),
        ] {
            let bytes = compact_carrier(kind, 7, &values);
            let Carrier::Surface(carrier) =
                parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                    .unwrap()
                    .unwrap()
            else {
                panic!("expected surface carrier");
            };
            let frame = match carrier.geometry.solved() {
                Some(SolvedSurfaceGeometry::Plane(plane)) => {
                    let (normal, u_axis) = (plane.frame().axis(), plane.frame().reference());
                    Some((*u_axis.as_raw(), normal.as_raw().cross(*u_axis.as_raw())))
                }
                Some(SolvedSurfaceGeometry::Cylinder(cylinder)) => Some((
                    *cylinder.frame().reference().as_raw(),
                    *cylinder.frame().axis().as_raw(),
                )),
                Some(SolvedSurfaceGeometry::Sphere(sphere)) => Some((
                    *sphere.frame().reference().as_raw(),
                    *sphere.frame().axis().as_raw(),
                )),
                Some(SolvedSurfaceGeometry::Torus(torus)) => Some((
                    *torus.frame().reference().as_raw(),
                    *torus.frame().axis().as_raw(),
                )),
                _ => None,
            };
            assert_eq!(frame, Some((reference, expected_v)));
        }
    }

    #[test]
    fn scan_does_not_skip_overlapping_carrier_starts() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = compact_carrier(tag::LINE, 7, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        bytes.truncate(60);
        bytes.extend(compact_carrier(
            tag::LINE,
            8,
            &[1.0, 2.0, 3.0, 0.0, 0.0, 1.0],
        ));

        let carriers = scan_carriers(&ctx, &bytes).expect("carrier scan");

        assert!(carriers.curve(7).is_some());
        assert!(carriers.curve(8).is_some());
    }

    #[test]
    fn parses_tripled_compact_carrier_at_structural_marker() {
        for has_ff in [false, true] {
            let bytes = tripled_compact_carrier(
                tag::LINE,
                7,
                [1, 0x2b00, 2, 3, 4],
                &[1_000_000_000.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                has_ff,
            );

            let Carrier::Curve(carrier) =
                parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                    .unwrap()
                    .expect("tripled compact carrier")
            else {
                panic!("expected curve carrier");
            };
            assert_eq!(carrier.attr, 7);
            assert_eq!(carrier.end, bytes.len());
            let Some(SolvedCurveGeometry::Line(line_curve)) = carrier.geometry.solved() else {
                panic!("expected line");
            };
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            assert_eq!(origin, Point3::new(1_000_000_000_000.0, 0.0, 0.0));
            assert_eq!(direction, Vector3::new(1.0, 0.0, 0.0));
        }
    }

    #[test]
    fn parses_verified_cone_layout() {
        let root_half = std::f64::consts::FRAC_1_SQRT_2;
        let bytes = compact_carrier(
            tag::CONE,
            7,
            &[
                0.0, 0.0, 0.0067, 0.0, 0.0, -1.0, 0.0015, root_half, root_half, -1.0, 0.0, 0.0,
            ],
        );
        let Carrier::Surface(carrier) =
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .expect("required invariant")
        else {
            panic!("expected surface carrier");
        };
        let Some(SolvedSurfaceGeometry::Cone(cone_surface)) = carrier.geometry.solved() else {
            panic!("expected cone");
        };
        let origin = cone_surface.origin().get();
        let axis = *cone_surface.frame().axis().as_raw();
        let ref_direction = *cone_surface.frame().reference().as_raw();
        let radius = cone_surface.radius().get();
        let ratio = cone_surface.ratio().get();
        let half_angle = cone_surface.half_angle().get();
        assert_eq!(origin, Point3::new(0.0, 0.0, 6.7));
        assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(ref_direction, Vector3::new(-1.0, 0.0, 0.0));
        assert!((radius - 1.5).abs() < 1.0e-12);
        assert_eq!(ratio, 1.0);
        assert!((half_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12);
    }

    #[test]
    fn parses_verified_torus_layout() {
        let bytes = compact_carrier(
            tag::TORUS,
            8,
            &[
                0.0, 0.0, 0.0002, 0.0, 0.0, -1.0, 0.0022, 0.0002, -1.0, 0.0, 0.0,
            ],
        );
        let Carrier::Surface(carrier) =
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .expect("required invariant")
        else {
            panic!("expected surface carrier");
        };
        let Some(SolvedSurfaceGeometry::Torus(torus_surface)) = carrier.geometry.solved() else {
            panic!("expected torus");
        };
        let center = torus_surface.center().get();
        let axis = *torus_surface.frame().axis().as_raw();
        let ref_direction = *torus_surface.frame().reference().as_raw();
        let major_radius = torus_surface.major_radius().get();
        let minor_radius = torus_surface.minor_radius().get();
        assert_eq!(center, Point3::new(0.0, 0.0, 0.2));
        assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(ref_direction, Vector3::new(-1.0, 0.0, 0.0));
        assert!((major_radius - 2.2).abs() < 1.0e-12);
        assert!((minor_radius - 0.2).abs() < 1.0e-12);
        assert!(!carrier.orientation_reversed);
    }

    #[test]
    fn rejects_nonorthogonal_analytic_frame() {
        let bytes = compact_carrier(
            tag::CIRCLE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.002],
        );

        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_invalid_analytic_radii() {
        let ellipse = compact_carrier(
            tag::ELLIPSE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.001, 0.002],
        );
        let torus = compact_carrier(
            tag::TORUS,
            9,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.0, 1.0, 0.0, 0.0],
        );

        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &ellipse, 0)
                .unwrap()
                .is_none()
        );
        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &torus, 0)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn parses_spindle_torus_with_minor_radius_over_major() {
        let bytes = compact_carrier(
            tag::TORUS,
            8,
            &[
                0.0, 0.0, 0.0002, 0.0, 0.0, -1.0, 0.0022, 0.0044, -1.0, 0.0, 0.0,
            ],
        );
        let Carrier::Surface(carrier) =
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .expect("spindle torus")
        else {
            panic!("expected surface carrier");
        };
        let Some(SolvedSurfaceGeometry::Torus(torus_surface)) = carrier.geometry.solved() else {
            panic!("expected torus");
        };
        let major_radius = torus_surface.major_radius().get();
        let minor_radius = torus_surface.minor_radius().get();
        assert!((major_radius - 2.2).abs() < 1.0e-12);
        assert!((minor_radius - 4.4).abs() < 1.0e-12);
    }

    #[test]
    fn normalizes_negative_torus_major_radius_and_marks_orientation_reversal() {
        let bytes = compact_carrier(
            tag::TORUS,
            8,
            &[
                0.0, 0.0, 0.0002, 0.0, 0.0, -1.0, -0.0022, 0.0044, -1.0, 0.0, 0.0,
            ],
        );
        let Carrier::Surface(carrier) =
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .expect("signed-major torus")
        else {
            panic!("expected surface carrier");
        };
        let Some(SolvedSurfaceGeometry::Torus(torus_surface)) = carrier.geometry.solved() else {
            panic!("expected torus");
        };
        let major_radius = torus_surface.major_radius().get();
        let minor_radius = torus_surface.minor_radius().get();
        assert!((major_radius - 2.2).abs() < 1.0e-12);
        assert!((minor_radius - 4.4).abs() < 1.0e-12);
        assert!(carrier.orientation_reversed);
    }

    #[test]
    fn rejects_nonunit_analytic_frames() {
        let cases = [
            (tag::LINE, vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0]),
            (
                tag::CIRCLE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 0.002],
            ),
            (
                tag::ELLIPSE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 0.002, 0.001],
            ),
            (
                tag::PLANE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0],
            ),
            (
                tag::CYLINDER,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.002, 2.0, 0.0, 0.0],
            ),
            (
                tag::CONE,
                vec![
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.001,
                    std::f64::consts::FRAC_1_SQRT_2,
                    std::f64::consts::FRAC_1_SQRT_2,
                    2.0,
                    0.0,
                    0.0,
                ],
            ),
            (
                tag::SPHERE,
                vec![0.0, 0.0, 0.0, 0.002, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0],
            ),
            (
                tag::TORUS,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.002, 0.001, 1.0, 0.0, 0.0],
            ),
        ];

        for (tag, values) in cases {
            let bytes = compact_carrier(tag, 9, &values);
            assert!(
                parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                    .unwrap()
                    .is_none(),
                "accepted tag {tag:#04x}"
            );
        }
    }

    /// A stored cone sine outside the unit interval is not a CONE record: the
    /// unit identity in `valid_carrier_scalars` refuses it, so it never reaches
    /// `asin`.
    #[test]
    fn a_cone_sine_outside_the_unit_interval_is_not_a_cone_record() {
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 2.0, 1.0, 1.0, 0.0, 0.0],
        );

        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .is_none()
        );
    }

    /// A stored cone sine inside the unit interval is its own arcsine.
    #[test]
    fn a_cone_sine_inside_the_unit_interval_is_its_own_arcsine() {
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.6, 0.8, 1.0, 0.0, 0.0],
        );
        let Carrier::Surface(carrier) =
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .expect("required invariant")
        else {
            panic!("expected surface carrier");
        };
        let Some(SolvedSurfaceGeometry::Cone(cone_surface)) = carrier.geometry.solved() else {
            panic!("expected cone");
        };

        assert_eq!(cone_surface.half_angle().get(), 0.6_f64.asin());
    }

    #[test]
    fn a_cone_sine_just_above_one_is_refused() {
        let sine = 1.0 + 4.0e-10;
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[
                0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, sine, 1.0e-8, 1.0, 0.0, 0.0,
            ],
        );
        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_negative_cone_sine_is_refused_without_changing_its_chart() {
        let root_half = std::f64::consts::FRAC_1_SQRT_2;
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[
                0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, -root_half, root_half, 1.0, 0.0, 0.0,
            ],
        );
        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_invalid_cone_angle_pair() {
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.5, 0.5, 1.0, 0.0, 0.0],
        );

        assert!(
            parse_carrier(&cadmpeg_test_support::service_decode_context(), &bytes, 0)
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(test)]
mod patch_tests;
