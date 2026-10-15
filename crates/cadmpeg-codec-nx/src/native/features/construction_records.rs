use super::block_reference::{BlockReferencePosition, FeatureBlockConstructionReference};
use super::body_scalar_triple::FeatureOperationBodyScalarTriple;
use super::extrude_32::{FeatureExtrude32Construction, FeatureExtrudePayload32Branch};
use super::joined_payload::JoinedPayload;
use super::object_frame::DataBlockObjectFrame;
use super::payload_content::FeaturePayloadContent;
use super::payload_name::FeaturePayloadName;
use super::point_scalar_lane::{FeaturePointConstructionScalarLane, PointScalarPositions};
use super::reference::ConstructionReference;
use super::swp104_branch::FeatureSwp104LeadingBranch;
use super::terminal_discriminator::FeatureOperationTerminalDiscriminator;
use super::{
    construction_payload_frames, format_feature_child_id, format_feature_history_id,
    offset_data_block_bytes, parse_sketch_point_name, visit_feature_history_operation_records,
    FeatureBlockConstruction, FeatureBlockDimension, FeatureBlockDimensions,
    FeatureBlockPayloadNamedRecord, FeatureBlockPayloadPoint, FeatureBlockPayloadPointGroup,
    FeatureBodyReference, FeatureConstructionMember, FeatureConstructionOwner,
    FeatureConstructionPayload, FeatureExtrudeConstructionProfile,
    FeatureExtrudeConstructionProfileReference, FeatureExtrudePayloadHeader,
    FeatureExtrudeProfileReference, FeatureInputBlock, FeatureOperationBody11Continuation,
    FeatureOperationBodyMember, FeatureOperationBodyOperand, FeatureOperationBodyReferenceLane,
    FeatureOperationBodyReferences, FeatureOperationLabel, FeatureParameterBinding,
    FeaturePayloadScalar, FeaturePayloadScalarPair, FeaturePointConstructionHeader,
    FeatureProjectedCurveConstructionString, FeatureProjectedCurveKind,
    FeatureProjectedCurveReference, FeatureScalarPairPayload, FeatureScalarPayload,
    FeatureSurfaceConstructionPayload, FeatureSurfaceConstructionReference,
    FeatureSurfaceConstructionString, FeatureThruCurveConstructionEnvelope,
};
use crate::container::Container;
use crate::native::om::{ExpressionDeclaration, ParameterFormula};
use crate::native::segments::SegmentBodyBinding;
use crate::om::compact::LocatedCompactIndex;
use crate::om::reference_index::PayloadIndexToken;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ResolvedFeaturePayloadReference {
    pub(super) section_key: String,
    pub(super) operation_ordinal: usize,
    pub(super) ordinal: usize,
    pub(super) token: PayloadIndexToken,
    pub(super) data_block: Option<String>,
    pub(super) source_offset: u64,
}

pub(super) fn resolved_feature_payload_references(
    container: &Container,
    decode: impl Fn(
        crate::om::operation_record::OperationPayload<'_>,
        u64,
    ) -> Option<Vec<(PayloadIndexToken, u64)>>,
) -> Result<Vec<ResolvedFeaturePayloadReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) = decode(record.payload_view(), entry_offset) else {
                return;
            };
            for (ordinal, (token, source_offset)) in decoded.into_iter().enumerate() {
                let data_block = unique_offset_data_block_id(&indexed, token.value());
                let section_key = section_key.to_owned();

                references.push(ResolvedFeaturePayloadReference {
                    section_key,
                    operation_ordinal,
                    ordinal,
                    token,
                    data_block,
                    source_offset,
                });
            }
        },
    )?;
    Ok(references)
}

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub(in crate::native) fn feature_projected_curve_references(
    container: &Container,
) -> Result<Vec<FeatureProjectedCurveReference>, CodecError> {
    let references = resolved_feature_payload_references(container, |record, base| {
        crate::om::projected_references::ProjectedCurveReferences::read(record).and_then(|field| {
            field
                .into_references()
                .into_iter()
                .map(|reference| {
                    Some((
                        reference.token,
                        base.checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))?,
                    ))
                })
                .collect()
        })
    })?;
    let mut output = Vec::new();
    for reference in references {
        let operation_label = format_feature_history_id(
            "operation-label",
            &reference.section_key,
            reference.operation_ordinal,
            None,
        );
        let id = format_feature_history_id(
            "projected-curve-reference",
            &reference.section_key,
            reference.operation_ordinal,
            Some(reference.ordinal),
        );

        output.push(FeatureProjectedCurveReference {
            id,
            operation_label,
            ordinal: u32::try_from(reference.ordinal).map_err(|_| {
                CodecError::malformed("NX projected curve reference ordinal exceeds u32")
            })?,
            token: reference.token,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        });
    }
    Ok(output)
}

/// Reconstruct ordered logical payloads from projected-curve reference fields.
pub(in crate::native) fn feature_projected_curve_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    labels: &[FeatureOperationLabel],
    references: &[FeatureProjectedCurveReference],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    let operations = references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    let mut payloads = Vec::new();
    for operation_label in operations {
        let Some(kind) = kinds.get(operation_label) else {
            continue;
        };
        let (operation_kind, expected_len) = match *kind {
            "CPROJ" => (FeatureProjectedCurveKind::Projected, 3),
            "CPROJ_CMB" => (FeatureProjectedCurveKind::Combined, 8),
            _ => continue,
        };

        let mut field = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        field.sort_by_key(|first| first.ordinal);
        if field.len() != expected_len
            || field
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| u32::try_from(ordinal).ok() != Some(reference.ordinal))
        {
            continue;
        }
        let Some(data_blocks) = field
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(store) = data_blocks
            .first()
            .and_then(|block| block.rsplit_once(":block#"))
            .map(|(store, _)| store)
        else {
            continue;
        };
        if data_blocks.iter().any(|block| {
            block
                .rsplit_once(":block#")
                .is_none_or(|(prefix, _)| prefix != store)
        }) {
            continue;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let Some((_, operation_key)) = operation_label.rsplit_once('#') else {
            continue;
        };
        let id = format!("nx:feature-history:projected-curve-construction-payload#{operation_key}");
        let operation_label = operation_label.to_owned();
        let construction_references = field
            .into_iter()
            .map(|reference| reference.id.clone())
            .collect();

        payloads.push(FeatureConstructionPayload {
            id,
            operation_label,
            owner: FeatureConstructionOwner::ProjectedCurve {
                operation_kind,
                construction_references,
            },
            content,
        });
    }
    Ok(payloads)
}

