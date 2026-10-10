// SPDX-License-Identifier: Apache-2.0
//! Stack candidates for the fixed one-edge and two-edge flange grammars.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

use crate::records::feature::sheet_metal::{
    DesignBendPosition, DesignEdgeFlangeEdge, DesignEdgeFlangeHeightExtent,
    DesignEdgeFlangeOperation, DesignEdgeFlangeSelection, DesignEdgeFlangeShape,
    DesignEdgeFlangeWidthParameterSource, DesignEdgeWidthMode, DesignFlangeEdgeWidth,
    DesignSheetMetalHeightDatum,
};

pub(super) enum FixedEdges {
    One(DesignEdgeFlangeEdge),
    Two([DesignEdgeFlangeEdge; 2]),
}

impl FixedEdges {
    pub(super) fn from_columns(
        wrappers: &[u32],
        groups: &[u32],
        aggregate_operands: &[u32],
    ) -> Option<Self> {
        let edge = |wrapper, group: u32, aggregate_operand_record_index| {
            Some(DesignEdgeFlangeEdge {
                wrapper,
                group_record_index: group.try_into().ok()?,
                aggregate_operand_record_index,
            })
        };
        match (wrappers, groups, aggregate_operands) {
            ([wrapper], [group], [aggregate]) => {
                Some(Self::One(edge(*wrapper, *group, *aggregate)?))
            }
            ([w0, w1], [g0, g1], [a0, a1]) => {
                Some(Self::Two([edge(*w0, *g0, *a0)?, edge(*w1, *g1, *a1)?]))
            }
            _ => None,
        }
    }

    fn retain(self, ctx: &DecodeContext<'_>) -> Result<Vec<DesignEdgeFlangeEdge>, CodecError> {
        match self {
            Self::One(edge) => {
                let mut out = ctx.vector_storage(1, "F3D fixed flange edges")?;
                ctx.push_vec(&mut out, edge, "F3D fixed flange edges")?;
                Ok(out)
            }
            Self::Two(edges) => {
                let mut out = ctx.vector_storage(2, "F3D fixed flange edges")?;
                for edge in edges {
                    ctx.push_vec(&mut out, edge, "F3D fixed flange edges")?;
                }
                Ok(out)
            }
        }
    }
}

pub(super) enum FixedShape {
    FullEdge {
        edges: FixedEdges,
        height: DesignEdgeFlangeHeightExtent,
    },
    Symmetric {
        edges: FixedEdges,
        owner: u32,
    },
    TwoSides {
        edges: FixedEdges,
        owners: [u32; 2],
    },
    SymmetricPerEdge([DesignFlangeEdgeWidth<u32>; 2]),
    TwoSidesPerEdge {
        edges: [DesignFlangeEdgeWidth<[u32; 2]>; 2],
        source: DesignEdgeFlangeWidthParameterSource,
    },
}

impl FixedShape {
    pub(super) fn distance(
        edges: FixedEdges,
        width_mode: Option<DesignEdgeWidthMode>,
        owners: &[u32],
        source: DesignEdgeFlangeWidthParameterSource,
    ) -> Option<Self> {
        let mode = width_mode.unwrap_or(match owners.len() {
            0 => DesignEdgeWidthMode::FullEdge,
            1 => DesignEdgeWidthMode::Symmetric,
            _ => DesignEdgeWidthMode::TwoSides,
        });
        if source == DesignEdgeFlangeWidthParameterSource::EdgeOffset
            && mode != DesignEdgeWidthMode::TwoSidesPerEdge
        {
            return None;
        }
        match (mode, owners, edges) {
            (DesignEdgeWidthMode::FullEdge, [], edges) => Some(Self::FullEdge {
                edges,
                height: DesignEdgeFlangeHeightExtent::Distance,
            }),
            (DesignEdgeWidthMode::Symmetric, [owner], edges) => Some(Self::Symmetric {
                edges,
                owner: *owner,
            }),
            (DesignEdgeWidthMode::TwoSides, [first, second], edges) => Some(Self::TwoSides {
                edges,
                owners: [*first, *second],
            }),
            (DesignEdgeWidthMode::SymmetricPerEdge, [first, second], FixedEdges::Two([e0, e1])) => {
                Some(Self::SymmetricPerEdge([
                    DesignFlangeEdgeWidth {
                        edge: e0,
                        owners: *first,
                    },
                    DesignFlangeEdgeWidth {
                        edge: e1,
                        owners: *second,
                    },
                ]))
            }
            (DesignEdgeWidthMode::TwoSidesPerEdge, [a, b, c, d], FixedEdges::Two([e0, e1])) => {
                Some(Self::TwoSidesPerEdge {
                    edges: [
                        DesignFlangeEdgeWidth {
                            edge: e0,
                            owners: [*a, *b],
                        },
                        DesignFlangeEdgeWidth {
                            edge: e1,
                            owners: [*c, *d],
                        },
                    ],
                    source,
                })
            }
            _ => None,
        }
    }

