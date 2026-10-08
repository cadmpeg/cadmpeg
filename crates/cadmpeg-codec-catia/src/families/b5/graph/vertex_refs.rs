//! Vertex tables and admitted edge bindings.

use super::super::vecmath::coordinates;
use super::B5LogicalVertex;
use cadmpeg_ir::features::FinitePoint3;
use std::collections::BTreeMap;

/// An endpoint in the raw or logical vertex table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::families) enum B5VertexRef {
    /// Raw `05 08 01` vertex-table index.
    Raw(usize),
    /// Native `5d` logical-vertex index.
    Logical(usize),
}

impl B5VertexRef {
    /// Combined ordinal used by emitted point and vertex identities.
    pub(in crate::families::b5) const fn combined_index(self, raw_count: usize) -> usize {
        match self {
            Self::Raw(index) => index,
            Self::Logical(index) => raw_count + index,
        }
    }

    fn point(self, vertices: &B5Vertices) -> [f64; 3] {
        coordinates(match self {
            Self::Raw(index) => vertices.raw[index],
            Self::Logical(index) => vertices.logical[index].point,
        })
    }
}

/// Vertex coordinates and edge references admitted against their table bounds.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct B5Vertices {
    raw: Vec<FinitePoint3>,
    logical: Vec<B5LogicalVertex>,
    edges: BTreeMap<u32, [B5VertexRef; 2]>,
}

impl B5Vertices {
    /// Admit vertex tables and references that select existing rows.
    pub(in crate::families) fn try_new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        raw: Vec<FinitePoint3>,
        logical: Vec<B5LogicalVertex>,
        edges: BTreeMap<u32, [B5VertexRef; 2]>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if raw.len().checked_add(logical.len()).is_none() {
            return Ok(None);
        }
        // Each binding holds exactly two endpoints.
        let in_range = ctx.all_by(
            &edges,
            |(_, vertices)| {
                Ok(vertices
                    .iter()
                    .all(|vertex| Self::in_range(*vertex, raw.len(), logical.len())))
            },
            "catia_b5_vertex_binding_admission",
        )?;
        if !in_range {
            return Ok(None);
        }
        Ok(Some(Self {
            raw,
            logical,
            edges,
        }))
    }

    fn in_range(vertex: B5VertexRef, raw_count: usize, logical_count: usize) -> bool {
        match vertex {
            B5VertexRef::Raw(index) => index < raw_count,
            B5VertexRef::Logical(index) => index < logical_count,
        }
    }

    /// Raw vertex coordinates in source order.
    pub(in crate::families) fn raw_points(&self) -> &[FinitePoint3] {
        &self.raw
    }

    /// Logical vertices in native identity order.
    pub(in crate::families) fn logical_vertices(&self) -> &[B5LogicalVertex] {
        &self.logical
    }

    /// Admitted per-edge endpoint references.
    pub(in crate::families::b5) fn edges(&self) -> &BTreeMap<u32, [B5VertexRef; 2]> {
        &self.edges
    }

    /// Endpoint coordinates for an admitted edge binding.
    pub(in crate::families::b5) fn edge_points(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        edge: u32,
    ) -> Result<Option<[[f64; 3]; 2]>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_btree_map(&self.edges, &edge, "catia_b5_edge_point_lookup")?
            .map(|vertices| vertices.map(|vertex| vertex.point(self))))
    }

    #[cfg(test)]
    /// Insert an edge only when both references select existing rows.
    pub(in crate::families::b5) fn insert_edge(
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
    pub(in crate::families::b5) fn remove_edge(&mut self, edge: u32) {
        self.edges.remove(&edge);
    }

    #[cfg(test)]
    /// Append a raw vertex row without changing existing reference slots.
    pub(in crate::families::b5) fn push_raw(&mut self, point: FinitePoint3) {
        self.raw.push(point);
    }
}

#[cfg(test)]
mod tests {
    use super::super::B5LogicalVertex;
    use super::{B5VertexRef, B5Vertices};
    use std::collections::BTreeMap;

    #[test]
    fn vertex_binding_admission_keeps_raw_and_logical_bounds_separate() {
        let logical = vec![B5LogicalVertex {
            object_id: 10,
            point: crate::test_support::test_b5::point([1.0, 0.0, 0.0]),
        }];
        crate::test_support::with_service_context(|ctx| {
            assert!(B5Vertices::try_new(
                ctx,
                vec![crate::test_support::test_b5::point([0.0; 3])],
                logical.clone(),
                BTreeMap::from([(1, [B5VertexRef::Raw(1); 2])])
            )
            .expect("service vertex admission budget")
            .is_none());
            assert!(B5Vertices::try_new(
                ctx,
                vec![crate::test_support::test_b5::point([0.0; 3])],
                logical.clone(),
                BTreeMap::from([(1, [B5VertexRef::Logical(1); 2])])
            )
            .expect("service vertex admission budget")
            .is_none());
            let mut vertices = B5Vertices::try_new(
                ctx,
                vec![crate::test_support::test_b5::point([0.0; 3])],
                logical,
                BTreeMap::from([(1, [B5VertexRef::Raw(0), B5VertexRef::Logical(0)])]),
            )
            .expect("service vertex admission budget")
            .expect("vertex references select existing rows");
            let original = vertices.clone();
            assert!(vertices
                .insert_edge(1, [B5VertexRef::Logical(1); 2])
                .is_err());
            assert_eq!(vertices, original);
            assert_eq!(
                vertices.edge_points(ctx, 1).expect("edge lookup budget"),
                Some([[0.0; 3], [1.0, 0.0, 0.0]])
            );
            assert_eq!(
                vertices.edges()[&1]
                    .map(|vertex| vertex.combined_index(vertices.raw_points().len())),
                [0, 1]
            );
        });
    }

    #[test]
    fn vertex_binding_admission_propagates_endpoint_scan_refusal() {
        crate::test_support::with_work_limit(1, |ctx| {
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = B5Vertices::try_new(
                ctx,
                vec![crate::test_support::test_b5::point([0.0; 3])],
                Vec::new(),
                BTreeMap::from([(1, [B5VertexRef::Raw(0); 2])]),
            ) else {
                panic!("endpoint scan must refuse");
            };
            assert_eq!(limit.operation, "catia_b5_vertex_binding_admission");
            assert_eq!(limit.used, 1);
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
        // One edge-map entry and the end of the scan; the two endpoint
        // references of an edge are a fixed-width check.
        let vertices = crate::test_support::with_work_limit(2, |ctx| {
            B5Vertices::try_new(
                ctx,
                vec![crate::test_support::test_b5::point([0.0; 3])],
                Vec::new(),
                BTreeMap::from([(1, [B5VertexRef::Raw(0); 2])]),
            )
        })
        .expect("one edge fits the work budget")
        .expect("valid vertex bindings");
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                vertices.edge_points(ctx, 1).expect("edge lookup budget"),
                Some([[0.0; 3]; 2])
            );
        });
    }
}
