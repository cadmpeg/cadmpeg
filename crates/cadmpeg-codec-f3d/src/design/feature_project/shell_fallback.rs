use crate::container::ContainerScan;
use crate::design::decode::scopes::shell_observation::{self, Form, Frame, Readout, Slot};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::face_resolve::resolved_historical_face_group;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, FaceSelection, Feature, FeatureDefinition, FeatureOperation, ShellJoin,
    ShellMode,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use std::collections::{HashMap, HashSet};

pub(crate) struct Inputs<'a> {
    pub(crate) scan: &'a ContainerScan<'a>,
    pub(crate) ctx: &'a DecodeContext<'a>,
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) faces: &'a [DesignFaceOperand],
}

fn only<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn is_native(feature: &Feature) -> bool {
    matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind.as_str() == "Shell")
}

pub(crate) fn project(
    features: &mut [Feature],
    inputs: &Inputs<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let candidates = features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| is_native(feature).then_some(index))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let native_refs = candidates
        .iter()
        .filter_map(|index| features[*index].native_ref.as_deref())
        .collect::<HashSet<_>>();
    let scopes = inputs
        .scopes
        .iter()
        .filter(|scope| native_refs.contains(scope.id.as_str()))
        .collect::<Vec<_>>();
    let mut readouts: HashMap<String, Vec<Readout>> = HashMap::new();
    for entry in inputs.scan.entries.iter().filter(|entry| {
        inputs
            .scan
            .is_design_stream(entry, ContainerRole::Bulkstream)
    }) {
        let stream = crate::ids::native_scope(&entry.name);
        let selected = scopes
            .iter()
            .copied()
            .filter(|scope| native_stream(&scope.id) == Some(stream.as_str()))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            continue;
        }
        let bytes = inputs.scan.entry_bytes(&entry.name)?;
        let records = IndexedRecordOffsets::build(bytes);
        for scope in selected {
            readouts
                .entry(scope.id.clone())
                .or_default()
                .push(shell_observation::read(inputs.ctx, bytes, &records, scope)?);
        }
    }
    let mut planned = Vec::new();
    let mut losses = Vec::new();
    for index in candidates {
        let mut feature = features[index].clone();
        losses.extend(project_one(&mut feature, inputs, &readouts)?);
        planned.push((index, feature));
    }
    for (index, feature) in planned {
        features[index] = feature;
    }
    Ok(losses)
}

#[derive(Clone, Copy)]
enum Wall {
    Unknown,
    Absent,
    Present(Option<f64>),
}

fn wall(slot: Option<&Slot>, scope: &DesignParameterScope, readout: &Readout) -> Wall {
    match slot {
        None => Wall::Unknown,
        Some(Slot { record: None, .. }) => Wall::Absent,
        Some(Slot {
            record: Some(record),
            ..
        }) => {
            let scalar = only(readout.scalars.iter().filter(|scalar| {
                scalar.record == *record
                    && scalar.owner.is_none_or(|owner| owner == scope.record_index)
                    && scalar.value > 0.0
            }));
            Wall::Present(scalar.map(|scalar| scalar.value))
        }
    }
}

fn selected_frame<'a>(readout: &'a Readout, scope: &DesignParameterScope) -> Option<&'a Frame> {
    let aligned = readout
        .frames
        .iter()
        .filter(|frame| frame.aligned)
        .collect::<Vec<_>>();
    match aligned.as_slice() {
        [frame] => Some(frame),
        [] => {
            let bound = readout
                .frames
                .iter()
                .filter(|frame| {
                    frame.slots.iter().any(|slot| {
                        matches!(wall(slot.as_ref(), scope, readout), Wall::Present(Some(_)))
                    })
                })
                .collect::<Vec<_>>();
            match bound.as_slice() {
                [frame] => Some(frame),
                [] => only(readout.frames.iter().filter(|frame| {
                    frame
                        .slots
                        .iter()
                        .flatten()
                        .any(|slot| slot.record.is_some())
                })),
                _ => None,
            }
        }
        _ => None,
    }
}