/// Decode canonical printable strings from reconstructed projected-curve payloads.
pub(in crate::native) fn feature_projected_curve_construction_strings(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeatureProjectedCurveConstructionString>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let mut strings = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks) else {
            continue;
        };
        for (ordinal, value) in crate::om::string_values(joined.bytes(), 0)
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = format!("{}-string-{ordinal:010}", payload.id);
            let operation_label = payload.operation_label.clone();
            let construction_payload = payload.id.clone();
            let text = value.value.as_str().to_owned();
            let value = crate::printable_string::PrintableString::new(text)
                .map_err(|error| CodecError::Malformed(error.into()))?;

            strings.push(FeatureProjectedCurveConstructionString {
                id,
                operation_label,
                construction_payload,
                ordinal: u32::try_from(ordinal).map_err(|_| {
                    CodecError::malformed("NX projected curve string ordinal exceeds u32")
                })?,
                value,
                payload_offset,
                source_offset,
            });
        }
    }
    Ok(strings)
}

/// Decode exact point-feature construction headers without assigning coordinate semantics.
pub(in crate::native) fn feature_point_construction_headers(
    container: &Container,
) -> Result<Vec<FeaturePointConstructionHeader>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut headers = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(header) = crate::om::point_feature_payload_header(record.payload_view())
            else {
                return;
            };
            let projected = (|| -> Result<(), CodecError> {
                let id = format_feature_history_id(
                    "point-construction-header",
                    section_key,
                    operation_ordinal,
                    None,
                );
                let operation_label = format_feature_history_id(
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                );
                let data_block =
                    unique_offset_data_block_id(&indexed, header.reference.token.value());
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(
                        header.reference.offset,
                    ))
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "NX point construction header source offset overflows",
                        )
                    })?;

                headers.push(FeaturePointConstructionHeader {
                    id,
                    operation_label,
                    token: header.reference.token,
                    data_block,
                    mode: header.mode,
                    source_offset,
                });
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(headers)
}

/// Decode exact scalar lanes selected by uniquely resolved point-feature headers.
pub(in crate::native) fn feature_point_construction_scalar_lanes(
    container: &Container,
    headers: &[FeaturePointConstructionHeader],
) -> Result<Vec<FeaturePointConstructionScalarLane>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut lanes = Vec::new();
    for header in headers {
        let Some(expected_target) = header.data_block.as_deref() else {
            continue;
        };
        let Ok(target_ordinal) = usize::try_from(header.token.value()) else {
            continue;
        };
        if target_ordinal < 2 {
            continue;
        }
        let Some((expected_section, expected_block)) = expected_target
            .strip_prefix("nx:om-data-blocks-")
            .and_then(|tail| tail.split_once(":block#"))
        else {
            continue;
        };
        let section_ordinal = expected_section.parse::<usize>().ok();
        let block_ordinal = expected_block.parse::<usize>().ok();
        let canonical = section_ordinal.is_some_and(|section| {
            expected_section.len()
                == section
                    .checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1)
        }) && block_ordinal.is_some_and(|block| {
            expected_block.len()
                == block
                    .checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1)
        });
        if !canonical || block_ordinal != Some(target_ordinal) {
            continue;
        }

        let mut candidate = None;
        let mut ambiguous = false;
        for (ordinal, (entry, section)) in indexed.iter().enumerate() {
            if section_ordinal != Some(ordinal) {
                continue;
            }
            let Some((_, _, records)) = section.as_offset_only() else {
                continue;
            };
            let (Some(preceding), Some(target)) = (
                records.get(target_ordinal - 2),
                records.get(target_ordinal - 1),
            ) else {
                continue;
            };
            let Some(lane) = crate::om::point_feature_scalar_lane(preceding.bytes, target.bytes)
            else {
                continue;
            };
            if candidate.is_some() {
                ambiguous = true;
                break;
            }
            candidate = Some((ordinal, *entry, preceding, target, lane));
        }
        if ambiguous {
            continue;
        }
        let Some((section_ordinal, entry, preceding, target, lane)) = candidate else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let Some(first_source_offset) = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(preceding.offset))
            .and_then(|base| {
                base.checked_add(cadmpeg_core::decode::u64_from_index(
                    lane.value_offsets()[0],
                ))
            })
        else {
            continue;
        };
        let Some(target_source_offset) =
            entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(target.offset))
        else {
            continue;
        };
        let Ok(positions) = PointScalarPositions::new(first_source_offset, target_source_offset)
        else {
            continue;
        };
        let id = header.id.replacen(
            "point-construction-header#",
            "point-construction-scalar-lane#",
            1,
        );
        let operation_label = header.operation_label.clone();
        let construction_header = header.id.clone();
        let first_block = format!(
            "nx:om-data-blocks-{section_ordinal}:block#{}",
            target_ordinal - 1
        );
        let second_block = format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}");
        let data_blocks = [first_block, second_block];

        lanes.push(FeaturePointConstructionScalarLane {
            id,
            operation_label,
            construction_header,
            data_blocks,
            scalars: lane.values,
            positions,
        });
    }
    Ok(lanes)
}

/// Decode and resolve the exact common reference envelope in surface-feature
/// payloads without assigning section or guide semantics to its slots.
pub(in crate::native) fn feature_surface_construction_references(
    container: &Container,
) -> Result<Vec<FeatureSurfaceConstructionReference>, CodecError> {
    let references = resolved_feature_payload_references(container, |record, base| {
        crate::om::surface_envelope::surface_feature_payload_references(record)
            .and_then(|field| field.relocate(base))
            .map(|field| field.references().into_iter().collect())
            .or_else(|| {
                crate::om::surface_envelope::thru_curve_payload_references(record)
                    .and_then(|field| field.relocate(base))
                    .map(|field| field.references().into_iter().collect())
            })
    })?;
    let mut output = Vec::new();
    for reference in references {
        let operation_label = format_feature_history_id(
            "operation-label",
            &reference.section_key,
            reference.operation_ordinal,
            None,
        );
        let id = format_feature_history_id(
            "surface-construction-reference",
            &reference.section_key,
            reference.operation_ordinal,
            Some(reference.ordinal),
        );

        output.push(FeatureSurfaceConstructionReference {
            id,
            operation_label,
            ordinal: u32::try_from(reference.ordinal).map_err(|_| {
                CodecError::malformed("NX surface construction reference ordinal exceeds u32")
            })?,
            token: reference.token,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        });
    }
    Ok(output)
}

