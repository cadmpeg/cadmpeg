// SPDX-License-Identifier: Apache-2.0
//! Corroborate the model-space vertex independently of the lifted edge ends.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::COINCIDENCE_TOLERANCE;

use super::super::{curve_point, Patch, GAP_MM};
use super::ANCHOR_AGREEMENT_MM;

const MAX_ENDPOINT_GAP_MM: f64 = 0.005;
const SAME_STATION_MM: f64 = 1.0e-6;

#[derive(Clone, Copy)]
struct Site {
    face: usize,
    edge: usize,
    point: Point3,
    primary: bool,
    affine: bool,
}

#[derive(Default)]
pub(super) struct EndpointSites {
    by_vertex: BTreeMap<((usize, usize), usize), Vec<Site>>,
}

impl EndpointSites {
    pub(super) fn record(
        &mut self,
        body: (usize, usize),
        face: usize,
        (vertex, edge, point, primary, affine): (usize, usize, Point3, bool, bool),
    ) {
        self.by_vertex
            .entry((body, vertex))
            .or_default()
            .push(Site {
                face,
                edge,
                point,
                primary,
                affine,
            });
    }

    pub(super) fn resolve(self, patches: &mut [Patch]) {
        let mut anchors = BTreeMap::new();
        let mut affine_anchors = BTreeMap::<_, (Point3, Vec<Point3>)>::new();
        for (key, sites) in self.by_vertex {
            if let Some(point) = corroborate(&sites) {
                anchors.insert(key, point);
            } else if let Some(point) = corroborate_affine(&sites) {
                affine_anchors.insert(key, (point, Vec::new()));
            }
        }
        let mut disputed = BTreeSet::new();
        for patch in &*patches {
            for side in &patch.sides {
                let parameters = side.curve.full_knot_endpoints().endpoints();
                for (&(vertex, point), parameter) in side.vertices.iter().zip(parameters) {
                    let key = (patch.body, vertex);
                    if let Some(anchor) = anchors.get(&key) {
                        let distance = anchor.distance(point);
                        if distance > MAX_ENDPOINT_GAP_MM {
                            anchors.remove(&key);
                        } else if distance > GAP_MM {
                            disputed.insert(key);
                        }
                    }
                    if let Some((_, ends)) = affine_anchors.get_mut(&key) {
                        if let Some(end) = curve_point(&side.curve, parameter) {
                            ends.push(end);
                        } else {
                            affine_anchors.remove(&key);
                        }
                    }
                }
            }
        }
        let mut affine_resolved = BTreeSet::new();
        for (key, (point, ends)) in affine_anchors {
            if let Some(point) = fit_affine_vertex(point, &ends) {
                disputed.insert(key);
                affine_resolved.insert(key);
                anchors.insert(key, point);
            }
        }
        for patch in patches {
            for (vertex, point) in patch.sides.iter_mut().flat_map(|side| &mut side.vertices) {
                let key = (patch.body, *vertex);
                if let Some(anchor) = anchors.get(&key).filter(|_| disputed.contains(&key)) {
                    if affine_resolved.contains(&key) {
                        patch.recovery.affine_vertex_shift_mm = patch
                            .recovery
                            .affine_vertex_shift_mm
                            .max(point.distance(*anchor));
                    }
                    *point = *anchor;
                }
            }
        }
    }
}

// A pair of explicit stations can establish the point. When only one is
// explicit, require the same station on both charts and another shared edge
// that corroborates the position.
fn corroborate(sites: &[Site]) -> Option<Point3> {
    let mut result = None::<Point3>;
    for site in sites.iter().filter(|site| site.primary) {
        let agrees = |other: &&Site| other.point.distance(site.point) <= ANCHOR_AGREEMENT_MM;
        let peers = sites.iter().filter(agrees).collect::<Vec<_>>();
        let same_edge = peers.iter().any(|peer| {
            peer.edge == site.edge
                && peer.face != site.face
                && peer.point.distance(site.point) <= SAME_STATION_MM
        });
        let dual_primary = peers
            .iter()
            .any(|peer| peer.edge == site.edge && peer.face != site.face && peer.primary);
        let another_edge = peers.iter().any(|peer| {
            peer.edge != site.edge
                && peers.iter().any(|other| {
                    other.edge == peer.edge
                        && other.face != peer.face
                        && other.point.distance(peer.point) <= SAME_STATION_MM
                })
        });
        if !(dual_primary || (same_edge && another_edge)) {
            continue;
        }
        if let Some(previous) = result {
            if previous.distance(site.point) > ANCHOR_AGREEMENT_MM {
                return None;
            }
        } else {
            result = Some(site.point);
        }
    }
    result
}

// Two affine-mapped edges must independently corroborate the station on
// their opposite face charts before the station can resolve conflicting lifts.
fn corroborate_affine(sites: &[Site]) -> Option<Point3> {
    let mut result: Option<Point3> = None;
    for site in sites.iter().filter(|site| site.affine) {
        let matching = sites
            .iter()
            .filter(|other| other.affine && other.point.distance(site.point) <= SAME_STATION_MM)
            .collect::<Vec<_>>();
        let confirmed = |site: &Site| {
            matching
                .iter()
                .any(|other| other.edge == site.edge && other.face != site.face)
        };
        if !confirmed(site)
            || !matching
                .iter()
                .any(|other| other.edge != site.edge && confirmed(other))
        {
            continue;
        }
        if let Some(prior) = result {
            if prior.distance(site.point) > ANCHOR_AGREEMENT_MM {
                return None;
            }
        } else {
            result = Some(site.point);
        }
    }
    result
}