fn project_one(
    feature: &mut Feature,
    inputs: &Inputs<'_>,
    readouts: &HashMap<String, Vec<Readout>>,
) -> Result<Vec<LossNote>, CodecError> {
    let scopes = inputs
        .scopes
        .iter()
        .filter(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        .collect::<Vec<_>>();
    let scope = only(scopes.iter().copied())
        .filter(|scope| matches!(scope.payload(), DesignScopePayload::Shell(_)));
    let readout = scope
        .and_then(|scope| readouts.get(&scope.id))
        .and_then(|rows| only(rows.iter()));
    let complete = scope.and_then(|scope| match scope.payload() {
        DesignScopePayload::Shell(Some(operation)) => Some(operation),
        _ => None,
    });
    let frame = if complete.is_none() {
        readout
            .zip(scope)
            .and_then(|(readout, scope)| selected_frame(readout, scope))
    } else {
        None
    };
    let groups = inputs
        .groups
        .iter()
        .filter(|group| {
            scopes.iter().any(|scope| {
                group.scope_record_index == scope.record_index
                    && native_stream(&group.id) == native_stream(&scope.id)
            })
        })
        .collect::<Vec<_>>();
    let owners = inputs
        .owners
        .iter()
        .filter(|owner| {
            scopes.iter().any(|scope| {
                owner.scope_record_index() == scope.record_index
                    && native_stream(owner.id()) == native_stream(&scope.id)
            })
        })
        .collect::<Vec<_>>();
    let parameters = inputs
        .parameters
        .iter()
        .filter(|parameter| {
            owners.iter().any(|owner| {
                parameter.record_index == owner.parameter_record_index()
                    && native_stream(&parameter.id) == native_stream(owner.id())
            })
        })
        .collect::<Vec<_>>();
    let mut losses = Vec::new();
    let mut note = |code: F3dLossCode, message: &str| {
        losses.push(code.note(format!("feature {}: {message}", feature.id)));
    };
    if complete.is_none() && frame.is_none() {
        note(F3dLossCode::FeatureShellSourceUnavailable, "a unique readable Shell envelope is unavailable; all source candidates remain in shell:source");
    }
    if let Some(frame) = frame {
        if !frame.aligned {
            note(F3dLossCode::FeatureShellEnvelopeAssumed, "the available wall references select a partial prefix; its incomplete tail is assumed to leave the wall slots unchanged");
        }
        if frame
            .control
            .as_ref()
            .is_some_and(|control| control.value != 0)
        {
            note(F3dLossCode::FeatureShellCountedControlAssumed, "the counted control byte is interpreted as a separate field before the aligned wall slots");
            note(F3dLossCode::FeatureShellSlotDirectionAssumed, "the aligned first and second slots of the nonzero-control counted envelope are assumed to mean inside and outside");
        }
        if frame.form != Form::Cornered {
            note(F3dLossCode::FeatureShellPrefixFlagAssumed, "the prefix Boolean is retained while the Shell option it selects remains unresolved");
        }
        if frame
            .groups
            .as_ref()
            .is_some_and(|run| run.entries.iter().any(Option::is_none))
        {
            note(F3dLossCode::FeatureShellSelectionUnresolved, "some group references are unreadable; the readable groups and full prefix remain in source evidence");
        }
    }
    let (walls, join) = if let Some(operation) = complete {
        let inside = Wall::Present(Some(operation.thickness.get()));
        let outside = operation
            .wall_slots
            .as_ref()
            .and_then(|slots| slots.outside.as_ref());
        let walls = match outside {
            Some(outside) => [inside, Wall::Present(Some(outside.thickness.get()))],
            None if operation.outward => [Wall::Absent, inside],
            None => [inside, Wall::Absent],
        };
        let join = operation.wall_slots.as_ref().map(|slots| {
            if slots.rounded_corners {
                ShellJoin::Arc
            } else {
                ShellJoin::Intersection
            }
        });
        (walls, join)
    } else if let (Some(scope), Some(readout), Some(frame)) = (scope, readout, frame) {
        let walls = std::array::from_fn(|index| wall(frame.slots[index].as_ref(), scope, readout));
        let join = frame.rounded.as_ref().map(|rounded| {
            if rounded.value == 1 {
                ShellJoin::Arc
            } else {
                ShellJoin::Intersection
            }
        });
        (walls, join)
    } else {
        ([Wall::Unknown; 2], None)
    };
    let (amount, outward, mode, unequal) = match walls {
        [Wall::Present(value), Wall::Absent] => (value, Some(false), None, false),
        [Wall::Absent, Wall::Present(value)] => (value, Some(true), None, false),
        [Wall::Present(inside), Wall::Present(outside)] => {
            let equal = inside
                .zip(outside)
                .filter(|(inside, outside)| inside.to_bits() == outside.to_bits());
            (
                equal.map(|(inside, outside)| inside + outside),
                None,
                Some(ShellMode::BothSides),
                inside.zip(outside).is_some() && equal.is_none(),
            )
        }
        _ => (None, None, None, false),
    };
    let amount = amount.or_else(|| {
        if complete.is_some() || frame.is_some() {
            return None;
        }
        let (scope, readout) = scope.zip(readout)?;
        let candidates = readout
            .frames
            .iter()
            .filter(|frame| {
                frame
                    .slots
                    .iter()
                    .flatten()
                    .any(|slot| slot.record.is_some())
            })
            .map(|frame| {
                match std::array::from_fn(|index| wall(frame.slots[index].as_ref(), scope, readout))
                {
                    [Wall::Present(Some(value)), Wall::Absent]
                    | [Wall::Absent, Wall::Present(Some(value))] => Some(value),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        let value = candidates.first().copied().flatten()?;
        candidates
            .iter()
            .all(|candidate| {
                candidate.is_some_and(|candidate| candidate.to_bits() == value.to_bits())
            })
            .then_some(value)
    });
    let thickness =
        amount.and_then(|value| PositiveLength::new(value * super::DESIGN_LENGTH_TO_MILLIMETRE));
    if unequal {
        note(F3dLossCode::FeatureShellIndependentWallsNative, "unequal inside and outside wall values remain independent in source evidence; neutral thickness is unset");
    } else if thickness.is_none() || (outward.is_none() && mode != Some(ShellMode::BothSides)) {
        note(F3dLossCode::FeatureShellWallValueUnresolved, "the neutral wall magnitude or side distribution is unresolved; independent source values and references are retained");
    }
    let eligible = |group: &&DesignConstructionOperandGroup| {
        scope.is_some_and(|scope| {
            group.scope_record_index == scope.record_index
                && native_stream(&group.id) == native_stream(&scope.id)
        }) && frame
            .and_then(|frame| frame.groups.as_ref())
            .is_none_or(|run| {
                run.entries
                    .iter()
                    .flatten()
                    .any(|entry| entry.value == group.record_index)
            })
    };
    if let (Some(scope), Some(frame)) = (scope, frame) {
        let mut expected = HashMap::<u32, usize>::new();
        for group in groups.iter().filter(|group| eligible(group)) {
            *expected.entry(group.record_index).or_default() += 1;
            for member in group.members() {
                *expected.entry(member.value).or_default() += 1;
            }
        }
        for record in frame.slots.iter().flatten().filter_map(|slot| slot.record) {
            *expected.entry(record).or_default() += 1;
        }
        let mut stored = HashMap::<u32, usize>::new();
        for record in scope.reference_members().values() {
            *stored.entry(*record).or_default() += 1;
        }
        if stored != expected {
            note(F3dLossCode::FeatureShellTableMembersAssumed, "the ordered table differs from the readable group memberships and wall references; that mismatch is assumed to leave the wall construction unchanged");
        }
    }
    let faces = only(
        groups
            .iter()
            .copied()
            .filter(|group| eligible(group) && group.role() == DesignOperandRole::ROLE_0X10),
    );
    let bodies = only(
        groups
            .iter()
            .copied()
            .filter(|group| eligible(group) && group.role() == DesignOperandRole::BODIES_A),
    );
    let removed_faces = match (scope, faces) {
        (Some(scope), Some(group)) => resolved_historical_face_group(
            scope,
            scope.previous_history_state_id(),
            group,
            inputs.faces,
        )
        .unwrap_or_else(|| FaceSelection::Native(group.id.clone())),
        _ => {
            note(F3dLossCode::FeatureShellSelectionUnresolved, "the removed-face selection has no unique readable group; all candidates are retained");
            FaceSelection::Unresolved
        }
    };
    if bodies.is_none()
        && groups
            .iter()
            .any(|group| eligible(group) && group.role() == DesignOperandRole::BODIES_A)
    {
        note(
            F3dLossCode::FeatureShellSelectionUnresolved,
            "the body selection has competing groups; all candidates are retained",
        );
    }
    if faces.is_some() && bodies.is_some() {
        note(F3dLossCode::FeatureShellGroupRolesAssumed, "the removed-face and target-body groups are assumed to participate jointly in the Shell");
    }
    note(F3dLossCode::FeatureShellControlsUninterpreted, "unmapped Shell controls, tangent-chain settings and auxiliary references remain in source evidence");
    let evidence = serde_json::json!({ "native": feature.evaluation.definition(), "scopes": scopes, "groups": groups, "owners": owners, "parameters": parameters, "readouts": feature.native_ref.as_ref().and_then(|id| readouts.get(id)) });
    let evidence = serde_json::to_string(&evidence).map_err(CodecError::malformed)?;
    inputs.ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(evidence.len()),
        "retain F3D partial Shell source",
    )?;
    feature
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("shell:source"), evidence);
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies: bodies.map(|group| BodySelection::Native(group.id.clone())),
            removed_faces,
            thickness,
            outward,
            mode,
            join,
            resolve_intersections: None,
            allow_self_intersections: None,
        }));
    Ok(losses)
}

#[cfg(test)]
mod tests;
