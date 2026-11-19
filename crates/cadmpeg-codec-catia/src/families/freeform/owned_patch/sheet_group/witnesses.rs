// SPDX-License-Identifier: Apache-2.0
//! Reconcile conflicting lifts of the same native sheet vertex.

use std::collections::BTreeMap;

use cadmpeg_ir::math::Point3;

use super::super::{edge_alignment, trimmed_face, Patch, Use, GAP_MM};
use super::{add_site, Sites, ANCHOR_AGREEMENT_MM, DISPUTED_VERTEX_PENALTY};

type SitesByFace = BTreeMap<usize, Option<Point3>>;

#[derive(Default)]
pub(super) struct PrimarySites {
    by_vertex: BTreeMap<((usize, usize), usize), SitesByFace>,
}

impl PrimarySites {
    pub(super) fn record(
        &mut self,
        body: (usize, usize),
        face: usize,
        points: impl Iterator<Item = (usize, Point3)>,
    ) {
        for (vertex, point) in points {
            self.by_vertex
                .entry((body, vertex))
                .or_default()
                .entry(face)
                .and_modify(|prior| {
                    if prior.is_some_and(|p| p.distance(point) > ANCHOR_AGREEMENT_MM) {
                        *prior = None;
                    }
                })
                .or_insert(Some(point));
        }
    }

    pub(super) fn corroborate(
        self,
        output: &mut BTreeMap<(usize, usize), BTreeMap<usize, Point3>>,
    ) {
        for ((body, vertex), points) in self.by_vertex {
            if points.len() < 2 {
                continue;
            }
            if let Some(&Some(point)) = points.values().next() {
                if points
                    .values()
                    .all(|p| p.is_some_and(|p| p.distance(point) <= ANCHOR_AGREEMENT_MM))
                {
                    output.entry(body).or_default().insert(vertex, point);
                }
            }
        }
    }
}

pub(super) fn disputed(patches: &[Patch]) -> BTreeMap<(usize, usize), BTreeMap<usize, Point3>> {
    let mut candidates = BTreeMap::<_, Vec<_>>::new();
    for patch in patches {
        for (id, point) in coherent_vertices(patch.sides.iter().flat_map(|side| side.vertices)) {
            candidates.entry((patch.body, id)).or_default().push(point);
        }
    }
    let mut witnesses = BTreeMap::<_, BTreeMap<_, _>>::new();
    for ((body, id), points) in candidates {
        if let Some(point) = majority(&points) {
            witnesses.entry(body).or_default().insert(id, point);
        }
    }
    witnesses
}

pub(super) fn retry_sites(patches: &[Patch]) -> BTreeMap<(usize, usize), BTreeMap<usize, Point3>> {
    let mut sites = Sites::new();
    for patch in patches {
        record_retry_sites(
            &mut sites,
            patch.body,
            patch.priority,
            patch.sides.iter().flat_map(|side| side.vertices),
        );
    }
    sites
        .into_iter()
        .map(|(body, vertices)| {
            (
                body,
                vertices
                    .into_iter()
                    .filter_map(|(id, point)| Some((id, point?)))
                    .collect(),
            )
        })
        .collect()
}

fn record_retry_sites(
    sites: &mut Sites,
    body: (usize, usize),
    priority: u8,
    vertices: impl Iterator<Item = (usize, Point3)>,
) {
    if priority >= DISPUTED_VERTEX_PENALTY {
        return;
    }
    for (id, point) in coherent_vertices(vertices) {
        add_site(sites, body, id, point);
    }
}

fn coherent_vertices(vertices: impl Iterator<Item = (usize, Point3)>) -> BTreeMap<usize, Point3> {
    let mut points = BTreeMap::<_, Option<Point3>>::new();
    for (id, point) in vertices {
        points
            .entry(id)
            .and_modify(|prior| {
                if prior.is_some_and(|p| p.distance(point) > ANCHOR_AGREEMENT_MM) {
                    *prior = None;
                }
            })
            .or_insert(Some(point));
    }
    points
        .into_iter()
        .filter_map(|(id, point)| Some((id, point?)))
        .collect()
}

