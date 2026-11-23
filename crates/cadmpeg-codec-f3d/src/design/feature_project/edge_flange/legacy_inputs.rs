use super::{boundary, native_stream, scalar, unique_feature_match, unresolved, ProjectInputs};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::sheet_metal::{
    DesignEdgeFlangeHeightExtent, DesignEdgeFlangeShape, DesignEdgeFlangeWidthParameterSource,
};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::features::selections::NonEmptyMembers;
use cadmpeg_ir::features::sheet_metal::{
    SheetMetalFlangeOffset, SheetMetalFlangeSymmetricWidth, SheetMetalFlangeTargetConstruction,
};
use cadmpeg_ir::features::sheet_metal_forms::{
    SheetMetalFlangeEdgeWidths, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
    SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Length, PositiveLength};

pub(super) fn height(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Option<SheetMetalFlangeHeight> {
    let operation = scope.edge_flange_operation()?;
    match &operation.selection.shape().height() {
        DesignEdgeFlangeHeightExtent::Distance => scalar(
            scope,
            Some(operation.height_owner_record_index),
            "FlangeHeight",
            inputs,
            losses,
        )
        .and_then(PositiveLength::new)
        .map(SheetMetalFlangeHeight::Distance),
        DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index,
            target_operand_record_index,
            offset_owner_record_index,
            ..
        } => {
            let selection =
                unique_feature_match(inputs.entity_selection_operands.iter().filter(|operand| {
                    native_stream(&operand.id) == native_stream(&scope.id)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == *target_group_record_index
                        && operand.record_index() == *target_operand_record_index
                }));
            let target = selection.and_then(|selection| {
                let index = u32::try_from(selection.primary_identity).ok()?.checked_add(1)?;
                unique_feature_match(inputs.scopes.iter().filter(|candidate| {
                    native_stream(&candidate.id) == native_stream(&scope.id) && candidate.record_index == index
                        && matches!(candidate.kind(), DesignFeatureKind::WorkPlane | DesignFeatureKind::WorkPoint)
                })).map(|candidate| SheetMetalFlangeHeightTarget::Feature(crate::ids::neutral_feature_id(candidate)))
            }).unwrap_or_else(|| {
                let (code, source) = selection.map_or_else(
                    || (F3dLossCode::EdgeHeightTargetScopeAssumed, scope.id.clone()),
                    |selection| (F3dLossCode::EdgeHeightTargetNative, selection.id.clone()),
                );
                losses.push(code.note(format!("EdgeFlange scope {} retains its unresolved height target through source {}.", scope.id, source)));
                SheetMetalFlangeHeightTarget::Native(source)
            });
            let offset = scalar(
                scope,
                Some(*offset_owner_record_index),
                "ToObjectOffset",
                inputs,
                losses,
            )
            .and_then(Length::new);
            if offset.is_none() {
                unresolved(
                    scope,
                    losses,
                    F3dLossCode::EdgeParameterUnresolved,
                    "target offset",
                );
            }
            Some(SheetMetalFlangeHeight::ToObject(
                SheetMetalFlangeTargetConstruction {
                    target: Some(target),
                    offset,
                },
            ))
        }
    }
}