/// Decode the exact leading construction envelope in each `THRU_CURVE` payload.
pub(in crate::native) fn feature_thru_curve_construction_envelopes(
    container: &Container,
) -> Result<Vec<FeatureThruCurveConstructionEnvelope>, cadmpeg_core::CodecError> {
    let mut envelopes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(field) =
                crate::om::surface_envelope::thru_curve_payload_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            let id = format_feature_history_id(
                "thru-curve-construction-envelope",
                section_key,
                operation_ordinal,
                None,
            );
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);

            envelopes.push(FeatureThruCurveConstructionEnvelope {
                id,
                operation_label,
                discriminator: field.discriminator,
                controls: field.controls,
                trailing_control: field.trailing_control,
                trailing_value: field.trailing_value.into(),
                source_offset: field.origin(),
            });
        },
    )?;
    Ok(envelopes)
}

/// Decode and resolve each exact leading `SWP104` construction branch.
pub(in crate::native) fn feature_swp104_leading_branches(
    container: &Container,
) -> Result<Vec<FeatureSwp104LeadingBranch>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut branches = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let branch = match crate::om::swp104_payload_leading_branch(record.payload_view()) {
                Ok(branch) => branch,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let Some(branch) = branch else {
                return;
            };
            let Some(source_offset) = entry_offset.checked_add(
                cadmpeg_core::decode::u64_from_index(record.payload_offset()),
            ) else {
                return;
            };
            let id = format_feature_history_id(
                "swp104-leading-branch",
                section_key,
                operation_ordinal,
                None,
            );
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);
            if let Some(branch) = FeatureSwp104LeadingBranch::from_source(
                id,
                operation_label,
                source_offset,
                branch,
                |token| unique_offset_data_block_id(&indexed, token.value()),
            ) {
                branches.push(branch);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(branches)
}

/// Reconstruct ordered logical payloads from complete surface-construction graphs.
pub(in crate::native) fn feature_surface_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    references: &[FeatureSurfaceConstructionReference],
) -> Result<Vec<FeatureSurfaceConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let labels = references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    let mut output = Vec::new();
    for operation_label in labels {
        let mut graph = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        graph.sort_by_key(|first| first.ordinal);
        if graph
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
        {
            continue;
        }
        let Ok(graph): Result<[&FeatureSurfaceConstructionReference; 14], _> = graph.try_into()
        else {
            continue;
        };
        let Some(data_blocks) = graph
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(store) = data_blocks
            .first()
            .and_then(|block| block.rsplit_once(":block#").map(|(store, _)| store))
        else {
            continue;
        };
        if data_blocks.iter().any(|block| {
            block
                .rsplit_once(":block#")
                .is_none_or(|(prefix, _)| prefix != store)
        }) {
            continue;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let Some((_, operation_key)) = operation_label.rsplit_once('#') else {
            continue;
        };
        let id = format!("nx:feature-history:surface-construction-payload#{operation_key}");
        let construction_references = graph.map(|reference| reference.id.clone());

        output.push(FeatureSurfaceConstructionPayload {
            id,
            operation_label: operation_label.to_owned(),
            construction_references,
            content,
        });
    }
    Ok(output)
}

/// Decode exact scalar-pair frames from reconstructed surface payloads.
pub(in crate::native) fn feature_surface_construction_scalar_pairs(
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Result<Vec<FeaturePayloadScalarPair>, cadmpeg_core::CodecError> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::binary64_pair::object_pairs,
        |payload, ordinal, pair, source_offset| {
            let Some((frame, first, second, source)) = (|| {
                Some((
                    pair.into_wire_frame()?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                    source_offset(pair.offset())?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(&payload.id, "-scalar-pair-", ordinal);
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("NX surface scalar pair ordinal exceeds u32"))?;
            Ok(Some(FeaturePayloadScalarPair {
                id,
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::SurfaceConstruction {
                    surface_construction_payload: payload.id.clone(),
                    frame,
                },
                ordinal,
                value_source_offsets: [first, second],
                source_offset: source,
            }))
        },
    )
}

/// Decode exact printable string frames from reconstructed surface payloads.
pub(in crate::native) fn feature_surface_construction_strings(
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Result<Vec<FeatureSurfaceConstructionString>, CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let mut strings = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks) else {
            continue;
        };
        for (ordinal, value) in crate::om::surface_payload_strings(joined.bytes())
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed("NX surface payload string ordinal exceeds u32")
            })?;
            let id = format_feature_child_id(&payload.id, "-string-", ordinal);
            let text = value.value.as_str().to_owned();
            let value = crate::payload_text::PayloadText::new(text)
                .map_err(|error| CodecError::Malformed(error.to_owned()))?;

            strings.push(FeatureSurfaceConstructionString {
                id,
                operation_label: payload.operation_label.clone(),
                surface_construction_payload: payload.id.clone(),
                ordinal: ordinal_u32,
                value,
                payload_offset,
                source_offset,
            });
        }
    }
    Ok(strings)
}

