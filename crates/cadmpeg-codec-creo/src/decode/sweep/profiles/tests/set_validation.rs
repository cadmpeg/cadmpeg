// SPDX-License-Identifier: Apache-2.0
use super::super::{
    ordered_extrusion_profiles, profile_segments_intersect, profile_strictly_contains,
    ExtrusionProfile, ProfileEntity, ProfileValidationWork, ValidatedProfile,
    EPS_GEOMETRY_AGREEMENT,
};
use super::{PcurveNurbs, Point2, SketchGeometry, SketchGeometryDefinition};
use crate::lane_refusal::LaneRefusals;
use cadmpeg_core::decode::DecodeContext;

type Ordered = Option<Vec<(ExtrusionProfile, f64)>>;

/// The all-pairs validation every profile set ran before the sweep.
fn all_pairs_ordered(ctx: &DecodeContext<'_>, profiles: Vec<ExtrusionProfile>) -> Ordered {
    if profiles.is_empty() || profiles.iter().any(Vec::is_empty) {
        return None;
    }
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|entity| entity.start().into_iter().chain(entity.end()))
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
    let intersects = |first, second, contacts| {
        profile_segments_intersect(ctx, first, second, tolerance, contacts).expect("intersection")
    };
    for profile in &profiles {
        for first in 0..profile.len() {
            for second in first + 1..profile.len() {
                let contacts = [
                    (second == first + 1).then_some(profile[first].end()),
                    (first == 0 && second + 1 == profile.len()).then_some(profile[first].start()),
                ];
                if intersects(&profile[first], &profile[second], contacts) {
                    return None;
                }
            }
        }
    }
    for first in 0..profiles.len() {
        for second in first + 1..profiles.len() {
            for first_segment in &profiles[first] {
                for second_segment in &profiles[second] {
                    if intersects(first_segment, second_segment, [None, None]) {
                        return None;
                    }
                }
            }
        }
    }
    let contains =
        |profile, point| profile_strictly_contains(ctx, profile, point).expect("contain");
    let outer = (0..profiles.len())
        .filter(|candidate| {
            (0..profiles.len())
                .filter(|index| index != candidate)
                .all(|index| contains(&profiles[*candidate], profiles[index][0].start()))
        })
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    for first in (0..profiles.len()).filter(|index| index != outer) {
        for second in (first + 1..profiles.len()).filter(|index| index != outer) {
            if contains(&profiles[first], profiles[second][0].start())
                || contains(&profiles[second], profiles[first][0].start())
            {
                return None;
            }
        }
    }
    let mut validated = Vec::new();
    for profile in profiles {
        validated.push(
            ValidatedProfile::new(
                ctx,
                profile,
                &mut ProfileValidationWork::new(usize::MAX / 256).expect("work"),
            )
            .expect("validate")?,
        );
    }
    let outer_area = validated[*outer].area();
    if validated.iter().enumerate().any(|(index, profile)| {
        index != *outer && profile.area().is_sign_positive() == outer_area.is_sign_positive()
    }) {
        return None;
    }
    validated.swap(0, *outer);
    Some(
        validated
            .into_iter()
            .map(|profile| (profile.entities().clone(), profile.area()))
            .collect(),
    )
}

fn swept_ordered(
    ctx: &DecodeContext<'_>,
    profiles: Vec<ExtrusionProfile>,
    refusal: &mut LaneRefusals,
) -> Ordered {
    ordered_extrusion_profiles(ctx, profiles, refusal)
        .expect("order the extrusion profiles")
        .map(|profiles| {
            profiles
                .into_iter()
                .map(|profile| (profile.entities().clone(), profile.area()))
                .collect()
        })
}

fn entity(ctx: &DecodeContext<'_>, geometry: SketchGeometry) -> ProfileEntity {
    ProfileEntity::new(ctx, geometry, false)
        .expect("construct the profile entity")
        .expect("finite endpoints")
}

fn line(ctx: &DecodeContext<'_>, start: [f64; 2], end: [f64; 2]) -> ProfileEntity {
    entity(
        ctx,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        })
        .expect("line"),
    )
}

fn arc(ctx: &DecodeContext<'_>, center: [f64; 2], radius: f64, angles: [f64; 2]) -> ProfileEntity {
    entity(
        ctx,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(center[0], center[1]),
            radius: cadmpeg_ir::scalar::Length::new(radius).expect("radius"),
            start_angle: cadmpeg_ir::scalar::Angle::new(angles[0]).expect("angle"),
            end_angle: cadmpeg_ir::scalar::Angle::new(angles[1]).expect("angle"),
        })
        .expect("arc"),
    )
}

fn polyline_nurbs(ctx: &DecodeContext<'_>, points: &[[f64; 2]]) -> ProfileEntity {
    let mut knots = vec![0.0, 0.0];
    for index in 1..points.len() - 1 {
        knots.push(cadmpeg_core::convert::f64_from_index(index).expect("index"));
    }
    let last = cadmpeg_core::convert::f64_from_index(points.len() - 1).expect("index");
    knots.extend([last, last]);
    let curve = PcurveNurbs::from_lanes(
        1,
        knots,
        points.iter().map(|p| Point2::new(p[0], p[1])).collect(),
        None,
        false,
    )
    .expect("curve");
    entity(ctx, SketchGeometry::nurbs(curve))
}

