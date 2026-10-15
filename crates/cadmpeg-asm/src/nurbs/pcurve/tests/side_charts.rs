// SPDX-License-Identifier: Apache-2.0
//! Each case builds a procedural construction with an analytic support and a
//! linear side pcurve in the support's native chart, decodes it, and requires
//! the decoded support evaluated at the decoded pcurve poles to reach the
//! points the native chart defines ([spec §6.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#62-analytic-surface-byte-layouts)).

use crate::nurbs::toks::{Cur, SubtypeTable};
use crate::sab::Token;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

/// Millimetre distance within which a decoded contact reaches its point.
const EPS_CONTACT_MM: f64 = 1.0e-9;
const CM_TO_MM: f64 = 10.0;

fn sense(reversed: bool) -> Token {
    if reversed {
        Token::True
    } else {
        Token::False
    }
}

fn unit(v: [f64; 3]) -> Vector3 {
    Vector3::from(v).unit().expect("nonzero direction")
}

fn mm(point: Vector3) -> Point3 {
    Point3::new(point.x * CM_TO_MM, point.y * CM_TO_MM, point.z * CM_TO_MM)
}

/// An embedded analytic support in native centimetre fields.
#[derive(Clone, Copy)]
enum Support {
    Plane {
        origin: [f64; 3],
        normal: [f64; 3],
        u_dir: [f64; 3],
        reverse_v: bool,
    },
    Cone {
        origin: [f64; 3],
        axis: [f64; 3],
        major: [f64; 3],
        sine: f64,
        cosine: f64,
        u_scale: f64,
        reverse_u: bool,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
        reference: [f64; 3],
        pole: [f64; 3],
        reverse_v: bool,
    },
}

impl Support {
    /// The embedded support tokens. A cone or sphere stores four absent
    /// bounds; a plane stores them when `plane_bounds` is set.
    fn tokens(self, plane_bounds: bool) -> Vec<Token> {
        let mut out = Vec::new();
        let bounds = match self {
            Self::Plane {
                origin,
                normal,
                u_dir,
                reverse_v,
            } => {
                out.extend([
                    Token::Ident("plane".into()),
                    Token::Position(origin),
                    Token::Vector3(normal),
                    Token::Vector3(u_dir),
                    sense(reverse_v),
                ]);
                plane_bounds
            }
            Self::Cone {
                origin,
                axis,
                major,
                sine,
                cosine,
                u_scale,
                reverse_u,
            } => {
                out.extend([
                    Token::Ident("cone".into()),
                    Token::Position(origin),
                    Token::Vector3(axis),
                    Token::Vector3(major),
                    Token::Double(1.0),
                    Token::False,
                    Token::False,
                    Token::Double(sine),
                    Token::Double(cosine),
                    Token::Double(u_scale),
                    sense(reverse_u),
                ]);
                true
            }
            Self::Sphere {
                center,
                radius,
                reference,
                pole,
                reverse_v,
            } => {
                out.extend([
                    Token::Ident("sphere".into()),
                    Token::Position(center),
                    Token::Double(radius),
                    Token::Vector3(reference),
                    Token::Vector3(pole),
                    sense(reverse_v),
                ]);
                true
            }
        };
        if bounds {
            out.extend(std::iter::repeat_n(Token::False, 4));
        }
        out
    }

    /// The native point at chart coordinates `(first, second)`, in
    /// millimetres.
    fn native_point(self, native: Point2) -> Point3 {
        match self {
            Self::Plane {
                origin,
                normal,
                u_dir,
                reverse_v,
            } => {
                let (n, u) = (unit(normal), unit(u_dir));
                let v = n.cross(u).scale(if reverse_v { -1.0 } else { 1.0 });
                mm(Vector3::from(origin) + u.scale(native.u) + v.scale(native.v))
            }
            Self::Cone {
                origin,
                axis,
                major,
                sine,
                cosine,
                u_scale,
                reverse_u,
            } => {
                let (z, x) = (unit(axis), unit(major));
                let y = z.cross(x);
                let sign = if reverse_u { -1.0 } else { 1.0 };
                let d = sign * native.u * cosine.abs() * u_scale;
                let rho = Vector3::from(major).norm() + d * sine / cosine;
                let theta = native.v;
                mm(Vector3::from(origin)
                    + x.scale(rho * theta.cos())
                    + y.scale(rho * theta.sin())
                    + z.scale(d))
            }
            Self::Sphere {
                center,
                radius,
                reference,
                pole,
                reverse_v,
            } => {
                let (z, x) = (unit(pole), unit(reference));
                let y = z.cross(x);
                let (latitude, longitude) =
                    (native.u, native.v * if reverse_v { -1.0 } else { 1.0 });
                mm(Vector3::from(center)
                    + (x.scale(longitude.cos()) + y.scale(longitude.sin()))
                        .scale(radius * latitude.cos())
                    + z.scale(radius * latitude.sin()))
            }
        }
    }
}