/// Decode and resolve the witnessed ordered profile list in extrusion payloads.
pub(in crate::native) fn feature_extrude_profile_references(
    container: &Container,
) -> Result<Vec<FeatureExtrudeProfileReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut references = Vec::new();
    let mut refusal: Option<CodecError> = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if refusal.is_some() {
                return;
            }
            let Some(decoded) =
                crate::om::extrude_profile::extrude_profile_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            let projected = (|| -> Result<(), CodecError> {
                for (ordinal, (token, source_offset, witness_source_offset)) in
                    decoded.references().enumerate()
                {
                    let id = format_feature_history_id(
                        "extrude-profile-reference",
                        section_key,
                        operation_ordinal,
                        Some(ordinal),
                    );
                    let operation_label = format_feature_history_id(
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    );
                    let data_block = unique_offset_data_block_id(&indexed, token.value());
                    let ordinal = u32::try_from(ordinal).map_err(|_| {
                        CodecError::malformed("NX extrude profile reference ordinal exceeds u32")
                    })?;

                    references.push(FeatureExtrudeProfileReference {
                        id,
                        operation_label,
                        ordinal,
                        field_tag: decoded.field_tag(),
                        witness_source_offset,
                        token,
                        data_block,
                        source_offset,
                    });
                }
                Ok(())
            })();
            if let Err(error) = projected {
                refusal = Some(error);
            }
        },
    )?;
    if let Some(error) = refusal {
        return Err(error);
    }
    Ok(references)
}

/// Decode fixed scalar headers from bounded extrusion payloads.
pub(in crate::native) fn feature_extrude_payload_headers(
    container: &Container,
) -> Result<Vec<FeatureExtrudePayloadHeader>, cadmpeg_core::CodecError> {
    let mut headers = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(header) = crate::om::extrude_payload_header(record.payload_view()) else {
                return;
            };
            let projected = (|| -> Result<(), CodecError> {
                let id = format_feature_history_id(
                    "extrude-payload-header",
                    section_key,
                    operation_ordinal,
                    None,
                );
                let operation_label = format_feature_history_id(
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                );
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(header.offset))
                    .ok_or_else(|| {
                        CodecError::malformed("NX extrude header source offset overflows")
                    })?;

                headers.push(FeatureExtrudePayloadHeader {
                    id,
                    operation_label,
                    scalars: header.scalars,
                    source_offset,
                });
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(headers)
}

/// Decode exact terminal discriminator lanes from bounded operation payloads.
pub(in crate::native) fn feature_operation_terminal_discriminators(
    container: &Container,
) -> Result<Vec<FeatureOperationTerminalDiscriminator>, cadmpeg_core::CodecError> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let frame = crate::om::terminal_discriminator::operation_terminal_discriminator(
                record.payload_view(),
            );
            let Some(frame) = frame.and_then(|frame| frame.relocate(entry_offset)) else {
                return;
            };
            let id = format_feature_history_id(
                "operation-terminal-discriminator",
                section_key,
                operation_ordinal,
                None,
            );
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);

            lanes.push(FeatureOperationTerminalDiscriminator {
                id,
                operation_label,
                frame,
            });
        },
    )?;
    Ok(lanes)
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub(in crate::native) fn feature_operation_body_scalar_triples(
    container: &Container,
) -> Result<Vec<FeatureOperationBodyScalarTriple>, cadmpeg_core::CodecError> {
    let mut triples = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let rows =
                crate::om::body_scalar_triple::operation_body_scalar_triples(record.body_view());
            for triple in rows {
                let Some(scalars) = triple.scalars.relocate(entry_offset) else {
                    continue;
                };
                let id = format!(
                    "nx:feature-history:operation-body-scalar-triple#{section_key}-{operation_ordinal:010}-{}",
                    triple.body_reference_ordinal
                );
                let operation_label = format_feature_history_id(
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                );
                triples.push(FeatureOperationBodyScalarTriple {
                    id,
                    operation_label,
                    body_reference_ordinal: triple.body_reference_ordinal,
                    body_object_index: triple.body_object_index,
                    branch: triple.branch,
                    scalars,
                });
            }
        },
    )?;
    Ok(triples)
}

