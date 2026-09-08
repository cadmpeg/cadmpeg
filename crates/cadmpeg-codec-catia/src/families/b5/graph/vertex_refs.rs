//! Vertex tables and admitted edge bindings.

use super::B5LogicalVertex;
use std::collections::BTreeMap;

/// An endpoint in the raw or logical vertex table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum B5VertexRef {
    /// Raw `05 08 01` vertex-table index.
    Raw(usize),
    /// Native `5d` logical-vertex index.
    Logical(usize),
}

impl B5VertexRef {
    /// Combined ordinal used by emitted point and vertex identities.
    pub const fn combined_index(self, raw_count: usize) -> usize {
        match self {
            Self::Raw(index) => index,
            Self::Logical(index) => raw_count + index,
        }
    }

    fn point(self, vertices: &B5Vertices) -> [f64; 3] {
        match self {
            Self::Raw(index) => vertices.raw[index],
            Self::Logical(index) => vertices.logical[index].point,
        }
    }
}

/// Vertex coordinates and edge references admitted against their table bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct B5Vertices {
    raw: Vec<[f64; 3]>,
    logical: Vec<B5LogicalVertex>,
    edges: BTreeMap<u32, [B5VertexRef; 2]>,
}

impl B5Vertices {
    /// Admit vertex tables and references that select existing rows.
    pub fn try_new(
        raw: Vec<[f64; 3]>,
        logical: Vec<B5LogicalVertex>,
        edges: BTreeMap<u32, [B5VertexRef; 2]>,
    ) -> Result<Self, &'static str> {
        raw.len()
            .checked_add(logical.len())
            .ok_or("vertex table count overflow")?;
        if edges
            .values()
            .flatten()
            .any(|vertex| !Self::in_range(*vertex, raw.len(), logical.len()))
        {
            return Err("edge_vertices references a missing vertex row");
        }
        Ok(Self {
            raw,
            logical,
            edges,
        })
    }

    fn in_range(vertex: B5VertexRef, raw_count: usize, logical_count: usize) -> bool {
        match vertex {
            B5VertexRef::Raw(index) => index < raw_count,
            B5VertexRef::Logical(index) => index < logical_count,
        }
    }

    /// Raw vertex coordinates in source order.
    pub fn raw_points(&self) -> &[[f64; 3]] {
        &self.raw
    }

    /// Logical vertices in native identity order.
    pub fn logical_vertices(&self) -> &[B5LogicalVertex] {
        &self.logical
    }

    /// Admitted per-edge endpoint references.
    pub fn edges(&self) -> &BTreeMap<u32, [B5VertexRef; 2]> {
        &self.edges
    }

    /// Endpoint coordinates for an admitted edge binding.
    pub fn edge_points(&self, edge: u32) -> Option<[[f64; 3]; 2]> {
        Some(self.edges.get(&edge)?.map(|vertex| vertex.point(self)))
    }

    #[cfg(test)]
    /// Insert an edge only when both references select existing rows.
    pub fn insert_edge(
        &mut self,
        edge: u32,
        vertices: [B5VertexRef; 2],
    ) -> Result<(), &'static str> {
        if vertices
            .iter()
            .any(|vertex| !Self::in_range(*vertex, self.raw.len(), self.logical.len()))
        {
            return Err("edge_vertices references a missing vertex row");
        }
        self.edges.insert(edge, vertices);
        Ok(())
    }

    #[cfg(test)]
    /// Remove an edge binding.
    pub fn remove_edge(&mut self, edge: u32) {
        self.edges.remove(&edge);
    }

    #[cfg(test)]
    /// Append a raw vertex row without changing existing reference slots.
    pub fn push_raw(&mut self, point: [f64; 3]) {
        self.raw.push(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_binding_admission_keeps_raw_and_logical_bounds_separate() {
        let logical = vec![B5LogicalVertex {
            object_id: 10,
            point: [1.0, 0.0, 0.0],
        }];
        assert!(B5Vertices::try_new(
            vec![[0.0; 3]],
            logical.clone(),
            BTreeMap::from([(1, [B5VertexRef::Raw(1); 2])])
        )
        .is_err());
        assert!(B5Vertices::try_new(
            vec![[0.0; 3]],
            logical.clone(),
            BTreeMap::from([(1, [B5VertexRef::Logical(1); 2])])
        )
        .is_err());
        let mut vertices = B5Vertices::try_new(
            vec![[0.0; 3]],
            logical,
            BTreeMap::from([(1, [B5VertexRef::Raw(0), B5VertexRef::Logical(0)])]),
        )
        .unwrap();
        let original = vertices.clone();
        assert!(vertices
            .insert_edge(1, [B5VertexRef::Logical(1); 2])
            .is_err());
        assert_eq!(vertices, original);
        assert_eq!(vertices.edge_points(1), Some([[0.0; 3], [1.0, 0.0, 0.0]]));
        assert_eq!(
            vertices.edges()[&1].map(|vertex| vertex.combined_index(vertices.raw_points().len())),
            [0, 1]
        );
    }
}
