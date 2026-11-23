use crate::bytes::reference_fields::ReadField;
use crate::design::decode::scopes::point_data::POINT_DATA_TYPE_GUID;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_axes::{
    AxisEndpointReading, AxisReference, AxisReferenceRun, DesignWorkAxisCandidate,
    DesignWorkAxisSource,
};
use cadmpeg_ir::design::construction::{
    ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::names::{Base64Bytes, NativeName, NativeNameValue};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, Witness,
};
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

/// The native name of a reference read in full: the target entity ID of a
/// same-segment reference, otherwise the reference's stored bytes.
fn native_name(reference: &AxisReference) -> Option<NativeNameValue> {
    if let Some(target) = reference
        .local()
        .and_then(|(target, _)| i64::try_from(target).ok())
    {
        return Some(NativeNameValue::Integers(vec![target]));
    }
    Some(NativeNameValue::Bytes(Base64Bytes(reference.encoded()?)))
}

/// An input whose reference the decoder could not resolve: a `Native` pick
/// named by the stored reference, with the point the source stores for it
/// as its witness.
fn native_input(
    name: NativeNameValue,
    position: Option<FinitePoint3>,
) -> Option<ConstructionInput> {
    let pick = Picked {
        target: PickTarget::Native { within: None },
        context: None,
        native: Some(NativeName {
            codec: cadmpeg_core::nonblank_literal!("f3d").into(),
            value: name,
        }),
        domain: None,
        witness: position.map(|point| Witness {
            point: Some(point),
            direction: None,
            endpoints: None,
            frame: None,
            curves: Vec::new(),
            surface: None,
            body: None,
        }),
        resolved: None,
    };
    Some(ConstructionInput {
        role: None,
        reference: Some(InputReference::Picks(OrderedSelection {
            at: Anchor::Input,
            picks: Picks::Named(Members::new(vec![pick]).ok()?),
            native: None,
        })),
        value: None,
        reversed: None,
    })
}

/// The input standing for the references the source stores and the decoder
/// did not read in full, however many there are.
fn unread_input() -> ConstructionInput {
    ConstructionInput {
        role: None,
        reference: Some(InputReference::Picks(OrderedSelection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        })),
        value: None,
        reversed: None,
    }
}

fn is_unread(input: &ConstructionInput) -> bool {
    matches!(
        &input.reference,
        Some(InputReference::Picks(OrderedSelection {
            picks: Picks::Unread,
            ..
        }))
    )
}

/// The inputs of a reference run, with the stored point of each reference
/// in run order. Each reference read in full is a `Native` pick; a null
/// reference states no input; the references of an incomplete run from the
/// first not read in full on are one `Unread` input.
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
        .filter_map(|reference| {
            let local = reference.local();
            let position = local
                .filter(|(_, inline)| {
                    inline.is_none_or(|guid| guid.eq_ignore_ascii_case(POINT_DATA_TYPE_GUID))
                })
                .and_then(|(target, _)| u32::try_from(target).ok())
                .and_then(|target| positions.get(&target).copied().flatten());
            points.push(position);
            native_input(native_name(reference)?, position.and_then(point))
        })
        .chain((!run.complete()).then(unread_input))
        .collect();
    (inputs, points)
}

