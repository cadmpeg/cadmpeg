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

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::layout::compact_analytic_header as analytic;

const EPS_BREP_UNIT_LENGTH_E9: f64 = 1.0e-9;
const EPS_BREP_ORTHONORMAL_E9: f64 = 1.0e-9;
const EPS_BREP_VALID_CARRIER_SCALARS_E9: f64 = 1.0e-9;

mod attrib;
mod blend;
pub(crate) mod entity;
mod index;
mod intersection;
mod offset;
pub(crate) mod spline;
mod subset;
mod sweep;
pub(crate) mod topology;
pub(crate) mod typed;

/// Millimetres per Parasolid model-space length unit (metres), [spec §12](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#9-units).
pub(crate) const LEN_TO_MM: f64 = 1000.0;

pub(crate) use self::graph::{decode, decode_bodies, Brep};
pub(crate) use self::spline::{patch_nurbs_curve, patch_nurbs_surface};
pub(crate) use self::topology::patch_point;

pub(crate) mod feature_source;
mod graph;

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

fn scale_point(v: &[f64]) -> Point3 {
    Point3::new(v[0] * LEN_TO_MM, v[1] * LEN_TO_MM, v[2] * LEN_TO_MM)
}

fn norm3(v: &[f64]) -> f64 {
    Vector3::from([v[0], v[1], v[2]]).norm()
}

fn unit(v: &[f64]) -> Vector3 {
    let original = Vector3::from([v[0], v[1], v[2]]);
    original.unit().unwrap_or(original)
}

// ---- compact analytic carriers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#71-compact-analytic-records)) -----------------------------------

/// Analytic surface/curve tags and the count of trailing f64 values each holds.
///
/// The partition record is `00 TT [ff]? attr:u16 ordinal:u32 refs:u16[5]
/// marker:u8(0x2b|0x2d) values:f64[n]`; deltas replace each reference with a
/// `[hi][lo][01]` triple ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#71-compact-analytic-records)). Offsets below are measured from the
/// tag byte; the optional `0xff` shifts everything after it by one.
pub(crate) mod tag {
    pub(crate) const LINE: u8 = 0x1e;
    pub(crate) const CIRCLE: u8 = 0x1f;
    pub(crate) const ELLIPSE: u8 = 0x20;
    pub(crate) const PLANE: u8 = 0x32;
    pub(crate) const CYLINDER: u8 = 0x33;
    pub(crate) const CONE: u8 = 0x34;
    pub(crate) const SPHERE: u8 = 0x35;
    pub(crate) const TORUS: u8 = 0x36;
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
                && values[7].abs() > f64::EPSILON
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
    pub(crate) attr: u16,
    pub(crate) offset: usize,
    pub(crate) end: usize,
    pub(crate) geometry: CurveGeometry,
    pub(crate) parameter_range: Option<[f64; 2]>,
}

#[derive(Debug, Clone)]
pub(crate) struct SurfaceCarrier {
    pub(crate) attr: u16,
    pub(crate) offset: usize,
    pub(crate) end: usize,
    pub(crate) geometry: SurfaceGeometry,
    pub(crate) orientation_reversed: bool,
}

impl SurfaceCarrier {
    pub(crate) fn frame(&self) -> Option<(Vector3, Vector3)> {
        match &self.geometry {
            SurfaceGeometry::Plane(plane_surface) => {
                let (_, normal, u_axis) = plane_surface.parts();
                Some((*u_axis, cross(*normal, *u_axis)))
            }
            SurfaceGeometry::Cylinder(cylinder_surface) => {
                let (_, axis, ref_direction, _) = cylinder_surface.parts();
                Some((*ref_direction, *axis))
            }
            SurfaceGeometry::Cone(cone_surface) => {
                let (_, axis, ref_direction, _, _, _) = cone_surface.parts();
                Some((*ref_direction, *axis))
            }
            SurfaceGeometry::Sphere(sphere_surface) => {
                let (_, axis, ref_direction, _) = sphere_surface.parts();
                Some((*ref_direction, *axis))
            }
            SurfaceGeometry::Torus(torus_surface) => {
                let (_, axis, ref_direction, _, _) = torus_surface.parts();
                Some((*ref_direction, *axis))
            }
            _ => None,
        }
    }
}

fn analytic_marker_candidates(body: &[u8], hdr: usize) -> Option<Vec<usize>> {
    let mut candidates = Vec::with_capacity(2);

    let partition_marker = hdr.checked_add(analytic::MARKER)?;
    if matches!(body.get(partition_marker), Some(0x2b | 0x2d)) {
        candidates.push(partition_marker);
    }

    // Deltas records encode each of the five references as [hi][lo][01].
    // The marker follows that fixed-width roster, so its position is not a
    // search result. The terminators distinguish this framing from arbitrary
    // marker-like bytes in the reference and ordinal fields.
    let refs_at = hdr.checked_add(analytic::REFS)?;
    let tripled_marker = hdr.checked_add(DELTAS_MARKER_OFFSET)?;
    let tripled_refs = (0..COMPACT_REF_COUNT).all(|index| {
        refs_at
            .checked_add(index * DELTAS_REF_STRIDE + DELTAS_REF_STRIDE - 1)
            .and_then(|at| body.get(at))
            == Some(&1)
    });
    if tripled_refs && matches!(body.get(tripled_marker), Some(0x2b | 0x2d)) {
        candidates.push(tripled_marker);
    }

    Some(candidates)
}