/// A degree-1 two-pole BS2 block over `[0, 1]`.
fn linear_pcurve(ends: [Point2; 2]) -> Vec<Token> {
    let mut out = vec![
        Token::Ident("nubs".into()),
        Token::Long(1),
        Token::Enum(0),
        Token::Long(2),
        Token::Double(0.0),
        Token::Long(1),
        Token::Double(1.0),
        Token::Long(1),
    ];
    for end in ends {
        out.extend([Token::Double(end.u), Token::Double(end.v)]);
    }
    out
}

/// A degree-1 two-pole 3D curve block over `[0, 1]`.
fn linear_curve() -> Vec<Token> {
    let mut out = vec![
        Token::Ident("nubs".into()),
        Token::Long(1),
        Token::Enum(0),
        Token::Long(2),
        Token::Double(0.0),
        Token::Long(1),
        Token::Double(1.0),
        Token::Long(1),
    ];
    out.extend(
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]
            .into_iter()
            .map(Token::Double),
    );
    out
}

/// Require the decoded support at each decoded pole to reach the native point
/// of the matching native pole, and return those points.
fn assert_contacts(
    surface: &SurfaceGeometry,
    pcurve: &PcurveNurbs,
    support: Support,
    ends: [Point2; 2],
) -> [Point3; 2] {
    let ctx = cadmpeg_test_support::service_decode_context();
    let surface = surface.solved().expect("solved analytic support");
    let points = pcurve.control_points();
    assert_eq!(points.len(), 2);
    std::array::from_fn(|index| {
        let uv = points[index].get();
        let point = cadmpeg_ir::eval::decode::surface_point_solved(&ctx, surface, uv.u, uv.v)
            .expect("finite support point")
            .get();
        let expected = support.native_point(ends[index]);
        let miss = point.distance(expected);
        assert!(
            miss <= EPS_CONTACT_MM,
            "contact {point:?} misses {expected:?} by {miss}"
        );
        point
    })
}

fn decode_context<T>(use_context: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    crate::test_support::with_service_context(&[], use_context).expect("empty test input fits")
}

/// A rolling-ball side on `support` with primary pcurve `ends`.
fn rolling_ball_side_tokens(support: Support, ends: [Point2; 2]) -> Vec<Token> {
    let mut out = vec![Token::Str("blend_support_surface".into())];
    out.extend(support.tokens(true));
    out.push(Token::Ident("null_curve".into()));
    out.extend(linear_pcurve(ends));
    out.extend([
        Token::Position([0.0; 3]),
        Token::Ident("nullbs".into()),
        Token::Long(0),
        Token::Ident("nullbs".into()),
    ]);
    out
}

fn decode_rolling_ball_side(support: Support, ends: [Point2; 2]) -> (SurfaceGeometry, PcurveNurbs) {
    let tokens = rolling_ball_side_tokens(support, ends);
    let side = decode_context(|ctx| {
        crate::nurbs::blend::rolling_ball_side(ctx, &mut Cur::at(&tokens, 0), None)
            .expect("side parses")
            .expect("no resource refusal")
    });
    (
        side.surface.expect("embedded support").surface,
        side.pcurve.expect("primary pcurve"),
    )
}

