// SPDX-License-Identifier: Apache-2.0
//! Exact fixed extrude, fillet and chamfer parameter scopes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use super::shared_frames::FixedScalarFrame;
use crate::bytes::lp_ascii_filtered;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::fixed_parameters::DesignFixedChamferDistance;
use crate::records::feature::fixed_parameters::DesignFixedChamferParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeDistance;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar;
use crate::records::feature::fixed_parameters::DesignFixedFilletGroup;
use crate::records::feature::fixed_parameters::DesignFixedFilletParameters;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;

pub(super) fn exact_fixed_extrude_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignFixedExtrudeParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Extrude)
        || scope
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent)
            != Some(DesignExtrudeExtent::OneSidedDistance)
    {
        return None;
    }
    let fixed_lanes = scope
        .reference_members()
        .values()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    let embedded_distances = scope
        .reference_members()
        .values()
        .filter_map(|record_index| {
            exact_embedded_extrude_distance(bytes, records, *record_index, scope.record_index)
                .map(|scalar| (*record_index, scalar))
        })
        .collect::<Vec<_>>();
    if fixed_lanes.len() > 2 || embedded_distances.len() > 1 {
        return None;
    }
    let mut along_distance = embedded_distances.first().map(|(record_index, lane)| {
        DesignFixedExtrudeDistance::DistanceConstruction(DesignFixedExtrudeScalar {
            value: lane.value,
            record_index: *record_index,
            value_offset: lane.value_offset,
        })
    });
    let mut taper_angle = None;
    let mut seen_fixed_ordinals = [false; 2];
    for (record_index, lane) in fixed_lanes {
        let ordinal = usize::from(lane.ordinal);
        if ordinal >= seen_fixed_ordinals.len() || seen_fixed_ordinals[ordinal] {
            return None;
        }
        seen_fixed_ordinals[ordinal] = true;
        let scalar = DesignFixedExtrudeScalar {
            value: lane.value,
            record_index,
            value_offset: lane.value_offset,
        };
        let source_kind = parameter_owners
            .iter()
            .find(|owner| {
                native_stream(owner.id()) == native_stream(&scope.id)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == record_index
            })
            .and_then(|owner| {
                parameters
                    .iter()
                    .find(|parameter| {
                        native_stream(&parameter.id) == native_stream(&scope.id)
                            && parameter.record_index == owner.parameter_record_index()
                    })
                    .map(crate::records::parameters::DesignParameter::source_kind)
            });
        match source_kind {
            Some("AlongDistance") if lane.value != 0.0 && along_distance.is_none() => {
                along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(scalar));
            }
            Some("TaperAngle") if taper_angle.is_none() => taper_angle = Some(scalar),
            Some("AlongDistance") if along_distance.is_some() && lane.value == 0.0 => {}
            Some(_) => return None,
            None => match lane.ordinal {
                0 if lane.value != 0.0 && along_distance.is_none() => {
                    along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(scalar));
                }
                0 if along_distance.is_some() && lane.value == 0.0 => {}
                1 if taper_angle.is_none() => taper_angle = Some(scalar),
                _ => return None,
            },
        }
    }
    if along_distance.is_none() && taper_angle.is_none() {
        return None;
    }
    Some(DesignFixedExtrudeParameters {
        along_distance,
        taper_angle,
    })
}

