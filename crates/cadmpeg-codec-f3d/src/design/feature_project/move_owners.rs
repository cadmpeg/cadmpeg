// SPDX-License-Identifier: Apache-2.0

use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;

const EPS_MOVE_PARAMETER_AGREEMENT: f64 = 1.0e-9;

/// The translation-distance or rotation-angle owners of a `Move` scope, and
/// whether their values agree with the stored matrix. `None` when the scope
/// references no such owner set or its direction code words have no
/// agreement rule.
pub(super) fn move_parameter_owners(
    scope: &DesignParameterScope,
    operation: &crate::records::feature::direct_face::DesignMoveOperation,
    owners: &[DesignParameterOwner],
) -> Option<(Vec<u32>, bool)> {
    use crate::records::feature::direct_face::DesignMoveForm;

    let stream = native_stream(&scope.id)?;
    let mut values = owners
        .iter()
        .filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
                && scope
                    .reference_members()
                    .values()
                    .any(|index| *index == owner.record_index())
        })
        .map(|owner| {
            (
                owner.local_ordinal(),
                owner.record_index(),
                owner.evaluated_value().get(),
            )
        })
        .collect::<Vec<_>>();
    values.sort_unstable_by_key(|&(ordinal, record_index, _)| (ordinal, record_index));
    let transform = &operation.transform;
    let agrees = match (operation.form, values.as_slice()) {
        (DesignMoveForm::Form0, [(0, _, x), (1, _, y), (2, _, z)]) => {
            let translation = [transform[0][3], transform[1][3], transform[2][3]];
            match operation.prefix.as_ref().map(|prefix| prefix.codes) {
                None | Some([0, 0]) => {
                    [*x, *y, *z]
                        .iter()
                        .zip(translation)
                        .all(|(value, component)| {
                            (value - component).abs() <= EPS_MOVE_PARAMETER_AGREEMENT
                        })
                }
                Some([0, 1]) => {
                    (length_of([*x, *y, *z]) - length_of(translation)).abs()
                        <= EPS_MOVE_PARAMETER_AGREEMENT
                }
                Some(_) => return None,
            }
        }
        (DesignMoveForm::Form1, [(0, _, angle)]) => {
            let skew = [
                transform[2][1] - transform[1][2],
                transform[0][2] - transform[2][0],
                transform[1][0] - transform[0][1],
            ];
            let rotation = (length_of(skew) / 2.0)
                .atan2((transform[0][0] + transform[1][1] + transform[2][2] - 1.0) / 2.0);
            let turned = angle.rem_euclid(std::f64::consts::TAU);
            (turned - rotation).abs() <= EPS_MOVE_PARAMETER_AGREEMENT
                || (std::f64::consts::TAU - turned - rotation).abs() <= EPS_MOVE_PARAMETER_AGREEMENT
        }
        _ => return None,
    };
    Some((
        values
            .iter()
            .map(|&(_, record_index, _)| record_index)
            .collect(),
        agrees,
    ))
}

fn length_of(vector: [f64; 3]) -> f64 {
    vector.iter().map(|value| value * value).sum::<f64>().sqrt()
}