#[test]
fn rolling_ball_contacts_lie_on_both_supports_and_on_the_ball() {
    // A ball of radius 0.5 cm rolls along `c(v) = (0.5, 2v, 0.5)` between the
    // plane z = 0 and the reverse_v plane x = 0, whose v runs along -z.
    let radius_cm = 0.5;
    let floor = Support::Plane {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
        u_dir: [1.0, 0.0, 0.0],
        reverse_v: false,
    };
    let wall = Support::Plane {
        origin: [0.0; 3],
        normal: [1.0, 0.0, 0.0],
        u_dir: [0.0, 1.0, 0.0],
        reverse_v: true,
    };
    let floor_ends = [Point2::new(0.5, 0.0), Point2::new(0.5, 2.0)];
    let wall_ends = [Point2::new(0.0, -0.5), Point2::new(2.0, -0.5)];
    let centers = [0.0, 2.0].map(|y| mm(Vector3::new(radius_cm, y, radius_cm)));
    for (support, ends) in [(floor, floor_ends), (wall, wall_ends)] {
        let (surface, pcurve) = decode_rolling_ball_side(support, ends);
        let contacts = assert_contacts(&surface, &pcurve, support, ends);
        for (contact, center) in contacts.into_iter().zip(centers) {
            let miss = (contact.distance(center) - radius_cm * CM_TO_MM).abs();
            assert!(
                miss <= EPS_CONTACT_MM,
                "contact {contact:?} is off the ball"
            );
        }
    }
}

#[test]
fn rolling_ball_contact_on_an_inward_reversed_cylinder_reaches_the_native_point() {
    let support = Support::Cone {
        origin: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        major: [0.0, 2.0, 0.0],
        sine: 0.0,
        cosine: -1.0,
        u_scale: 2.0,
        reverse_u: true,
    };
    let ends = [Point2::new(0.25, 0.5), Point2::new(-0.75, 2.0)];
    let (surface, pcurve) = decode_rolling_ball_side(support, ends);
    assert_contacts(&surface, &pcurve, support, ends);
}

#[test]
fn rolling_ball_third_side_contact_reaches_the_native_point() {
    let support = Support::Plane {
        origin: [1.0, 2.0, 3.0],
        normal: [0.0, 1.0, 0.0],
        u_dir: [0.0, 0.0, 1.0],
        reverse_v: true,
    };
    let ends = [Point2::new(0.25, -1.0), Point2::new(1.5, 0.5)];
    let mut tokens = vec![Token::Str("third".into())];
    tokens.extend(support.tokens(false));
    tokens.extend(linear_curve());
    tokens.extend(linear_pcurve(ends));
    tokens.extend([
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Ident("nullbs".into()),
        Token::Long(0),
        Token::Ident("nullbs".into()),
        Token::False,
    ]);
    let side = crate::nurbs::blend::rolling_ball_third_side(&mut Cur::at(&tokens, 0))
        .expect("third side parses");
    assert_contacts(
        &side.surface,
        side.pcurve.as_ref().expect("pcurve"),
        support,
        ends,
    );
}

#[test]
fn g2_side_first_pcurve_reaches_the_native_point() {
    let support = Support::Sphere {
        center: [0.0, 1.0, 0.0],
        radius: -1.5,
        reference: [1.0, 0.0, 0.0],
        pole: [0.0, 0.0, 1.0],
        reverse_v: false,
    };
    let ends = [Point2::new(0.25, 0.5), Point2::new(-0.5, 2.0)];
    let mut tokens = vec![Token::Str("left".into())];
    tokens.extend(support.tokens(false));
    tokens.extend(linear_curve());
    tokens.extend(linear_pcurve(ends));
    tokens.extend([
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Ident("nullbs".into()),
    ]);
    let side =
        crate::nurbs::proc_surface::g2_side(&mut Cur::at(&tokens, 0)).expect("G2 side parses");
    assert_contacts(
        &side.surface,
        side.pcurves[0].as_ref().expect("first pcurve"),
        support,
        ends,
    );
}