fn parse_carrier_at_marker(
    body: &[u8],
    off: usize,
    tt: u8,
    attr: u16,
    n: usize,
    marker_at: usize,
) -> Option<Carrier> {
    let values_at = marker_at.checked_add(1)?;
    let end = values_at.checked_add(n.checked_mul(8)?)?;
    let mut view = View::over_retained(body);
    view.seek(values_at)?;
    let vals = view.read_counted(n as u64, 8, View::f64_be)?;
    if vals.iter().any(|value| !value.is_finite()) {
        return None;
    }
    if !valid_carrier_frame(tt, &vals) || !valid_carrier_scalars(tt, &vals) {
        return None;
    }

    decode_carrier_values(tt, &vals, attr, off, end)
}

/// Try to parse a compact analytic carrier whose tag byte pair `00 TT` begins at
/// `off`. The partition and deltas framings are both considered, and a carrier
/// is returned only when exactly one framing passes all structural and geometry
/// invariants.
pub(crate) fn parse_carrier(body: &[u8], off: usize) -> Option<Carrier> {
    if body.get(off) != Some(&0x00) {
        return None;
    }
    let tt = *body.get(off.checked_add(1)?)?;
    let n = analytic_value_count(tt)?;

    // The optional 0xff after the tag shifts the fixed header by one byte.
    let tag_end = off.checked_add(2)?;
    let has_ff = body.get(tag_end) == Some(&0xff);
    let hdr = tag_end.checked_add(usize::from(has_ff))?;
    let attr = View::u16_be_at(body, hdr)?;
    let mut candidates = analytic_marker_candidates(body, hdr)?
        .into_iter()
        .filter_map(|marker_at| parse_carrier_at_marker(body, off, tt, attr, n, marker_at));
    let carrier = candidates.next()?;
    candidates.next().is_none().then_some(carrier)
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    let c = a.cross(b);
    c.unit().unwrap_or(c)
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
        tag::LINE => curve(CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(scale_point(&v[0..3]), unit(&v[3..6])).ok()?,
        )),
        tag::CIRCLE => curve(CurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                scale_point(&v[0..3]),
                unit(&v[3..6]),
                unit(&v[6..9]),
                v[9] * LEN_TO_MM,
            )
            .ok()?,
        )),
        tag::ELLIPSE => curve(CurveGeometry::Ellipse(
            cadmpeg_ir::geometry::EllipseCurve::try_new(
                scale_point(&v[0..3]),
                unit(&v[3..6]),
                unit(&v[6..9]),
                v[9] * LEN_TO_MM,
                v[10] * LEN_TO_MM,
            )
            .ok()?,
        )),
        tag::PLANE => surface(SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                scale_point(&v[0..3]),
                unit(&v[3..6]),
                unit(&v[6..9]),
            )
            .ok()?,
        )),
        tag::CYLINDER => surface(SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                scale_point(&v[0..3]),
                unit(&v[3..6]),
                unit(&v[7..10]),
                v[6] * LEN_TO_MM,
            )
            .ok()?,
        )),
        tag::CONE => {
            // origin(3) axis(3) radius sin cos refdir(3): half-angle from the
            // stored sine, which satisfies sin^2+cos^2=1 in the observed sample.
            let sin = v[7];
            return Some(surface(SurfaceGeometry::Cone(
                cadmpeg_ir::geometry::ConeSurface::try_new(
                    scale_point(&v[0..3]),
                    unit(&v[3..6]),
                    unit(&v[9..12]),
                    v[6] * LEN_TO_MM,
                    1.0,
                    sin.abs().clamp(0.0, 1.0).asin(),
                )
                .ok()?,
            )));
        }
        tag::SPHERE => surface(SurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::SphereSurface::try_new(
                scale_point(&v[0..3]),
                unit(&v[4..7]),
                unit(&v[7..10]),
                v[3] * LEN_TO_MM,
            )
            .ok()?,
        )),
        tag::TORUS => {
            return Some(surface(SurfaceGeometry::Torus(
                cadmpeg_ir::geometry::TorusSurface::try_new(
                    scale_point(&v[0..3]),
                    unit(&v[3..6]),
                    unit(&v[8..11]),
                    v[6].abs() * LEN_TO_MM,
                    v[7] * LEN_TO_MM,
                )
                .ok()?,
            )));
        }
        _ => return None,
    };
    Some(g)
}