/// Decode ordered member lanes following branch-`11` operation body clauses.
pub(in crate::native) fn feature_operation_body_members(
    container: &Container,
) -> Result<Vec<FeatureOperationBodyMember>, cadmpeg_core::CodecError> {
    let mut members = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let groups = crate::om::operation_body_members(record.body_view());
            let projected = (|| -> Result<(), CodecError> {
                for group in groups {
                    for (ordinal, member) in group.members.into_iter().enumerate() {
                        let ordinal = u32::try_from(ordinal).map_err(|_| {
                            CodecError::malformed("NX operation body member ordinal exceeds u32")
                        })?;
                        let id = format!("nx:feature-history:operation-body-member#{section_key}-{operation_ordinal:010}-{}-{ordinal}",
                                group.body_reference_ordinal);
                        let operation_label = format_feature_history_id(
                            "operation-label",
                            section_key,
                            operation_ordinal,
                            None,
                        );
                        let offset = entry_offset
                            .checked_add(cadmpeg_core::decode::u64_from_index(member.offset))
                            .ok_or_else(|| {
                                CodecError::malformed(
                                    "NX operation body member source offset overflows",
                                )
                            })?;

                        members.push(FeatureOperationBodyMember {
                            id,
                            operation_label,
                            body_reference_ordinal: group.body_reference_ordinal,
                            body_object_index: group.body_object_index,
                            ordinal,
                            member: LocatedCompactIndex {
                                atom: member.atom,
                                offset,
                            },
                        });
                    }
                }
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(members)
}

/// Resolve wrapped operation members that name known feature-body identities.
pub(in crate::native) fn feature_operation_body_operands(
    members: &[FeatureOperationBodyMember],
    references: &[FeatureBodyReference],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureOperationBodyOperand> {
    let mut operands = Vec::new();
    for member in members {
        if member.member.atom.value() == member.body_object_index {
            continue;
        }
        let (has_inputs, member_store) = unique_operation_store(inputs, &member.operation_label);
        if member_store.is_none() && has_inputs {
            continue;
        }
        let operand_data_block = member_store
            .map(|store| format!("{store}:block#{}", member.member.atom.value()))
            .filter(|id| blocks.iter().any(|block| block.id == *id));
        if member_store.is_some() && operand_data_block.is_none() {
            continue;
        }
        let same_namespace_reference = references.iter().any(|reference| match member_store {
            Some(store) => {
                unique_operation_store(inputs, &reference.operation_label).1 == Some(store)
            }
            None => !unique_operation_store(inputs, &reference.operation_label).0,
        });
        let mut segment_body_bindings = Vec::new();
        if member_store.is_none() {
            for binding in bindings.iter().filter(|binding| {
                binding.body_object_index == member.member.atom.value()
                    || binding.body_alias_object_index == member.member.atom.value()
            }) {
                segment_body_bindings.push(binding.id.clone());
            }
        }
        if !same_namespace_reference && segment_body_bindings.is_empty() {
            continue;
        }

        operands.push(FeatureOperationBodyOperand {
            id: member
                .id
                .replacen("operation-body-member", "operation-body-operand", 1),
            operation_label: member.operation_label.clone(),
            body_object_index: member.body_object_index,
            body_reference_ordinal: member.body_reference_ordinal,
            ordinal: member.ordinal,
            operand: member.member,
            operand_data_block,
            segment_body_bindings,
        });
    }
    operands
}

fn unique_operation_store<'a>(
    inputs: &'a [FeatureInputBlock],
    operation: &str,
) -> (bool, Option<&'a str>) {
    let mut has_inputs = false;
    let mut store = None;
    for input in inputs
        .iter()
        .filter(|input| input.operation_label == operation)
    {
        has_inputs = true;
        if let Some((candidate, _)) = input.data_block.rsplit_once(":block#") {
            match store {
                None => store = Some(candidate),
                Some(existing) if existing != candidate => return (true, None),
                Some(_) => {}
            }
        }
    }
    (has_inputs, store)
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub(in crate::native) fn feature_operation_body_11_continuations(
    container: &Container,
) -> Result<Vec<FeatureOperationBody11Continuation>, cadmpeg_core::CodecError> {
    let mut continuations = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let rows = crate::om::operation_body_11_continuations(record.body_view());
            let projected = (|| -> Result<(), CodecError> {
                for continuation in rows {
                    let id = format!("nx:feature-history:trim-body-11-continuation#{section_key}-{operation_ordinal:010}-{}",
                            continuation.body_reference_ordinal);
                    let operation_label = format_feature_history_id(
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    );
                    let offset = entry_offset
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            continuation.continuation.offset,
                        ))
                        .ok_or_else(|| {
                            CodecError::malformed("NX trim body continuation offset overflows")
                        })?;
                    let terminal_source_offset = entry_offset
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            continuation.terminal.offset,
                        ))
                        .ok_or_else(|| {
                            CodecError::malformed("NX trim body terminal offset overflows")
                        })?;

                    continuations.push(FeatureOperationBody11Continuation {
                        id,
                        operation_label,
                        body_reference_ordinal: continuation.body_reference_ordinal,
                        body_object_index: continuation.body_object_index,
                        continuation: crate::om::compact::LocatedCompactIndex {
                            atom: continuation.continuation.atom,
                            offset,
                        },
                        terminal: continuation.terminal.token,
                        terminal_source_offset,
                    });
                }
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(continuations)
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub(in crate::native) fn feature_operation_body_reference_lanes(
    container: &Container,
) -> Result<Vec<FeatureOperationBodyReferenceLane>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut lanes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let parsed = crate::om::operation_body_reference_lanes(record.body_view());
            let projected = (|| -> Result<(), CodecError> {
                for lane in parsed {
                    let references = match lane.values {
                        crate::om::OperationBodyReferenceLaneValues::CompactIndex(values) => {
                            let mut references = Vec::new();
                            for value in values {
                                let data_block =
                                    unique_offset_data_block_id(&indexed, value.atom.value());
                                let source_offset = entry_offset
                                    .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                                    .ok_or_else(|| {
                                        CodecError::malformed(
                                            "NX body compact reference offset overflows",
                                        )
                                    })?;

                                references.push(ConstructionReference {
                                    token: value.atom,
                                    data_block,
                                    source_offset,
                                });
                            }
                            FeatureOperationBodyReferences::CompactIndex(references)
                        }
                        crate::om::OperationBodyReferenceLaneValues::PayloadObjectIndex(values) => {
                            let mut references = Vec::new();
                            for value in values {
                                let data_block =
                                    unique_offset_data_block_id(&indexed, value.token.value());
                                let source_offset = entry_offset
                                    .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                                    .ok_or_else(|| {
                                        CodecError::malformed(
                                            "NX body object reference offset overflows",
                                        )
                                    })?;

                                references.push(ConstructionReference {
                                    token: value.token,
                                    data_block,
                                    source_offset,
                                });
                            }
                            FeatureOperationBodyReferences::PayloadObjectIndex(references)
                        }
                    };
                    let id = format!("nx:feature-history:operation-body-reference-lane#{section_key}-{operation_ordinal:010}-{}",
                        lane.body_reference_ordinal);
                    let operation_label = format_feature_history_id(
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    );

                    lanes.push(FeatureOperationBodyReferenceLane {
                        id,
                        operation_label,
                        body_reference_ordinal: lane.body_reference_ordinal,
                        body_object_index: lane.body_object_index,
                        branch: lane.branch,
                        references,
                    });
                }
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(lanes)
}

/// Join the two exact encodings of an extrusion construction profile.
pub(in crate::native) fn feature_extrude_construction_profiles(
    references: &[FeatureExtrudeProfileReference],
) -> Vec<FeatureExtrudeConstructionProfile> {
    let operations = references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    for operation_label in operations {
        let mut operation_references = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        operation_references.sort_by_key(|first| first.ordinal);
        if operation_references
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
        {
            continue;
        }
        if operation_references.iter().any(|reference| {
            reference.data_block.is_none() || reference.witness_source_offset.is_none()
        }) {
            continue;
        }
        let mut profile_references = Vec::new();
        for reference in operation_references {
            let Some((block, witness_source_offset)) = reference
                .data_block
                .as_deref()
                .zip(reference.witness_source_offset)
            else {
                continue;
            };
            let data_block = block.to_owned();

            profile_references.push(FeatureExtrudeConstructionProfileReference {
                object_index: reference.token.value(),
                data_block,
                profile_source_offset: reference.source_offset,
                witness_source_offset,
            });
        }
        let id = operation_label.replacen("operation-label", "extrude-construction-profile", 1);
        let operation_label = operation_label.to_owned();

        profiles.push(FeatureExtrudeConstructionProfile {
            id,
            operation_label,
            references: profile_references,
        });
    }
    profiles
}

/// Decode structured `32` branches following extrusion body-reference fields.
pub(in crate::native) fn feature_extrude_payload_32_branches(
    container: &Container,
) -> Result<Vec<FeatureExtrudePayload32Branch>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut branches = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let frame = crate::om::extrude_32::extrude_payload_32_branch(record.body_view());
            let Some(frame) = frame.and_then(|frame| frame.relocate(entry_offset)) else {
                return;
            };
            let frame =
                frame.map_bindings(|index, ()| unique_offset_data_block_id(&indexed, index));
            let id = format_feature_history_id(
                "extrude-payload-32-branch",
                section_key,
                operation_ordinal,
                None,
            );
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);

            branches.push(FeatureExtrudePayload32Branch {
                id,
                operation_label,
                frame,
            });
        },
    )?;
    Ok(branches)
}

