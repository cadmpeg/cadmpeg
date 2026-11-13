use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, GeneratedSweepSection, PathRef,
    SolidSweepOperation, SweepCircularRegion, SweepPathExtent, SweepSection, SweepShape,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Fraction, PositiveLength};

use super::{circular_losses, circular_pipe_sweep, only, surface_index, PipeSectionInputs};
use crate::container::ContainerScan;
use crate::design::decode::scopes::pipe::{observe_sources, PipeObservation, PipeSources};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPipeSectionShape;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;

#[derive(serde::Serialize)]
struct Evidence<'a> {
    scopes: Vec<&'a DesignParameterScope>,
    owners: Vec<&'a DesignParameterOwner>,
    groups: Vec<&'a DesignConstructionOperandGroup>,
    observation: Option<&'a PipeObservation>,
    section_shape: Option<DesignPipeSectionShape>,
    parameters: &'a BTreeMap<NonBlankString, String>,
}

pub(crate) fn project(
    scan: &ContainerScan,
    sources: &PipeSources<'_>,
    features: &mut [Feature],
    inputs: &PipeSectionInputs<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    if !features.iter().any(is_native_pipe) {
        return Ok(Vec::new());
    }
    let surfaces = surface_index(inputs.surfaces);
    let mut losses = Vec::new();
    let mut planned = Vec::new();
    for (index, feature) in features
        .iter()
        .enumerate()
        .filter(|(_, feature)| is_native_pipe(feature))
    {
        let mut replacement = feature.clone();
        if let Some((definition, section, extent)) =
            circular_pipe_sweep(feature, features, inputs, &surfaces)
        {
            replacement.evaluation.set_definition(definition);
            losses.extend(circular_losses(feature, section, extent));
        }
        planned.push((index, replacement));
    }
    let incomplete = inputs
        .scopes
        .iter()
        .filter(|scope| {
            matches!(scope.payload(), DesignScopePayload::Pipe(None))
                && planned.iter().any(|(_, feature)| {
                    is_native_pipe(feature)
                        && feature.native_ref.as_deref() == Some(scope.id.as_str())
                })
        })
        .collect::<Vec<_>>();
    let observations = observe_sources(scan, sources, &incomplete)?;
    for (_, feature) in planned
        .iter_mut()
        .filter(|(_, feature)| is_native_pipe(feature))
    {
        losses.extend(project_fallback(feature, inputs, &observations)?);
    }
    for (index, replacement) in planned {
        features[index] = replacement;
    }
    Ok(losses)
}

fn is_native_pipe(feature: &Feature) -> bool {
    matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind.as_str() == "Pipe")
}

