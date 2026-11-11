// SPDX-License-Identifier: Apache-2.0
//! Reconcile planar Extrude termination recipes with their stored caps.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_ir::features::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeStart, FaceSelection, Feature, FeatureDefinition,
    FeatureOperation, LinearTermination,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::Sketch;

use crate::history_records::{AsmHistoricalPlane, AsmHistoricalTopology, AsmHistory};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, face::DesignFaceOperand,
};

pub(crate) struct Inputs<'a> {
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a mut [DesignFaceOperand],
    pub(crate) histories: &'a [AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
}

pub(crate) fn bind(
    features: &mut [Feature],
    sketches: &[Sketch],
    input: &mut Inputs<'_>,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for feature in features {
        if !matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided { side }, ..
            }) if matches!(side.termination, LinearTermination::ToFace { .. })
        ) {
            continue;
        }
        let Some(state) = super::face_selection_state(
            feature,
            input.scopes,
            input.scope_histories,
            input.histories,
        ) else {
            continue;
        };
        let history = &input.histories[state.history];
        let current = &history.states()[state.state];
        let previous = &history.states()[state.previous];
        let Some(transition) = current.transition.as_ref() else {
            continue;
        };
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                direction,
                start,
                extent: ExtrudeExtent::OneSided { side },
                ..
            }) = definition else {
                return;
            };
            let LinearTermination::ToFace { face, offset } = &mut side.termination else {
                return;
            };
            let Some(native) = native(face) else { return };
            let mut groups = input.groups.iter().filter(|group| group.id == native);
            let Some(group) = groups.next() else { return };
            if groups.next().is_some() || group.members().len() != 1 {
                return;
            }
            let mut operands = input.operands.iter_mut().filter(|operand| {
                operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.record_index() == group.members()[0].value
                    && crate::ids::native_stream(&operand.id) == crate::ids::native_stream(&group.id)
            });
            let Some(operand) = operands.next() else { return };
            if operands.next().is_some() { return; }
            let referenced: BTreeSet<_> = operand.recipe_references.iter()
                .flat_map(super::effective_faces)
                .filter_map(|face| super::stable_ref(face.as_str()))
                .collect();
            if referenced.is_empty() { return; }
            if selection_slots(face).is_some_and(|slots| {
                !slots.is_empty() && slots.iter().all(|slot| referenced.contains(slot))
            }) {
                return;
            }
            let retained: BTreeSet<_> = selection_slots(face).unwrap_or_default().into_iter().collect();
            let preceding = &operand.preceding_candidate_faces;
            let candidate = match preceding.as_slice() {
                [face] => Some(face),
                _ => match operand.changed_candidate_faces.as_slice() {
                    [face] if preceding.contains(face) => Some(face),
                    _ => None,
                },
            };
            let preceding: BTreeSet<_> = candidate.and_then(|face| super::stable_ref(face.as_str()))
                .into_iter().collect();
            let Some((before, after)) = history.topology(previous).zip(history.topology(current)) else {
                return;
            };
            let sketch = crate::design::face_resolve::extrude_profile_sketch_id(profile)
                .and_then(|id| sketches.iter().find(|sketch| sketch.id == *id))
                .and_then(Sketch::resolved_placement);
            let selected = sketch.and_then(|(origin, normal, _)| {
                let sweep = match direction {
                    ExtrudeDirection::ProfileNormal {} => normal.get(),
                    ExtrudeDirection::ReversedProfileNormal {} => normal.get().scale(-1.0),
                    ExtrudeDirection::Explicit { vector, .. } => vector.get(),
                    ExtrudeDirection::Unresolved {} => return None,
                };
                let sweep = sweep.unit()?;
                let start = start_origin(start, origin.get(), normal.get(), sweep, before);
                let caps: Vec<_> = transition.topology.faces.inserted.iter()
                    .chain(&transition.topology.faces.updated)
                    .filter_map(|slot| exact_plane(after, *slot))
                    .collect();
                let terminal_caps: Vec<_> = caps.iter().copied().filter(|cap| {
                    start.is_some_and(|start| {
                        (cap.normal.dot(sweep).abs() - 1.0).abs() <= input.angular_tolerance
                            && cap.origin.vector_from(start).dot(sweep).abs() > input.linear_tolerance
                    })
                }).collect();
                let retained_caps = if terminal_caps.is_empty() { caps.as_slice() }
                    else { terminal_caps.as_slice() };
                let cap_match = |candidates, start, caps: &[&AsmHistoricalPlane]| select_target(
                    candidates,
                    before,
                    caps,
                    start,
                    sweep,
                    offset.map_or(0.0, Length::get),
                    (input.linear_tolerance, input.angular_tolerance),
                );
                start.and_then(|start| cap_match(&referenced, Some(start), &caps))
                    .or_else(|| cap_match(&retained, None, retained_caps))
                    .or_else(|| cap_match(&preceding, None, retained_caps))
            });
            if let Some(slot) = selected {
                if let Some(selection) = crate::design::face_resolve::historical_face_selection_with_native(
                    state.scope, state.previous_state_id, vec![slot], native,
                ) {
                    operand.resolved_face_slots = vec![slot];
                    operand.resolved_active_face = None;
                    *face = selection;
                }
            } else {
                let was_resolved = !matches!(face, FaceSelection::Native(_));
                *face = FaceSelection::Native(native);
                operand.resolved_face_slots.clear();
                operand.resolved_active_face = None;
                if was_resolved {
                    losses.push(crate::loss::F3dLossCode::FeatureExtrudeTargetDisputed.note(format!(
                        "Extrude termination {} retains its native recipe because its derived faces and recipe candidates have no unique stored-cap agreement.", operand.id
                    )));
                }
            }
        });
    }
    losses
}