/// Charges the null references of `run`, which state no input.
fn note_null(scope: &DesignParameterScope, run: &AxisReferenceRun, losses: &mut Vec<LossNote>) {
    let null = run
        .references
        .iter()
        .filter(|reference| reference.complete && reference.present == ReadField::Value(0))
        .count();
    if null > 0 {
        losses.push(F3dLossCode::FeatureWorkAxisInputNull.note(format!("WorkAxis scope {} states no construction input for {null} null input reference(s); the run's occurrences remain in its native evidence.", scope.id)));
    }
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
    let count = run.map_or(0, |v| v.references.len()) + 1;
    let width = std::mem::size_of::<ConstructionInput>()
        + std::mem::size_of::<Option<[f64; 3]>>()
        + std::mem::size_of::<Picked>();
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
        None => axis.inputs.push(unread_input()),
        Some(DesignWorkAxisSource::DirectCarrier {
            value_count,
            reserved,
            reference_count,
            reference_preamble,
            ..
        }) => {
            axis.inputs.push(unread_input());
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
            note_null(scope, run, losses);
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
            note_null(scope, run, losses);
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
        let line_origin = origin.and_then(point);
        let line_direction = displacement.and_then(|value| {
            let vector = Vector3::new(value[0], value[1], value[2]);
            (value[0].hypot(value[1]).hypot(value[2]) > f64::EPSILON)
                .then(|| cadmpeg_ir::units::UnitVector3::normalized(vector))
                .flatten()
                .map(FeatureDirection3::from)
        });
        if origin.is_some() && line_origin.is_none()
            || displacement.is_some() && line_direction.is_none()
        {
            losses.push(F3dLossCode::FeatureWorkAxisGeometryUnrepresentable.note(format!("WorkAxis scope {} states no solved line because its source origin conversion or nonzero direction admission is unresolved; the stored values remain in its native evidence.", scope.id)));
        } else if origin.is_some() != displacement.is_some() {
            losses.push(F3dLossCode::FeatureWorkAxisPartialLineNative.note(format!("WorkAxis scope {} states no solved line because only its {} was read; the part read remains in its native evidence.", scope.id, if origin.is_some() { "origin" } else { "direction" })));
        }
        if let (Some(line_origin), Some(line_direction)) = (line_origin, line_direction) {
            axis.origin = Some(line_origin);
            axis.direction = Some(line_direction);
        }
    }
    if axis.inputs.iter().all(is_unread) && axis.origin.is_none() {
        return Ok(FeatureDefinition::Operation(
            DatumAxisDraft::default()
                .into_operation()
                .map_err(cadmpeg_core::CodecError::malformed)?,
        ));
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
    fn repeated_work_axis_inputs_are_native_picks_witnessed_by_their_agreed_points() {
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
                assert!(input.role.is_none() && input.value.is_none());
                let Some(InputReference::Picks(selection)) = &input.reference else {
                    panic!("a selection")
                };
                assert!(matches!(selection.at, Anchor::Input));
                let Picks::Named(picks) = &selection.picks else {
                    panic!("named picks")
                };
                let [pick] = picks.as_slice() else {
                    panic!("one pick")
                };
                assert!(matches!(pick.target, PickTarget::Native { within: None }));
                let target = if index % 2 == 0 { 42 } else { 43 };
                assert_eq!(
                    pick.native.as_ref().map(|name| &name.value),
                    Some(&NativeNameValue::Integers(vec![target]))
                );
                let witness = pick.witness.as_ref().and_then(|witness| witness.point);
                if index % 2 == 0 {
                    assert_eq!(witness.unwrap().get(), Point3::new(10.0, 20.0, 30.0));
                } else {
                    assert!(witness.is_none(), "conflicting readings state no witness");
                }
            }
        }
    }

    #[test]
    fn source_less_lines_project_whole_with_an_unread_input_or_as_the_unresolved_family() {
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
                losses
                    .iter()
                    .all(|loss| loss.code == F3dLossCode::FeatureWorkAxisPartialLineNative.kind()),
                "source-less values do not assert a damaged source carrier"
            );
            assert_eq!(losses.len(), usize::from(origin != direction));
            if origin && direction {
                let cadmpeg_ir::features::FeatureOperation::DatumAxis { axis } =
                    definition.operation()
                else {
                    panic!("datum axis")
                };
                assert!(axis.origin().is_some() && axis.direction().is_some());
                let [input] = axis.inputs() else {
                    panic!("one input for the unread references")
                };
                assert!(is_unread(input) && input.role.is_none());
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

    fn axis_scope(candidate: DesignWorkAxisCandidate) -> DesignParameterScope {
        let mut scope = DesignParameterScope::empty(
            "f3d:test/BulkStream.dat:scope#2",
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
        scope
    }

    fn native_value(input: &ConstructionInput) -> Option<&NativeNameValue> {
        let Some(InputReference::Picks(selection)) = &input.reference else {
            return None;
        };
        let Picks::Named(picks) = &selection.picks else {
            return None;
        };
        let [pick] = picks.as_slice() else {
            return None;
        };
        matches!(pick.target, PickTarget::Native { within: None })
            .then_some(())
            .and(pick.native.as_ref().map(|name| &name.value))
    }

    #[test]
    fn each_reference_read_in_full_is_named_and_an_unread_tail_is_one_unread_input() {
        let real = |v| ReadField::Value(FiniteReal::new(v).unwrap());
        let read = |bytes: &[u8]| {
            crate::bytes::reference_fields::read_reference_fields(bytes, 0)
                .fields
                .into_owned()
        };
        let mut local = vec![1];
        local.extend_from_slice(&42u64.to_le_bytes());
        local.extend_from_slice(&[0, 0]);
        let mut segment = vec![1];
        segment.extend_from_slice(&43u64.to_le_bytes());
        segment.extend_from_slice(&[0, 1]);
        segment.extend_from_slice(&7u32.to_le_bytes());
        let truncated = &segment[..12];
        for (declared, references, unread) in [
            (4, vec![read(&local), read(&[0]), read(&segment)], true),
            (3, vec![read(&local), read(&[0]), read(truncated)], true),
            (3, vec![read(&local), read(&[0]), read(&segment)], false),
        ] {
            let candidate = DesignWorkAxisCandidate {
                origin: [real(0.0); 3],
                displacement: [real(0.0), real(0.0), real(1.0)],
                source: Some(DesignWorkAxisSource::PointDirection {
                    bounding_box_index: ReadField::Value(8),
                    point_parameters: [real(0.0); 2],
                    reference_type: ReadField::Value(18),
                    tangent_prefix: ReadField::Absent,
                    tangent: [ReadField::Absent; 3],
                    inputs: AxisReferenceRun {
                        declared: ReadField::Value(declared),
                        references,
                    },
                }),
                complete: true,
                ..Default::default()
            };
            let mut losses = Vec::new();
            let ctx = cadmpeg_core::decode::test_support::service_decode_context();
            let definition = project(&ctx, &axis_scope(candidate), &mut losses).unwrap();
            let cadmpeg_ir::features::FeatureOperation::DatumAxis { axis } = definition.operation()
            else {
                panic!("datum axis")
            };
            assert!(axis.geometry().is_some());
            assert!(losses
                .iter()
                .any(|loss| loss.code == F3dLossCode::FeatureWorkAxisInputNull.kind()));
            let named = axis
                .inputs()
                .iter()
                .filter_map(native_value)
                .collect::<Vec<_>>();
            let truncated_tail = declared == 3 && unread;
            let mut expected = vec![NativeNameValue::Integers(vec![42])];
            if !truncated_tail {
                expected.push(NativeNameValue::Bytes(Base64Bytes(segment.clone())));
            }
            assert_eq!(named, expected.iter().collect::<Vec<_>>());
            assert_eq!(
                axis.inputs()
                    .iter()
                    .filter(|input| is_unread(input))
                    .count(),
                usize::from(unread)
            );
            assert_eq!(axis.inputs().len(), expected.len() + usize::from(unread));
            assert_eq!(axis.inputs().last().is_some_and(is_unread), unread);
        }
    }

    #[test]
    fn a_direct_carrier_axis_states_its_line_and_one_unread_input() {
        let real = |v| ReadField::Value(FiniteReal::new(v).unwrap());
        let candidate = DesignWorkAxisCandidate {
            origin: [real(1.0), real(2.0), real(3.0)],
            displacement: [real(0.0), real(0.0), real(2.0)],
            source: Some(DesignWorkAxisSource::DirectCarrier {
                value_count: ReadField::Value(8),
                reserved: [real(0.0); 2],
                reference_count: ReadField::Value(6),
                reference_preamble: ReadField::Value(1),
                support_record_index: 9,
            }),
            complete: true,
            ..Default::default()
        };
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let definition = project(&ctx, &axis_scope(candidate), &mut Vec::new()).unwrap();
        let cadmpeg_ir::features::FeatureOperation::DatumAxis { axis } = definition.operation()
        else {
            panic!("datum axis")
        };
        assert!(axis.geometry().is_some());
        let [input] = axis.inputs() else {
            panic!("one input")
        };
        assert!(is_unread(input) && input.role.is_none() && input.value.is_none());
    }
}
