use crate::container::ContainerScan;
use crate::design::decode::scopes::shared_frames::{exact_fixed_scalar, extrude_operation_at};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::native_stream;
use crate::layout::compact_sweep_prologue as prefix;
use crate::loss::F3dLossCode;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::{DesignEdgeIdentityOperand, DesignEdgeOperand};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, PathRef,
    PlanarProfileRef, ProfileRef, SheetSweepSection, SolidSweepOperation, SweepGuideRail,
    SweepMode, SweepOrientation, SweepPathExtent, SweepSection, SweepShape,
};
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Fraction};
use cadmpeg_ir::topology::{Body, BodyKind};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub(crate) struct Inputs<'a> {
    pub(crate) scan: &'a ContainerScan<'a>,
    pub(crate) ctx: &'a DecodeContext<'a>,
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) edges: &'a [DesignEdgeOperand],
    pub(crate) edge_identities: &'a [DesignEdgeIdentityOperand],
    pub(crate) faces: &'a [DesignFaceOperand],
}

#[derive(Default)]
pub(crate) struct OutputFacts(HashMap<FeatureId, BodyKind>);

impl OutputFacts {
    pub(crate) fn record(&mut self, feature: &Feature, bodies: &[Body], complete: bool) {
        if !complete || !is_native(feature) || feature.evaluation.outputs().is_empty() {
            return;
        }
        let mut kind = None;
        for id in feature.evaluation.outputs() {
            let Some(body) = only(bodies.iter().filter(|body| &body.id == id)) else {
                return;
            };
            if !matches!(body.kind, BodyKind::Solid | BodyKind::Sheet)
                || kind.is_some_and(|kind| kind != body.kind)
            {
                return;
            }
            kind = Some(body.kind);
        }
        if let Some(kind) = kind {
            self.0.insert(feature.id.clone(), kind);
        }
    }
}

#[derive(Default, Serialize)]
struct Readout {
    prefix: Option<NativeBytes>,
    compact_operation: Option<DesignExtrudeOperation>,
    assumed_join_offset: Option<u64>,
    scalars: Vec<Scalar>,
    fragments: Vec<Fragment>,
}

#[derive(Serialize)]
struct Scalar {
    record: u32,
    reference_ordinal: usize,
    owner: Option<u32>,
    ordinal: u8,
    value: f64,
    value_offset: u64,
}

#[derive(Serialize)]
struct Fragment {
    record: u32,
    offset: u64,
    bytes: NativeBytes,
}

fn only<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn is_native(feature: &Feature) -> bool {
    matches!(feature.evaluation.definition(), FeatureDefinition::Operation(
        FeatureOperation::Native { kind, .. }) if kind.as_str() == "Sweep")
}

fn read_scope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Readout {
    let mut result = Readout::default();
    let mut headers = records
        .records()
        .flat_map(|(_, offsets)| offsets.iter().copied())
        .collect::<Vec<_>>();
    headers.sort_unstable();
    headers.dedup();
    if let (Ok(start), Ok(end)) = (
        usize::try_from(scope.byte_offset()),
        usize::try_from(scope.reference_count_offset()),
    ) {
        if let Some(prefix_bytes) = bytes.get(start..end) {
            result.prefix = Some(NativeBytes::new(prefix_bytes.to_vec()));
            let compact = prefix_bytes
                .get(prefix::ZERO_RUN..prefix::OPERATION)
                .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0));
            if compact && prefix_bytes.get(prefix::MARKER) == Some(&1) {
                result.compact_operation = extrude_operation_at(prefix_bytes, prefix::OPERATION);
            }
            if compact
                && prefix_bytes.get(prefix::MARKER) == Some(&0)
                && View::u32_le_at(prefix_bytes, prefix::OPERATION) == Some(1)
            {
                result.assumed_join_offset = u64::try_from(prefix::OPERATION)
                    .ok()
                    .and_then(|offset| scope.byte_offset().checked_add(offset));
            }
        }
    }
    for (reference_ordinal, &record) in scope.reference_members().values().enumerate() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, record) {
            result.scalars.push(Scalar {
                record,
                reference_ordinal,
                owner: scalar.owner_record_index,
                ordinal: scalar.ordinal,
                value: scalar.value.get(),
                value_offset: scalar.value_offset,
            });
        }
        for &start in records.offsets(record) {
            let end = headers
                .get(headers.partition_point(|offset| *offset <= start))
                .copied()
                .unwrap_or(bytes.len());
            if let (Some(data), Ok(offset)) = (bytes.get(start..end), u64::try_from(start)) {
                result.fragments.push(Fragment {
                    record,
                    offset,
                    bytes: NativeBytes::new(data.to_vec()),
                });
            }
        }
    }
    result
}