fn majority(points: &[Point3]) -> Option<Point3> {
    let origin = *points.first()?;
    if points.len() < 3 || points.iter().all(|p| p.distance(origin) <= GAP_MM) {
        return None;
    }
    let mut best = None;
    for &point in points {
        let supporters = points
            .iter()
            .filter(|p| p.distance(point) <= ANCHOR_AGREEMENT_MM)
            .count();
        if supporters <= points.len() / 2 {
            continue;
        }
        match best {
            Some((prior, _)) if point.distance(prior) > ANCHOR_AGREEMENT_MM => return None,
            Some((_, n)) if n >= supporters => (),
            _ => best = Some((point, supporters)),
        }
    }
    best.map(|(point, _)| point)
}

pub(super) fn disagrees(patch: &Patch, witnesses: &BTreeMap<usize, Point3>) -> bool {
    patch
        .sides
        .iter()
        .flat_map(|side| &side.vertices)
        .any(|(id, point)| {
            witnesses
                .get(id)
                .is_some_and(|known| point.distance(*known) > GAP_MM)
        })
}

pub(super) fn repair_boundary(
    patch: &Patch,
    sites: &BTreeMap<usize, Point3>,
    edges: &BTreeMap<usize, Vec<(usize, &Use)>>,
) -> Option<Patch> {
    let mut sides = Vec::new();
    for side in &patch.sides {
        if disagrees_with_sites(side, sites) {
            let &(_, edge) = edges
                .get(&side.edge)?
                .iter()
                .find(|(face, _)| *face != patch.face)?;
            if disagrees_with_sites(edge, sites) {
                return None;
            }
            sides.push(edge_alignment::reproject_corroborated(
                edge,
                side,
                &patch.surface,
            )?);
        } else {
            sides.push(side.clone());
        }
    }
    let patch = Patch {
        face: patch.face,
        recovery: patch.recovery,
        body: patch.body,
        forward: patch.forward,
        sense_inferred: patch.sense_inferred,
        surface_fit: patch.surface_fit,
        surface: patch.surface.clone(),
        priority: patch.priority,
        inner_starts: patch.inner_starts.clone(),
        sides,
    };
    trimmed_face::holes_are_contained(&patch)?.then_some(patch)
}

fn disagrees_with_sites(side: &Use, sites: &BTreeMap<usize, Point3>) -> bool {
    side.vertices.iter().any(|(id, point)| {
        sites
            .get(id)
            .is_some_and(|known| point.distance(*known) > GAP_MM)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_and_inconsistent_faces_preserve_an_independent_retry_site() {
        let point = Point3::new(2.0, 3.0, 4.0);
        let other = Point3::new(2.02, 3.0, 4.0);
        let mut sites = Sites::new();
        record_retry_sites(&mut sites, (0, 10), 0, [(7, point)].into_iter());
        record_retry_sites(
            &mut sites,
            (0, 10),
            DISPUTED_VERTEX_PENALTY + 2,
            [(7, other)].into_iter(),
        );
        record_retry_sites(&mut sites, (0, 10), 5, [(7, other), (7, point)].into_iter());
        assert_eq!(sites[&(0, 10)][&7], Some(point));
    }

    #[test]
    fn independent_retry_site_conflicts_remain_unresolved_within_their_body() {
        let point = Point3::new(2.0, 3.0, 4.0);
        let other = Point3::new(2.02, 3.0, 4.0);
        let mut sites = Sites::new();
        record_retry_sites(&mut sites, (0, 10), 0, [(7, point)].into_iter());
        record_retry_sites(&mut sites, (0, 20), 0, [(7, other)].into_iter());
        record_retry_sites(&mut sites, (0, 10), 0, [(7, other)].into_iter());
        assert_eq!(sites[&(0, 10)][&7], None);
        assert_eq!(sites[&(0, 20)][&7], Some(other));
    }

    #[test]
    fn two_independent_lifts_resolve_a_disputed_vertex() {
        let a = Point3::new(1.0, 2.0, 3.0);
        let b = Point3::new(1.0001, 2.0, 3.0);
        let c = Point3::new(6.0, 2.0, 3.0);
        assert_eq!(majority(&[c, a, b]), Some(a));
        assert_eq!(majority(&[a, c]), None);
        assert_eq!(majority(&[a, b, a]), None);
    }
}
