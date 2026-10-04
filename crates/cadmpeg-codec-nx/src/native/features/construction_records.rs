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
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub(super) struct ResolvedFeaturePayloadReference {
    pub(super) section_key: String,
    pub(super) operation_ordinal: usize,
    pub(super) ordinal: usize,
    pub(super) token: PayloadIndexToken,
    pub(super) data_block: Option<String>,
    pub(super) source_offset: u64,
}

pub(super) fn resolved_feature_payload_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    decode: impl Fn(
        crate::om::operation_record::OperationPayload<'_>,
        u64,
    ) -> Result<Option<Vec<(PayloadIndexToken, u64)>>, CodecError>,
) -> Result<Vec<ResolvedFeaturePayloadReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let decoded = match decode(record.payload_view(), entry_offset) {
                Ok(decoded) => decoded,
                Err(error) => { failure = Some(error); return; }
            };
            let Some(decoded) = decoded else {
                return;
            };
            let projected = (|| -> Result<(), CodecError> {
                for (ordinal, (token, source_offset)) in decoded.into_iter().enumerate() {
                    let data_block =
                        charged_unique_offset_data_block(ctx, &indexed, token.value())?;
                    let section_key = ctx
                        .copy_retained_text(section_key, "NX resolved feature reference section")?;
                    ctx.reserve_vec(&mut references, 1, "NX resolved feature payload references")?;
                    references.push(ResolvedFeaturePayloadReference {
                        section_key,
                        operation_ordinal,
                        ordinal,
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

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub(in crate::native) fn feature_projected_curve_references(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureProjectedCurveReference>, CodecError> {
    let references = resolved_feature_payload_references(ctx, container, |record, base| {
        Ok(crate::om::projected_references::ProjectedCurveReferences::read(ctx, record)?.and_then(|field| {
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
        }))
    })?;
    let mut output = Vec::new();
    for reference in references {
        let operation_label = format_feature_history_id(
            ctx,
            "operation-label",
            &reference.section_key,
            reference.operation_ordinal,
            None,
        )?;
        let id = format_feature_history_id(
            ctx,
            "projected-curve-reference",
            &reference.section_key,
            reference.operation_ordinal,
            Some(reference.ordinal),
        )?;
        ctx.reserve_vec(&mut output, 1, "NX projected curve references")?;
        output.push(FeatureProjectedCurveReference {
            id,
            operation_label,
            ordinal: u32::try_from(reference.ordinal).map_err(|_| {
                ctx.refuse_codec_limit("NX projected curve reference ordinal", 0, 1)
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
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut kinds = BTreeMap::new();
    let mut kind_reservation = ctx.reserve_scoped(0, "NX projected curve kinds")?;
    for label in labels {
        kind_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(&str, &str)>() * 4,
        ))?;
        ctx.insert_btree_map(
            &mut kinds,
            label.id.as_str(),
            label.value.as_str(),
            "NX projected curve kinds",
        )?;
    }
    let mut operations = BTreeSet::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX projected curve operations")?;
    for reference in references {
        operation_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<&str>() * 4,
        ))?;
        ctx.insert_btree_set(
            &mut operations,
            reference.operation_label.as_str(),
            "NX projected curve operations",
        )?;
    }
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "join NX projected curve references",
        )?;
        let mut field = Vec::new();
        let mut field_reservation = ctx.reserve_scoped(0, "NX projected curve reference field")?;
        for reference in references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
        {
            ctx.reserve_scoped_vec(
                &mut field_reservation,
                &mut field,
                1,
                "NX projected curve reference field",
            )?;
            field.push(reference);
        }
        ctx.stable_sort_by(
            &mut field,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX projected curve references",
        )?;
        if field.len() != expected_len
            || field
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| u32::try_from(ordinal).ok() != Some(reference.ordinal))
        {
            continue;
        }
        let mut data_blocks = Vec::new();
        let mut block_reservation = ctx.reserve_scoped(0, "NX projected curve block IDs")?;
        let mut complete = true;
        for reference in &field {
            let Some(block) = reference.data_block.as_deref() else {
                complete = false;
                break;
            };

            block_reservation.with_storage(|| {
                ctx.reserve_vec(&mut data_blocks, 1, "NX projected curve block IDs")
            })?;
            let mut copy = String::new();
            ctx.try_reserve_retained_text(
                &mut copy,
                block.len(),
                "copy NX projected curve block ID",
            )?;
            copy.push_str(block);
            data_blocks.push(copy);
        }
        if !complete {
            continue;
        }
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
        drop(block_reservation);
        let Some((_, operation_key)) = operation_label.rsplit_once('#') else {
            continue;
        };
        let id = ctx.format_retained(
            format_args!("nx:feature-history:projected-curve-construction-payload#{operation_key}"),
            "NX projected curve payload identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(operation_label, "NX projected curve payload operation")?;
        let mut construction_references = Vec::new();
        for reference in field {
            let identity =
                ctx.copy_retained_text(&reference.id, "NX projected curve construction reference")?;
            ctx.reserve_vec(
                &mut construction_references,
                1,
                "NX projected curve construction references",
            )?;
            construction_references.push(identity);
        }
        ctx.reserve_vec(&mut payloads, 1, "NX projected curve payloads")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeatureProjectedCurveConstructionString>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut strings = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, value) in crate::om::string_values(ctx, joined.bytes(), 0)?
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("{}-string-{ordinal:010}", payload.id),
                "NX projected curve string identity",
            )?;
            let operation_label = ctx.copy_retained_text(
                &payload.operation_label,
                "NX projected curve string operation",
            )?;
            let construction_payload =
                ctx.copy_retained_text(&payload.id, "NX projected curve string payload")?;
            let text =
                ctx.copy_retained_text(value.value.as_str(), "NX projected curve string value")?;
            let value = crate::printable_string::PrintableString::from_wire(ctx, text)?
                .map_err(|error| CodecError::Malformed(error.into()))?;
            ctx.reserve_vec(&mut strings, 1, "NX projected curve strings")?;
            strings.push(FeatureProjectedCurveConstructionString {
                id,
                operation_label,
                construction_payload,
                ordinal: u32::try_from(ordinal).map_err(|_| {
                    ctx.refuse_codec_limit("NX projected curve string ordinal", 0, 1)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeaturePointConstructionHeader>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut headers = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
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
                    ctx,
                    "point-construction-header",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let data_block = charged_unique_offset_data_block(
                    ctx,
                    &indexed,
                    header.reference.token.value(),
                )?;
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(
                        header.reference.offset,
                    ))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX point construction header source offset", 0, 1)
                    })?;
                ctx.reserve_vec(&mut headers, 1, "NX point construction headers")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    headers: &[FeaturePointConstructionHeader],
) -> Result<Vec<FeaturePointConstructionScalarLane>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(indexed.len()),
            "match NX point scalar lane",
        )?;
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
        let id = replace_operation_text(
            ctx,
            &header.id,
            "point-construction-header#",
            "point-construction-scalar-lane#",
            "NX point scalar lane identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(&header.operation_label, "NX point scalar lane operation")?;
        let construction_header =
            ctx.copy_retained_text(&header.id, "NX point scalar lane header")?;
        let first_block = ctx.format_retained(
            format_args!(
                "nx:om-data-blocks-{section_ordinal}:block#{}",
                target_ordinal - 1
            ),
            "NX point scalar lane first block",
        )?;
        let second_block = ctx.format_retained(
            format_args!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}"),
            "NX point scalar lane second block",
        )?;
        let data_blocks = [first_block, second_block];
        ctx.reserve_vec(&mut lanes, 1, "NX point scalar lanes")?;
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
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureSurfaceConstructionReference>, CodecError> {
    let references = resolved_feature_payload_references(ctx, container, |record, base| {
        let surface = crate::om::surface_envelope::surface_feature_payload_references(ctx, record)?
            .map(|field| field.relocate(ctx, base)).transpose()?.flatten();
        Ok(match surface {
            Some(field) => Some(field.references().into_iter().collect()),
            None => crate::om::surface_envelope::thru_curve_payload_references(record)
                .and_then(|field| field.relocate(base))
                .map(|field| field.references().into_iter().collect()),
        })
    })?;
    let mut output = Vec::new();
    for reference in references {
        let operation_label = format_feature_history_id(
            ctx,
            "operation-label",
            &reference.section_key,
            reference.operation_ordinal,
            None,
        )?;
        let id = format_feature_history_id(
            ctx,
            "surface-construction-reference",
            &reference.section_key,
            reference.operation_ordinal,
            Some(reference.ordinal),
        )?;
        ctx.reserve_vec(&mut output, 1, "NX surface construction references")?;
        output.push(FeatureSurfaceConstructionReference {
            id,
            operation_label,
            ordinal: u32::try_from(reference.ordinal).map_err(|_| {
                ctx.refuse_codec_limit("NX surface construction reference ordinal", 0, 1)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureThruCurveConstructionEnvelope>, cadmpeg_core::CodecError> {
    let mut envelopes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(field) =
                crate::om::surface_envelope::thru_curve_payload_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            let projected = (|| -> Result<_, CodecError> {
                let id = format_feature_history_id(
                    ctx,
                    "thru-curve-construction-envelope",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.reserve_vec(&mut envelopes, 1, "NX thru-curve construction envelopes")?;
                Ok(FeatureThruCurveConstructionEnvelope {
                    id,
                    operation_label,
                    discriminator: field.discriminator,
                    controls: field.controls,
                    trailing_control: field.trailing_control,
                    trailing_value: field.trailing_value,
                    source_offset: field.origin(),
                })
            })();
            match projected {
                Ok(envelope) => envelopes.push(envelope),
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(envelopes)
}

/// Decode and resolve each exact leading `SWP104` construction branch.
pub(in crate::native) fn feature_swp104_leading_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureSwp104LeadingBranch>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut branches = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let branch = match crate::om::swp104_payload_leading_branch(ctx, record.payload_view())
            {
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
            let resolved = (|| -> Result<Option<_>, CodecError> {
                let id = format_feature_history_id(
                    ctx,
                    "swp104-leading-branch",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                FeatureSwp104LeadingBranch::from_source(
                    ctx,
                    id,
                    operation_label,
                    source_offset,
                    branch,
                    |token| charged_unique_offset_data_block(ctx, &indexed, token.value()),
                )
            })();
            match resolved {
                Ok(Some(branch)) => {
                    let admitted = (|| -> Result<(), CodecError> {
                        ctx.reserve_vec(&mut branches, 1, "NX SWP104 leading branches")?;
                        branches.push(branch);
                        Ok(())
                    })();
                    if let Err(error) = admitted {
                        failure = Some(error);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    failure = Some(error);
                }
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
    let blocks = offset_data_block_bytes(ctx, container)?;
    let label_bytes = references
        .len()
        .checked_mul(std::mem::size_of::<&str>() * 4)
        .ok_or_else(|| ctx.refuse_codec_limit("NX surface payload labels", 0, 1))?;
    let _labels_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(label_bytes),
        "NX surface payload labels",
    )?;
    let mut labels = BTreeSet::new();
    for reference in references {
        ctx.charge_work(1, "scan NX surface payload labels")?;
        ctx.insert_btree_set(
            &mut labels,
            reference.operation_label.as_str(),
            "NX surface payload labels",
        )?;
    }
    let mut output = Vec::new();
    for operation_label in labels {
        let scan_work = references
            .len()
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("scan NX surface construction graph", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(scan_work),
            "scan NX surface construction graph",
        )?;
        let graph_count = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .count();

        let (mut graph, _graph_reservation) =
            ctx.temporary_vec(graph_count, "NX surface construction graph")?;
        graph.extend(
            references
                .iter()
                .filter(|reference| reference.operation_label == operation_label),
        );
        ctx.stable_sort_by(
            &mut graph,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX surface construction graph",
        )?;
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
        let mut source_id_storage = ctx.reserve_scoped(0, "NX payload source identity headers")?;
        let mut data_blocks = Vec::new();
        source_id_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut data_blocks,
                14,
                "NX surface construction source blocks",
            )
        })?;
        for reference in graph {
            let Some(block) = reference.data_block.as_deref() else {
                break;
            };
            data_blocks
                .push(ctx.copy_retained_text(block, "NX surface construction source block")?);
        }
        if data_blocks.len() != 14 {
            continue;
        }
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
        let prefix = "nx:feature-history:surface-construction-payload#";
        let id_len = prefix
            .len()
            .checked_add(operation_key.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("NX surface construction payload identity", 0, 1)
            })?;
        let mut id = ctx.retained_string(id_len, "NX surface construction payload identity")?;
        id.push_str(prefix);
        id.push_str(operation_key);
        let mut construction_references: [String; 14] = std::array::from_fn(|_| String::new());
        for (slot, reference) in graph.into_iter().enumerate() {
            construction_references[slot] = ctx
                .copy_retained_text(&reference.id, "NX surface construction reference identity")?;
        }
        ctx.reserve_vec(&mut output, 1, "NX surface construction payloads")?;
        output.push(FeatureSurfaceConstructionPayload {
            id,
            operation_label: ctx
                .copy_retained_text(operation_label, "NX surface construction operation label")?,
            construction_references,
            content,
        });
    }
    Ok(output)
}

/// Decode exact scalar-pair frames from reconstructed surface payloads.
pub(in crate::native) fn feature_surface_construction_scalar_pairs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Result<Vec<FeaturePayloadScalarPair>, cadmpeg_core::CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::binary64_pair::object_pairs(ctx, bytes),
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
            let id = format_feature_child_id(ctx, &payload.id, "-scalar-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX surface scalar pair ordinal", 0, 1))?;
            Ok(Some(FeaturePayloadScalarPair {
                id,
                operation_label: ctx
                    .copy_retained_text(&payload.operation_label, "NX surface scalar pair label")?,
                payload: FeatureScalarPairPayload::SurfaceConstruction {
                    surface_construction_payload: ctx
                        .copy_retained_text(&payload.id, "NX surface scalar pair payload")?,
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
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Result<Vec<FeatureSurfaceConstructionString>, CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut strings = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, value) in crate::om::surface_payload_strings(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit("nx surface payload string ordinal", 0, u64::MAX)
            })?;
            let id = format_feature_child_id(ctx, &payload.id, "-string-", ordinal)?;
            let text =
                ctx.copy_retained_text(value.value.as_str(), "NX surface payload string text")?;
            let value = crate::payload_text::PayloadText::from_wire(ctx, text)?
                .map_err(|error| CodecError::Malformed(error.to_owned()))?;
            ctx.reserve_vec(&mut strings, 1, "NX surface payload strings")?;
            strings.push(FeatureSurfaceConstructionString {
                id,
                operation_label: ctx.copy_retained_text(
                    &payload.operation_label,
                    "NX surface payload string label",
                )?,
                surface_construction_payload: ctx
                    .copy_retained_text(&payload.id, "NX surface payload string owner")?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureExtrudeProfileReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    let mut refusal: Option<CodecError> = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if refusal.is_some() {
                return;
            }
            let decoded = match crate::om::extrude_profile::extrude_profile_references(
                ctx,
                record.payload_view(),
            ) {
                Ok(Some(field)) => field,
                Ok(None) => return,
                Err(error) => {
                    refusal = Some(error);
                    return;
                }
            };
            let decoded = match decoded.relocate(ctx, entry_offset) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => return,
                Err(error) => { refusal = Some(error); return; }
            };
            let projected = (|| -> Result<(), CodecError> {
                for (ordinal, (token, source_offset, witness_source_offset)) in
                    decoded.references().enumerate()
                {
                    let id = format_feature_history_id(
                        ctx,
                        "extrude-profile-reference",
                        section_key,
                        operation_ordinal,
                        Some(ordinal),
                    )?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    let data_block =
                        charged_unique_offset_data_block(ctx, &indexed, token.value())?;
                    let ordinal = u32::try_from(ordinal).map_err(|_| {
                        ctx.refuse_codec_limit("NX extrude profile reference ordinal", 0, 1)
                    })?;
                    ctx.reserve_vec(&mut references, 1, "NX extrude profile references")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureExtrudePayloadHeader>, cadmpeg_core::CodecError> {
    let mut headers = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
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
                    ctx,
                    "extrude-payload-header",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let source_offset = entry_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(header.offset))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX extrude header source offset", 0, 1)
                    })?;
                ctx.reserve_vec(&mut headers, 1, "NX extrude payload headers")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationTerminalDiscriminator>, cadmpeg_core::CodecError> {
    let mut lanes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let frame = match crate::om::terminal_discriminator::operation_terminal_discriminator(
                ctx,
                record.payload_view(),
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let Some(frame) = frame.and_then(|frame| frame.relocate(entry_offset)) else {
                return;
            };
            let projected = (|| -> Result<(), CodecError> {
                let id = format_feature_history_id(
                    ctx,
                    "operation-terminal-discriminator",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.reserve_vec(&mut lanes, 1, "NX operation terminal discriminators")?;
                lanes.push(FeatureOperationTerminalDiscriminator {
                    id,
                    operation_label,
                    frame,
                });
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(lanes)
    }
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub(in crate::native) fn feature_operation_body_scalar_triples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationBodyScalarTriple>, cadmpeg_core::CodecError> {
    let mut triples = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let rows = match crate::om::body_scalar_triple::operation_body_scalar_triples(
                ctx,
                record.body_view(),
            ) {
                Ok(rows) => rows,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            for triple in rows {
                let Some(scalars) = triple.scalars.relocate(entry_offset) else {
                    continue;
                };
                let projected = (|| -> Result<(), CodecError> {
                    let id = ctx.format_retained(format_args!("nx:feature-history:operation-body-scalar-triple#{section_key}-{operation_ordinal:010}-{}",
                            triple.body_reference_ordinal), "NX operation body scalar triple identity")?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    ctx.reserve_vec(&mut triples, 1, "NX operation body scalar triples")?;
                    triples.push(FeatureOperationBodyScalarTriple {
                        id,
                        operation_label,
                        body_reference_ordinal: triple.body_reference_ordinal,
                        body_object_index: triple.body_object_index,
                        branch: triple.branch,
                        scalars,
                    });
                    Ok(())
                })();
                if let Err(error) = projected {
                    failure = Some(error);
                    break;
                }
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(triples)
}

/// Decode ordered member lanes following branch-`11` operation body clauses.
pub(in crate::native) fn feature_operation_body_members(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationBodyMember>, cadmpeg_core::CodecError> {
    let mut members = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let groups = match crate::om::operation_body_members(ctx, record.body_view()) {
                Ok(groups) => groups,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let projected = (|| -> Result<(), CodecError> {
                for group in groups {
                    for (ordinal, member) in group.members.into_iter().enumerate() {
                        let ordinal = u32::try_from(ordinal).map_err(|_| {
                            ctx.refuse_codec_limit("NX operation body member ordinal", 0, 1)
                        })?;
                        let id = ctx.format_retained(format_args!("nx:feature-history:operation-body-member#{section_key}-{operation_ordinal:010}-{}-{ordinal}",
                                group.body_reference_ordinal), "NX operation body member identity")?;
                        let operation_label = format_feature_history_id(
                            ctx,
                            "operation-label",
                            section_key,
                            operation_ordinal,
                            None,
                        )?;
                        let offset = entry_offset
                            .checked_add(cadmpeg_core::decode::u64_from_index(member.offset))
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "NX operation body member source offset",
                                    0,
                                    1,
                                )
                            })?;
                        ctx.reserve_vec(&mut members, 1, "NX operation body members")?;
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
    ctx: &DecodeContext<'_>,
    members: &[FeatureOperationBodyMember],
    references: &[FeatureBodyReference],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
    bindings: &[SegmentBodyBinding],
) -> Result<Vec<FeatureOperationBodyOperand>, CodecError> {
    let work = members
        .len()
        .checked_mul(inputs.len())
        .and_then(|count| {
            members
                .len()
                .checked_mul(references.len())?
                .checked_mul(inputs.len())?
                .checked_add(count)
        })
        .and_then(|count| members.len().checked_mul(blocks.len())?.checked_add(count))
        .and_then(|count| {
            members
                .len()
                .checked_mul(bindings.len())?
                .checked_add(count)
        })
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX feature operation body operands", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "scan NX feature operation body operands",
    )?;
    let mut operands = Vec::new();
    for member in members {
        if member.member.atom.value() == member.body_object_index {
            continue;
        }
        let (has_inputs, member_store) = unique_operation_store(inputs, &member.operation_label);
        if member_store.is_none() && has_inputs {
            continue;
        }
        let operand_data_block = if let Some(store) = member_store {
            let length = store
                .len()
                .checked_add(7 + 10)
                .ok_or_else(|| ctx.refuse_codec_limit("retain NX operand data block id", 0, 1))?;
            let mut candidate = ctx.reserve_scoped(0, "format NX operand data block id")?;
            let mut id = String::new();
            candidate.with_storage(|| {
                ctx.try_reserve_retained_text(&mut id, length, "allocate NX operand data block id")
            })?;
            write!(id, "{store}:block#{}", member.member.atom.value())
                .map_err(|_| ctx.refuse_codec_limit("write NX operand data block id", 0, 1))?;
            if blocks.iter().any(|block| block.id == id) {
                candidate.commit()?;
                Some(id)
            } else {
                None
            }
        } else {
            None
        };
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
                ctx.reserve_vec(
                    &mut segment_body_bindings,
                    1,
                    "NX operation operand bindings",
                )?;
                segment_body_bindings.push(
                    ctx.copy_retained_text(&binding.id, "retain NX operation operand binding id")?,
                );
            }
        }
        if !same_namespace_reference && segment_body_bindings.is_empty() {
            continue;
        }
        ctx.reserve_vec(&mut operands, 1, "NX feature operation body operands")?;
        operands.push(FeatureOperationBodyOperand {
            id: replace_operation_text(
                ctx,
                &member.id,
                "operation-body-member",
                "operation-body-operand",
                "retain NX operation body operand id",
            )?,
            operation_label: ctx.copy_retained_text(
                &member.operation_label,
                "retain NX operation body operand label",
            )?,
            body_object_index: member.body_object_index,
            body_reference_ordinal: member.body_reference_ordinal,
            ordinal: member.ordinal,
            operand: member.member,
            operand_data_block,
            segment_body_bindings,
        });
    }
    Ok(operands)
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

pub(super) fn replace_operation_text(
    ctx: &DecodeContext<'_>,
    value: &str,
    old: &str,
    new: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let (prefix, suffix) = if let Some(position) = value.find(old) {
        (&value[..position], &value[position + old.len()..])
    } else {
        (value, "")
    };
    let length = prefix
        .len()
        .checked_add(suffix.len())
        .and_then(|length| {
            length.checked_add(if prefix.len() == value.len() {
                0
            } else {
                new.len()
            })
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    let mut result = ctx.retained_string(length, operation)?;
    result.push_str(prefix);
    if prefix.len() != value.len() {
        result.push_str(new);
    }
    result.push_str(suffix);
    Ok(result)
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub(in crate::native) fn feature_operation_body_11_continuations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationBody11Continuation>, cadmpeg_core::CodecError> {
    let mut continuations = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let rows = match crate::om::operation_body_11_continuations(ctx, record.body_view()) {
                Ok(rows) => rows,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let projected = (|| -> Result<(), CodecError> {
                for continuation in rows {
                    let id = ctx.format_retained(format_args!("nx:feature-history:trim-body-11-continuation#{section_key}-{operation_ordinal:010}-{}",
                            continuation.body_reference_ordinal), "NX trim body continuation identity")?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    let offset = entry_offset
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            continuation.continuation.offset,
                        ))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit("NX trim body continuation offset", 0, 1)
                        })?;
                    let terminal_source_offset = entry_offset
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            continuation.terminal.offset,
                        ))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit("NX trim body terminal offset", 0, 1)
                        })?;
                    ctx.reserve_vec(&mut continuations, 1, "NX trim body continuations")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationBodyReferenceLane>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut lanes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let parsed = match crate::om::operation_body_reference_lanes(ctx, record.body_view()) {
                Ok(parsed) => parsed,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let projected = (|| -> Result<(), CodecError> {
                for lane in parsed {
                    let references = match lane.values {
                        crate::om::OperationBodyReferenceLaneValues::CompactIndex(values) => {
                            let mut references = Vec::new();
                            for value in values {
                                let data_block = charged_unique_offset_data_block(
                                    ctx,
                                    &indexed,
                                    value.atom.value(),
                                )?;
                                let source_offset = entry_offset
                                    .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                                    .ok_or_else(|| {
                                        ctx.refuse_codec_limit(
                                            "NX body compact reference offset",
                                            0,
                                            1,
                                        )
                                    })?;
                                ctx.reserve_vec(&mut references, 1, "NX body compact references")?;
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
                                let data_block = charged_unique_offset_data_block(
                                    ctx,
                                    &indexed,
                                    value.token.value(),
                                )?;
                                let source_offset = entry_offset
                                    .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                                    .ok_or_else(|| {
                                        ctx.refuse_codec_limit(
                                            "NX body object reference offset",
                                            0,
                                            1,
                                        )
                                    })?;
                                ctx.reserve_vec(&mut references, 1, "NX body object references")?;
                                references.push(ConstructionReference {
                                    token: value.token,
                                    data_block,
                                    source_offset,
                                });
                            }
                            FeatureOperationBodyReferences::PayloadObjectIndex(references)
                        }
                    };
                    let id = ctx.format_retained(format_args!("nx:feature-history:operation-body-reference-lane#{section_key}-{operation_ordinal:010}-{}",
                        lane.body_reference_ordinal), "NX operation body reference lane identity")?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    ctx.reserve_vec(&mut lanes, 1, "NX operation body reference lanes")?;
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
    ctx: &DecodeContext<'_>,
    references: &[FeatureExtrudeProfileReference],
) -> Result<Vec<FeatureExtrudeConstructionProfile>, CodecError> {
    let mut operations = BTreeSet::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX extrude profile operations")?;
    for reference in references {
        operation_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<&str>() * 4,
        ))?;
        ctx.insert_btree_set(
            &mut operations,
            reference.operation_label.as_str(),
            "NX extrude profile operations",
        )?;
    }
    let mut profiles = Vec::new();
    for operation_label in operations {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "join NX extrude profile references",
        )?;
        let mut operation_references = Vec::new();
        let mut reference_reservation =
            ctx.reserve_scoped(0, "NX extrude profile reference order")?;
        for reference in references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
        {
            ctx.reserve_scoped_vec(
                &mut reference_reservation,
                &mut operation_references,
                1,
                "NX extrude profile reference order",
            )?;
            operation_references.push(reference);
        }
        ctx.stable_sort_by(
            &mut operation_references,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX extrude profile references",
        )?;
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
            let data_block =
                ctx.copy_retained_text(block, "NX extrude construction profile block")?;
            ctx.reserve_vec(
                &mut profile_references,
                1,
                "NX extrude construction profile references",
            )?;
            profile_references.push(FeatureExtrudeConstructionProfileReference {
                object_index: reference.token.value(),
                data_block,
                profile_source_offset: reference.source_offset,
                witness_source_offset,
            });
        }
        let id = replace_operation_text(
            ctx,
            operation_label,
            "operation-label",
            "extrude-construction-profile",
            "NX extrude construction profile identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(operation_label, "NX extrude construction profile operation")?;
        ctx.reserve_vec(&mut profiles, 1, "NX extrude construction profiles")?;
        profiles.push(FeatureExtrudeConstructionProfile {
            id,
            operation_label,
            references: profile_references,
        });
    }
    Ok(profiles)
}

/// Decode structured `32` branches following extrusion body-reference fields.
pub(in crate::native) fn feature_extrude_payload_32_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureExtrudePayload32Branch>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut branches = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let frame =
                match crate::om::extrude_32::extrude_payload_32_branch(ctx, record.body_view()) {
                    Ok(frame) => frame,
                    Err(error) => {
                        failure = Some(error);
                        return;
                    }
                };
            let Some(frame) = frame.and_then(|frame| frame.relocate(entry_offset)) else {
                return;
            };
            let frame = match frame.map_bindings(ctx, |index, ()| {
                charged_unique_offset_data_block(ctx, &indexed, index)
            }) {
                Ok(frame) => frame,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let projected = (|| -> Result<(), CodecError> {
                let id = format_feature_history_id(
                    ctx,
                    "extrude-payload-32-branch",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.reserve_vec(&mut branches, 1, "NX extrude payload 32 branches")?;
                branches.push(FeatureExtrudePayload32Branch {
                    id,
                    operation_label,
                    frame,
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
    Ok(branches)
}

/// Join exact profile fields to self-witnessed structured extrusion branches.
pub(in crate::native) fn feature_extrude_32_constructions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[FeatureExtrudeProfileReference],
    branches: &[FeatureExtrudePayload32Branch],
) -> Result<Vec<FeatureExtrude32Construction>, cadmpeg_core::CodecError> {
    fn copy_bindings<T>(
        ctx: &DecodeContext<'_>,
        items: &crate::om::branch_items::BranchItems<(T, Option<String>)>,
        operation: &'static str,
    ) -> Result<Option<crate::om::branch_items::BranchItems<String>>, CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(items.len()), operation)?;
        if items
            .as_slice()
            .iter()
            .any(|(_, binding)| binding.is_none())
        {
            return Ok(None);
        }
        let mut copied = Vec::new();
        for (_, binding) in items.as_slice() {
            let Some(binding) = binding.as_deref() else {
                return Ok(None);
            };
            let id = ctx.copy_retained_text(binding, operation)?;
            ctx.reserve_vec(&mut copied, 1, operation)?;
            copied.push(id);
        }
        crate::om::branch_items::BranchItems::new(copied)
            .map(Some)
            .map_err(|error| CodecError::Malformed(error.into()))
    }

    let mut operations = BTreeSet::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX extrude 32 operations")?;
    for branch in branches {
        operation_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<&str>() * 4,
        ))?;
        ctx.insert_btree_set(
            &mut operations,
            branch.operation_label.as_str(),
            "NX extrude 32 operations",
        )?;
    }
    let mut constructions = Vec::new();
    for operation_label in operations {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(branches.len()),
            "match NX extrude 32 branch",
        )?;
        let mut matching = branches
            .iter()
            .filter(|branch| branch.operation_label == operation_label);
        let Some(branch) = matching.next() else {
            continue;
        };
        if matching.next().is_some() {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "match NX extrude 32 profile",
        )?;
        let mut profile = Vec::new();
        let mut profile_reservation = ctx.reserve_scoped(0, "NX extrude 32 profile order")?;
        for reference in references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
        {
            ctx.reserve_scoped_vec(
                &mut profile_reservation,
                &mut profile,
                1,
                "NX extrude 32 profile order",
            )?;
            profile.push(reference);
        }
        ctx.stable_sort_by(
            &mut profile,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX extrude 32 profiles",
        )?;
        let Ok(profile) = crate::om::branch_items::BranchItems::new(profile) else {
            continue;
        };
        if profile
            .as_slice()
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
        {
            continue;
        }
        if profile
            .as_slice()
            .iter()
            .any(|reference| reference.data_block.is_none())
        {
            continue;
        }
        let profiles = profile.try_map_indexed_charged(ctx, |_, reference| {
            let Some(block) = reference.data_block.as_deref() else {
                return Err(ctx.refuse_codec_limit("NX extrude 32 profile block", 0, 1));
            };
            Ok(FeatureConstructionMember {
                reference: ctx
                    .copy_retained_text(&reference.id, "NX extrude 32 profile reference")?,
                data_block: ctx.copy_retained_text(block, "NX extrude 32 profile block")?,
            })
        })?;
        let Some(atom_data_blocks) = copy_bindings(
            ctx,
            branch.frame.atom_members(),
            "NX extrude 32 atom blocks",
        )?
        else {
            continue;
        };
        let Some(first_data_blocks) = copy_bindings(
            ctx,
            branch.frame.first_members(),
            "NX extrude 32 first blocks",
        )?
        else {
            continue;
        };
        let Some(second_data_blocks) = copy_bindings(
            ctx,
            branch.frame.second_members(),
            "NX extrude 32 second blocks",
        )?
        else {
            continue;
        };
        let id = replace_operation_text(
            ctx,
            &branch.id,
            "extrude-payload-32-branch",
            "extrude-32-construction",
            "NX extrude 32 construction identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &branch.operation_label,
            "NX extrude 32 construction operation",
        )?;
        let branch_id = ctx.copy_retained_text(&branch.id, "NX extrude 32 construction branch")?;
        ctx.reserve_vec(&mut constructions, 1, "NX extrude 32 constructions")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureBlockConstructionReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
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
                    let ordinal = usize::try_from(position.ordinal())
                        .map_err(|_| ctx.refuse_codec_limit("NX block reference ordinal", 0, 1))?;
                    let id = format_feature_history_id(
                        ctx,
                        "block-construction-reference",
                        section_key,
                        operation_ordinal,
                        Some(ordinal),
                    )?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    let data_block =
                        charged_unique_offset_data_block(ctx, &indexed, token.value())?;
                    ctx.reserve_vec(&mut references, 1, "NX block construction references")?;
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
    ctx: &DecodeContext<'_>,
    references: &[FeatureBlockConstructionReference],
) -> Result<Vec<FeatureBlockConstruction>, CodecError> {
    let mut operations = BTreeSet::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX block construction operations")?;
    for reference in references {
        operation_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<&str>() * 4,
        ))?;
        ctx.insert_btree_set(
            &mut operations,
            reference.operation_label.as_str(),
            "NX block construction operations",
        )?;
    }
    let mut constructions = Vec::new();
    for operation_label in operations {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "join NX block construction references",
        )?;
        let mut field = Vec::new();
        let mut field_reservation = ctx.reserve_scoped(0, "NX block construction field order")?;
        for reference in references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
        {
            ctx.reserve_scoped_vec(
                &mut field_reservation,
                &mut field,
                1,
                "NX block construction field order",
            )?;
            field.push(reference);
        }
        ctx.stable_sort_by_key(
            &mut field,
            |value| value.position.ordinal(),
            Ord::cmp,
            "sort NX block construction field",
        )?;
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
        let member_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(
                members.len() * std::mem::size_of::<FeatureConstructionMember>(),
            ),
            "NX block construction member array",
        )?;
        ctx.reserve_vec(
            &mut owned_members,
            members.len(),
            "NX block construction member array",
        )?;
        for reference in members {
            let Some(block) = reference.data_block.as_deref() else {
                continue;
            };
            owned_members.push(FeatureConstructionMember {
                reference: ctx
                    .copy_retained_text(&reference.id, "NX block construction member reference")?,
                data_block: ctx.copy_retained_text(block, "NX block construction member block")?,
            });
        }
        let Ok(members): Result<[FeatureConstructionMember; 18], _> = owned_members.try_into()
        else {
            continue;
        };
        drop(member_reservation);
        let Some(terminal_block) = terminal.data_block.as_deref() else {
            continue;
        };
        let terminal_data_block =
            ctx.copy_retained_text(terminal_block, "NX block construction terminal block")?;
        let terminal_reference =
            ctx.copy_retained_text(&terminal.id, "NX block construction terminal reference")?;
        let id = replace_operation_text(
            ctx,
            operation_label,
            "operation-label",
            "block-construction",
            "NX block construction identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(operation_label, "NX block construction operation")?;
        ctx.reserve_vec(&mut constructions, 1, "NX block constructions")?;
        constructions.push(FeatureBlockConstruction {
            id,
            operation_label,
            control: field[0].control,
            members,
            terminal_reference,
            terminal_data_block,
        });
    }
    Ok(constructions)
}

/// Reconstruct complete `BLOCK` construction payloads in reference order.
pub(in crate::native) fn feature_block_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureBlockConstruction],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut payloads = Vec::new();
    for construction in constructions {
        let mut reservation = ctx.reserve_scoped(0, "NX block construction source blocks")?;
        let mut data_blocks = Vec::new();
        for source in construction
            .members
            .iter()
            .map(|member| member.data_block.as_str())
            .chain(std::iter::once(construction.terminal_data_block.as_str()))
        {
            ctx.charge_work(1, "NX block construction source blocks")?;
            let owned = ctx.copy_retained_text(source, "NX block construction source blocks")?;
            reservation.with_storage(|| {
                ctx.push_vec(
                    &mut data_blocks,
                    owned,
                    "NX block construction source blocks",
                )
            })?;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        drop(reservation);
        let id = replace_operation_text(
            ctx,
            &construction.id,
            "block-construction",
            "block-construction-payload",
            "NX block construction payload identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &construction.operation_label,
            "NX block construction payload operation",
        )?;
        let construction_id =
            ctx.copy_retained_text(&construction.id, "NX block construction payload owner")?;
        ctx.reserve_vec(&mut payloads, 1, "NX block construction payloads")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePayloadScalar>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut scalars = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, field) in crate::om::construction_payload_scalar_fields(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(field.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("{}-scalar-{ordinal}", payload.id),
                "NX block payload scalar identity",
            )?;
            let operation_label = ctx.copy_retained_text(
                &payload.operation_label,
                "NX block payload scalar operation",
            )?;
            let construction_payload =
                ctx.copy_retained_text(&payload.id, "NX block payload scalar owner")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX block payload scalar ordinal", 0, 1))?;
            ctx.reserve_vec(&mut scalars, 1, "NX block payload scalars")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePayloadName>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut names = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, field) in crate::om::name_field::scan(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let Some(source_offset) =
                joined.source_offset(cadmpeg_core::decode::u64_from_index(field.offset()))
            else {
                continue;
            };
            let Some(frame) = field.into_native(ctx, |offset| joined.source_offset(offset))? else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("{}-name-{ordinal}", payload.id),
                "NX block payload name identity",
            )?;
            let operation_label = ctx
                .copy_retained_text(&payload.operation_label, "NX block payload name operation")?;
            let construction_payload =
                ctx.copy_retained_text(&payload.id, "NX block payload name owner")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX block payload name ordinal", 0, 1))?;
            ctx.reserve_vec(&mut names, 1, "NX block payload names")?;
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
    ctx: &DecodeContext<'_>,
    payloads: &[FeatureConstructionPayload],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Result<Vec<FeatureBlockPayloadNamedRecord>, CodecError> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = Vec::new();
        let mut name_reservation = ctx.reserve_scoped(0, "NX block payload names for join")?;
        for name in names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
        {
            ctx.reserve_scoped_vec(
                &mut name_reservation,
                &mut payload_names,
                1,
                "NX block payload names for join",
            )?;
            payload_names.push(name);
        }
        ctx.stable_sort_by_key(
            &mut payload_names,
            |value| value.frame.offset(),
            Ord::cmp,
            "sort NX block payload names",
        )?;
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.frame.offset());
            let mut scalar_fields = Vec::new();
            let mut scalar_reservation =
                ctx.reserve_scoped(0, "NX block payload scalars for join")?;
            for scalar in scalars.iter().filter(|scalar| {
                scalar.payload.id() == payload.id
                    && scalar.payload_offset > name.frame.offset()
                    && scalar.payload_offset < end
            }) {
                ctx.reserve_scoped_vec(
                    &mut scalar_reservation,
                    &mut scalar_fields,
                    1,
                    "NX block payload scalars for join",
                )?;
                scalar_fields.push(scalar);
            }
            ctx.stable_sort_by(
                &mut scalar_fields,
                |value| &value.payload_offset,
                Ord::cmp,
                "sort NX block payload scalars",
            )?;
            let id = ctx.format_retained(
                format_args!("{}-record-{ordinal}", payload.id),
                "NX block payload named record identity",
            )?;
            let operation_label = ctx.copy_retained_text(
                &payload.operation_label,
                "NX block payload named record operation",
            )?;
            let construction_payload =
                ctx.copy_retained_text(&payload.id, "NX block payload named record owner")?;
            let name_field =
                ctx.copy_retained_text(&name.id, "NX block payload named record name field")?;
            let mut scalar_ids = Vec::new();
            for scalar in scalar_fields {
                let scalar_id = ctx
                    .copy_retained_text(&scalar.id, "NX block payload named record scalar field")?;
                ctx.reserve_vec(
                    &mut scalar_ids,
                    1,
                    "NX block payload named record scalar fields",
                )?;
                scalar_ids.push(scalar_id);
            }
            ctx.reserve_vec(&mut records, 1, "NX block payload named records")?;
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
    Ok(records)
}

/// Type exact two-scalar `Point<positive decimal>` `BLOCK` payload intervals.
pub(in crate::native) fn feature_block_payload_points(
    ctx: &DecodeContext<'_>,
    records: &[FeatureBlockPayloadNamedRecord],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Result<Vec<FeatureBlockPayloadPoint>, CodecError> {
    let mut points = Vec::new();
    let scan_work = scalars
        .len()
        .checked_mul(2)
        .and_then(|scalar_scans| names.len().checked_add(scalar_scans))
        .and_then(|per_record| per_record.checked_mul(records.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX block payload points", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "scan NX block payload points",
    )?;
    for record in records {
        let Some(name) = names.iter().rev().find(|name| name.id == record.name_field) else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(name.frame.value().len()),
            "parse NX block payload point name",
        )?;
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
        let id = ctx.format_retained(
            format_args!("{}-point", record.id),
            "NX block payload point identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(&record.operation_label, "NX block payload point operation")?;
        let named_record =
            ctx.copy_retained_text(&record.id, "NX block payload point named record")?;
        let name = ctx.copy_retained_text(name.frame.value(), "NX block payload point name")?;
        let first_id = ctx.copy_retained_text(&first.id, "NX block payload point first scalar")?;
        let second_id =
            ctx.copy_retained_text(&second.id, "NX block payload point second scalar")?;
        ctx.reserve_vec(&mut points, 1, "NX block payload points")?;
        points.push(FeatureBlockPayloadPoint {
            id,
            operation_label,
            named_record,
            name,
            scalar_fields: [first_id, second_id],
            coordinates: [first.scalar.value(), second.scalar.value()].into(),
        });
    }
    Ok(points)
}

/// Group every bit-identical same-name `BLOCK` construction-point witness.
pub(in crate::native) fn feature_block_payload_point_groups(
    ctx: &DecodeContext<'_>,
    points: &[FeatureBlockPayloadPoint],
) -> Result<Vec<FeatureBlockPayloadPointGroup>, CodecError> {
    let mut groups = Vec::new();
    let scan_work = points
        .len()
        .checked_mul(points.len())
        .and_then(|comparisons| comparisons.checked_mul(3))
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX block payload point groups", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "scan NX block payload point groups",
    )?;
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
            let id = ctx.copy_retained_text(&witness.id, "NX block payload point group witness")?;
            ctx.reserve_vec(&mut witnesses, 1, "NX block payload point group witnesses")?;
            witnesses.push(id);
        }
        let id = ctx.format_retained(
            format_args!("{}-group", point.id),
            "NX block payload point group identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &point.operation_label,
            "NX block payload point group operation",
        )?;
        let name = ctx.copy_retained_text(&point.name, "NX block payload point group name")?;
        ctx.reserve_vec(&mut groups, 1, "NX block payload point groups")?;
        groups.push(FeatureBlockPayloadPointGroup {
            id,
            operation_label,
            name,
            points: witnesses,
            coordinates: point.coordinates,
        });
    }
    Ok(groups)
}

/// Resolve the consecutive three-parameter dimension run of `BLOCK` features.
pub(in crate::native) fn feature_block_dimensions(
    ctx: &DecodeContext<'_>,
    constructions: &[FeatureBlockConstruction],
    bindings: &[FeatureParameterBinding],
    declarations: &[ExpressionDeclaration],
    expressions: &[ParameterFormula],
) -> Result<Vec<FeatureBlockDimensions>, CodecError> {
    let mut dimensions = Vec::new();
    for construction in constructions {
        let scan_work = bindings
            .len()
            .checked_add(declarations.len())
            .and_then(|count| {
                expressions
                    .len()
                    .checked_mul(3)
                    .and_then(|expressions| count.checked_add(expressions))
            })
            .ok_or_else(|| ctx.refuse_codec_limit("scan NX block dimensions", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(scan_work),
            "scan NX block dimensions",
        )?;
        let mut operation_bindings = Vec::new();
        let mut binding_reservation = ctx.reserve_scoped(0, "NX block dimension bindings")?;
        for binding in bindings
            .iter()
            .filter(|binding| binding.operation_label == construction.operation_label)
        {
            ctx.reserve_scoped_vec(
                &mut binding_reservation,
                &mut operation_bindings,
                1,
                "NX block dimension bindings",
            )?;
            operation_bindings.push(binding);
        }
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
        ctx.stable_sort_by_key(
            &mut operation_bindings,
            |value| (value.input_slot, value.reference_ordinal),
            Ord::cmp,
            "sort NX block dimension bindings",
        )?;
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
            ctx.format_retained(
                format_args!("{prefix}block-dimensions{suffix}"),
                "NX block dimensions identity",
            )?
        } else {
            ctx.copy_retained_text(&construction.id, "NX block dimensions identity")?
        };
        let operation_label = ctx.copy_retained_text(
            &construction.operation_label,
            "NX block dimensions operation",
        )?;
        let construction_id =
            ctx.copy_retained_text(&construction.id, "NX block dimensions construction")?;
        let mut anchor_bindings = Vec::new();
        for binding in operation_bindings {
            let id = ctx.copy_retained_text(&binding.id, "NX block dimension anchor binding")?;
            ctx.reserve_vec(
                &mut anchor_bindings,
                1,
                "NX block dimension anchor bindings",
            )?;
            anchor_bindings.push(id);
        }
        let dimension = |slot: usize| -> Result<FeatureBlockDimension, CodecError> {
            Ok(FeatureBlockDimension {
                declaration: ctx
                    .copy_retained_text(&run[slot].id, "NX block dimension declaration")?,
                expression: ctx
                    .copy_retained_text(&resolved[slot].0.id, "NX block dimension expression")?,
                value: resolved[slot].1,
            })
        };
        let values = [dimension(0)?, dimension(1)?, dimension(2)?];
        ctx.reserve_vec(&mut dimensions, 1, "NX block dimensions")?;
        dimensions.push(FeatureBlockDimensions {
            id,
            operation_label,
            construction: construction_id,
            anchor_bindings,
            dimensions: values,
        });
    }
    Ok(dimensions)
}

/// Decode persistent object frames from bounded offset-store blocks.
pub(in crate::native) fn data_block_object_frames(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockObjectFrame>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut out = Vec::new();
    for (data_block, (bytes, source_offset)) in blocks.iter() {
        for (ordinal, frame) in crate::om::data_block_object_frames(ctx, bytes)?
            .into_iter()
            .enumerate()
        {
            let id = data_block_object_frame_id(ctx, data_block, ordinal)?;
            let data_block =
                ctx.copy_retained_text(data_block, "retain NX data block object frame source")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX data block object frame ordinal", 0, 1))?;
            let offset = source_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(frame.offset))
                .ok_or_else(|| ctx.refuse_codec_limit("NX data block object frame offset", 0, 1))?;
            ctx.charge_entities(1, "NX data block object frame")?;
            ctx.reserve_vec(&mut out, 1, "NX data block object frames")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data_block: &str,
    ordinal: usize,
) -> Result<String, cadmpeg_core::CodecError> {
    use std::fmt::Write;

    let Some((section, block)) = data_block
        .strip_prefix("nx:om-data-blocks-")
        .and_then(|rest| rest.split_once(":block#"))
    else {
        return Err(cadmpeg_core::CodecError::Malformed(
            "invalid NX data block object frame source".into(),
        ));
    };
    let prefix = "nx:om-data-block-object-frames-";
    let infix = ":block-frame#";
    let length = prefix
        .len()
        .checked_add(section.len())
        .and_then(|length| length.checked_add(infix.len()))
        .and_then(|length| length.checked_add(block.len()))
        .and_then(|length| length.checked_add(1))
        .and_then(|length| {
            length.checked_add(
                ordinal
                    .checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
            )
        })
        .ok_or_else(|| ctx.refuse_codec_limit("retain NX data block object frame id", 0, 1))?;
    let mut id = ctx.retained_string(length, "retain NX data block object frame id")?;
    write!(&mut id, "{prefix}{section}{infix}{block}-{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit("format NX data block object frame id", 0, 1))?;
    Ok(id)
}

pub(super) fn charged_unique_offset_data_block(
    ctx: &DecodeContext<'_>,
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    object_index: u32,
) -> Result<Option<String>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(indexed.len()),
        "resolve NX source block",
    )?;
    let Some(section_ordinal) = unique_offset_data_store(indexed, &[object_index]) else {
        return Ok(None);
    };
    Ok(Some(format_offset_data_block_id(
        ctx,
        section_ordinal,
        object_index,
    )?))
}

pub(super) fn format_offset_data_block_id(
    ctx: &DecodeContext<'_>,
    section_ordinal: usize,
    object_index: u32,
) -> Result<String, CodecError> {
    let digits = |value: usize| {
        value
            .checked_ilog10()
            .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
    };
    let section_digits = digits(section_ordinal);
    let object_digits = digits(usize::try_from(object_index).map_err(|_| {
        ctx.refuse_codec_limit("NX source block index", 0, u64::from(object_index))
    })?);
    let length = "nx:om-data-blocks-"
        .len()
        .checked_add(section_digits)
        .and_then(|length| length.checked_add(":block#".len()))
        .and_then(|length| length.checked_add(object_digits))
        .ok_or_else(|| ctx.refuse_codec_limit("NX source block identity", 0, 1))?;
    let mut id = ctx.retained_string(length, "NX source block identity")?;
    write!(
        &mut id,
        "nx:om-data-blocks-{section_ordinal}:block#{object_index}"
    )
    .map_err(|_| ctx.refuse_codec_limit("write NX source block identity", 0, 1))?;
    Ok(id)
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
