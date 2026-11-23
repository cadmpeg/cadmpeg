use crate::bytes::reference_fields::ReadField;
use crate::design::decode::scopes::point_data::POINT_DATA_TYPE_GUID;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_axes::{
    AxisEndpointReading, AxisReferenceRun, DesignWorkAxisCandidate, DesignWorkAxisSource,
};
use cadmpeg_ir::design::construction::{
    AnyConstruction, Construction, ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::features::datums::DatumAxisDraft;
use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, Length};
use std::collections::HashMap;

const EPS_ENDPOINT_CM: f64 = 1.0e-9;
const CM_TO_MM: f64 = 10.0;

fn values<const N: usize>(fields: &[ReadField<FiniteReal>; N]) -> Option<[f64; N]> {
    fields
        .iter()
        .map(|field| field.value().copied().map(FiniteReal::get))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn point(values: [f64; 3]) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(
        values[0] * CM_TO_MM,
        values[1] * CM_TO_MM,
        values[2] * CM_TO_MM,
    ))
}

fn endpoint(readings: &[AxisEndpointReading]) -> Option<[f64; 3]> {
    let mut eligible = readings
        .iter()
        .filter(|reading| reading.complete || readings.len() == 1);
    let first = values(&eligible.next()?.position)?;
    eligible.next().is_none().then_some(first)
}

fn inputs(
    candidate: &DesignWorkAxisCandidate,
    run: &AxisReferenceRun,
) -> (Vec<ConstructionInput>, Vec<Option<[f64; 3]>>) {
    let mut positions = HashMap::new();
    for evidence in candidate.endpoints.iter().filter(|v| v.unique_frame) {
        let position = endpoint(&evidence.readings);
        positions
            .entry(evidence.record_index)
            .and_modify(|known| {
                if *known != position {
                    *known = None;
                }
            })
            .or_insert(position);
    }
    let mut points = Vec::new();
    let inputs = run
        .references
        .iter()
        .map(|reference| {
            let position = reference
                .local()
                .filter(|(_, inline)| {
                    inline.is_none_or(|guid| guid.eq_ignore_ascii_case(POINT_DATA_TYPE_GUID))
                })
                .and_then(|(target, _)| u32::try_from(target).ok())
                .and_then(|target| positions.get(&target).copied().flatten());
            points.push(position);
            ConstructionInput {
                role: None,
                reference: position.and_then(point).map(|position| {
                    InputReference::Built(Box::new(AnyConstruction::Point(Construction {
                        resolved: Some(position),
                        ..Default::default()
                    })))
                }),
                value: None,
                reversed: None,
            }
        })
        .collect();
    (inputs, points)
}

fn scalar_option(
    options: &mut NativeOptions,
    key: cadmpeg_core::text::NonBlankString,
    value: ReadField<FiniteReal>,
) {
    if let ReadField::Value(value) = value {
        options.insert(key, NativeOption::Real(Dimension::Literal(value)));
    }
}

fn integer_option(
    options: &mut NativeOptions,
    key: cadmpeg_core::text::NonBlankString,
    value: ReadField<u32>,
) {
    if let ReadField::Value(value) = value {
        options.insert(
            key,
            NativeOption::Integer(Dimension::Literal(i64::from(value))),
        );
    }
}

