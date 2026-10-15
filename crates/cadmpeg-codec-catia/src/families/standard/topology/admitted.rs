//! Immutable standard topology with closed cycles and owned-table references.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, NonEmptyMembers};

use super::{CoedgeUse, EdgeRow, StandardTopologyDraft};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Boundary {
    coedges: NonEmptyMembers<CoedgeUse>,
}

impl Boundary {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        coedges: NonEmptyMembers<CoedgeUse>,
    ) -> Result<Option<Self>, CodecError> {
        let Some(first) = coedges.first() else {
            return Ok(None);
        };
        let mut next_start = first.start_vertex;
        let adjacent = ctx.all_by(
            coedges.as_slice(),
            |coedge| {
                let adjacent = coedge.start_vertex == next_start;
                next_start = coedge.end_vertex;
                Ok(adjacent)
            },
            "catia_closed_boundary_admission",
        )?;
        if !adjacent || next_start != first.start_vertex {
            return Ok(None);
        }
        Ok(Some(Self { coedges }))
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
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        draft: StandardTopologyDraft,
    ) -> Result<Option<Self>, CodecError> {
        let StandardTopologyDraft {
            faces: draft_faces,
            edge_rows,
            vertex_points,
            logical_vertex_count,
        } = draft;
        let Some(vertex_points) = ctx.collect_fallible_options(
            vertex_points.into_iter().map(|[x, y, z]| {
                Ok::<_, CodecError>(FinitePoint3::new(cadmpeg_ir::math::Point3::new(x, y, z)))
            }),
            "catia_admitted_topology_points",
        )?
        else {
            return Ok(None);
        };
        let mut faces = Vec::new();
        let mut draft_faces = draft_faces.into_iter();
        while let Some(face) =
            ctx.next_charged(&mut draft_faces, "catia_admitted_topology_faces")?
        {
            let mut boundaries = Vec::new();
            let mut draft_boundaries = face.boundaries.into_iter();
            while let Some(boundary) =
                ctx.next_charged(&mut draft_boundaries, "catia_admitted_topology_boundaries")?
            {
                let Some(boundary) = Boundary::new(ctx, boundary.coedges)? else {
                    return Ok(None);
                };
                if ctx.any_by(
                    boundary.coedges(),
                    |coedge| {
                        Ok(coedge.edge_row >= edge_rows.len()
                            || coedge.start_vertex >= logical_vertex_count
                            || coedge.end_vertex >= logical_vertex_count)
                    },
                    "catia_topology_reference_admission",
                )? {
                    return Ok(None);
                }
                ctx.push_vec(
                    &mut boundaries,
                    boundary,
                    "catia_admitted_topology_boundaries",
                )?;
            }
            ctx.push_vec(
                &mut faces,
                FaceTopology { boundaries },
                "catia_admitted_topology_faces",
            )?;
        }
        Ok(Some(Self {
            faces,
            edge_rows,
            vertex_points,
            logical_vertex_count,
        }))
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
                assert!(StandardTopology::new(ctx, invalid)
                    .expect("service admission")
                    .is_none());
            }
            let mut missing_rows = draft(coedge(0, 0, 0));
            missing_rows.edge_rows.clear();
            assert!(missing_rows
                .edge_vertices(ctx)
                .expect("checked draft access")
                .is_none());
            assert!(StandardTopology::new(ctx, missing_rows)
                .expect("service admission")
                .is_none());
            let valid = StandardTopology::new(ctx, draft(coedge(0, 0, 0)))
                .expect("service admission")
                .expect("owned references");
            assert_eq!(
                valid.faces()[0].boundaries()[0].coedges()[0],
                coedge(0, 0, 0)
            );
        });
    }

    #[test]
    fn topology_admission_stops_before_unvisited_faces_and_boundaries() {
        crate::test_support::with_work_limit(64, |ctx| {
            let mut invalid = draft(coedge(1, 0, 0));
            for _ in 0..1024 {
                invalid.faces.push(super::super::FaceTopologyDraft {
                    boundaries: vec![
                        super::super::BoundaryDraft::new(vec![coedge(0, 0, 0)]).expect("boundary")
                    ],
                });
            }
            assert!(StandardTopology::new(ctx, invalid)
                .expect("first face is visited")
                .is_none());
        });
        crate::test_support::with_work_limit(64, |ctx| {
            let mut invalid = draft(coedge(1, 0, 0));
            for _ in 0..1024 {
                invalid.faces[0].boundaries.push(
                    super::super::BoundaryDraft::new(vec![coedge(0, 0, 0)]).expect("boundary"),
                );
            }
            assert!(StandardTopology::new(ctx, invalid)
                .expect("first boundary is visited")
                .is_none());
        });
    }

    #[test]
    fn topology_admission_rejects_nonfinite_coordinates() {
        crate::test_support::with_service_context(|ctx| {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut invalid = draft(coedge(0, 0, 0));
                invalid.vertex_points[0][0] = value;
                assert!(StandardTopology::new(ctx, invalid)
                    .expect("service admission")
                    .is_none());
            }
            assert!(StandardTopology::new(ctx, draft(coedge(0, 0, 0)))
                .expect("service admission")
                .is_some());
        });
    }

    #[test]
    fn boundary_admission_requires_adjacency_and_last_to_first_closure() {
        crate::test_support::with_service_context(|ctx| {
            for invalid in [
                vec![coedge(0, 0, 1)],
                vec![coedge(0, 0, 1), coedge(1, 2, 0)],
                vec![coedge(0, 0, 1), coedge(1, 1, 2)],
            ] {
                assert!(Boundary::new(
                    ctx,
                    NonEmptyMembers::try_from(invalid).expect("nonempty fixture")
                )
                .expect("service admission")
                .is_none());
            }
            let cycle = vec![coedge(0, 0, 1), coedge(1, 1, 0)];
            let boundary = Boundary::new(
                ctx,
                NonEmptyMembers::try_from(cycle.clone()).expect("nonempty fixture"),
            )
            .expect("service admission")
            .expect("closed cycle");
            assert_eq!(boundary.coedges(), cycle);
        });
    }

    #[test]
    fn closed_boundary_admission_propagates_caller_work_refusal() {
        crate::test_support::with_work_limit(0, |ctx| {
            let error = Boundary::new(ctx, NonEmptyMembers::one(coedge(0, 0, 0)))
                .expect_err("closure scan requires work");
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("work refusal")
            };
            assert_eq!(limit.operation, "catia_closed_boundary_admission");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }
}