/// Return the typed curve carried by one stream-local attribute.
pub(crate) fn curve_by_attr(body: &[u8], attr: u16) -> Option<CurveGeometry> {
    Some(scan_carriers(body).curve(attr)?.geometry.clone())
}

/// Replace the scalar run of one compact analytic carrier.
pub(crate) fn patch_compact_values(body: &mut [u8], attr: u16, values: &[f64]) -> bool {
    let carriers = scan_carriers(body);
    let Some(carrier) = carriers.curve(attr) else {
        return false;
    };
    let Some(start) = carrier.end.checked_sub(values.len() * 8) else {
        return false;
    };
    let Some(bytes) = body.get_mut(start..carrier.end) else {
        return false;
    };
    for (slot, value) in bytes.chunks_exact_mut(8).zip(values) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    true
}

/// Patch one stream-local NURBS curve without changing its storage shape.
pub(crate) fn patch_nurbs_by_attr(
    body: &mut [u8],
    attr: u16,
    new: &cadmpeg_ir::geometry::NurbsCurve,
) -> bool {
    let carriers = scan_carriers(body);
    let Some(carrier) = carriers.curve(attr) else {
        return false;
    };
    let CurveGeometry::Nurbs(old) = &carrier.geometry else {
        return false;
    };
    patch_nurbs_curve(body, carrier.offset, old, new, 0.001).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let Carrier::Surface(carrier) = parse_carrier(&bytes, 0).unwrap() else {
                panic!("expected surface carrier");
            };
            assert_eq!(carrier.frame(), Some((reference, expected_v)));
        }
    }

    #[test]
    fn scan_does_not_skip_overlapping_carrier_starts() {
        let mut bytes = compact_carrier(tag::LINE, 7, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        bytes.truncate(60);
        bytes.extend(compact_carrier(
            tag::LINE,
            8,
            &[1.0, 2.0, 3.0, 0.0, 0.0, 1.0],
        ));

        let carriers = scan_carriers(&bytes);

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
                parse_carrier(&bytes, 0).expect("tripled compact carrier")
            else {
                panic!("expected curve carrier");
            };
            assert_eq!(carrier.attr, 7);
            assert_eq!(carrier.end, bytes.len());
            let CurveGeometry::Line(line_curve) = carrier.geometry else {
                panic!("expected line");
            };
            let (&origin, &direction) = line_curve.parts();
            assert_eq!(origin, Point3::new(1_000_000_000_000.0, 0.0, 0.0));
            assert_eq!(direction, Vector3::new(1.0, 0.0, 0.0));
        }
    }

    #[test]
    fn merge_retains_zero_offset_blend_support_pairs() {
        let mut base = index::CarrierIndex::default();
        let mut delta = index::CarrierIndex::default();
        delta.insert_blend_support_pair(
            9,
            blend::SupportPairCarrier {
                supports: [11, 12],
                intersection: 13,
            },
        );

        base.merge_missing(delta);

        let pair = base.blend_support_pair(9).expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
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
        let Carrier::Surface(carrier) = parse_carrier(&bytes, 0).expect("required invariant")
        else {
            panic!("expected surface carrier");
        };
        let SurfaceGeometry::Cone(cone_surface) = carrier.geometry else {
            panic!("expected cone");
        };
        let (&origin, &axis, &ref_direction, &radius, &ratio, &half_angle) = cone_surface.parts();
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
        let Carrier::Surface(carrier) = parse_carrier(&bytes, 0).expect("required invariant")
        else {
            panic!("expected surface carrier");
        };
        let SurfaceGeometry::Torus(torus_surface) = carrier.geometry else {
            panic!("expected torus");
        };
        let (&center, &axis, &ref_direction, &major_radius, &minor_radius) = torus_surface.parts();
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

        assert!(parse_carrier(&bytes, 0).is_none());
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

        assert!(parse_carrier(&ellipse, 0).is_none());
        assert!(parse_carrier(&torus, 0).is_none());
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
        let Carrier::Surface(carrier) = parse_carrier(&bytes, 0).expect("spindle torus") else {
            panic!("expected surface carrier");
        };
        let SurfaceGeometry::Torus(torus_surface) = carrier.geometry else {
            panic!("expected torus");
        };
        let (_, _, _, &major_radius, &minor_radius) = torus_surface.parts();
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
        let Carrier::Surface(carrier) = parse_carrier(&bytes, 0).expect("signed-major torus")
        else {
            panic!("expected surface carrier");
        };
        let SurfaceGeometry::Torus(torus_surface) = carrier.geometry else {
            panic!("expected torus");
        };
        let (_, _, _, &major_radius, &minor_radius) = torus_surface.parts();
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
                parse_carrier(&bytes, 0).is_none(),
                "accepted tag {tag:#04x}"
            );
        }
    }

    #[test]
    fn rejects_invalid_cone_angle_pair() {
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.5, 0.5, 1.0, 0.0, 0.0],
        );

        assert!(parse_carrier(&bytes, 0).is_none());
    }
}
