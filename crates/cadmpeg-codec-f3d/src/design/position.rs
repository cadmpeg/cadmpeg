use crate::design::components::paths::LocalOccurrencePaths;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::features::{
    CapturedComponentPosition, CapturedPositionTransform, Feature, FeatureDefinition,
    FeatureOperation,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

const DESIGN_LENGTH_TO_MILLIMETRE: f64 = 10.0;

pub(crate) fn project(
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Option<FeatureDefinition> {
    let captured = scope.captured_positions.as_ref()?;
    let native_scope = cadmpeg_ir::ids::NativeId::mint(scope.id.clone()).ok()?;
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let positions = captured
        .rows
        .iter()
        .enumerate()
        .map(|(ordinal, position)| {
            let affine = if let Some(matrix) = &position.matrix {
                let values = matrix.value.map(cadmpeg_ir::scalar::FiniteReal::get);
                if values[12..] == [0.0, 0.0, 0.0, 1.0] {
                    let mut rows = std::array::from_fn(|r| std::array::from_fn(|c| values[r * 4 + c]));
                    for row in &mut rows {
                        row[3] *= DESIGN_LENGTH_TO_MILLIMETRE;
                    }
                    Transform::affine(rows)
                } else {
                    None
                }
            } else {
                Some(Transform::identity())
            };
            let transform = match affine {
                Some(transform) => CapturedPositionTransform::Affine { transform },
                None => {
                    losses.push(F3dLossCode::PositionTransformUnprojected.note(format!(
                        "Position scope {} row {ordinal} retains its native matrix coefficients because the bottom row is non-affine or centimetre-to-millimetre translation overflows.", scope.id
                    )));
                    CapturedPositionTransform::Native { scope: native_scope.clone(), row: ordinal }
                }
            };
            let selector = cadmpeg_ir::ids::NativeId::mint(format!(
                "{stream}:design-record#{}",
                position.target.value
            ))
            .ok()?;
            Some(CapturedComponentPosition { selector, occurrence: None, transform })
        })
        .collect::<Option<Vec<_>>>();
    let positions = positions?;
    losses.push(F3dLossCode::PositionSemanticsAssumed.note(format!(
        "Position scope {} assumes that its ordered matrices capture independent occurrence poses in each occurrence's assembly context, with centimetre translations; captured-position semantics and frame remain inferred (DR-550).", scope.id
    )));
    if captured
        .rows
        .iter()
        .any(|position| position.matrix.is_none())
    {
        losses.push(F3dLossCode::PositionIdentityAssumed.note(format!(
            "Position scope {} assumes identity for each omitted-matrix marker (DR-550).",
            scope.id
        )));
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::CapturedComponentPositions { positions },
    ))
}

pub(crate) fn bind_occurrences(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    paths: &LocalOccurrencePaths,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let captured_scopes = scopes
        .iter()
        .filter(|scope| scope.captured_positions.is_some())
        .map(|scope| (scope.id.as_str(), scope))
        .collect::<std::collections::BTreeMap<_, _>>();
    for feature in features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| captured_scopes.get(id))
        else {
            continue;
        };
        let Some(captured) = &scope.captured_positions else {
            continue;
        };
        let mut definition = feature.evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::CapturedComponentPositions {
            positions,
        }) = &mut definition
        else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let bindings = captured
            .paths
            .iter()
            .map(|path| {
                if !path.occurrence_lists().is_empty() {
                    return None;
                }
                let keys = path
                    .occurrence_guids()
                    .iter()
                    .map(|key| key.value.as_str().to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let (id, consumed) = paths.resolve(stream, &keys)?;
                (consumed == keys.len()).then_some(id)
            })
            .collect::<Vec<_>>();
        for (ordinal, position) in positions.iter_mut().enumerate() {
            position.occurrence = captured
                .rows
                .get(ordinal)
                .and_then(|row| row.path_index)
                .and_then(|index| bindings.get(index))
                .cloned()
                .flatten();
            if position.occurrence.is_none() {
                losses.push(F3dLossCode::PositionOccurrenceUnresolved.note(format!(
                    "Position scope {} row {ordinal} retains selector {} and its transform without a unique complete occurrence-path binding.", scope.id, position.selector.as_str()
                )));
            }
        }
        feature.evaluation.set_definition(definition);
    }
    losses
}

#[cfg(test)]
mod tests;