/// Join exact profile fields to self-witnessed structured extrusion branches.
pub(in crate::native) fn feature_extrude_32_constructions(
    references: &[FeatureExtrudeProfileReference],
    branches: &[FeatureExtrudePayload32Branch],
) -> Result<Vec<FeatureExtrude32Construction>, cadmpeg_core::CodecError> {
    fn copy_bindings<T>(
        items: &crate::om::branch_items::BranchItems<(T, Option<String>)>,
    ) -> Result<Option<crate::om::branch_items::BranchItems<String>>, CodecError> {
        let Some(copied) = items
            .as_slice()
            .iter()
            .map(|(_, binding)| binding.clone())
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        crate::om::branch_items::BranchItems::new(copied)
            .map(Some)
            .map_err(|error| CodecError::Malformed(error.into()))
    }

    let operations = branches
        .iter()
        .map(|branch| branch.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    let mut constructions = Vec::new();
    for operation_label in operations {
        let mut matching = branches
            .iter()
            .filter(|branch| branch.operation_label == operation_label);
        let Some(branch) = matching.next() else {
            continue;
        };
        if matching.next().is_some() {
            continue;
        }

        let mut profile = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        profile.sort_by_key(|first| first.ordinal);
        if profile
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
        {
            continue;
        }
        let Some(members) = profile
            .iter()
            .map(|reference| {
                Some(FeatureConstructionMember {
                    reference: reference.id.clone(),
                    data_block: reference.data_block.clone()?,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Ok(profiles) = crate::om::branch_items::BranchItems::new(members) else {
            continue;
        };
        let Some(atom_data_blocks) = copy_bindings(branch.frame.atom_members())? else {
            continue;
        };
        let Some(first_data_blocks) = copy_bindings(branch.frame.first_members())? else {
            continue;
        };
        let Some(second_data_blocks) = copy_bindings(branch.frame.second_members())? else {
            continue;
        };
        let id = branch
            .id
            .replacen("extrude-payload-32-branch", "extrude-32-construction", 1);
        let operation_label = branch.operation_label.clone();
        let branch_id = branch.id.clone();

        constructions.push(FeatureExtrude32Construction {
            id,
            operation_label,
            branch: branch_id,
            body_object_index: branch.frame.terminal().value(),
            profiles,
            atom_data_blocks,
            first_data_blocks,
            second_data_blocks,
        });
    }
    Ok(constructions)
}

/// Decode and resolve ordered construction references in `BLOCK` payloads.
pub(in crate::native) fn feature_block_construction_references(
    container: &Container,
) -> Result<Vec<FeatureBlockConstructionReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(field) =
                crate::om::block_construction::block_construction_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            let projected = (|| -> Result<(), CodecError> {
                for (position, (token, source_offset)) in
                    BlockReferencePosition::enumerate(field.references())
                {
                    let ordinal = usize::try_from(position.ordinal()).map_err(|_| {
                        CodecError::malformed("NX block reference ordinal exceeds usize")
                    })?;
                    let id = format_feature_history_id(
                        "block-construction-reference",
                        section_key,
                        operation_ordinal,
                        Some(ordinal),
                    );
                    let operation_label = format_feature_history_id(
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    );
                    let data_block = unique_offset_data_block_id(&indexed, token.value());

                    references.push(FeatureBlockConstructionReference {
                        id,
                        operation_label,
                        control: field.control(),
                        position,
                        token,
                        data_block,
                        source_offset,
                    });
                }
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(references)
}

/// Join complete, uniquely resolved `BLOCK` construction-reference fields.
pub(in crate::native) fn feature_block_constructions(
    references: &[FeatureBlockConstructionReference],
) -> Vec<FeatureBlockConstruction> {
    let operations = references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    let mut constructions = Vec::new();
    for operation_label in operations {
        let mut field = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        field.sort_by_key(|first| first.position.ordinal());
        let Ok(field): Result<[_; 19], _> = field.try_into() else {
            continue;
        };
        if field.iter().enumerate().any(|(ordinal, reference)| {
            u32::try_from(ordinal) != Ok(reference.position.ordinal())
                || reference.control != field[0].control
        }) {
            continue;
        }
        let [members @ .., terminal] = &field;
        if field.iter().any(|reference| reference.data_block.is_none()) {
            continue;
        }
        let mut owned_members = Vec::new();
        for reference in members {
            let Some(block) = reference.data_block.as_deref() else {
                continue;
            };
            owned_members.push(FeatureConstructionMember {
                reference: reference.id.clone(),
                data_block: block.to_owned(),
            });
        }
        let Ok(members): Result<[FeatureConstructionMember; 18], _> = owned_members.try_into()
        else {
            continue;
        };
        let Some(terminal_block) = terminal.data_block.as_deref() else {
            continue;
        };
        let terminal_data_block = terminal_block.to_owned();
        let terminal_reference = terminal.id.clone();
        let id = operation_label.replacen("operation-label", "block-construction", 1);
        let operation_label = operation_label.to_owned();

        constructions.push(FeatureBlockConstruction {
            id,
            operation_label,
            control: field[0].control,
            members,
            terminal_reference,
            terminal_data_block,
        });
    }
    constructions
}

/// Reconstruct complete `BLOCK` construction payloads in reference order.
pub(in crate::native) fn feature_block_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureBlockConstruction],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let mut payloads = Vec::new();
    for construction in constructions {
        let data_blocks = construction
            .members
            .iter()
            .map(|member| member.data_block.clone())
            .chain(std::iter::once(construction.terminal_data_block.clone()))
            .collect::<Vec<_>>();
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let id = construction
            .id
            .replacen("block-construction", "block-construction-payload", 1);
        let operation_label = construction.operation_label.clone();
        let construction_id = construction.id.clone();

        payloads.push(FeatureConstructionPayload {
            id,
            operation_label,
            owner: FeatureConstructionOwner::Block {
                construction: construction_id,
            },
            content,
        });
    }
    Ok(payloads)
}

/// Decode exact framed scalar fields across reconstructed `BLOCK` payloads.
pub(in crate::native) fn feature_block_payload_scalars(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePayloadScalar>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let mut scalars = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks) else {
            continue;
        };
        for (ordinal, field) in crate::om::construction_payload_scalar_fields(joined.bytes())
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(field.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = format!("{}-scalar-{ordinal}", payload.id);
            let operation_label = payload.operation_label.clone();
            let construction_payload = payload.id.clone();
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed("NX block payload scalar ordinal exceeds u32")
            })?;

            scalars.push(FeaturePayloadScalar {
                id,
                operation_label,
                payload: FeatureScalarPayload::Construction {
                    construction_payload,
                },
                ordinal,
                field_code: field.field_code,
                scalar: field.scalar,
                payload_offset,
                source_offset,
            });
        }
    }
    Ok(scalars)
}