fn observations(
    inputs: &Inputs<'_>,
    scopes: &[&DesignParameterScope],
) -> Result<HashMap<String, Vec<Readout>>, CodecError> {
    let mut result: HashMap<String, Vec<Readout>> = HashMap::new();
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
            result
                .entry(scope.id.clone())
                .or_default()
                .push(read_scope(bytes, &records, scope));
        }
    }
    Ok(result)
}

pub(crate) fn project(
    features: &mut [Feature],
    inputs: &Inputs<'_>,
    outputs: &OutputFacts,
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
    let observations = observations(inputs, &scopes)?;
    let mut planned = Vec::new();
    let mut losses = Vec::new();
    for index in candidates {
        let mut replacement = features[index].clone();
        losses.extend(project_one(
            &mut replacement,
            inputs,
            outputs,
            &observations,
        )?);
        planned.push((index, replacement));
    }
    for (index, replacement) in planned {
        features[index] = replacement;
    }
    Ok(losses)
}

fn project_one(
    feature: &mut Feature,
    inputs: &Inputs<'_>,
    outputs: &OutputFacts,
    observations: &HashMap<String, Vec<Readout>>,
) -> Result<Vec<LossNote>, CodecError> {
    let scopes = inputs
        .scopes
        .iter()
        .filter(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        .collect::<Vec<_>>();
    let scope = only(scopes.iter().copied())
        .filter(|scope| matches!(scope.payload(), DesignScopePayload::Sweep(_)));
    let stream = scope.and_then(|scope| native_stream(&scope.id));
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
    let readout = scope
        .and_then(|scope| observations.get(&scope.id))
        .and_then(|values| only(values.iter()));
    let construction = scope.and_then(|scope| match scope.payload() {
        DesignScopePayload::Sweep(Some(value)) => value.construction.as_ref(),
        _ => None,
    });
    let operation = construction
        .map(|value| value.operation)
        .or_else(|| readout.and_then(|value| value.compact_operation))
        .or_else(|| {
            readout.and_then(|value| {
                value
                    .assumed_join_offset
                    .map(|_| DesignExtrudeOperation::Join)
            })
        });
    let values: [Option<f64>; 6] = std::array::from_fn(|ordinal| {
        construction
            .and_then(|value| value.values.get(ordinal))
            .map(|value| value.get())
            .or_else(|| {
                let scope = scope?;
                only(readout?.scalars.iter().filter(|value| {
                    value.owner == Some(scope.record_index) && usize::from(value.ordinal) == ordinal
                }))
                .map(|value| value.value)
            })
    });
    let mut losses = Vec::new();
    let mut note = |code: F3dLossCode, text: &str| {
        losses.push(code.note(format!("feature {}: {text}", feature.id)));
    };
    if scope.is_none() || readout.is_none() {
        note(F3dLossCode::FeatureSweepSourceUnavailable, "Sweep source bytes or unique scope are unavailable; independently decoded records are retained");
    }
    if construction.is_none() && readout.is_some_and(|value| value.assumed_join_offset.is_some()) {
        note(F3dLossCode::FeatureSweepOperationPrefixAssumed, "the compact offset-21 word is read as Join despite the zero offset-25 control; its control semantics remain unresolved");
    }
    if operation.is_none() {
        note(
            F3dLossCode::FeatureSweepOperationUnresolved,
            "the Sweep operation has no unique supported reading",
        );
    }
    let owned = groups
        .iter()
        .copied()
        .filter(|group| {
            scope.is_some_and(|scope| group.scope_record_index == scope.record_index)
                && native_stream(&group.id) == stream
        })
        .collect::<Vec<_>>();
    let profiles = owned
        .iter()
        .copied()
        .filter(|group| group.role() == DesignOperandRole::PROFILE)
        .collect::<Vec<_>>();
    let alternative = owned
        .iter()
        .copied()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X43)
        .collect::<Vec<_>>();
    let profile = only(profiles.iter().copied()).or_else(|| {
        if profiles.is_empty() {
            only(alternative.iter().copied())
        } else {
            None
        }
    });
    let section: SheetSweepSection = if let (Some(scope), Some(group)) = (scope, profile) {
        if group.role() == DesignOperandRole::ROLE_0X43 {
            note(
                F3dLossCode::FeatureSweepSectionRoleAssumed,
                "the unique role-0x43 group is interpreted as the Sweep section",
            );
        }
        let carrier = scope.sweep_profile().filter(|operand| {
            group
                .members()
                .iter()
                .map(|member| member.value)
                .eq([operand.record_index])
        });
        let resolved = super::resolved_profile_face_group(scope, group, inputs.faces)?;
        if let Some(ProfileRef::Planar(profile)) = resolved {
            SweepSection::Profile(profile)
        } else if let Some(carrier) = carrier {
            note(F3dLossCode::FeatureSweepProfileCarrierAssumed, "the profile carrier's explicit selection supplies the section without a separate selection group");
            SweepSection::Profile(PlanarProfileRef::Native(format!(
                "{}:design-record-header#{}",
                stream.unwrap_or_default(),
                carrier.byte_offset()
            )))
        } else {
            SweepSection::Profile(PlanarProfileRef::Native(group.id.clone()))
        }
    } else {
        note(
            F3dLossCode::FeatureSweepSelectionUnresolved,
            "the section has no unique source carrier; all candidates are retained",
        );
        SweepSection::Unresolved(None)
    };
    let mut paths = owned
        .iter()
        .copied()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X5)
        .collect::<Vec<_>>();
    paths.sort_by_key(|group| group.scope_reference_ordinal);
    let paths_ordered = paths
        .windows(2)
        .all(|pair| pair[0].scope_reference_ordinal != pair[1].scope_reference_ordinal)
        && paths.len() <= 2;
    let resolve_path = |group: &DesignConstructionOperandGroup| match scope {
        Some(scope) => super::resolved_loft_path(
            group,
            inputs.groups,
            inputs.edges,
            inputs.edge_identities,
            scope,
            inputs.ctx,
        ),
        None => Ok(PathRef::Native(group.id.clone())),
    };
    let path = if paths_ordered {
        paths.first().map(|group| resolve_path(group)).transpose()?
    } else {
        None
    };
    if path.is_none() {
        note(
            F3dLossCode::FeatureSweepSelectionUnresolved,
            "the path has no unique source ordering; all path groups are retained",
        );
    }
    let extent = |first: usize| {
        Some(SweepPathExtent {
            along_fraction: Fraction::new(values[first]?)?,
            against_fraction: Fraction::new(values[first + 1]?)?,
        })
    };
    let path_extent = extent(0);
    if path_extent.is_none() {
        note(
            F3dLossCode::FeatureSweepScalarUnresolved,
            "the directed path fractions cannot form a unique bounded extent",
        );
    }
    if values[4].is_none() || values[5].is_none() {
        note(
            F3dLossCode::FeatureSweepScalarUnresolved,
            "the twist or taper lane has no unique finite owner-linked value",
        );
    }
    let guide_rail = match (paths_ordered, paths.get(1), extent(2)) {
        (true, Some(group), Some(extent)) => Some(SweepGuideRail {
            path: resolve_path(group)?,
            extent,
        }),
        _ => None,
    };
    if paths.len() > 1 && guide_rail.is_none() {
        note(
            F3dLossCode::FeatureSweepScalarUnresolved,
            "the guide rail or its directed fractions are unresolved",
        );
    }
    let guide = only(
        owned
            .iter()
            .copied()
            .filter(|group| group.role() == DesignOperandRole::FACES),
    );
    if guide.is_none()
        && owned
            .iter()
            .any(|group| group.role() == DesignOperandRole::FACES)
    {
        note(
            F3dLossCode::FeatureSweepSelectionUnresolved,
            "the guide surface has competing source groups; all candidates are retained",
        );
    }
    let orientation = guide.map(|group| SweepOrientation::GuideSurface {
        faces: scope
            .and_then(|scope| {
                super::resolved_historical_face_group(
                    scope,
                    scope.previous_history_state_id(),
                    group,
                    inputs.faces,
                )
            })
            .unwrap_or_else(|| FaceSelection::Native(group.id.clone())),
    });
    if owned.iter().any(|group| {
        matches!(
            group.role(),
            DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
        )
    }) {
        note(
            F3dLossCode::FeatureSweepTargetsNative,
            "Sweep input body targets remain in source evidence",
        );
    }
    let mode = match (outputs.0.get(&feature.id), operation) {
        (Some(BodyKind::Solid), Some(operation)) => SweepMode::Solid {
            op: match operation {
                DesignExtrudeOperation::NewBody => SolidSweepOperation::NewBody,
                DesignExtrudeOperation::Join => SolidSweepOperation::Join,
                DesignExtrudeOperation::Cut => SolidSweepOperation::Cut,
                DesignExtrudeOperation::Intersect => SolidSweepOperation::Intersect,
            },
        },
        (Some(BodyKind::Sheet), Some(DesignExtrudeOperation::NewBody)) => SweepMode::Surface {},
        _ => SweepMode::Unresolved {},
    };
    if matches!(mode, SweepMode::Unresolved {}) {
        note(
            F3dLossCode::FeatureSweepResultModeUnresolved,
            "complete output kinds and source operation do not establish the result family",
        );
    } else {
        note(F3dLossCode::FeatureSweepResultModeInferred, "the result family follows complete bound output kinds with the source operation preserved");
    }
    note(
        F3dLossCode::FeatureSweepPathTangentAssumedFalse,
        "the undecoded path-tangent control is assumed false",
    );
    note(
        F3dLossCode::FeatureSweepLinearizeAssumedFalse,
        "the undecoded linearize control is assumed false",
    );
    note(
        F3dLossCode::FeatureSweepControlsUninterpreted,
        "unclassified Sweep controls and settings remain in source evidence",
    );
    let readouts = feature
        .native_ref
        .as_ref()
        .and_then(|id| observations.get(id));
    let evidence = serde_json::json!({ "scopes": scopes, "groups": groups, "owners": owners, "parameters": parameters, "readouts": readouts, "operation": operation, "values": values });
    feature.source_properties.insert(
        cadmpeg_core::nonblank_literal!("sweep:source"),
        serde_json::to_string(&evidence).map_err(CodecError::malformed)?,
    );
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::sheet_sections(mode, section, Vec::new()),
            path,
            orientation,
            path_extent,
            guide_rail,
            twist: values[4].and_then(Angle::new),
            taper: values[5].and_then(Angle::new),
            path_tangent: false,
            linearize: false,
            transition: None,
            transformation: None,
            scale: None,
            allow_multi_profile_faces: None,
        }));
    Ok(losses)
}

#[cfg(test)]
mod tests;
