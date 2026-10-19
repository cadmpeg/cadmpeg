// SPDX-License-Identifier: Apache-2.0
//! Conservative spatial candidates for exact boundary intersection tests.

use cadmpeg_ir::math::Point2;

#[derive(Clone, Copy)]
struct Bounds {
    min: [f64; 2],
    max: [f64; 2],
}

impl Bounds {
    fn segment(left: Point2, right: Point2) -> Self {
        Self {
            min: [left.u.min(right.u), left.v.min(right.v)],
            max: [left.u.max(right.u), left.v.max(right.v)],
        }
    }
    fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }
    fn expanded(self, tolerance: f64) -> Self {
        // Round outward. Moving the tolerance from the exact predicate's
        // right-hand box to this query must not exclude an ulp boundary.
        Self {
            min: self.min.map(|value| (value - tolerance).next_down()),
            max: self.max.map(|value| (value + tolerance).next_up()),
        }
    }
    fn intersects(self, other: Self) -> bool {
        (0..2).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }
}

struct Edge {
    polygon: usize,
    ordinal: usize,
    count: usize,
    start: Point2,
    end: Point2,
}
struct Node {
    bounds: Bounds,
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}

pub(super) struct BoundaryIndex {
    edges: Vec<Edge>,
    items: Vec<(Bounds, usize)>,
    nodes: Vec<Node>,
}

impl BoundaryIndex {
    pub(super) fn new(polygons: &[Vec<Point2>]) -> Self {
        let mut edges = Vec::new();
        let mut items = Vec::new();
        for (polygon, points) in polygons.iter().enumerate() {
            for (ordinal, &start) in points.iter().enumerate() {
                let end = points[(ordinal + 1) % points.len()];
                items.push((Bounds::segment(start, end), edges.len()));
                edges.push(Edge {
                    polygon,
                    ordinal,
                    count: points.len(),
                    start,
                    end,
                });
            }
        }
        let mut index = Self {
            edges,
            items,
            nodes: Vec::new(),
        };
        if index.items.is_empty() {
            return index;
        }
        index.nodes.push(Node {
            bounds: index.items[0].0,
            range: 0..index.items.len(),
            children: None,
        });
        let mut tasks = vec![0];
        while let Some(slot) = tasks.pop() {
            let range = index.nodes[slot].range.clone();
            let bounds = index.items[range.clone()]
                .iter()
                .fold(index.items[range.start].0, |bounds, (next, _)| {
                    bounds.union(*next)
                });
            index.nodes[slot].bounds = bounds;
            if range.len() <= 8 {
                continue;
            }
            let axis = usize::from(bounds.max[0] - bounds.min[0] < bounds.max[1] - bounds.min[1]);
            index.items[range.clone()]
                .sort_unstable_by(|(a, _), (b, _)| a.min[axis].total_cmp(&b.min[axis]));
            let middle = range.start + range.len() / 2;
            let left = index.nodes.len();
            for range in [range.start..middle, middle..range.end] {
                index.nodes.push(Node {
                    bounds,
                    range,
                    children: None,
                });
            }
            index.nodes[slot].children = Some([left, left + 1]);
            tasks.extend([left + 1, left]);
        }
        index
    }
    pub(super) fn intersects(&self, tolerance: f64) -> bool {
        self.any_pair(tolerance, |left, right| {
            super::segments_intersect_or_touch(
                left.start,
                left.end,
                right.start,
                right.end,
                tolerance,
            )
        })
    }
    fn any_pair(&self, tolerance: f64, mut predicate: impl FnMut(&Edge, &Edge) -> bool) -> bool {
        let mut stack = Vec::new();
        for (id, edge) in self.edges.iter().enumerate() {
            let query = Bounds::segment(edge.start, edge.end).expanded(tolerance);
            stack.push(0);
            while let Some(slot) = stack.pop() {
                let node = &self.nodes[slot];
                if !node.bounds.intersects(query) {
                    continue;
                }
                if let Some(children) = node.children {
                    stack.extend(children);
                    continue;
                }
                for &(bounds, other) in &self.items[node.range.clone()] {
                    if other <= id || !bounds.intersects(query) {
                        continue;
                    }
                    let other = &self.edges[other];
                    if edge.polygon == other.polygon
                        && ((edge.ordinal + 1) % edge.count == other.ordinal
                            || (other.ordinal + 1) % edge.count == edge.ordinal)
                    {
                        continue;
                    }
                    if predicate(edge, other)
                        || (edge.polygon == other.polygon && predicate(other, edge))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;
