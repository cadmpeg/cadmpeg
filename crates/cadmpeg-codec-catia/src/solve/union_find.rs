//! Disjoint-set (union-find) over contiguous integer nodes.
//!
//! This is the single implementation shared by the topology combinatorial
//! solvers and the decode-time face-component grouping. It carries no byte
//! knowledge: callers map their domain onto `0..len` node indices.

/// A disjoint-set forest with path compression on `find`.
#[derive(Debug, Clone)]
pub(crate) struct UnionFind {
    parents: Vec<usize>,
}

impl UnionFind {
    /// Creates `length` singleton sets, one per node `0..length`.
    pub(crate) fn new(length: usize) -> Self {
        Self {
            parents: (0..length).collect(),
        }
    }

    /// Returns the number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.parents.len()
    }

    /// Appends a new singleton node and returns its index.
    pub(crate) fn push(&mut self) -> usize {
        let index = self.parents.len();
        self.parents.push(index);
        index
    }

    /// Returns the representative of `node`, compressing the path to it.
    pub(crate) fn find(&mut self, node: usize) -> usize {
        let parent = self.parents[node];
        if parent != node {
            self.parents[node] = self.find(parent);
        }
        self.parents[node]
    }

    /// Returns the representative of `node` without mutating the forest.
    pub(crate) fn root(&self, mut node: usize) -> usize {
        while self.parents[node] != node {
            node = self.parents[node];
        }
        node
    }

    /// Merges the sets containing `left` and `right`.
    pub(crate) fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}
