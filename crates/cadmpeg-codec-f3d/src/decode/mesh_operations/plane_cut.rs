use super::{scope_bytes, Notes, MILLIMETRES_PER_DESIGN_LENGTH};
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::plane_cut::{MeshPlaneCutScope, PlaneCutReference};
use crate::design::decode::mesh_operations::{MeshOperationRecords, MESH_GROUP_GUID};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::mesh::{
    MeshModification, MeshPlaneCut, MeshPlaneCutFill, MeshPlaneCutMode, MeshPlaneCutSide,
    MeshSelection,
};
use cadmpeg_ir::features::selections::{NativeSelections, SelectionReference};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

fn reference(notes: &mut Notes<'_>, role: &str, value: PlaneCutReference<'_>) {
    let key = format!("mesh-plane-cut:{role}");
    match value {
        PlaneCutReference::Null => notes.property(key, "null"),
        PlaneCutReference::Local(index) => notes.property(key, index),
        PlaneCutReference::TypedLocal {
            target,
            inline_type,
        } => {
            notes.property(format!("{key}:inline-type"), inline_type);
            notes.property(key, target);
        }
        PlaneCutReference::Other(bytes) => notes.property(key, format!("{bytes:02x?}").as_str()),
    }
}

fn group_members(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    group: Option<PlaneCutReference<'_>>,
    role: &str,
    notes: &mut Notes<'_>,
) -> Result<Option<NativeSelections>, CodecError> {
    let group = match group {
        Some(PlaneCutReference::Local(group)) => Some(group),
        Some(PlaneCutReference::TypedLocal {
            target,
            inline_type,
        }) if inline_type.eq_ignore_ascii_case(MESH_GROUP_GUID) => Some(target),
        _ => None,
    };
    let mut members = None;
    if let Some(group) = group {
        let captured = records.group_body_references(ctx, group)?;
        ctx.charge_entities(
            u64_from_index(captured.member_references.len()),
            "F3D mesh plane cut member facts",
        )?;
        if let Some(count) = captured.declared_count {
            notes.property(format!("mesh-plane-cut:{role}-member-count"), count);
        }
        for (ordinal, (member, body)) in captured
            .member_references
            .iter()
            .zip(&captured.references)
            .enumerate()
        {
            for (field, index) in [("member", member), ("body", body)] {
                let key = format!("mesh-plane-cut:{role}-{field}:{ordinal}");
                if *index == 0 {
                    notes.property(key, "unread");
                } else {
                    notes.property(key, index);
                }
            }
        }
        if let Some(error) = captured.framing {
            notes.property(
                format!("mesh-plane-cut:{role}-framing"),
                error.to_string().as_str(),
            );
        } else if !captured.references.contains(&0) {
            members = NativeSelections::try_from(
                captured
                    .references
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>(),
            )
            .ok();
        }
    }
    Ok(members)
}

