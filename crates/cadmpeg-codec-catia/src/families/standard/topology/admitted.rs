//! Immutable standard topology with closed cycles and owned-table references.

use cadmpeg_ir::features::{FinitePoint3, NonEmptyMembers};

use super::{CoedgeUse, EdgeRow, StandardTopologyDraft};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Boundary {
    coedges: NonEmptyMembers<CoedgeUse>,
}

impl Boundary {
    pub(crate) fn new(coedges: NonEmptyMembers<CoedgeUse>) -> Option<Self> {
        let first = coedges.first()?;
        let mut next_start = first.start_vertex;
        for coedge in &coedges {
            if coedge.start_vertex != next_start {
                return None;
            }
            next_start = coedge.end_vertex;
        }
        if next_start != first.start_vertex {
            return None;
        }
        Some(Self { coedges })
    }

    pub(crate) fn coedges(&self) -> &[CoedgeUse] {
        self.coedges.as_slice()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FaceTopology {
    boundaries: Vec<Boundary>,
}

impl FaceTopology {
    pub(crate) fn boundaries(&self) -> &[Boundary] {
        &self.boundaries
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct StandardTopology {
    faces: Vec<FaceTopology>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<FinitePoint3>,
    logical_vertex_count: usize,
}

impl StandardTopology {
    pub(crate) fn new(draft: StandardTopologyDraft) -> Option<Self> {
        let StandardTopologyDraft {
            faces: draft_faces,
            edge_rows,
            vertex_points,
            logical_vertex_count,
        } = draft;
        let vertex_points = vertex_points
            .into_iter()
            .map(|[x, y, z]| FinitePoint3::new(cadmpeg_ir::math::Point3::new(x, y, z)))
            .collect::<Option<Vec<_>>>()?;
        let mut faces = Vec::new();
        for face in draft_faces {
            let mut boundaries = Vec::new();
            for boundary in face.boundaries {
                let boundary = Boundary::new(boundary.coedges)?;

                if boundary.coedges().iter().any(|coedge| {
                    coedge.edge_row >= edge_rows.len()
                        || coedge.start_vertex >= logical_vertex_count
                        || coedge.end_vertex >= logical_vertex_count
                }) {
                    return None;
                }
                boundaries.push(boundary);
            }
            faces.push(FaceTopology { boundaries });
        }
        Some(Self {
            faces,
            edge_rows,
            vertex_points,
            logical_vertex_count,
        })
    }

    pub(crate) fn faces(&self) -> &[FaceTopology] {
        &self.faces
    }
    pub(crate) fn edge_rows(&self) -> &[EdgeRow] {
        &self.edge_rows
    }
    pub(crate) fn vertex_points(&self) -> &[FinitePoint3] {
        &self.vertex_points
    }
    pub(crate) fn logical_vertex_count(&self) -> usize {
        self.logical_vertex_count
    }
}

#[cfg(test)]
mod tests {
    use super::{Boundary, StandardTopology};
    use crate::families::standard::topology::{
        BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft,
        StandardTopologyDraft,
    };
    use cadmpeg_ir::features::NonEmptyMembers;

    fn coedge(edge_row: usize, start_vertex: usize, end_vertex: usize) -> CoedgeUse {
        CoedgeUse {
            edge_row,
            reversed: false,
            start_vertex,
            end_vertex,
        }
    }

    fn draft(use_: CoedgeUse) -> StandardTopologyDraft {
        StandardTopologyDraft {
            faces: vec![FaceTopologyDraft {
                boundaries: vec![BoundaryDraft::new(vec![use_]).expect("nonempty draft")],
            }],
            edge_rows: vec![
                EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("edge row"),
            ],
            vertex_points: vec![[0.0, 0.0, 0.0]],
            logical_vertex_count: 1,
        }
    }

    #[test]
    fn topology_admission_rejects_foreign_edge_and_vertex_indices() {
        crate::test_support::with_service_context(|ctx| {
            for use_ in [coedge(1, 0, 0), coedge(0, 1, 1)] {
                let invalid = draft(use_);
                assert!(StandardTopology::new(invalid).is_none());
            }
            let mut missing_rows = draft(coedge(0, 0, 0));
            missing_rows.edge_rows.clear();
            assert!(missing_rows
                .edge_vertices(ctx)
                .expect("checked draft access")
                .is_none());
            assert!(StandardTopology::new(missing_rows).is_none());
            let valid = StandardTopology::new(draft(coedge(0, 0, 0))).expect("owned references");
            assert_eq!(
                valid.faces()[0].boundaries()[0].coedges()[0],
                coedge(0, 0, 0)
            );
        });
    }

    #[test]
    fn topology_admission_rejects_nonfinite_coordinates() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = draft(coedge(0, 0, 0));
            invalid.vertex_points[0][0] = value;
            assert!(StandardTopology::new(invalid).is_none());
        }
        assert!(StandardTopology::new(draft(coedge(0, 0, 0))).is_some());
    }

    #[test]
    fn boundary_admission_requires_adjacency_and_last_to_first_closure() {
        for invalid in [
            vec![coedge(0, 0, 1)],
            vec![coedge(0, 0, 1), coedge(1, 2, 0)],
            vec![coedge(0, 0, 1), coedge(1, 1, 2)],
        ] {
            assert!(
                Boundary::new(NonEmptyMembers::try_from(invalid).expect("nonempty fixture"))
                    .is_none()
            );
        }
        let cycle = vec![coedge(0, 0, 1), coedge(1, 1, 0)];
        let boundary =
            Boundary::new(NonEmptyMembers::try_from(cycle.clone()).expect("nonempty fixture"))
                .expect("closed cycle");
        assert_eq!(boundary.coedges(), cycle);
    }
}