/// Decode exact compact-code name fields across reconstructed `BLOCK` payloads.
pub(in crate::native) fn feature_block_payload_names(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePayloadName>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let mut names = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks) else {
            continue;
        };
        for (ordinal, field) in crate::om::name_field::scan(joined.bytes())
            .into_iter()
            .enumerate()
        {
            let Some(source_offset) =
                joined.source_offset(cadmpeg_core::decode::u64_from_index(field.offset()))
            else {
                continue;
            };
            let Some(frame) = field.into_native(|offset| joined.source_offset(offset)) else {
                continue;
            };
            let id = format!("{}-name-{ordinal}", payload.id);
            let operation_label = payload.operation_label.clone();
            let construction_payload = payload.id.clone();
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("NX block payload name ordinal exceeds u32"))?;

            names.push(FeaturePayloadName {
                id,
                operation_label,
                construction_payload,
                ordinal,
                frame,
                source_offset,
            });
        }
    }
    Ok(names)
}

/// Join complete `BLOCK` payload names to scalar fields in their intervals.
pub(in crate::native) fn feature_block_payload_named_records(
    payloads: &[FeatureConstructionPayload],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureBlockPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|first| first.frame.offset());
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.frame.offset());
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.payload.id() == payload.id
                        && scalar.payload_offset > name.frame.offset()
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|first| first.payload_offset);
            let id = format!("{}-record-{ordinal}", payload.id);
            let operation_label = payload.operation_label.clone();
            let construction_payload = payload.id.clone();
            let name_field = name.id.clone();
            let scalar_ids = scalar_fields
                .into_iter()
                .map(|scalar| scalar.id.clone())
                .collect();

            records.push(FeatureBlockPayloadNamedRecord {
                id,
                operation_label,
                construction_payload,
                name_field,
                scalar_fields: scalar_ids,
                payload_start_offset: name.frame.offset(),
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Type exact two-scalar `Point<positive decimal>` `BLOCK` payload intervals.
pub(in crate::native) fn feature_block_payload_points(
    records: &[FeatureBlockPayloadNamedRecord],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureBlockPayloadPoint> {
    let mut points = Vec::new();

    for record in records {
        let Some(name) = names.iter().rev().find(|name| name.id == record.name_field) else {
            continue;
        };

        if parse_sketch_point_name(name.frame.value()).is_none() {
            continue;
        }
        let [first_id, second_id] = record.scalar_fields.as_slice() else {
            continue;
        };
        let Some(first) = scalars.iter().rev().find(|scalar| scalar.id == *first_id) else {
            continue;
        };
        let Some(second) = scalars.iter().rev().find(|scalar| scalar.id == *second_id) else {
            continue;
        };
        let id = format!("{}-point", record.id);
        let operation_label = record.operation_label.clone();
        let named_record = record.id.clone();
        let name = name.frame.value().to_owned();
        let first_id = first.id.clone();
        let second_id = second.id.clone();

        points.push(FeatureBlockPayloadPoint {
            id,
            operation_label,
            named_record,
            name,
            scalar_fields: [first_id, second_id],
            coordinates: [first.scalar.value(), second.scalar.value()].into(),
        });
    }
    points
}

/// Group every bit-identical same-name `BLOCK` construction-point witness.
pub(in crate::native) fn feature_block_payload_point_groups(
    points: &[FeatureBlockPayloadPoint],
) -> Vec<FeatureBlockPayloadPointGroup> {
    let mut groups = Vec::new();

    for (ordinal, point) in points.iter().enumerate() {
        if points[..ordinal].iter().any(|candidate| {
            candidate.operation_label == point.operation_label && candidate.name == point.name
        }) {
            continue;
        }
        if points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .any(|candidate| {
                candidate
                    .coordinates
                    .iter()
                    .zip(point.coordinates)
                    .any(|(first, second)| first.to_bits() != second.to_bits())
            })
        {
            continue;
        }
        let mut witnesses = Vec::new();
        for witness in points.iter().filter(|candidate| {
            candidate.operation_label == point.operation_label && candidate.name == point.name
        }) {
            let id = witness.id.clone();

            witnesses.push(id);
        }
        let id = format!("{}-group", point.id);
        let operation_label = point.operation_label.clone();
        let name = point.name.clone();

        groups.push(FeatureBlockPayloadPointGroup {
            id,
            operation_label,
            name,
            points: witnesses,
            coordinates: point.coordinates,
        });
    }
    groups
}

/// Resolve the consecutive three-parameter dimension run of `BLOCK` features.
pub(in crate::native) fn feature_block_dimensions(
    constructions: &[FeatureBlockConstruction],
    bindings: &[FeatureParameterBinding],
    declarations: &[ExpressionDeclaration],
    expressions: &[ParameterFormula],
) -> Vec<FeatureBlockDimensions> {
    let mut dimensions = Vec::new();
    for construction in constructions {
        let mut operation_bindings = bindings
            .iter()
            .filter(|binding| binding.operation_label == construction.operation_label)
            .collect::<Vec<_>>();
        let Some(anchor) = operation_bindings
            .first()
            .map(|binding| binding.expression_declaration.as_str())
        else {
            continue;
        };
        if operation_bindings
            .iter()
            .any(|binding| binding.expression_declaration != anchor)
        {
            continue;
        }
        operation_bindings.sort_by(|first, second| {
            (first.input_slot, first.reference_ordinal)
                .cmp(&(second.input_slot, second.reference_ordinal))
        });
        let Some(start) = declarations
            .iter()
            .position(|declaration| declaration.id == anchor)
        else {
            continue;
        };
        let Some(end) = start.checked_add(3) else {
            continue;
        };
        let Some(run_slice) = declarations.get(start..end) else {
            continue;
        };
        let run = [&run_slice[0], &run_slice[1], &run_slice[2]];
        let first = run[0].name.index();
        if run.iter().enumerate().any(|(ordinal, declaration)| {
            declaration.record.split_once(":entry#").map(|pair| pair.0)
                != run[0].record.split_once(":entry#").map(|pair| pair.0)
                || declaration.source_entry != run[0].source_entry
                || Some(declaration.name.index())
                    != u32::try_from(ordinal)
                        .ok()
                        .and_then(|ordinal| first.checked_add(ordinal))
                || declaration.name.as_str() != format!("p{}", declaration.name.index())
        }) {
            continue;
        }
        let resolve = |declaration: &ExpressionDeclaration| {
            let mut matches = expressions
                .iter()
                .filter(|expression| expression.declaration.as_deref() == Some(&declaration.id));
            let expression = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            Some((
                expression,
                cadmpeg_ir::scalar::FiniteReal::new(
                    crate::native::om::expression_length_in_millimeters(
                        &expression.unit,
                        expression.value?.get(),
                    )?,
                )?,
            ))
        };
        let (Some(first_resolved), Some(second_resolved), Some(third_resolved)) =
            (resolve(run[0]), resolve(run[1]), resolve(run[2]))
        else {
            continue;
        };
        let resolved = [first_resolved, second_resolved, third_resolved];
        if resolved
            .iter()
            .zip(run.iter())
            .any(|((expression, _), declaration)| {
                expression.source_entry != declaration.source_entry
                    || expression.source_table != resolved[0].0.source_table
            })
        {
            continue;
        }
        let id = if let Some((prefix, suffix)) = construction.id.split_once("block-construction") {
            format!("{prefix}block-dimensions{suffix}")
        } else {
            construction.id.clone()
        };
        let operation_label = construction.operation_label.clone();
        let construction_id = construction.id.clone();
        let mut anchor_bindings = Vec::new();
        for binding in operation_bindings {
            let id = binding.id.clone();

            anchor_bindings.push(id);
        }
        let values = [0, 1, 2].map(|slot| FeatureBlockDimension {
            declaration: run[slot].id.clone(),
            expression: resolved[slot].0.id.clone(),
            value: resolved[slot].1,
        });

        dimensions.push(FeatureBlockDimensions {
            id,
            operation_label,
            construction: construction_id,
            anchor_bindings,
            dimensions: values,
        });
    }
    dimensions
}

/// Decode persistent object frames from bounded offset-store blocks.
pub(in crate::native) fn data_block_object_frames(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockObjectFrame>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(container)?;
    let mut out = Vec::new();
    for (data_block, (bytes, source_offset)) in blocks.iter() {
        for (ordinal, frame) in crate::om::data_block_object_frames(bytes)
            .into_iter()
            .enumerate()
        {
            let id = data_block_object_frame_id(data_block, ordinal)?;
            let data_block = data_block.to_owned();
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed("NX data block object frame ordinal exceeds u32")
            })?;
            let offset = source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(frame.offset))
                .ok_or_else(|| {
                    CodecError::malformed("NX data block object frame offset overflows")
                })?;
            ctx.charge_entities(1, "NX data block object frame")?;

            out.push(DataBlockObjectFrame {
                id,
                data_block,
                ordinal,
                object: LocatedCompactIndex {
                    atom: frame.atom,
                    offset,
                },
            });
        }
    }
    Ok(out)
}