fn native(face: &FaceSelection) -> Option<String> {
    match face {
        FaceSelection::Native(native) | FaceSelection::Resolved { native, .. } => {
            Some(native.clone())
        }
        FaceSelection::Historical { native, .. }
        | FaceSelection::HistoricalPartial { native, .. } => Some(native.to_string()),
        _ => None,
    }
}

fn selection_slots(face: &FaceSelection) -> Option<Vec<i64>> {
    match face {
        FaceSelection::Resolved { faces, .. } => faces
            .iter()
            .map(|face| super::stable_ref(face.as_str()))
            .collect(),
        FaceSelection::Historical { faces, .. } => faces
            .iter()
            .map(|face| face.as_str().rsplit_once(':')?.1.parse().ok())
            .collect(),
        _ => None,
    }
}

fn exact_plane(topology: &AsmHistoricalTopology, slot: i64) -> Option<&AsmHistoricalPlane> {
    if !topology.faces.contains(&slot) {
        return None;
    }
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == slot);
    let binding = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|plane| plane.surface == binding.carrier);
    let plane = planes.next()?;
    planes.next().is_none().then_some(plane)
}

fn start_origin(
    start: &ExtrudeStart,
    origin: Point3,
    normal: Vector3,
    sweep: Vector3,
    before: &AsmHistoricalTopology,
) -> Option<Point3> {
    match start {
        ExtrudeStart::ProfilePlane {} => Some(origin),
        ExtrudeStart::OffsetProfilePlane { offset } => {
            Some(origin.translated(normal, offset.get()))
        }
        ExtrudeStart::FromFace { face, offset } => {
            let slots = selection_slots(face)?;
            let [slot] = slots.as_slice() else {
                return None;
            };
            Some(
                exact_plane(before, *slot)?
                    .origin
                    .translated(sweep, offset.map_or(0.0, Length::get)),
            )
        }
        _ => None,
    }
}

fn select_target(
    referenced: &BTreeSet<i64>,
    before: &AsmHistoricalTopology,
    caps: &[&AsmHistoricalPlane],
    start: Option<Point3>,
    sweep: Vector3,
    offset: f64,
    tolerances: (f64, f64),
) -> Option<i64> {
    let (linear, angular) = tolerances;
    let length = sweep.norm();
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let sweep = sweep.scale(length.recip());
    let aligned = |normal: Vector3| (normal.dot(sweep).abs() - 1.0).abs() <= angular;
    let mut matching = referenced.iter().copied().filter(|slot| {
        let Some(target) = exact_plane(before, *slot) else {
            return false;
        };
        if !aligned(target.normal) {
            return false;
        }
        let end = target.origin.translated(sweep, offset);
        start.is_none_or(|start| end.vector_from(start).dot(sweep) > linear)
            && caps.iter().any(|cap| {
                aligned(cap.normal) && cap.origin.vector_from(end).dot(sweep).abs() <= linear
            })
    });
    let selected = matching.next()?;
    matching.next().is_none().then_some(selected)
}

#[cfg(test)]
mod tests;