// Keep the vertex within half the fit allowance of the mapped station and
// inside the coincidence bound of each independently lifted edge endpoint.
fn fit_affine_vertex(point: Point3, ends: &[Point3]) -> Option<Point3> {
    let (end, gap) = ends
        .iter()
        .map(|&end| (end, end.distance(point)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let limit = COINCIDENCE_TOLERANCE - GAP_MM / 10.0;
    let movement = (gap - limit).max(0.0);
    if gap <= MAX_ENDPOINT_GAP_MM || movement > GAP_MM / 2.0 {
        return None;
    }
    let vertex = point.translated(end.vector_from(point), movement / gap);
    ends.iter()
        .all(|&end| end.distance(vertex) <= COINCIDENCE_TOLERANCE)
        .then_some(vertex)
}

#[cfg(test)]
mod tests {
    use super::super::super::SheetBoundaryRecovery;
    use super::*;

    #[test]
    fn shared_edge_stations_can_disambiguate_a_vertex() {
        let correct = Point3::new(0.0, 0.0, 0.0);
        let competing = Point3::new(0.002, 0.0, 0.0);
        let site = |face, edge, point, primary| Site {
            face,
            edge,
            point,
            primary,
            affine: false,
        };
        let sites = [
            site(1, 1, correct, true),
            site(2, 1, correct, false),
            site(1, 2, correct, false),
            site(3, 2, correct, false),
            site(2, 3, competing, true),
            site(3, 3, competing, false),
        ];
        assert_eq!(corroborate(&sites), Some(correct));
        assert_eq!(corroborate(&sites[4..]), None);
        let mut ambiguous = sites.to_vec();
        ambiguous.extend([site(1, 4, competing, false), site(2, 4, competing, false)]);
        assert_eq!(corroborate(&ambiguous), None);
    }

    fn affine_sites(point: Point3) -> Vec<Site> {
        [(1, 1), (2, 1), (1, 2), (3, 2)]
            .map(|(face, edge)| Site {
                face,
                edge,
                point,
                primary: false,
                affine: true,
            })
            .to_vec()
    }

    #[test]
    fn affine_vertex_requires_two_edges_corroborated_across_faces() {
        let point = Point3::new(0.0, 0.0, 0.0);
        let sites = affine_sites(point);
        assert_eq!(corroborate_affine(&sites), Some(point));
        assert_eq!(corroborate_affine(&sites[..2]), None);
        let mut same_face = sites.clone();
        for site in &mut same_face {
            site.face = 1;
        }
        assert_eq!(corroborate_affine(&same_face), None);
        let mut unconfirmed = sites.clone();
        for site in &mut unconfirmed {
            site.affine = false;
        }
        assert_eq!(corroborate_affine(&unconfirmed), None);
        let mut conflicting = sites;
        conflicting.extend(affine_sites(Point3::new(0.002, 0.0, 0.0)));
        assert_eq!(corroborate_affine(&conflicting), None);
    }

    #[test]
    fn affine_vertex_shift_stays_within_the_fit_and_coincidence_bounds() {
        let point = Point3::new(0.0, 0.0, 0.0);
        assert_eq!(
            fit_affine_vertex(point, &[Point3::new(0.0098, 0.0, 0.0)]),
            Some(point)
        );
        let shifted =
            fit_affine_vertex(point, &[Point3::new(0.0102, 0.0, 0.0)]).expect("bounded shift");
        assert!(point.distance(shifted) <= GAP_MM / 2.0);
        assert!(shifted.distance(Point3::new(0.0102, 0.0, 0.0)) <= COINCIDENCE_TOLERANCE);
        assert_eq!(
            fit_affine_vertex(point, &[Point3::new(0.0106, 0.0, 0.0)]),
            None
        );
        assert_eq!(
            fit_affine_vertex(
                point,
                &[
                    Point3::new(0.0102, 0.0, 0.0),
                    Point3::new(-0.0102, 0.0, 0.0)
                ]
            ),
            None
        );
        assert_eq!(fit_affine_vertex(point, &[]), None);
    }

    #[test]
    fn affine_resolution_records_the_selected_vertex_discrepancy() {
        use super::super::super::{line_pcurve, Use};
        use cadmpeg_ir::geometry::{
            analytic::PlaneSurface, nurbs::NurbsCurve, SolvedSurfaceGeometry,
        };
        use cadmpeg_ir::math::Vector3;
        let origin = Point3::new(0.0, 0.0, 0.0);
        let end = Point3::new(1.0, 0.0, 0.0);
        let mut patch = Patch {
            face: 1,
            body: (0, 10),
            forward: true,
            sense_inferred: false,
            surface_fit: None,
            recovery: SheetBoundaryRecovery::default(),
            priority: 0,
            inner_starts: vec![],
            surface: SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    origin,
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("plane"),
            ),
            sides: vec![Use {
                edge: 1,
                vertices: [(0, origin), (1, end)],
                curve: NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![origin, end],
                    None,
                    false,
                )
                .expect("edge"),
                pcurve: line_pcurve([0.0, 1.0], [[0.0, 0.0], [1.0, 0.0]]).expect("pcurve"),
                forward: true,
                inferred_accuracy: None,
                lift_error_mm: 0.0,
            }],
        };
        let selected = Point3::new(0.0098, 0.0, 0.0);
        let mut sites = EndpointSites::default();
        for site in affine_sites(selected) {
            sites.record(
                (0, 10),
                site.face,
                (0, site.edge, site.point, site.primary, site.affine),
            );
        }
        sites.resolve(std::slice::from_mut(&mut patch));
        assert_eq!(patch.sides[0].vertices[0].1, selected);
        assert_eq!(patch.recovery.affine_vertex_shift_mm, 0.0098);
    }
}