pub(super) fn project(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, cadmpeg_core::CodecError> {
    let Some(evidence) = scope.work_axis_construction() else {
        return Ok(FeatureDefinition::Operation(
            DatumAxisDraft::default()
                .into_operation()
                .map_err(cadmpeg_core::CodecError::malformed)?,
        ));
    };
    let [candidate] = evidence.candidates.as_slice() else {
        if evidence.candidates.len() > 1 {
            losses.push(F3dLossCode::FeatureWorkAxisCandidatesUnresolved.note(format!("WorkAxis scope {} retains {} independently attributable carrier readings; their competition leaves the neutral line and construction unresolved.", scope.id, evidence.candidates.len())));
        }
        return Ok(FeatureDefinition::Operation(
            DatumAxisDraft::default()
                .into_operation()
                .map_err(cadmpeg_core::CodecError::malformed)?,
        ));
    };
    let run = match &candidate.source {
        Some(
            DesignWorkAxisSource::PointDirection { inputs, .. }
            | DesignWorkAxisSource::TwoPoint { inputs, .. },
        ) => Some(inputs),
        _ => None,
    };
    let count = run.map_or(0, |v| v.references.len());
    let width = std::mem::size_of::<ConstructionInput>()
        + std::mem::size_of::<Option<[f64; 3]>>()
        + std::mem::size_of::<AnyConstruction>();
    let bytes = count
        .checked_mul(width)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("WorkAxis input allocation overflow"))?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes),
        "WorkAxis neutral inputs",
    )?;
    let mut axis = DatumAxisDraft::default();
    let leading_layout_assumed =
        matches!(candidate.prefix.leading_block, ReadField::Value(value) if value != 0);
    let legacy_version_assumed = matches!(
        candidate.source,
        Some(DesignWorkAxisSource::TwoPoint { .. } | DesignWorkAxisSource::DirectCarrier { .. })
    ) && candidate
        .version
        .value()
        .is_some_and(|version| !matches!(version, 1 | 4));
    let origin = values(&candidate.origin);
    let displacement = values(&candidate.displacement);
    let mut geometry = true;
    let mut two_point = None;
    let mut parameters_zero = true;
    match &candidate.source {
        None => {}
        Some(DesignWorkAxisSource::DirectCarrier {
            value_count,
            reserved,
            reference_count,
            reference_preamble,
            ..
        }) => {
            geometry = *value_count == ReadField::Value(8)
                && values(reserved) == Some([0.0, 0.0])
                && *reference_count == ReadField::Value(6)
                && *reference_preamble == ReadField::Value(1);
        }
        Some(DesignWorkAxisSource::TwoPoint {
            value_count,
            reserved,
            inputs: run,
        }) => {
            geometry = *value_count == ReadField::Value(8) && values(reserved) == Some([0.0, 0.0]);
            let (known_inputs, points) = inputs(candidate, run);
            axis.inputs = known_inputs;
            two_point = Some((
                run.complete() && run.declared == ReadField::Value(2),
                points,
            ));
        }
        Some(DesignWorkAxisSource::PointDirection {
            bounding_box_index,
            point_parameters,
            reference_type,
            tangent_prefix,
            tangent,
            inputs: run,
        }) => {
            geometry = *bounding_box_index == ReadField::Value(8);
            integer_option(
                &mut axis.native_options,
                cadmpeg_core::nonblank_literal!("f3d:bounding_box_index"),
                *bounding_box_index,
            );
            integer_option(
                &mut axis.native_options,
                cadmpeg_core::nonblank_literal!("f3d:reference_type"),
                *reference_type,
            );
            for (key, value) in [
                cadmpeg_core::nonblank_literal!("f3d:point_parameter_0"),
                cadmpeg_core::nonblank_literal!("f3d:point_parameter_1"),
            ]
            .into_iter()
            .zip(point_parameters)
            {
                scalar_option(&mut axis.native_options, key, *value);
            }
            if let ReadField::Value(prefix) = tangent_prefix {
                integer_option(
                    &mut axis.native_options,
                    cadmpeg_core::nonblank_literal!("f3d:tangent_prefix"),
                    ReadField::Value(u32::from(*prefix)),
                );
            }
            for ((index, value), key) in tangent.iter().enumerate().zip([
                cadmpeg_core::nonblank_literal!("f3d:tangent_0"),
                cadmpeg_core::nonblank_literal!("f3d:tangent_1"),
                cadmpeg_core::nonblank_literal!("f3d:tangent_2"),
            ]) {
                if let ReadField::Value(value) = value {
                    if let Some(length) = Length::new(value.get() * CM_TO_MM) {
                        axis.native_options
                            .insert(key, NativeOption::Length(Dimension::Literal(length)));
                    } else {
                        losses.push(F3dLossCode::FeatureWorkAxisGeometryUnrepresentable.note(format!("WorkAxis scope {} retains tangent component {index} natively because its millimetre value is not finite.", scope.id)));
                    }
                }
            }
            let (known_inputs, points) = inputs(candidate, run);
            axis.inputs = known_inputs;
            parameters_zero = values(point_parameters) == Some([0.0, 0.0]);
            if point_parameters
                .iter()
                .filter_map(ReadField::value)
                .any(|value| value.get() != 0.0)
                && geometry
            {
                losses.push(F3dLossCode::FeatureWorkAxisNonzeroParametersSolvedAssumed.note(format!("WorkAxis scope {} assumes its stored position and direction remain the solved line with nonzero point parameters; all source parameters remain in its native evidence.", scope.id)));
            }
            if *reference_type == ReadField::Value(12) {
                two_point = Some((
                    run.complete() && run.declared == ReadField::Value(2),
                    points,
                ));
            } else if reference_type.value().is_some() {
                losses.push(F3dLossCode::FeatureWorkAxisRuleUnassigned.note(format!("WorkAxis scope {} retains refType {:?} and its input occurrences with unresolved construction roles; its position and direction supply the stored solved line where representable.", scope.id, reference_type.value())));
            }
        }
    }
    if geometry && !candidate.complete && candidate.source.is_some() {
        losses.push(F3dLossCode::FeatureWorkAxisPartialCarrierSolvedAssumed.note(format!("WorkAxis scope {} assumes independently readable stored line components retain their solved meaning after a damaged carrier member; unreadable parts retain the source bytes.", scope.id)));
    }
    if geometry && leading_layout_assumed {
        losses.push(F3dLossCode::FeatureWorkAxisLeadingBlockLayoutAssumed.note(format!("WorkAxis scope {} assumes its nonzero leading-block byte has the prior one-byte extent before the property and stored-line fields; the prologue remains retained as unreadable source evidence.", scope.id)));
    }
    if geometry
        && matches!(
            candidate.source,
            Some(
                DesignWorkAxisSource::TwoPoint { .. } | DesignWorkAxisSource::DirectCarrier { .. }
            )
        )
    {
        if legacy_version_assumed {
            losses.push(F3dLossCode::FeatureWorkAxisUnsupportedVersionLegacyLayoutAssumed.note(format!("WorkAxis scope {} assumes its registered unsupported point-direction revision {:?} retains the independently framed legacy carrier and endpoint layout; the stored line and known operand points follow that alternative reading with unresolved authored roles.", scope.id, candidate.version.value())));
        } else if matches!(
            candidate.source,
            Some(DesignWorkAxisSource::DirectCarrier { .. })
        ) {
            losses.push(F3dLossCode::FeatureWorkAxisLegacyLayoutAssumed.note(format!("WorkAxis scope {} assumes its admitted scope and fixed carrier/support frames identify the legacy solved-axis layout independently of segment-local class spellings.", scope.id)));
        }
    }
    if let Some((arity, points)) = two_point {
        let expected = origin.zip(displacement).map(|(origin, displacement)| {
            [
                origin,
                std::array::from_fn(|index| origin[index] + displacement[index]),
            ]
        });
        let mut deviation: f64 = 0.0;
        let mut comparable = false;
        let corroborated = if let (Some(expected), [Some(first), Some(second)]) =
            (expected, points.as_slice())
        {
            comparable = expected.iter().flatten().all(|value| value.is_finite());
            if comparable {
                deviation = [*first, *second]
                    .iter()
                    .flatten()
                    .zip(expected.iter().flatten())
                    .map(|(actual, expected)| (actual - expected).abs())
                    .fold(0.0, f64::max);
            }
            arity
                && geometry
                && !leading_layout_assumed
                && !legacy_version_assumed
                && comparable
                && parameters_zero
                && displacement
                    .is_some_and(|value| value[0].hypot(value[1]).hypot(value[2]) > f64::EPSILON)
                && deviation <= EPS_ENDPOINT_CM
        } else {
            false
        };
        if corroborated {
            for input in &mut axis.inputs {
                input.role = Some(InputRole {
                    subject: Subject::Whole,
                    relation: Relation::Coincident,
                });
            }
            if deviation > 0.0 {
                losses.push(F3dLossCode::FeatureWorkAxisEndpointToleranceAssumed.note(format!("WorkAxis scope {} assumes its source point operands agree with the stored endpoints within {EPS_ENDPOINT_CM:e} cm; maximum deviation is {deviation:e} cm.", scope.id)));
            }
        } else if geometry {
            let code = if comparable && deviation > EPS_ENDPOINT_CM {
                F3dLossCode::FeatureWorkAxisTwoPointConflictSolvedAssumed
            } else {
                F3dLossCode::FeatureWorkAxisTwoPointOperandsUnresolvedSolvedAssumed
            };
            losses.push(code.note(format!("WorkAxis scope {} assumes its separately stored line remains the solved result while its two-point construction is unresolved; known operand points retain unknown roles.", scope.id)));
        }
    }
    if geometry {
        axis.origin = origin.and_then(point);
        axis.direction = displacement.and_then(|value| {
            let vector = Vector3::new(value[0], value[1], value[2]);
            (value[0].hypot(value[1]).hypot(value[2]) > f64::EPSILON)
                .then(|| cadmpeg_ir::units::UnitVector3::normalized(vector))
                .flatten()
                .map(FeatureDirection3::from)
        });
        if origin.is_some() && axis.origin.is_none()
            || displacement.is_some() && axis.direction.is_none()
        {
            losses.push(F3dLossCode::FeatureWorkAxisGeometryUnrepresentable.note(format!("WorkAxis scope {} retains source geometry whose origin conversion or nonzero direction admission is unresolved; each representable component remains typed.", scope.id)));
        }
    }
    Ok(FeatureDefinition::Operation(
        axis.into_operation()
            .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
    use crate::records::feature::work_axes::{AxisEndpoint, DesignWorkAxisConstruction};

    #[test]
    fn repeated_work_axis_inputs_keep_points_and_unresolved_conflicting_targets() {
        let first = [1.0, 2.0, 3.0];
        let other = [4.0, 5.0, 6.0];
        for readings in [
            [Some(first), Some(other), Some(first)],
            [Some(other), Some(first), Some(first)],
            [Some(first), None, Some(first)],
            [None, Some(first), Some(first)],
        ] {
            let real = |v| ReadField::Value(FiniteReal::new(v).unwrap());
            let reference = |target: u64| {
                let mut bytes = vec![1];
                bytes.extend_from_slice(&target.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
                crate::bytes::reference_fields::read_reference_fields(&bytes, 0)
                    .fields
                    .into_owned()
            };
            let endpoint = |record_index, position: Option<[f64; 3]>| AxisEndpoint {
                record_index,
                unique_frame: true,
                readings: vec![AxisEndpointReading {
                    position: position.map_or([ReadField::Unread; 3], |v| v.map(real)),
                    complete: position.is_some(),
                    ..Default::default()
                }],
            };
            let mut endpoints = vec![endpoint(42, Some(first))];
            endpoints.extend(readings.into_iter().map(|point| endpoint(43, point)));
            let candidate = DesignWorkAxisCandidate {
                origin: [real(0.0); 3],
                displacement: [real(0.0), real(0.0), real(1.0)],
                source: Some(DesignWorkAxisSource::TwoPoint {
                    value_count: ReadField::Value(8),
                    reserved: [real(0.0); 2],
                    inputs: AxisReferenceRun {
                        declared: ReadField::Value(4),
                        references: [42, 43, 42, 43].map(reference).into(),
                    },
                }),
                endpoints,
                complete: true,
                ..Default::default()
            };
            let mut scope = DesignParameterScope::empty(
                "f3d:test/BulkStream.dat:scope#repeated",
                DesignFeatureKind::WorkAxis,
                1,
            );
            let DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() else {
                panic!("axis payload")
            };
            *slot = Some(DesignWorkAxisConstruction {
                candidates: vec![candidate],
                ..Default::default()
            });
            let ctx = cadmpeg_core::decode::test_support::service_decode_context();
            let definition = project(&ctx, &scope, &mut Vec::new()).unwrap();
            let cadmpeg_ir::features::FeatureOperation::DatumAxis { axis } = definition.operation()
            else {
                panic!("datum axis")
            };
            assert_eq!(axis.inputs().len(), 4);
            for (index, input) in axis.inputs().iter().enumerate() {
                assert!(input.role.is_none());
                if index % 2 == 0 {
                    let Some(InputReference::Built(value)) = &input.reference else {
                        panic!("known point")
                    };
                    let AnyConstruction::Point(value) = value.as_ref() else {
                        panic!("point construction")
                    };
                    assert_eq!(value.resolved.unwrap().get(), Point3::new(10.0, 20.0, 30.0));
                } else {
                    assert!(input.reference.is_none());
                }
            }
        }
    }

    #[test]
    fn source_less_solved_and_partial_native_values_project_without_a_source_archive() {
        let real = |v| ReadField::Value(FiniteReal::new(v).unwrap());
        for (origin, direction) in [(true, true), (true, false), (false, true), (false, false)] {
            let candidate = DesignWorkAxisCandidate {
                origin: if origin {
                    [real(1.0), real(2.0), real(3.0)]
                } else {
                    [ReadField::Unread; 3]
                },
                displacement: if direction {
                    [real(0.0), real(0.0), real(2.0)]
                } else {
                    [ReadField::Unread; 3]
                },
                ..Default::default()
            };
            let mut scope = DesignParameterScope::empty(
                "f3d:test/BulkStream.dat:scope#1",
                DesignFeatureKind::WorkAxis,
                1,
            );
            let DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() else {
                panic!("axis payload")
            };
            *slot = Some(DesignWorkAxisConstruction {
                candidates: vec![candidate],
                ..Default::default()
            });
            let mut losses = Vec::new();
            let ctx = cadmpeg_core::decode::test_support::service_decode_context();
            let definition = project(&ctx, &scope, &mut losses).unwrap();
            assert!(
                losses.is_empty(),
                "source-less values do not assert a damaged source carrier"
            );
            if origin || direction {
                let cadmpeg_ir::features::FeatureOperation::DatumAxis { axis } =
                    definition.operation()
                else {
                    panic!("partial datum")
                };
                assert_eq!(axis.origin().is_some(), origin);
                assert_eq!(axis.direction().is_some(), direction);
            } else {
                assert!(matches!(
                    definition.operation(),
                    cadmpeg_ir::features::FeatureOperation::Unresolved {
                        family: cadmpeg_ir::features::UnresolvedFamily::DatumAxis
                    }
                ));
            }
        }
    }
}
