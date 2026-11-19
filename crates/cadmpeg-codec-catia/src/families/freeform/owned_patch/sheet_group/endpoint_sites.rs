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

    pub(super) fn plane_refit_sites(
        &self,
        patches: &[Patch],
    ) -> BTreeMap<usize, BTreeMap<usize, Point3>> {
        use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

        let mut disputed = BTreeMap::<_, Vec<_>>::new();
        for patch in patches {
            if !matches!(patch.surface, SolvedSurfaceGeometry::Plane(_)) {
                continue;
            }
            for side in &patch.sides {
                for (vertex, point) in side.vertices {
                    let key = (patch.body, vertex);
                    if let Some(sites) = self.by_vertex.get(&key) {
                        for site in sites.iter().filter(|site| {
                            site.primary
                                && site.face == patch.face
                                && site.edge == side.edge
                                && site.point.distance(point) > GAP_MM
                        }) {
                            disputed.entry((key, side.edge)).or_default().push(*site);
                        }
                    }
                }
            }
        }

        let mut witnesses = BTreeMap::<usize, BTreeMap<usize, Option<Point3>>>::new();
        for patch in patches {
            for side in &patch.sides {
                for (vertex, point) in side.vertices {
                    let Some(sites) = disputed.get(&((patch.body, vertex), side.edge)) else {
                        continue;
                    };
                    for site in sites.iter().filter(|site| {
                        site.face != patch.face && site.point.distance(point) <= ANCHOR_AGREEMENT_MM
                    }) {
                        witnesses
                            .entry(site.face)
                            .or_default()
                            .entry(vertex)
                            .and_modify(|prior| {
                                if prior
                                    .is_some_and(|p| p.distance(site.point) > ANCHOR_AGREEMENT_MM)
                                {
                                    *prior = None;
                                }
                            })
                            .or_insert(Some(site.point));
                    }
                }
            }
        }
        witnesses
            .into_iter()
            .map(|(face, vertices)| {
                (
                    face,
                    vertices
                        .into_iter()
                        .filter_map(|(id, point)| Some((id, point?)))
                        .collect(),
                )
            })
            .collect()
    }

    pub(super) fn resolve(self, patches: &mut [Patch]) {
        let mut anchors = BTreeMap::new();
        let mut affine_anchors = BTreeMap::<_, (Point3, Vec<Point3>)>::new();
        let mut boundary_anchors = BTreeMap::<_, (Point3, Vec<Point3>)>::new();
        for (key, sites) in self.by_vertex {
            if let Some(point) = corroborate(&sites) {
                anchors.insert(key, point);
            } else if let Some(point) = corroborate_affine(&sites) {
                affine_anchors.insert(key, (point, Vec::new()));
            } else if let Some(point) = boundary_station(&sites) {
                boundary_anchors.insert(key, (point, Vec::new()));
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
                    if let Some((_, ends)) = boundary_anchors.get_mut(&key) {
                        ends.push(point);
                        if let Some(end) = curve_point(&side.curve, parameter) {
                            ends.push(end);
                        } else {
                            boundary_anchors.remove(&key);
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
        let mut boundary_resolved = BTreeSet::new();
        for (key, (point, ends)) in boundary_anchors {
            if let Some(point) = fit_boundary_vertex(point, &ends) {
                disputed.insert(key);
                boundary_resolved.insert(key);
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
                    if boundary_resolved.contains(&key) {
                        patch.recovery.endpoint_vertex_shift = patch
                            .recovery
                            .endpoint_vertex_shift
                            .max(point.distance(*anchor));
                    }
                    *point = *anchor;
                }
            }
        }
    }
}

fn boundary_station(sites: &[Site]) -> Option<Point3> {
    let first = sites.first()?;
    if sites.iter().any(|site| site.face != first.face)
        || sites
            .iter()
            .map(|site| site.edge)
            .collect::<BTreeSet<_>>()
            .len()
            != 2
    {
        return None;
    }
    let mut primary = sites.iter().filter(|site| site.primary);
    let point = primary.next()?.point;
    primary
        .all(|site| site.point.distance(point) <= SAME_STATION_MM)
        .then_some(point)
}

fn fit_boundary_vertex(point: Point3, ends: &[Point3]) -> Option<Point3> {
    let (end, gap) = ends
        .iter()
        .map(|&end| (end, end.distance(point)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if gap <= GAP_MM {
        return None;
    }
    let center = Point3::new(
        point.x.midpoint(end.x),
        point.y.midpoint(end.y),
        point.z.midpoint(end.z),
    );
    (point.distance(center) <= GAP_MM && ends.iter().all(|end| end.distance(center) <= GAP_MM))
        .then_some(center)
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
    fn boundary_station_requires_one_face_and_two_edges() {
        let point = Point3::new(0.0015, 0.0, 0.0);
        let mut sites = vec![
            Site {
                face: 1,
                edge: 1,
                point,
                primary: true,
                affine: false,
            },
            Site {
                face: 1,
                edge: 2,
                point: Point3::new(0.0, 0.0, 0.0),
                primary: false,
                affine: false,
            },
        ];
        assert_eq!(boundary_station(&sites), Some(point));
        assert_eq!(boundary_station(&sites[..1]), None);
        sites[1].face = 2;
        assert_eq!(boundary_station(&sites), None);
        sites[1].face = 1;
        sites[1].primary = true;
        assert_eq!(boundary_station(&sites), None);
    }

    #[test]
    fn boundary_vertex_bounds_the_native_station_and_each_incident_endpoint() {
        let point = Point3::new(0.0015, 0.0, 0.0);
        let ends = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0001, 0.0, 0.0)];
        let vertex = fit_boundary_vertex(point, &ends).expect("common tolerance ball");
        assert!((vertex.x - 0.00075).abs() < 1.0e-12);
        assert!(point.distance(vertex) <= GAP_MM);
        assert!(ends.iter().all(|end| end.distance(vertex) <= GAP_MM));
        assert_eq!(
            fit_boundary_vertex(Point3::new(0.003, 0.0, 0.0), &ends),
            None
        );
        assert_eq!(
            fit_boundary_vertex(point, &[ends[0], Point3::new(0.004, 0.0, 0.0)]),
            None
        );
        assert_eq!(fit_boundary_vertex(point, &[]), None);
        assert_eq!(fit_boundary_vertex(ends[0], &ends), None);
    }

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
