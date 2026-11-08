// SPDX-License-Identifier: Apache-2.0
//! Corroborate the model-space vertex independently of the lifted edge ends.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::math::Point3;

use super::super::{Patch, GAP_MM};
use super::ANCHOR_AGREEMENT_MM;

// A bounded endpoint discrepancy may be retained when separate native
// occurrences agree on the topological vertex. Keep the edge fits intact.
const MAX_ENDPOINT_GAP_MM: f64 = 0.005;
const SAME_STATION_MM: f64 = 1.0e-6;

#[derive(Clone, Copy)]
struct Site {
    face: usize,
    edge: usize,
    point: Point3,
    primary: bool,
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
        (vertex, edge, point, primary): (usize, usize, Point3, bool),
    ) {
        self.by_vertex
            .entry((body, vertex))
            .or_default()
            .push(Site {
                face,
                edge,
                point,
                primary,
            });
    }

    pub(super) fn resolve(self, patches: &mut [Patch]) {
        let mut anchors = self
            .by_vertex
            .into_iter()
            .filter_map(|(key, sites)| Some((key, corroborate(&sites)?)))
            .collect::<BTreeMap<_, _>>();
        let mut disputed = BTreeSet::new();
        for patch in &*patches {
            for &(vertex, point) in patch.sides.iter().flat_map(|side| &side.vertices) {
                let key = (patch.body, vertex);
                if let Some(anchor) = anchors.get(&key) {
                    let distance = anchor.distance(point);
                    if distance > MAX_ENDPOINT_GAP_MM {
                        anchors.remove(&key);
                    } else if distance > GAP_MM {
                        disputed.insert(key);
                    }
                }
            }
        }
        for patch in patches {
            for (vertex, point) in patch.sides.iter_mut().flat_map(|side| &mut side.vertices) {
                let key = (patch.body, *vertex);
                if let Some(anchor) = anchors.get(&key).filter(|_| disputed.contains(&key)) {
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

#[cfg(test)]
mod tests {
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
}