#[test]
fn vertex_blend_pcurve_boundary_reaches_the_native_point() {
    let support = Support::Plane {
        origin: [0.5, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        u_dir: [0.6, 0.8, 0.0],
        reverse_v: true,
    };
    let ends = [Point2::new(0.25, 0.75), Point2::new(-1.0, 1.5)];
    let mut tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("VBL_SURF".into()),
        Token::Long(1),
        Token::Str("pcurve".into()),
        Token::False,
        Token::Position([0.0; 3]),
        Token::False,
        Token::False,
        Token::Double(0.5),
    ];
    tokens.extend(support.tokens(false));
    tokens.extend(linear_pcurve(ends));
    tokens.extend([
        Token::False,
        Token::Double(1.0e-6),
        Token::Long(4),
        Token::Double(1.0e-3),
        Token::SubtypeClose,
    ]);
    let decoded = decode_context(|ctx| {
        crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            ctx,
            &tokens,
            &SubtypeTable::from_records(&[]),
        )
        .expect("vertex blend parses")
        .expect("no resource refusal")
    });
    let (definition, _) = decoded.into_parts();
    let crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition::VertexBlend(blend) =
        definition
    else {
        panic!("expected a vertex blend");
    };
    let crate::nurbs::proc_surface::EmbeddedVertexBlendBoundaryGeometry::Pcurve {
        surface,
        pcurve,
        ..
    } = &blend.boundaries[0].geometry
    else {
        panic!("expected a pcurve boundary");
    };
    assert_contacts(surface, pcurve.as_ref().expect("pcurve"), support, ends);
}

#[test]
fn loft_profile_pcurve_reaches_the_native_point() {
    let support = Support::Cone {
        origin: [0.0; 3],
        axis: [0.0, 1.0, 0.0],
        major: [1.0, 0.0, 0.0],
        sine: -0.5,
        cosine: 0.75_f64.sqrt(),
        u_scale: 1.0,
        reverse_u: false,
    };
    let ends = [Point2::new(0.25, 0.5), Point2::new(0.75, 1.5)];
    let mut tokens = support.tokens(false);
    tokens.extend(linear_pcurve(ends));
    tokens.extend([
        Token::False,
        Token::Long(0),
        Token::Long(0),
        Token::Long(0),
        Token::Long(0),
        Token::False,
    ]);
    let data = crate::nurbs::proc_surface::loft_profile_data(&mut Cur::at(&tokens, 0), 1)
        .expect("loft profile parses");
    assert_contacts(
        &data.surface,
        data.pcurve.as_ref().expect("pcurve"),
        support,
        ends,
    );
}

#[test]
fn taper_pcurve_reaches_the_native_point() {
    let support = Support::Plane {
        origin: [0.0, 0.0, 1.0],
        normal: [0.0, 0.0, -1.0],
        u_dir: [1.0, 0.0, 0.0],
        reverse_v: true,
    };
    let ends = [Point2::new(0.5, 0.25), Point2::new(-1.5, 2.0)];
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("taper_spl_sur".into())];
    tokens.extend(support.tokens(false));
    tokens.extend(linear_curve());
    tokens.extend(linear_pcurve(ends));
    tokens.push(Token::Double(0.25));
    tokens.extend([
        Token::Ident("nubs".into()),
        Token::Long(1),
        Token::Long(1),
        Token::Enum(0),
        Token::Enum(0),
        Token::Enum(0),
        Token::Enum(0),
        Token::Long(2),
        Token::Long(2),
    ]);
    for _ in 0..2 {
        tokens.extend([
            Token::Double(0.0),
            Token::Long(1),
            Token::Double(1.0),
            Token::Long(1),
        ]);
    }
    tokens.extend(
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0]
            .into_iter()
            .map(Token::Double),
    );
    tokens.extend([Token::Double(1.0e-3), Token::SubtypeClose]);
    let decoded = decode_context(|ctx| {
        crate::nurbs::proc_surface::procedural_surface_resolving_refs(
            ctx,
            &tokens,
            &SubtypeTable::from_records(&[]),
        )
        .expect("taper parses")
        .expect("no resource refusal")
    });
    let (definition, _) = decoded.into_parts();
    let crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Taper {
        support: surface,
        pcurve,
        ..
    } = definition
    else {
        panic!("expected a taper");
    };
    assert_contacts(&surface, pcurve.as_ref().expect("pcurve"), support, ends);
}