fn source(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    groups: [Option<PlaneCutReference<'_>>; 2],
    notes: &mut Notes<'_>,
) -> Result<MeshSelection, CodecError> {
    let members = group_members(ctx, records, groups[0], "source", notes)?;
    group_members(ctx, records, groups[1], "output", notes)?;
    if members.is_some() {
        notes.losses.push(F3dLossCode::FeatureMeshPlaneCutMembershipAssumed.note(format!(
            "Mesh plane cut {} reads ordered source-member body references as whole-body input selections.", notes.source
        )));
    } else {
        notes.losses.push(F3dLossCode::FeatureMeshPlaneCutSelectionUnresolved.note(format!(
            "Mesh plane cut {} retains its source-group and member facts with unresolved complete membership.", notes.source
        )));
    }
    notes.losses.push(F3dLossCode::FeatureMeshPlaneCutSourceUnresolved.note(format!(
        "Mesh plane cut {} retains historical input and output geometry identities with unresolved tessellations.", notes.source
    )));
    let native = SelectionReference::try_from(notes.source.to_owned())
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    Ok(MeshSelection::Native { native, members })
}

fn controls(parsed: &MeshPlaneCutScope<'_>, notes: &mut Notes<'_>) -> MeshPlaneCut {
    let mut cut = MeshPlaneCut::default();
    if parsed.matrix.iter().any(Option::is_some) {
        for (code, message) in [
            (
                F3dLossCode::FeatureMeshPlaneCutMatrixOrientationAssumed,
                "reads the serialized matrix as row-major with translation column and plane z-axis",
            ),
            (
                F3dLossCode::FeatureMeshPlaneCutUnitsAssumed,
                "reads matrix translations in centimetres",
            ),
            (
                F3dLossCode::FeatureMeshPlaneCutModelSpaceAssumed,
                "reads the plane in model coordinates",
            ),
        ] {
            notes
                .losses
                .push(code.note(format!("Mesh plane cut {} {message}.", notes.source)));
        }
        let triple = |indices: [usize; 3]| -> Option<[f64; 3]> {
            Some([
                f64::from_bits(parsed.matrix[indices[0]]?),
                f64::from_bits(parsed.matrix[indices[1]]?),
                f64::from_bits(parsed.matrix[indices[2]]?),
            ])
        };
        cut.origin = triple([3, 7, 11]).and_then(|[x, y, z]| {
            FinitePoint3::new(Point3::new(
                x * MILLIMETRES_PER_DESIGN_LENGTH,
                y * MILLIMETRES_PER_DESIGN_LENGTH,
                z * MILLIMETRES_PER_DESIGN_LENGTH,
            ))
        });
        cut.normal =
            triple([2, 6, 10]).and_then(|[x, y, z]| FeatureDirection3::new(Vector3::new(x, y, z)));
        if cut.origin.is_none() || cut.normal.is_none() {
            notes
                .losses
                .push(F3dLossCode::FeatureMeshPlaneCutPlanePartial.note(format!(
                "Mesh plane cut {} retains matrix bits and independently checked plane components.",
                notes.source
            )));
        }
    }
    cut.mode = match parsed.mode {
        Some(0) => Some(MeshPlaneCutMode::Trim),
        Some(1) => Some(MeshPlaneCutMode::SplitBodies),
        Some(2) => {
            notes.losses.push(
                F3dLossCode::FeatureMeshPlaneCutSplitFacesAssumed.note(format!(
                    "Mesh plane cut {} reads mode selector 2 as Split Faces.",
                    notes.source
                )),
            );
            Some(MeshPlaneCutMode::SplitFaces)
        }
        _ => None,
    };
    cut.fill = match parsed.fill {
        Some(0) => Some(MeshPlaneCutFill::NoFill),
        Some(1) => Some(MeshPlaneCutFill::Minimal),
        Some(2) => {
            notes.losses.push(
                F3dLossCode::FeatureMeshPlaneCutUniformFillAssumed.note(format!(
                    "Mesh plane cut {} reads fill selector 2 as Uniform.",
                    notes.source
                )),
            );
            Some(MeshPlaneCutFill::Uniform)
        }
        _ => None,
    };
    cut.retained_side = match parsed.side {
        Some(value @ 0..=1) => {
            notes.losses.push(F3dLossCode::FeatureMeshPlaneCutRetainedSideAssumed.note(format!("Mesh plane cut {} reads control byte {value} as the retained half-space, with 0 positive and 1 negative.", notes.source)));
            Some(if value == 0 {
                MeshPlaneCutSide::Positive
            } else {
                MeshPlaneCutSide::Negative
            })
        }
        _ => None,
    };
    if parsed.mode.is_some() && cut.mode.is_none()
        || parsed.fill.is_some() && cut.fill.is_none()
        || parsed.side.is_some() && cut.retained_side.is_none()
    {
        notes.losses.push(F3dLossCode::FeatureMeshPlaneCutSelectorUnruled.note(format!("Mesh plane cut {} retains unsupported mode, fill or side selectors in source properties.", notes.source)));
    }
    cut
}

fn captured(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    parsed: &MeshPlaneCutScope<'_>,
    notes: &mut Notes<'_>,
) -> Result<FeatureOperation, CodecError> {
    notes.property(
        "mesh-plane-cut:prefix-bytes",
        format!("{:02x?}", parsed.prefix).as_str(),
    );
    for (name, value) in [
        ("flag", parsed.flag.map(u32::from)),
        ("prefix-word", parsed.prefix_word),
        ("mode", parsed.mode),
        ("matrix-count", parsed.matrix_count),
        ("fill", parsed.fill),
        ("control-byte", parsed.side.map(u32::from)),
        ("tail-word", parsed.tail_word),
        ("tail-byte", parsed.tail_byte.map(u32::from)),
    ] {
        if let Some(value) = value {
            notes.property(format!("mesh-plane-cut:{name}"), value);
        }
    }
    for (index, bits) in parsed.matrix.iter().enumerate() {
        if let Some(bits) = bits {
            notes.property(
                format!("mesh-plane-cut:matrix-bits:{index}"),
                format!("{bits:016x}").as_str(),
            );
        }
    }
    for (role, value) in [
        "source-group",
        "output-group",
        "construction-group",
        "output-geometry",
        "input-geometry",
        "source-group-repeat",
    ]
    .into_iter()
    .zip(parsed.references)
    {
        if let Some(value) = value {
            reference(notes, role, value);
        }
    }
    if parsed.prefix_word.is_some() {
        notes
            .losses
            .push(F3dLossCode::MeshScopePrefixUninterpreted.note(format!(
                "Mesh plane cut {} retains its flagged prefix word.",
                notes.source
            )));
    }
    if parsed.prefix.iter().any(|byte| *byte != 0)
        || parsed.tail_word.is_some_and(|value| value != 0)
        || parsed.tail_byte.is_some_and(|value| value != 0)
        || parsed.references[2].is_some()
        || parsed.references[5].is_some_and(|value| Some(value) != parsed.references[0])
    {
        notes.losses.push(F3dLossCode::FeatureMeshPlaneCutFieldsUninterpreted.note(format!("Mesh plane cut {} retains its construction reference and fixed-field deviations as source properties.", notes.source)));
    }
    if let Some(error) = parsed.framing {
        notes.property("mesh-plane-cut:framing", error.to_string().as_str());
        notes
            .losses
            .push(F3dLossCode::FeatureMeshPlaneCutTailPartial.note(format!(
                "Mesh plane cut {} retains fields before its unresolved {error}.",
                notes.source
            )));
    }
    let cut = controls(parsed, notes);
    let source = source(
        ctx,
        records,
        [parsed.references[0], parsed.references[1]],
        notes,
    )?;
    Ok(FeatureOperation::MeshModify {
        source,
        modification: MeshModification::PlaneCut { cut },
    })
}

pub(in crate::decode) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut cache = HashMap::<String, Option<MeshOperationRecords<'_>>>::new();
    for feature in features {
        if feature.source_tag.as_deref() != Some("MeshPlaneCut") {
            continue;
        }
        let mut candidates = scopes
            .iter()
            .filter(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()));
        let scope = candidates.next().filter(|_| candidates.next().is_none());
        let anchor = scope.map_or_else(|| feature.id.to_string(), |scope| scope.id.clone());
        let mut notes = Notes {
            source: &anchor,
            properties: Vec::new(),
            losses: Vec::new(),
        };
        if let FeatureDefinition::Operation(FeatureOperation::Native { parameters, .. }) =
            feature.evaluation.definition()
        {
            for (name, expression) in parameters {
                notes.property(format!("parameter:{name}"), expression);
            }
        }
        let mut operation = FeatureOperation::MeshModify {
            source: MeshSelection::Unresolved {
                feature: feature.id.clone(),
            },
            modification: MeshModification::PlaneCut {
                cut: MeshPlaneCut::default(),
            },
        };
        let mut admitted = false;
        if let Some((scope, (entry, bytes))) =
            scope.and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
        {
            let indexed = match cache.entry(entry) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let meta_name = entry
                        .key()
                        .strip_suffix("BulkStream.dat")
                        .map(|prefix| format!("{prefix}MetaStream.dat"));
                    let unique = scan
                        .entries
                        .iter()
                        .filter(|candidate| candidate.name == *entry.key())
                        .count()
                        == 1
                        && meta_name.as_ref().is_some_and(|name| {
                            scan.entries
                                .iter()
                                .filter(|candidate| candidate.name == *name)
                                .count()
                                == 1
                        });
                    let meta = if unique {
                        match crate::design::decode::meta::metadata_for_bulk_stream(
                            scan,
                            entry.key(),
                        ) {
                            Ok(meta) => meta,
                            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };
                    entry.insert(meta.map(|meta| MeshOperationRecords::index(bytes, meta)))
                }
            };
            if let Some(records) = indexed {
                match records.plane_cut_scope(scope.record_index) {
                    Ok(parsed) => {
                        operation = captured(ctx, records, &parsed, &mut notes)?;
                        admitted = true;
                    }
                    Err(error) => {
                        notes.property("mesh-plane-cut:scope-framing", error.to_string().as_str());
                    }
                }
            }
        }
        if !admitted {
            notes.losses.push(F3dLossCode::FeatureMeshPlaneCutScopeUnparsed.note(format!("Mesh plane cut {anchor} retains its actual feature anchor with unresolved scope, stream or metadata.")));
        }
        for (key, value) in notes.properties {
            let key = NonBlankString::new(key)
                .ok_or_else(|| CodecError::malformed("blank mesh plane cut property key"))?;
            feature.source_properties.insert(key, value);
        }
        losses.append(&mut notes.losses);
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(operation));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