pub(super) fn width(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<SheetMetalFlangeWidth>, CodecError> {
    let Some(operation) = scope.edge_flange_operation() else {
        return Ok(None);
    };
    let positive = |index, kind, losses: &mut Vec<LossNote>| {
        scalar(scope, Some(index), kind, inputs, losses).and_then(PositiveLength::new)
    };
    let count = operation.selection.shape().edges().count();
    let value = match operation.selection.shape() {
        DesignEdgeFlangeShape::FullEdge { .. } => Some(SheetMetalFlangeWidth::FullEdge),
        DesignEdgeFlangeShape::Symmetric { owner, .. } => {
            let width = positive(*owner, "EdgeWidth", losses);
            Some(match width {
                Some(width) => SheetMetalFlangeWidth::symmetric(width),
                None => {
                    unresolved(
                        scope,
                        losses,
                        F3dLossCode::EdgeParameterUnresolved,
                        "symmetric width",
                    );
                    SheetMetalFlangeWidth::Symmetric(SheetMetalFlangeSymmetricWidth::default())
                }
            })
        }
        DesignEdgeFlangeShape::SymmetricPerEdge(edges) => {
            let mut rows = ctx.alloc_filled(
                count,
                SheetMetalFlangeSymmetricWidth::default(),
                "projecting per-edge symmetric widths",
            )?;
            for (row, edge) in rows.iter_mut().zip(edges) {
                row.width = positive(edge.owners, "EdgeWidth", losses);
                if row.width.is_none() {
                    unresolved(
                        scope,
                        losses,
                        F3dLossCode::EdgeParameterUnresolved,
                        "per-edge symmetric width",
                    );
                }
            }
            NonEmptyMembers::try_from(rows)
                .ok()
                .map(|widths| SheetMetalFlangeWidth::SymmetricPerEdge { widths })
        }
        DesignEdgeFlangeShape::TwoSidesPerEdge {
            edges,
            source: DesignEdgeFlangeWidthParameterSource::EdgeOffset,
        } => {
            let mut rows = ctx.alloc_filled(
                count,
                SheetMetalFlangeOffset::default(),
                "projecting legacy signed offsets",
            )?;
            for (row, edge) in rows.iter_mut().zip(edges) {
                row.first = scalar(scope, Some(edge.owners[0]), "EdgeOffset_1", inputs, losses)
                    .and_then(Length::new);
                row.second = scalar(scope, Some(edge.owners[1]), "EdgeOffset_2", inputs, losses)
                    .and_then(Length::new);
                row.first_reference = boundary(scope, None, inputs, losses);
                row.second_reference = boundary(scope, None, inputs, losses);
                if row.first.is_none() || row.second.is_none() {
                    unresolved(
                        scope,
                        losses,
                        F3dLossCode::EdgeParameterUnresolved,
                        "legacy edge offset",
                    );
                }
            }
            losses.push(F3dLossCode::EdgeSignedOffsetsAssumed.note(format!(
                "EdgeFlange scope {} reads signed EdgeOffset pairs in source edge order.",
                scope.id
            )));
            NonEmptyMembers::try_from(rows)
                .ok()
                .map(|offsets| SheetMetalFlangeWidth::TwoOffsetsPerEdge { offsets })
        }
        DesignEdgeFlangeShape::TwoSides { owners, .. } => {
            let first = positive(owners[0], "EdgeWidth_1", losses);
            let second = positive(owners[1], "EdgeWidth_2", losses);
            if first.is_none() || second.is_none() {
                unresolved(
                    scope,
                    losses,
                    F3dLossCode::EdgeParameterUnresolved,
                    "two-sided width",
                );
            }
            Some(SheetMetalFlangeWidth::TwoSides(
                SheetMetalFlangeTwoSidedWidth { first, second },
            ))
        }
        DesignEdgeFlangeShape::TwoSidesPerEdge {
            edges,
            source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
        } => {
            let mut rows = ctx.alloc_filled(
                count,
                SheetMetalFlangeTwoSidedWidth::default(),
                "projecting per-edge two-sided widths",
            )?;
            for (row, edge) in rows.iter_mut().zip(edges) {
                row.first = positive(edge.owners[0], "EdgeWidth_1", losses);
                row.second = positive(edge.owners[1], "EdgeWidth_2", losses);
                if row.first.is_none() || row.second.is_none() {
                    unresolved(
                        scope,
                        losses,
                        F3dLossCode::EdgeParameterUnresolved,
                        "per-edge two-sided width",
                    );
                }
            }
            SheetMetalFlangeEdgeWidths::new(rows)
                .ok()
                .map(|widths| SheetMetalFlangeWidth::TwoSidesPerEdge { widths })
        }
    };
    Ok(value)
}