pub(super) fn data_block_object_frame_id(
    data_block: &str,
    ordinal: usize,
) -> Result<String, cadmpeg_core::CodecError> {
    let Some((section, block)) = data_block
        .strip_prefix("nx:om-data-blocks-")
        .and_then(|rest| rest.split_once(":block#"))
    else {
        return Err(cadmpeg_core::CodecError::Malformed(
            "invalid NX data block object frame source".into(),
        ));
    };
    Ok(format!(
        "nx:om-data-block-object-frames-{section}:block-frame#{block}-{ordinal}"
    ))
}

pub(super) fn unique_offset_data_block_id(
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    object_index: u32,
) -> Option<String> {
    unique_offset_data_store(indexed, &[object_index])
        .map(|section_ordinal| format_offset_data_block_id(section_ordinal, object_index))
}

pub(super) fn format_offset_data_block_id(section_ordinal: usize, object_index: u32) -> String {
    format!("nx:om-data-blocks-{section_ordinal}:block#{object_index}")
}

pub(super) fn unique_offset_data_store(
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    object_indices: &[u32],
) -> Option<usize> {
    if object_indices.is_empty() || object_indices.contains(&0) {
        return None;
    }
    let mut unique = None;
    for (section_ordinal, (_, candidate)) in indexed.iter().enumerate() {
        let matches = candidate.as_offset_only().is_some_and(|(_, _, records)| {
            object_indices.iter().all(|object_index| {
                usize::try_from(*object_index)
                    .ok()
                    .is_some_and(|ordinal| ordinal <= records.len())
            })
        });
        if !matches {
            continue;
        }
        if unique.replace(section_ordinal).is_some() {
            return None;
        }
    }
    unique
}