fn exact_embedded_extrude_distance(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<FixedScalarFrame> {
    let candidates = records
        .frames(record_index)
        .filter_map(|(start, end)| {
            (end.checked_sub(start)? == 100).then_some(())?;
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            let first_auxiliary = record_index.checked_add(1)?;
            let second_auxiliary = record_index.checked_add(2)?;
            if after_tag != start + 7
                || class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || bytes.get(start + 11..start + 21) != Some(&[0; 10])
                || marked_record_reference(bytes, start + 21)? != scope_record_index
                || bytes.get(start + 26..start + 32) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 32)? != 1
                || marked_record_reference(bytes, start + 36).is_none()
                || bytes.get(start + 41..start + 47) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 47)? != 210
                || View::u32_le_at(bytes, start + 59)? != 210
                || marked_record_reference(bytes, start + 63)? != second_auxiliary
                || bytes.get(start + 68..start + 74) != Some(&[0; 6])
                || bytes.get(start + 74..start + 77) != Some(&[1, 0, 0])
                || marked_record_reference(bytes, start + 77)? != first_auxiliary
                || bytes.get(start + 82..start + 89) != Some(&[0; 7])
                || marked_record_reference(bytes, start + 89)? != scope_record_index
                || bytes.get(start + 94..start + 100) != Some(&[0; 6])
            {
                return None;
            }
            let value = View::f64_le_at(bytes, start + 51)?;
            (value.is_finite() && value > 0.0).then_some(FixedScalarFrame {
                owner_record_index: Some(scope_record_index),
                ordinal: 0,
                value,
                value_offset: u64::try_from(start + 51).ok()?,
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn exact_fixed_fillet_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignFixedFilletParameters> {
    use crate::records::feature::fixed_parameters::{
        DesignFixedFilletIntermediate, DesignFixedFilletLaw, DesignFixedFilletScalar,
    };
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Fillet) {
        return None;
    }
    let lanes = scope
        .reference_members()
        .values()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    if lanes.is_empty()
        || lanes
            .iter()
            .enumerate()
            .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
    {
        return None;
    }

    let scalar = |(record_index, scalar): &(u32, FixedScalarFrame)| DesignFixedFilletScalar {
        value: scalar.value,
        record_index: *record_index,
        value_offset: scalar.value_offset,
    };
    let group = |tangency_lane: Option<&(u32, FixedScalarFrame)>, law: DesignFixedFilletLaw| {
        DesignFixedFilletGroup::try_new(tangency_lane.map(scalar), law).ok()
    };
    let groups = if lanes.len() == 1 {
        vec![group(
            None,
            DesignFixedFilletLaw::Constant(scalar(&lanes[0])),
        )?]
    } else if lanes.len() % 2 == 0 {
        lanes
            .chunks_exact(2)
            .map(|pair| {
                group(
                    Some(&pair[0]),
                    DesignFixedFilletLaw::Constant(scalar(&pair[1])),
                )
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        vec![group(
            Some(&lanes[0]),
            DesignFixedFilletLaw::Variable {
                start: scalar(&lanes[1]),
                end: scalar(&lanes[2]),
                intermediate: lanes[3..]
                    .chunks_exact(2)
                    .map(|pair| DesignFixedFilletIntermediate {
                        radius: scalar(&pair[0]),
                        parameter: scalar(&pair[1]),
                    })
                    .collect(),
            },
        )?]
    };
    Some(DesignFixedFilletParameters { groups })
}

pub(super) fn exact_fixed_chamfer_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFixedChamferParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Chamfer) {
        return None;
    }
    let stream = native_stream(&scope.id);
    if parameter_owners.iter().any(|owner| {
        stream.is_some()
            && native_stream(owner.id()) == stream
            && owner.scope_record_index() == scope.record_index
    }) {
        return None;
    }
    let lanes = scope
        .reference_members()
        .values()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    if !(1..=2).contains(&lanes.len())
        || lanes
            .iter()
            .enumerate()
            .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
        || lanes.iter().any(|(_, scalar)| scalar.value <= 0.0)
    {
        return None;
    }
    let mut distances =
        lanes
            .into_iter()
            .map(|(record_index, scalar)| DesignFixedChamferDistance {
                value: scalar.value,
                record_index,
                value_offset: scalar.value_offset,
            });
    let first = distances.next()?;
    Some(match distances.next() {
        Some(second) => DesignFixedChamferParameters::TwoDistances { first, second },
        None => DesignFixedChamferParameters::EqualDistance { distance: first },
    })
}

pub(super) fn unique_revolve_angle_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: Option<u32>,
) -> Option<&'a DesignParameterOwner> {
    let mut candidates = parameter_owners.iter().filter(|owner| {
        native_stream(owner.id()) == native_stream(&scope.id)
            && owner.scope_record_index() == scope.record_index
            && scope
                .reference_members()
                .values()
                .any(|value| value == &owner.record_index())
            && record_index.is_none_or(|index| owner.record_index() == index)
            && owner.local_ordinal() == 0
            && owner.evaluated_value().is_finite()
            && owner.evaluated_value() > 0.0
    });
    let angle = candidates.next()?;
    candidates.next().is_none().then_some(angle)
}