fn project_fallback(
    feature: &mut Feature,
    inputs: &PipeSectionInputs<'_>,
    observations: &HashMap<String, Option<PipeObservation>>,
) -> Result<Vec<LossNote>, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Native { parameters, .. }) =
        feature.evaluation.definition()
    else {
        return Ok(Vec::new());
    };
    let scopes = inputs
        .scopes
        .iter()
        .filter(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        .collect::<Vec<_>>();
    let scope = only(scopes.iter().copied())
        .filter(|scope| matches!(scope.payload(), DesignScopePayload::Pipe(_)));
    let construction = scope.and_then(|scope| match scope.payload() {
        DesignScopePayload::Pipe(construction) => construction.as_ref(),
        _ => None,
    });
    let observation = scope
        .and_then(|scope| observations.get(&scope.id))
        .and_then(Option::as_ref);
    let prefix = observation.and_then(|observation| observation.prefix.as_ref());
    let operation = construction
        .map(|construction| construction.operation)
        .or_else(|| prefix.and_then(|prefix| prefix.operation));
    let hollow = construction
        .map(|construction| construction.is_hollow)
        .or_else(|| match prefix.and_then(|prefix| prefix.is_hollow_value) {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        });
    let belongs = |id: &str, record_index: u32| {
        scopes.iter().any(|scope| {
            native_stream(&scope.id).is_some_and(|stream| native_stream(id) == Some(stream))
                && record_index == scope.record_index
        })
    };
    let owners = inputs
        .owners
        .iter()
        .filter(|owner| belongs(owner.id(), owner.scope_record_index()))
        .collect::<Vec<_>>();
    let groups = inputs
        .groups
        .iter()
        .filter(|group| belongs(&group.id, group.scope_record_index))
        .collect::<Vec<_>>();
    let values: [Option<f64>; 4] = std::array::from_fn(|ordinal| {
        let id = construction
            .map(|construction| construction.record_indexes[ordinal])
            .or_else(|| only(observation?.owners[ordinal].iter().copied()))?;
        only(owners.iter().filter(|owner| {
            owner.record_index() == id
                && usize::try_from(owner.local_ordinal()).ok() == Some(ordinal)
        }))
        .map(|owner| owner.evaluated_value().get())
    });
    let section_shape = construction
        .and_then(|construction| construction.section)
        .or_else(|| observation.and_then(|observation| observation.section))
        .and_then(|section| section.shape());
    let evidence = Evidence {
        scopes,
        owners,
        groups,
        observation,
        section_shape,
        parameters,
    };
    let mut properties = feature.source_properties.clone();
    let mut put = |name: &str, value: String| -> Result<(), CodecError> {
        let key = NonBlankString::new(format!("pipe:{name}"))
            .ok_or_else(|| CodecError::malformed("Pipe source property key is blank"))?;
        properties.insert(key, value);
        Ok(())
    };
    put(
        "source",
        serde_json::to_string(&evidence).map_err(CodecError::malformed)?,
    )?;
    let mut losses = Vec::new();
    let mut note = |code: F3dLossCode, reason: &str| {
        losses.push(code.note(format!("feature {}: {reason}", feature.id)))
    };
    if construction.is_none() {
        note(F3dLossCode::FeaturePipeSourceUnresolved, "the complete Pipe source relation is unresolved; independent source facts are retained");
    }
    for (name, value) in [
        "forward-fraction",
        "reverse-fraction",
        "section-size-cm",
        "section-thickness-cm",
    ]
    .into_iter()
    .zip(values)
    {
        if let Some(value) = value {
            put(name, value.to_string())?;
        } else {
            note(
                F3dLossCode::FeaturePipeScalarUnresolved,
                &format!("Pipe {name} has no unique locally admitted scalar owner"),
            );
        }
    }
    if let Some(hollow) = hollow {
        put("is-hollow", hollow.to_string())?;
    } else {
        note(
            F3dLossCode::FeaturePipeHollowUnresolved,
            "the Pipe hollow control is unresolved; its readable raw field is retained",
        );
    }
    let circular =
        operation.and_then(|_| read_circular_section(section_shape, hollow, values[2], values[3]));
    let section_unresolved = circular.is_none();
    let shape = match operation {
        Some(operation) => {
            let op = match operation {
                DesignExtrudeOperation::NewBody => SolidSweepOperation::NewBody,
                DesignExtrudeOperation::Join => SolidSweepOperation::Join,
                DesignExtrudeOperation::Cut => SolidSweepOperation::Cut,
                DesignExtrudeOperation::Intersect => SolidSweepOperation::Intersect,
            };
            if operation != DesignExtrudeOperation::NewBody {
                let target = only(evidence.groups.iter().copied().filter(|group| {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                    ) && !group.members().is_empty()
                        && group_is_claimed(scope, group)
                }));
                if operation == DesignExtrudeOperation::Intersect || target.is_none() {
                    note(F3dLossCode::FeaturePipeTargetsUnresolved, "the Pipe Boolean target relation is unresolved; available source groups and references are retained");
                } else {
                    note(
                        F3dLossCode::FeaturePipeTargetsRetainedNative,
                        "the Pipe Boolean input selections are retained in source evidence",
                    );
                }
            }
            SweepShape::Solid {
                op,
                section: circular
                    .map(SweepSection::Generated)
                    .unwrap_or_else(|| SweepSection::Unresolved(None)),
                sections: Vec::new(),
            }
        }
        None => {
            note(
                F3dLossCode::FeaturePipeOperationUnresolved,
                "the Pipe result operation is unresolved",
            );
            SweepShape::unresolved(None)
        }
    };
    if section_unresolved {
        note(F3dLossCode::FeaturePipeSectionUnresolved, "the Pipe section construction is unresolved; decoded shape, size, hollow state and thickness are retained in source evidence");
    }
    let path = only(evidence.groups.iter().copied().filter(|group| {
        group.role() == DesignOperandRole::ROLE_0X5
            && !group.members().is_empty()
            && group_is_claimed(scope, group)
    }))
    .map(|group| PathRef::Native(group.id.clone()));
    if path.is_none() {
        note(
            F3dLossCode::FeaturePipePathUnresolved,
            "the Pipe has no unique nonempty native path group",
        );
    } else if matches!(operation, None | Some(DesignExtrudeOperation::Intersect)) {
        note(
            F3dLossCode::FeaturePipePathRoleAssumed,
            "the unique role-0x5 group is assumed to carry the Pipe path",
        );
    }
    let path_extent = values[0].zip(values[1]).and_then(|(along, against)| {
        Some(SweepPathExtent {
            along_fraction: Fraction::new(along)?,
            against_fraction: Fraction::new(against)?,
        })
    });
    if path_extent.is_some() {
        note(F3dLossCode::FeaturePipePathParameterizationAssumed, "Pipe forward and reverse fractions are assumed to be the neutral along and against fractions; native origin and orientation remain unresolved");
    } else {
        note(F3dLossCode::FeaturePipePathExtentUnresolved, "the Pipe fractions cannot form a unique bounded neutral extent; decoded source values are retained");
    }
    if construction.is_some_and(|construction| construction.point_selection.is_some())
        || observation.is_some_and(|observation| !observation.point_records.is_empty())
    {
        note(F3dLossCode::FeaturePipePointOriginUnresolved, "Pipe point-selection coordinates, inputs and references are retained as source evidence; their path-origin interpretation is unresolved");
    }
    note(
        F3dLossCode::FeaturePipePathTangentAssumedFalse,
        "the undecoded Pipe path-tangent control is assumed false",
    );
    note(
        F3dLossCode::FeaturePipeLinearizeAssumedFalse,
        "the undecoded Pipe linearize control is assumed false",
    );
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            path,
            path_extent,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        }));
    feature.source_properties = properties;
    Ok(losses)
}

fn read_circular_section(
    shape: Option<DesignPipeSectionShape>,
    hollow: Option<bool>,
    size: Option<f64>,
    thickness: Option<f64>,
) -> Option<GeneratedSweepSection> {
    if shape != Some(DesignPipeSectionShape::Circular) {
        return None;
    }
    let radius = PositiveLength::new(size? * 10.0 / 2.0)?;
    let wall = if hollow? {
        Some(PositiveLength::new(thickness? * 10.0)?)
    } else {
        None
    };
    Some(GeneratedSweepSection::CircularRegion {
        region: SweepCircularRegion::new(radius, wall).ok()?,
    })
}

fn group_is_claimed(
    scope: Option<&DesignParameterScope>,
    group: &DesignConstructionOperandGroup,
) -> bool {
    scope.is_some_and(|scope| {
        usize::try_from(group.scope_reference_ordinal)
            .ok()
            .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
            == Some(&group.record_index)
    })
}