    fn single_edge(&self) -> Option<&DesignEdgeFlangeEdge> {
        match self {
            Self::FullEdge {
                edges: FixedEdges::One(edge),
                ..
            }
            | Self::Symmetric {
                edges: FixedEdges::One(edge),
                ..
            }
            | Self::TwoSides {
                edges: FixedEdges::One(edge),
                ..
            } => Some(edge),
            _ => None,
        }
    }

    fn retain(self, ctx: &DecodeContext<'_>) -> Result<DesignEdgeFlangeShape, CodecError> {
        Ok(match self {
            Self::FullEdge { edges, height } => DesignEdgeFlangeShape::FullEdge {
                edges: edges.retain(ctx)?,
                height,
            },
            Self::Symmetric { edges, owner } => DesignEdgeFlangeShape::Symmetric {
                edges: edges.retain(ctx)?,
                owner,
            },
            Self::TwoSides { edges, owners } => DesignEdgeFlangeShape::TwoSides {
                edges: edges.retain(ctx)?,
                owners,
            },
            Self::SymmetricPerEdge(edges) => {
                let mut out = ctx.vector_storage(2, "F3D fixed flange width edges")?;
                for edge in edges {
                    ctx.push_vec(&mut out, edge, "F3D fixed flange width edges")?;
                }
                DesignEdgeFlangeShape::SymmetricPerEdge(out)
            }
            Self::TwoSidesPerEdge { edges, source } => {
                let mut out = ctx.vector_storage(2, "F3D fixed flange width edges")?;
                for edge in edges {
                    ctx.push_vec(&mut out, edge, "F3D fixed flange width edges")?;
                }
                DesignEdgeFlangeShape::TwoSidesPerEdge { edges: out, source }
            }
        })
    }
}

pub(super) enum FixedAuxiliaryReferences {
    None,
    Extended([u32; 12]),
}

pub(super) struct EdgeFlangeFrame {
    pub(super) shape: FixedShape,
    pub(super) aggregate_group_record_index: u32,
    pub(super) height_owner_record_index: u32,
    pub(super) angle_owner_record_index: u32,
    pub(super) auxiliary_references: FixedAuxiliaryReferences,
    pub(super) settings_record_index: u32,
    pub(super) bend_radius: PositiveReal,
    pub(super) bend_radius_offset: u64,
    pub(super) height_datum: DesignSheetMetalHeightDatum,
    pub(super) bend_position: DesignBendPosition,
}

impl EdgeFlangeFrame {
    pub(super) fn retain(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<DesignEdgeFlangeOperation>, CodecError> {
        ctx.charge_work(0, "F3D fixed flange operation")?;
        if self.shape.single_edge().is_some_and(|edge| {
            Some(edge.aggregate_operand_record_index)
                != self.aggregate_group_record_index.checked_add(3)
        }) {
            return Ok(None);
        }
        let (operation, storage) = ctx.with_scoped_storage(
            "F3D fixed flange operation",
            || -> Result<Option<DesignEdgeFlangeOperation>, CodecError> {
                let shape = self.shape.retain(ctx)?;
                let Ok(selection) =
                    DesignEdgeFlangeSelection::try_new(shape, self.aggregate_group_record_index)
                else {
                    return Ok(None);
                };
                let auxiliary_reference_record_indices = match self.auxiliary_references {
                    FixedAuxiliaryReferences::None => Vec::new(),
                    FixedAuxiliaryReferences::Extended(references) => {
                        let mut out =
                            ctx.vector_storage(12, "F3D fixed flange auxiliary references")?;
                        for reference in references {
                            ctx.push_vec(
                                &mut out,
                                reference,
                                "F3D fixed flange auxiliary references",
                            )?;
                        }
                        out
                    }
                };
                Ok(Some(DesignEdgeFlangeOperation {
                    selection,
                    height_owner_record_index: self.height_owner_record_index,
                    angle_owner_record_index: self.angle_owner_record_index,
                    auxiliary_reference_record_indices,
                    settings_record_index: self.settings_record_index,
                    bend_radius: self.bend_radius,
                    bend_radius_offset: self.bend_radius_offset,
                    height_datum: self.height_datum,
                    bend_position: self.bend_position,
                }))
            },
        )?;
        match operation {
            Some(operation) => Ok(Some(storage.commit_value(operation)?)),
            None => Ok(None),
        }
    }
}