fn polygon(ctx: &DecodeContext<'_>, points: &[[f64; 2]]) -> ExtrusionProfile {
    (0..points.len())
        .map(|index| line(ctx, points[index], points[(index + 1) % points.len()]))
        .collect()
}

struct Xorshift(u64);

impl Xorshift {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }

    fn coordinate(&mut self) -> f64 {
        f64::from(u32::try_from(self.below(9)).expect("small"))
    }

    fn point(&mut self) -> [f64; 2] {
        [self.coordinate(), self.coordinate()]
    }
}

fn random_profile(ctx: &DecodeContext<'_>, random: &mut Xorshift) -> ExtrusionProfile {
    let center = random.point();
    let radius = 0.5 + random.coordinate() / 2.0;
    let pi = std::f64::consts::PI;
    match random.below(5) {
        0 => {
            let points = (0..3 + random.below(4))
                .map(|_| random.point())
                .collect::<Vec<_>>();
            polygon(ctx, &points)
        }
        1 => {
            let mut points = (0..3 + random.below(4))
                .map(|_| random.point())
                .collect::<Vec<_>>();
            let split = 1 + usize::try_from(random.below(2)).expect("small");
            points.push(points[0]);
            vec![
                polyline_nurbs(ctx, &points[..=split]),
                polyline_nurbs(ctx, &points[split..]),
            ]
        }
        2 => vec![super::profile_circle(center, radius, random.below(2) == 0)],
        3 => vec![
            arc(ctx, center, radius, [0.0, pi]),
            arc(ctx, center, radius, [pi, 2.0 * pi]),
        ],
        _ => vec![
            arc(ctx, center, radius, [0.0, pi]),
            line(
                ctx,
                [center[0] - radius, center[1]],
                [center[0] + radius, center[1]],
            ),
        ],
    }
}

#[test]
fn swept_profile_set_validation_matches_all_pairs_validation() {
    crate::decode::with_service_context(|ctx| {
        let mut random = Xorshift(0x9e37_79b9_7f4a_7c15);
        let mut accepted = 0;
        for _ in 0..600 {
            let profiles = (0..=random.below(4))
                .map(|_| random_profile(ctx, &mut random))
                .collect::<Vec<_>>();
            let mut refusal = LaneRefusals::new();
            let swept = swept_ordered(ctx, profiles.clone(), &mut refusal);
            assert_eq!(
                swept,
                all_pairs_ordered(ctx, profiles.clone()),
                "{profiles:?}"
            );
            assert!(refusal.take_records().is_empty());
            accepted += usize::from(swept.is_some());
        }
        assert!(accepted > 50, "{accepted} sets accepted");
    });
}

#[test]
fn many_sided_polygon_with_holes_is_ordered() {
    crate::decode::with_service_context(|ctx| {
        let sides = 20_000_u32;
        let outer = (0..sides)
            .map(|index| {
                let angle = std::f64::consts::TAU * f64::from(index) / f64::from(sides);
                [100.0 * angle.cos(), 100.0 * angle.sin()]
            })
            .collect::<Vec<_>>();
        let mut profiles = vec![polygon(ctx, &outer)];
        for row in 0..10_i32 {
            for column in 0..10_i32 {
                let center = [f64::from(row * 10 - 45), f64::from(column * 10 - 45)];
                profiles.push(vec![super::profile_circle(center, 2.0, true)]);
            }
        }
        let mut refusal = LaneRefusals::new();
        let ordered = swept_ordered(ctx, profiles, &mut refusal).expect("valid profile set");
        assert_eq!(ordered.len(), 101);
        assert_eq!(ordered[0].0.len(), 20_000);
        assert!(refusal.take_records().is_empty());
    });
}

#[test]
fn serpentine_profile_stops_at_the_validation_work_limit() {
    crate::decode::with_service_context(|ctx| {
        let mut points = Vec::new();
        for row in 0..20_000_u32 {
            let y = f64::from(row);
            if row % 2 == 0 {
                points.extend([[0.0, y], [1000.0, y]]);
            } else {
                points.extend([[1000.0, y], [0.0, y]]);
            }
        }
        let top = f64::from(19_999_u32);
        points.extend([[-1.0, top], [-1.0, 0.0]]);
        let mut refusal = LaneRefusals::new();
        assert_eq!(
            swept_ordered(ctx, vec![polygon(ctx, &points)], &mut refusal),
            None
        );
        let records = refusal.take_records();
        assert_eq!(records.len(), 1);
        assert!(
            records[0].contains("PROFILE_VALIDATION_WORK_PER_PIECE"),
            "{records:?}"
        );
    });
}

#[test]
fn nurbs_profile_point_cap_refuses_the_profile_set() {
    crate::decode::with_service_context(|ctx| {
        let mut points = (0..300_000_u32)
            .map(|index| [f64::from(index), f64::from(index % 2)])
            .collect::<Vec<_>>();
        points.extend([[300_000.0, -5.0], [0.0, -5.0], [0.0, 0.0]]);
        let mut refusal = LaneRefusals::new();
        assert_eq!(
            swept_ordered(ctx, vec![vec![polyline_nurbs(ctx, &points)]], &mut refusal),
            None
        );
        let records = refusal.take_records();
        assert_eq!(records.len(), 1);
        assert!(
            records[0].contains("MAX_NURBS_PROFILE_POINTS"),
            "{records:?}"
        );
    });
}
