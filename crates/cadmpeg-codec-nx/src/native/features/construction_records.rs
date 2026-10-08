use super::block_reference::{BlockReferencePosition, FeatureBlockConstructionReference};
use super::body_scalar_triple::FeatureOperationBodyScalarTriple;
use super::extrude_32::{FeatureExtrude32Construction, FeatureExtrudePayload32Branch};
use super::joined_payload::JoinedPayload;
use super::object_frame::DataBlockObjectFrame;
use super::payload_content::{
    block_store, copy_block_ids, operation_key, shared_block_store, FeaturePayloadBlock,
    FeaturePayloadContent,
};
use super::payload_name::FeaturePayloadName;
use super::point_scalar_lane::{FeaturePointConstructionScalarLane, PointScalarPositions};
use super::reference::ConstructionReference;
use super::swp104_branch::FeatureSwp104LeadingBranch;
use super::terminal_discriminator::FeatureOperationTerminalDiscriminator;
use super::{
    construction_payload_frames, format_feature_child_id, format_feature_history_id,
    offset_data_block_bytes, parse_sketch_point_name, ConstructionPayloadFrames,
    FeatureBlockConstruction, FeatureBlockDimension, FeatureBlockDimensions,
    FeatureBlockPayloadNamedRecord, FeatureBlockPayloadPoint, FeatureBlockPayloadPointGroup,
    FeatureBodyReference, FeatureConstructionMember, FeatureConstructionOwner,
    FeatureConstructionPayload, FeatureExtrudeConstructionProfile,
    FeatureExtrudeConstructionProfileReference, FeatureExtrudePayloadHeader,
    FeatureExtrudeProfileReference, FeatureHistory, FeatureInputBlock,
    FeatureOperationBody11Continuation, FeatureOperationBodyMember, FeatureOperationBodyOperand,
    FeatureOperationBodyReferenceLane, FeatureOperationBodyReferences, FeatureOperationLabel,
    FeatureParameterBinding, FeaturePayloadScalar, FeaturePayloadScalarPair,
    FeaturePointConstructionHeader, FeatureProjectedCurveConstructionString,
    FeatureProjectedCurveKind, FeatureProjectedCurveReference, FeatureScalarPairPayload,
    FeatureScalarPayload, FeatureSurfaceConstructionPayload, FeatureSurfaceConstructionReference,
    FeatureSurfaceConstructionString, FeatureThruCurveConstructionEnvelope,
};
use crate::container::Container;
use crate::native::om::{ExpressionDeclaration, ParameterFormula};
use crate::native::segments::SegmentBodyBinding;
use crate::om::compact::LocatedCompactIndex;
use crate::om::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::fmt::Write;

pub(super) struct ResolvedFeaturePayloadReference<'h> {
    pub(super) section_key: &'h str,
    pub(super) operation_ordinal: usize,
    pub(super) ordinal: usize,
    pub(super) token: PayloadIndexToken,
    pub(super) data_block: Option<String>,
    pub(super) source_offset: u64,
}

/// Resolved payload references, held under the returned scoped reservation.
pub(super) fn resolved_feature_payload_references<'h, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    history: &'h FeatureHistory<'_, '_, '_>,
    decode: impl Fn(
        crate::om::operation_record::OperationPayload<'_>,
        u64,
    ) -> Result<Option<Vec<(PayloadIndexToken, u64)>>, cadmpeg_core::CodecError>,
) -> Result<
    (
        Vec<ResolvedFeaturePayloadReference<'h>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut storage = ctx.reserve_scoped(0, "NX resolved feature payload references")?;
    let mut references = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(decoded) = decode(record.payload_view(), entry_offset)? else {
                continue;
            };
            for (ordinal, (token, source_offset)) in decoded.into_iter().enumerate() {
                let data_block = charged_unique_offset_data_block(ctx, &indexed, token.value())?;
                ctx.reserve_scoped_vec(
                    &mut storage,
                    &mut references,
                    1,
                    "NX resolved feature payload references",
                )?;
                references.push(ResolvedFeaturePayloadReference {
                    section_key,
                    operation_ordinal,
                    ordinal,
                    token,
                    data_block,
                    source_offset,
                });
            }
        }
    }
    Ok((references, storage))
}

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub(in crate::native) fn feature_projected_curve_references(
    ctx: &DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureProjectedCurveReference>, CodecError> {
    let (references, _references_storage) =
        resolved_feature_payload_references(ctx, history, |record, base| {
            let Some(field) =
                crate::om::projected_references::ProjectedCurveReferences::read(ctx, record)?
            else {
                return Ok(None);
            };
            Ok(field
                .into_references()
                .map(|reference| {
                    Some((
                        reference.token,
                        base.checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))?,
                    ))
                })
                .collect())
        })?;
    let mut output = Vec::new();
    for reference in ctx.admit_iter(references, "build NX projected-curve references")? {
        let operation_label = format_feature_history_id(
            ctx,
            "operation-label",
            reference.section_key,
            reference.operation_ordinal,
            None,
        )?;
        let id = format_feature_history_id(
            ctx,
            "projected-curve-reference",
            reference.section_key,
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
    for label in ctx.admit_iter(labels, "index NX projected-curve labels")? {
        kind_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut kinds,
                label.id.as_str(),
                label.value.as_str(),
                "NX projected curve kinds",
            )
        })?;
    }
    let (groups, _groups_reservation) = ctx.collect_scoped_btree_groups(
        references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "group NX projected-curve references",
    )?;
    let mut payloads = Vec::new();
    for (operation_label, mut field) in
        ctx.admit_iter(groups, "visit NX projected-curve operations")?
    {
        let Some(kind) =
            ctx.get_btree_map(&kinds, &operation_label, "find NX projected-curve kind")?
        else {
            continue;
        };
        let (operation_kind, expected_len) = match *kind {
            "CPROJ" => (FeatureProjectedCurveKind::Projected, 3),
            "CPROJ_CMB" => (FeatureProjectedCurveKind::Combined, 8),
            _ => continue,
        };
        if field.len() != expected_len {
            continue;
        }
        ctx.stable_sort_by(
            &mut field,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX projected curve references",
        )?;
        if field
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal).ok() != Some(reference.ordinal))
        {
            continue;
        }
        let Some((data_blocks, block_reservation)) = copy_block_ids(
            ctx,
            field
                .iter()
                .map(|reference| reference.data_block.as_deref()),
            "NX projected curve block IDs",
        )?
        else {
            continue;
        };
        if shared_block_store(
            ctx,
            &data_blocks,
            "validate NX projected-curve block owners",
        )?
        .is_none()
        {
            continue;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        drop(block_reservation);
        let Some(operation_key) = operation_key(
            ctx,
            operation_label,
            "find NX projected-curve operation key",
        )?
        else {
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
    for payload in ctx.admit_iter(payloads, "scan NX projected-curve string payloads")? {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            payload.content.block_ids(),
            payload.content.blocks().len(),
            &blocks,
        )?
        else {
            continue;
        };
        for (ordinal, value) in ctx
            .admit_iter(
                crate::om::string_values(ctx, joined.bytes(), 0)?,
                "visit NX projected-curve payload strings",
            )?
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(ctx, payload_offset)? else {
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
            let value = value
                .value
                .try_into_owned_for_decode(ctx, "NX projected curve string value")?;
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
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeaturePointConstructionHeader>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut headers = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(header) = crate::om::point_feature_payload_header(record.payload_view())
            else {
                continue;
            };
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
            let data_block =
                charged_unique_offset_data_block(ctx, &indexed, header.reference.token.value())?;
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
        }
    }
    Ok(headers)
}

/// Decode exact scalar lanes selected by uniquely resolved point-feature headers.
pub(in crate::native) fn feature_point_construction_scalar_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    headers: &[FeaturePointConstructionHeader],
) -> Result<Vec<FeaturePointConstructionScalarLane>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = container.indexed_om_sections(ctx)?;
    let mut lanes = Vec::new();
    for header in ctx.admit_iter(headers, "build NX point scalar lanes")? {
        let Some(expected_target) = header.data_block.as_deref() else {
            continue;
        };
        let Ok(target_ordinal) = usize::try_from(header.token.value()) else {
            continue;
        };
        if target_ordinal < 2 {
            continue;
        }
        let Some(tail) = ctx.strip_prefix(
            expected_target,
            "nx:om-data-blocks-",
            "split NX point scalar lane target",
        )?
        else {
            continue;
        };
        let Some((expected_section, expected_block)) =
            ctx.split_once(tail, ":block#", "split NX point scalar lane target")?
        else {
            continue;
        };
        let section_ordinal = ctx
            .parse_text::<usize>(
                expected_section,
                "parse NX point scalar lane section ordinal",
            )?
            .ok();
        let block_ordinal = ctx
            .parse_text::<usize>(expected_block, "parse NX point scalar lane block ordinal")?
            .ok();
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
        let Some(section_ordinal) = section_ordinal else {
            continue;
        };
        let Some((entry, section)) = indexed.get(section_ordinal) else {
            continue;
        };
        let Some((_, _, records)) = section.as_offset_only() else {
            continue;
        };
        let (Some(preceding), Some(target)) = (
            records.get(target_ordinal - 2),
            records.get(target_ordinal - 1),
        ) else {
            continue;
        };
        let Some(lane) = crate::om::point_feature_scalar_lane(preceding.bytes, target.bytes) else {
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
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureSurfaceConstructionReference>, CodecError> {
    let (references, _references_storage) =
        resolved_feature_payload_references(ctx, history, |record, base| {
            if let Some(field) =
                crate::om::surface_envelope::surface_feature_payload_references(ctx, record)?
            {
                if let Some(field) = field.relocate(base) {
                    return Ok(Some(field.references().into_iter().collect()));
                }
            }
            Ok(
                crate::om::surface_envelope::thru_curve_payload_references(record)
                    .and_then(|field| field.relocate(base))
                    .map(|field| field.references().into_iter().collect()),
            )
        })?;
    let mut output = Vec::new();
    for reference in ctx.admit_iter(references, "build NX surface construction references")? {
        let operation_label = format_feature_history_id(
            ctx,
            "operation-label",
            reference.section_key,
            reference.operation_ordinal,
            None,
        )?;
        let id = format_feature_history_id(
            ctx,
            "surface-construction-reference",
            reference.section_key,
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
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureThruCurveConstructionEnvelope>, cadmpeg_core::CodecError> {
    let mut envelopes = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(field) =
                crate::om::surface_envelope::thru_curve_payload_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                continue;
            };
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
            let envelope = FeatureThruCurveConstructionEnvelope {
                id,
                operation_label,
                discriminator: field.discriminator,
                controls: field.controls,
                trailing_control: field.trailing_control,
                trailing_value: field.trailing_value,
                source_offset: field.origin(),
            };
            envelopes.push(envelope);
        }
    }
    Ok(envelopes)
}

/// Decode and resolve each exact leading `SWP104` construction branch.
pub(in crate::native) fn feature_swp104_leading_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureSwp104LeadingBranch>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut branches = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let branch = crate::om::swp104_payload_leading_branch(ctx, record.payload_view())?;
            let Some(branch) = branch else {
                continue;
            };
            let Some(source_offset) = entry_offset.checked_add(
                cadmpeg_core::decode::u64_from_index(record.payload_offset()),
            ) else {
                continue;
            };
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
            let Some(branch) = FeatureSwp104LeadingBranch::from_source(
                ctx,
                id,
                operation_label,
                source_offset,
                branch,
                |token| charged_unique_offset_data_block(ctx, &indexed, token.value()),
            )?
            else {
                continue;
            };
            ctx.reserve_vec(&mut branches, 1, "NX SWP104 leading branches")?;
            branches.push(branch);
        }
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
    let (groups, _groups_reservation) = ctx.collect_scoped_btree_groups(
        references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "group NX surface construction references",
    )?;
    let mut output = Vec::new();
    for (operation_label, mut graph) in
        ctx.admit_iter(groups, "visit NX surface construction operations")?
    {
        if graph.len() != 14 {
            continue;
        }
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
        let Some((data_blocks, _source_id_storage)) = copy_block_ids(
            ctx,
            graph
                .iter()
                .map(|reference| reference.data_block.as_deref()),
            "NX surface construction source blocks",
        )?
        else {
            continue;
        };
        if shared_block_store(
            ctx,
            &data_blocks,
            "validate NX surface construction block owners",
        )?
        .is_none()
        {
            continue;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let Some(operation_key) = operation_key(
            ctx,
            operation_label,
            "find NX surface construction operation key",
        )?
        else {
            continue;
        };
        let id = ctx.format_retained(
            format_args!("nx:feature-history:surface-construction-payload#{operation_key}"),
            "NX surface construction payload identity",
        )?;
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
    construction_payload_frames::<SurfaceConstructionScalarPairs>(ctx, container, payloads)
}

/// Frames of the surface construction scalar pairs family.
struct SurfaceConstructionScalarPairs;

impl ConstructionPayloadFrames for SurfaceConstructionScalarPairs {
    type Payload = FeatureSurfaceConstructionPayload;
    type Row = crate::om::binary64_pair::Binary64Pair<crate::om::binary64_pair::ObjectPairForm>;
    type Record = FeaturePayloadScalarPair;

    fn blocks(payload: &Self::Payload) -> &[FeaturePayloadBlock] {
        payload.content.blocks()
    }

    fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Self::Row>, CodecError> {
        crate::om::binary64_pair::object_pairs(ctx, bytes)
    }

    fn build(
        ctx: &DecodeContext<'_>,
        payload: &Self::Payload,
        ordinal: usize,
        pair: Self::Row,
        joined: &JoinedPayload<'_>,
    ) -> Result<Option<Self::Record>, CodecError> {
        let Some(frame) = pair.into_wire_frame() else {
            return Ok(None);
        };
        let Some(first) = joined.source_at(ctx, pair.value_offsets()[0])? else {
            return Ok(None);
        };
        let Some(second) = joined.source_at(ctx, pair.value_offsets()[1])? else {
            return Ok(None);
        };
        let Some(source) = joined.source_at(ctx, pair.offset())? else {
            return Ok(None);
        };
        let id = format_feature_child_id(ctx, &payload.id, "-scalar-pair-", ordinal)?;
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX surface scalar pair ordinal", 0, 1))?;
        let record = FeaturePayloadScalarPair {
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
        };
        Ok(Some(record))
    }
}

/// Decode exact printable string frames from reconstructed surface payloads.
pub(in crate::native) fn feature_surface_construction_strings(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Result<Vec<FeatureSurfaceConstructionString>, CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut strings = Vec::new();
    for payload in ctx.admit_iter(payloads, "scan NX surface payload strings")? {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            payload.content.block_ids(),
            payload.content.blocks().len(),
            &blocks,
        )?
        else {
            continue;
        };
        for (ordinal, value) in ctx
            .admit_iter(
                crate::om::surface_payload_strings(ctx, joined.bytes())?,
                "visit NX surface payload strings",
            )?
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(ctx, payload_offset)? else {
                continue;
            };
            let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit("nx surface payload string ordinal", 0, u64::MAX)
            })?;
            let id = format_feature_child_id(ctx, &payload.id, "-string-", ordinal)?;
            let value = value
                .value
                .try_into_owned_for_decode(ctx, "NX surface payload string text")?;
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
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureExtrudeProfileReference>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let (decoded, _profile_storage) =
                match crate::om::extrude_profile::extrude_profile_references(
                    ctx,
                    record.payload_view(),
                ) {
                    Ok((Some(field), storage)) => (field, storage),
                    Ok((None, _)) => continue,
                    Err(error) => {
                        return Err(error);
                    }
                };
            let Some(decoded) = decoded.relocate(ctx, entry_offset)? else {
                continue;
            };
            for (ordinal, (token, source_offset, witness_source_offset)) in
                decoded.references().enumerate()
            {
                ctx.charge_work(1, "visit NX extrusion profile references")?;
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
                let data_block = charged_unique_offset_data_block(ctx, &indexed, token.value())?;
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
        }
    }
    Ok(references)
}

/// Decode fixed scalar headers from bounded extrusion payloads.
pub(in crate::native) fn feature_extrude_payload_headers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureExtrudePayloadHeader>, cadmpeg_core::CodecError> {
    let mut headers = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(header) = crate::om::extrude_payload_header(record.payload_view()) else {
                continue;
            };
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
                .ok_or_else(|| ctx.refuse_codec_limit("NX extrude header source offset", 0, 1))?;
            ctx.reserve_vec(&mut headers, 1, "NX extrude payload headers")?;
            headers.push(FeatureExtrudePayloadHeader {
                id,
                operation_label,
                scalars: header.scalars,
                source_offset,
            });
        }
    }
    Ok(headers)
}

/// Decode exact terminal discriminator lanes from bounded operation payloads.
pub(in crate::native) fn feature_operation_terminal_discriminators(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureOperationTerminalDiscriminator>, cadmpeg_core::CodecError> {
    let mut lanes = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let frame = crate::om::terminal_discriminator::operation_terminal_discriminator(
                ctx,
                record.payload_view(),
            )?;
            let Some(frame) = frame.and_then(|frame| frame.relocate(entry_offset)) else {
                continue;
            };
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
        }
    }
    Ok(lanes)
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub(in crate::native) fn feature_operation_body_scalar_triples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureOperationBodyScalarTriple>, cadmpeg_core::CodecError> {
    let mut triples = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let (rows, _triple_storage) =
                crate::om::body_scalar_triple::operation_body_scalar_triples(
                    ctx,
                    record.body_view(),
                )?;
            for triple in ctx.admit_iter(rows, "visit NX operation body scalar triples")? {
                let Some(scalars) = triple.scalars.relocate(entry_offset) else {
                    continue;
                };
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
            }
        }
    }
    Ok(triples)
}

/// Decode ordered member lanes following branch-`11` operation body clauses.
pub(in crate::native) fn feature_operation_body_members(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureOperationBodyMember>, cadmpeg_core::CodecError> {
    let mut members = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let groups = crate::om::operation_body_members(ctx, record.body_view())?;
            for group in ctx.admit_iter(groups, "visit NX operation body member groups")? {
                for (ordinal, member) in ctx
                    .admit_iter(group.members, "visit NX operation body members")?
                    .enumerate()
                {
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
                            ctx.refuse_codec_limit("NX operation body member source offset", 0, 1)
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
        }
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
    let mut operands = Vec::new();
    let input_stores = OperationInputStores::new(ctx, inputs)?;
    let mut block_ids = BTreeMap::new();
    let mut block_id_storage = ctx.reserve_scoped(0, "NX operation operand data block index")?;
    for (position, block) in ctx
        .admit_iter(blocks, "index NX operation operand data blocks")?
        .enumerate()
    {
        block_id_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut block_ids,
                block.id.as_str(),
                position,
                "NX operation operand data block index",
            )
        })?;
    }
    let mut reference_stores = BTreeMap::new();
    let mut reference_storage = ctx.reserve_scoped(0, "NX operation body reference stores")?;
    let mut reference_without_inputs = false;
    for (position, reference) in ctx
        .admit_iter(references, "index NX operation body references")?
        .enumerate()
    {
        let (has_inputs, store) = input_stores.get(ctx, &reference.operation_label)?;
        reference_without_inputs |= !has_inputs;
        if let Some(store) = store {
            reference_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut reference_stores,
                    store,
                    position,
                    "NX operation body reference stores",
                )
            })?;
        }
    }
    let (binding_groups, _binding_storage) = ctx.collect_scoped_btree_groups(
        ctx.admit_iter(bindings, "walk NX operation operand segment bindings")?
            .flat_map(|binding| {
                let alias = (binding.body_alias_object_index != binding.body_object_index)
                    .then_some(binding.body_alias_object_index);
                std::iter::once(binding.body_object_index)
                    .chain(alias)
                    .map(move |index| (index, binding))
            }),
        "index NX operation operand segment bindings",
    )?;
    for member in ctx.admit_iter(members, "build NX feature operation body operands")? {
        if member.member.atom.value() == member.body_object_index {
            continue;
        }
        let (has_inputs, member_store) = input_stores.get(ctx, &member.operation_label)?;
        if member_store.is_none() && has_inputs {
            continue;
        }
        let operand_data_block = if let Some(store) = member_store {
            let (id, candidate) = ctx.format_scoped(
                format_args!("{store}:block#{}", member.member.atom.value()),
                "format NX operand data block id",
            )?;
            if ctx.contains_key_btree_map(
                &block_ids,
                id.as_str(),
                "find NX operation operand data block",
            )? {
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
        let same_namespace_reference = match member_store {
            Some(store) => ctx.contains_key_btree_map(
                &reference_stores,
                store,
                "match NX operation body reference store",
            )?,
            None => reference_without_inputs,
        };
        let mut segment_body_bindings = Vec::new();
        if member_store.is_none() {
            let key = member.member.atom.value();
            if let Some(group) = ctx.get_btree_map(
                &binding_groups,
                &key,
                "find NX operation operand segment bindings",
            )? {
                for binding in ctx.admit_iter(group, "copy NX operation operand bindings")? {
                    ctx.reserve_vec(
                        &mut segment_body_bindings,
                        1,
                        "NX operation operand bindings",
                    )?;
                    segment_body_bindings.push(ctx.copy_retained_text(
                        &binding.id,
                        "retain NX operation operand binding id",
                    )?);
                }
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

/// The data-block store shared by one operation's input blocks.
struct InputStore<'a> {
    store: Option<&'a str>,
    conflict: bool,
}

/// Every operation's input data-block store, built once from the inputs.
///
/// An operation has a store when its inputs name exactly one owner store;
/// inputs without a store do not count, and two different stores leave the
/// operation without one.
pub(super) struct OperationInputStores<'a, 's> {
    stores: BTreeMap<&'a str, InputStore<'a>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'s>,
}

impl<'a, 's> OperationInputStores<'a, 's> {
    pub(super) fn new(
        ctx: &'s DecodeContext<'_>,
        inputs: &'a [FeatureInputBlock],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index NX feature operation input stores";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut stores = BTreeMap::new();
        for input in ctx.admit_iter(inputs, OPERATION)? {
            let candidate = block_store(ctx, &input.data_block, OPERATION)?;
            let label = input.operation_label.as_str();
            let mut fresh = InputStore {
                store: None,
                conflict: false,
            };
            match ctx.get_mut_btree_map(&mut stores, &label, OPERATION)? {
                Some(entry) => Self::merge(ctx, entry, candidate)?,
                None => {
                    Self::merge(ctx, &mut fresh, candidate)?;
                    storage.with_storage(|| {
                        ctx.insert_btree_map(&mut stores, label, fresh, OPERATION)
                    })?;
                }
            }
        }
        Ok(Self {
            stores,
            _storage: storage,
        })
    }

    fn merge(
        ctx: &DecodeContext<'_>,
        entry: &mut InputStore<'a>,
        candidate: Option<&'a str>,
    ) -> Result<(), CodecError> {
        if entry.conflict {
            return Ok(());
        }
        let Some(candidate) = candidate else {
            return Ok(());
        };
        match entry.store {
            None => entry.store = Some(candidate),
            Some(existing) => {
                if !ctx.equal(
                    existing,
                    candidate,
                    "compare NX feature operation input stores",
                )? {
                    entry.store = None;
                    entry.conflict = true;
                }
            }
        }
        Ok(())
    }

    /// Whether the operation has inputs, and its shared store when unique.
    pub(super) fn get(
        &self,
        ctx: &DecodeContext<'_>,
        operation_label: &str,
    ) -> Result<(bool, Option<&'a str>), CodecError> {
        Ok(
            match ctx.get_btree_map(
                &self.stores,
                &operation_label,
                "find NX feature operation input store",
            )? {
                Some(entry) => (true, entry.store),
                None => (false, None),
            },
        )
    }
}

pub(super) fn replace_operation_text(
    ctx: &DecodeContext<'_>,
    value: &str,
    old: &str,
    new: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    match ctx.split_once(value, old, operation)? {
        Some((prefix, suffix)) => {
            ctx.format_retained(format_args!("{prefix}{new}{suffix}"), operation)
        }
        None => ctx.copy_retained_text(value, operation),
    }
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub(in crate::native) fn feature_operation_body_11_continuations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureOperationBody11Continuation>, cadmpeg_core::CodecError> {
    let mut continuations = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let rows = crate::om::operation_body_11_continuations(ctx, record.body_view())?;
            for continuation in ctx.admit_iter(rows, "visit NX trim-body continuations")? {
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
                    .ok_or_else(|| ctx.refuse_codec_limit("NX trim body terminal offset", 0, 1))?;
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
        }
    }
    Ok(continuations)
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub(in crate::native) fn feature_operation_body_reference_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureOperationBodyReferenceLane>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut lanes = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let parsed = crate::om::operation_body_reference_lanes(ctx, record.body_view())?;
            for lane in ctx.admit_iter(parsed, "visit NX operation body reference lanes")? {
                let references = match lane.values {
                    crate::om::OperationBodyReferenceLaneValues::CompactIndex(values) => {
                        let mut references = Vec::new();
                        for value in ctx.admit_iter(values, "resolve NX body compact references")? {
                            let data_block = charged_unique_offset_data_block(
                                ctx,
                                &indexed,
                                value.atom.value(),
                            )?;
                            let source_offset = entry_offset
                                .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                                .ok_or_else(|| {
                                    ctx.refuse_codec_limit("NX body compact reference offset", 0, 1)
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
                        for value in ctx.admit_iter(values, "resolve NX body object references")? {
                            let data_block = charged_unique_offset_data_block(
                                ctx,
                                &indexed,
                                value.token.value(),
                            )?;
                            let source_offset = entry_offset
                                .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
                                .ok_or_else(|| {
                                    ctx.refuse_codec_limit("NX body object reference offset", 0, 1)
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
        }
    }
    Ok(lanes)
}

/// Join the two exact encodings of an extrusion construction profile.
pub(in crate::native) fn feature_extrude_construction_profiles(
    ctx: &DecodeContext<'_>,
    references: &[FeatureExtrudeProfileReference],
) -> Result<Vec<FeatureExtrudeConstructionProfile>, CodecError> {
    let (groups, _groups_reservation) = ctx.collect_scoped_btree_groups(
        references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "group NX extrusion profile references",
    )?;
    let mut profiles = Vec::new();
    for (operation_label, mut operation_references) in
        ctx.admit_iter(groups, "visit NX extrusion profile operations")?
    {
        ctx.stable_sort_by(
            &mut operation_references,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX extrude profile references",
        )?;
        let mut ordinal = 0usize;
        if ctx.any_by(
            operation_references.as_slice(),
            |reference| {
                let misordered = u32::try_from(ordinal) != Ok(reference.ordinal);
                ordinal += 1;
                Ok(misordered)
            },
            "validate NX extrusion profile order",
        )? {
            continue;
        }
        if ctx.any_by(
            operation_references.as_slice(),
            |reference| {
                Ok(reference.data_block.is_none() || reference.witness_source_offset.is_none())
            },
            "validate NX extrusion profile targets",
        )? {
            continue;
        }
        let mut profile_references = Vec::new();
        for reference in
            ctx.admit_iter(operation_references, "copy NX extrusion profile references")?
        {
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
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureExtrudePayload32Branch>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut branches = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let (frame, frame_storage) =
                crate::om::extrude_32::extrude_payload_32_branch(ctx, record.body_view())?;
            let Some(frame) = frame.and_then(|frame| frame.relocate(entry_offset)) else {
                continue;
            };
            let frame = frame.map_bindings(ctx, |index, ()| {
                charged_unique_offset_data_block(ctx, &indexed, index)
            })?;
            drop(frame_storage);
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
        }
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
        if ctx.any_by(
            items.as_slice(),
            |(_, binding)| Ok(binding.is_none()),
            operation,
        )? {
            return Ok(None);
        }
        let mut copied = Vec::new();
        for (_, binding) in ctx.admit_iter(items.as_slice(), operation)? {
            let Some(binding) = binding.as_deref() else {
                return Ok(None);
            };
            let id = ctx.copy_retained_text(binding, operation)?;
            ctx.reserve_vec(&mut copied, 1, operation)?;
            copied.push(id);
        }
        crate::om::branch_items::BranchItems::new(copied)
            .map(Some)
            .map_err(CodecError::malformed)
    }

    let (branch_groups, _branch_reservation) = ctx.collect_scoped_btree_groups(
        branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
        "group NX extrusion branches",
    )?;
    let (mut profile_groups, _profile_reservation) = ctx.collect_scoped_btree_groups(
        references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "group NX extrusion profiles",
    )?;
    let mut constructions = Vec::new();
    for (operation_label, matching) in
        ctx.admit_iter(branch_groups, "visit NX extrusion branch operations")?
    {
        let [branch] = matching.as_slice() else {
            continue;
        };
        let Some(mut profile) = ctx
            .get_mut_btree_map(
                &mut profile_groups,
                &operation_label,
                "find NX extrusion profile",
            )?
            .map(std::mem::take)
        else {
            continue;
        };
        ctx.stable_sort_by(
            &mut profile,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX extrude 32 profiles",
        )?;
        let Ok(profile) = crate::om::branch_items::BranchItems::new(profile) else {
            continue;
        };
        let mut ordinal = 0usize;
        if ctx.any_by(
            profile.as_slice(),
            |reference| {
                let misordered = u32::try_from(ordinal) != Ok(reference.ordinal);
                ordinal += 1;
                Ok(misordered)
            },
            "validate NX extrusion profile order",
        )? {
            continue;
        }
        if ctx.any_by(
            profile.as_slice(),
            |reference| Ok(reference.data_block.is_none()),
            "validate NX extrusion profile targets",
        )? {
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
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureBlockConstructionReference>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(field) =
                crate::om::block_construction::block_construction_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                continue;
            };
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
                let data_block = charged_unique_offset_data_block(ctx, &indexed, token.value())?;
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
        }
    }
    Ok(references)
}

/// Join complete, uniquely resolved `BLOCK` construction-reference fields.
pub(in crate::native) fn feature_block_constructions(
    ctx: &DecodeContext<'_>,
    references: &[FeatureBlockConstructionReference],
) -> Result<Vec<FeatureBlockConstruction>, CodecError> {
    let (groups, _groups_reservation) = ctx.collect_scoped_btree_groups(
        references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "group NX block construction references",
    )?;
    let mut constructions = Vec::new();
    for (operation_label, mut field) in
        ctx.admit_iter(groups, "visit NX block construction operations")?
    {
        if field.len() != 19 {
            continue;
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
    for construction in ctx.admit_iter(constructions, "build NX block construction payloads")? {
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
    for payload in ctx.admit_iter(payloads, "scan NX block scalar payloads")? {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            payload.content.block_ids(),
            payload.content.blocks().len(),
            &blocks,
        )?
        else {
            continue;
        };
        for (ordinal, field) in ctx
            .admit_iter(
                crate::om::construction_payload_scalar_fields(ctx, joined.bytes())?,
                "visit NX block payload scalar fields",
            )?
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(field.offset);
            let Some(source_offset) = joined.source_offset(ctx, payload_offset)? else {
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
    for payload in ctx.admit_iter(payloads, "scan NX block name payloads")? {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            payload.content.block_ids(),
            payload.content.blocks().len(),
            &blocks,
        )?
        else {
            continue;
        };
        for (ordinal, field) in ctx
            .admit_iter(
                crate::om::name_field::scan(ctx, joined.bytes())?,
                "visit NX block payload name fields",
            )?
            .enumerate()
        {
            let Some(source_offset) =
                joined.source_offset(ctx, cadmpeg_core::decode::u64_from_index(field.offset()))?
            else {
                continue;
            };
            let Some(frame) = field.into_native(ctx, |offset| joined.source_offset(ctx, offset))?
            else {
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
    let (name_groups, _name_groups_reservation) = ctx.collect_scoped_btree_groups(
        names
            .iter()
            .map(|name| (name.construction_payload.as_str(), name)),
        "group NX block payload names",
    )?;
    let (scalar_groups, _scalar_groups_reservation) = ctx.collect_scoped_btree_groups(
        scalars.iter().map(|scalar| (scalar.payload.id(), scalar)),
        "group NX block payload scalars",
    )?;
    for payload in ctx.admit_iter(payloads, "join NX block payload names")? {
        let Some(name_group) = ctx.get_btree_map(
            &name_groups,
            payload.id.as_str(),
            "find NX block payload names",
        )?
        else {
            continue;
        };
        let mut payload_names = Vec::new();
        let mut name_reservation = ctx.reserve_scoped(0, "NX block payload names for join")?;
        for name in ctx.admit_iter(name_group, "copy NX block payload names")? {
            ctx.push_scoped_vec(
                &mut name_reservation,
                &mut payload_names,
                *name,
                "NX block payload names for join",
            )?;
        }
        ctx.stable_sort_by_key(
            &mut payload_names,
            |value| value.frame.offset(),
            Ord::cmp,
            "sort NX block payload names",
        )?;
        let mut payload_scalars = Vec::new();
        let mut scalar_reservation = ctx.reserve_scoped(0, "NX block payload scalars for join")?;
        if let Some(scalar_group) = ctx.get_btree_map(
            &scalar_groups,
            payload.id.as_str(),
            "find NX block payload scalars",
        )? {
            for scalar in ctx.admit_iter(scalar_group, "copy NX block payload scalars")? {
                ctx.push_scoped_vec(
                    &mut scalar_reservation,
                    &mut payload_scalars,
                    *scalar,
                    "NX block payload scalars for join",
                )?;
            }
        }
        ctx.stable_sort_by(
            &mut payload_scalars,
            |value| &value.payload_offset,
            Ord::cmp,
            "sort NX block payload scalars",
        )?;
        let payload_end = payload.content.byte_len(ctx)?;
        for (ordinal, name) in ctx
            .admit_iter(payload_names.as_slice(), "visit NX block payload names")?
            .enumerate()
        {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload_end, |next| next.frame.offset());
            let start = name.frame.offset();
            let first = ctx.partition_point(
                &payload_scalars,
                |scalar| Ok(scalar.payload_offset <= start),
                "find NX block payload scalar interval start",
            )?;
            let last = ctx.partition_point(
                &payload_scalars,
                |scalar| Ok(scalar.payload_offset < end),
                "find NX block payload scalar interval end",
            )?;
            let scalar_fields = payload_scalars.get(first..last).unwrap_or_default();
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
            for scalar in ctx.admit_iter(scalar_fields, "copy NX block payload scalar IDs")? {
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
    let mut name_index = BTreeMap::new();
    let mut name_storage = ctx.reserve_scoped(0, "NX block payload point name index")?;
    for name in ctx.admit_iter(names, "index NX block payload point names")? {
        name_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut name_index,
                name.id.as_str(),
                name,
                "NX block payload point name index",
            )
        })?;
    }
    let mut scalar_index = BTreeMap::new();
    let mut scalar_storage = ctx.reserve_scoped(0, "NX block payload point scalar index")?;
    for scalar in ctx.admit_iter(scalars, "index NX block payload point scalars")? {
        scalar_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut scalar_index,
                scalar.id.as_str(),
                scalar,
                "NX block payload point scalar index",
            )
        })?;
    }
    for record in ctx.admit_iter(records, "scan NX block payload records")? {
        let Some(name) = ctx
            .get_btree_map(
                &name_index,
                record.name_field.as_str(),
                "find NX block payload point name",
            )?
            .copied()
        else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(name.frame.value().len()),
            "parse NX block payload point name",
        )?;
        if parse_sketch_point_name(ctx, name.frame.value())?.is_none() {
            continue;
        }
        let [first_id, second_id] = record.scalar_fields.as_slice() else {
            continue;
        };
        let Some(first) = ctx
            .get_btree_map(
                &scalar_index,
                first_id.as_str(),
                "find NX block payload first point scalar",
            )?
            .copied()
        else {
            continue;
        };
        let Some(second) = ctx
            .get_btree_map(
                &scalar_index,
                second_id.as_str(),
                "find NX block payload second point scalar",
            )?
            .copied()
        else {
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
    let (witness_groups, _witness_reservation) = ctx.collect_scoped_btree_groups(
        points
            .iter()
            .map(|point| ((point.operation_label.as_str(), point.name.as_str()), point)),
        "group NX block payload point witnesses",
    )?;
    for point in ctx.admit_iter(points, "scan NX block payload point groups")? {
        let Some(group) = ctx.get_btree_map(
            &witness_groups,
            &(point.operation_label.as_str(), point.name.as_str()),
            "find NX block payload point witnesses",
        )?
        else {
            continue;
        };
        if !group
            .first()
            .is_some_and(|first| std::ptr::eq(*first, point))
        {
            continue;
        }
        let coordinates_differ = ctx.any_by(
            group,
            |candidate| {
                Ok(candidate
                    .coordinates
                    .iter()
                    .zip(*point.coordinates)
                    .any(|(first, second)| first.to_bits() != second.to_bits()))
            },
            "compare NX block payload point witnesses",
        )?;
        if coordinates_differ {
            continue;
        }
        let mut witnesses = Vec::new();
        for witness in ctx.admit_iter(group, "copy NX block payload point witnesses")? {
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
    let (binding_groups, _binding_reservation) = ctx.collect_scoped_btree_groups(
        bindings
            .iter()
            .map(|binding| (binding.operation_label.as_str(), binding)),
        "group NX block dimension bindings",
    )?;
    let (expression_groups, _expression_reservation) = ctx.collect_scoped_btree_groups(
        ctx.admit_iter(expressions, "index NX block dimension expressions")?
            .filter_map(|expression| {
                expression
                    .declaration
                    .as_deref()
                    .map(|declaration| (declaration, expression))
            }),
        "group NX block dimension expressions",
    )?;
    let mut declaration_index = BTreeMap::new();
    let mut declaration_storage = ctx.reserve_scoped(0, "NX block dimension declaration index")?;
    for (position, declaration) in ctx
        .admit_iter(declarations, "index NX block dimension declarations")?
        .enumerate()
    {
        if ctx.contains_key_btree_map(
            &declaration_index,
            declaration.id.as_str(),
            "find NX block dimension declaration",
        )? {
            continue;
        }
        declaration_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut declaration_index,
                declaration.id.as_str(),
                position,
                "NX block dimension declaration index",
            )
        })?;
    }
    for construction in ctx.admit_iter(constructions, "scan NX block dimensions")? {
        let Some(binding_group) = ctx.get_btree_map(
            &binding_groups,
            construction.operation_label.as_str(),
            "find NX block dimension bindings",
        )?
        else {
            continue;
        };
        let mut operation_bindings = Vec::new();
        let mut binding_reservation = ctx.reserve_scoped(0, "NX block dimension bindings")?;
        for binding in ctx.admit_iter(binding_group, "copy NX block dimension bindings")? {
            ctx.push_scoped_vec(
                &mut binding_reservation,
                &mut operation_bindings,
                *binding,
                "NX block dimension bindings",
            )?;
        }
        let Some(anchor) = operation_bindings
            .first()
            .map(|binding| binding.expression_declaration.as_str())
        else {
            continue;
        };
        if ctx.any_by(
            operation_bindings.as_slice(),
            |binding| {
                Ok(!ctx.equal(
                    binding.expression_declaration.as_str(),
                    anchor,
                    "compare NX block dimension binding anchors",
                )?)
            },
            "validate NX block dimension binding anchors",
        )? {
            continue;
        }
        ctx.stable_sort_by_key(
            &mut operation_bindings,
            |value| (value.input_slot, value.reference_ordinal),
            Ord::cmp,
            "sort NX block dimension bindings",
        )?;
        let Some(&start) = ctx.get_btree_map(
            &declaration_index,
            anchor,
            "find NX block dimension declaration",
        )?
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
        let first_entry = ctx
            .split_once(
                run[0].record.as_str(),
                ":entry#",
                "split NX block dimension record",
            )?
            .map(|pair| pair.0);
        let mut run_valid = true;
        for (ordinal, declaration) in run.iter().enumerate() {
            let entry = ctx
                .split_once(
                    declaration.record.as_str(),
                    ":entry#",
                    "split NX block dimension record",
                )?
                .map(|pair| pair.0);
            let (expected_name, _expected_reservation) = ctx.format_scoped(
                format_args!("p{}", declaration.name.index()),
                "format NX block dimension name",
            )?;
            if !ctx.equal(&entry, &first_entry, "compare NX block dimension records")?
                || !ctx.equal(
                    declaration.source_entry.as_str(),
                    run[0].source_entry.as_str(),
                    "compare NX block dimension entries",
                )?
                || Some(declaration.name.index())
                    != u32::try_from(ordinal)
                        .ok()
                        .and_then(|ordinal| first.checked_add(ordinal))
                || !ctx.equal(
                    declaration.name.as_str(),
                    expected_name.as_str(),
                    "compare NX block dimension names",
                )?
            {
                run_valid = false;
                break;
            }
        }
        if !run_valid {
            continue;
        }
        let mut slots = [None, None, None];
        for (slot, declaration) in slots.iter_mut().zip(run) {
            let Some(group) = ctx.get_btree_map(
                &expression_groups,
                declaration.id.as_str(),
                "find NX block dimension expression",
            )?
            else {
                break;
            };
            let [expression] = group.as_slice() else {
                break;
            };
            let Some(value) = expression.value else {
                break;
            };
            let Some(length) =
                crate::native::om::expression_length_in_millimeters(&expression.unit, value.get())
            else {
                break;
            };
            let Some(length) = cadmpeg_ir::scalar::FiniteReal::new(length) else {
                break;
            };
            *slot = Some((*expression, length));
        }
        let [Some(first_resolved), Some(second_resolved), Some(third_resolved)] = slots else {
            continue;
        };
        let resolved = [first_resolved, second_resolved, third_resolved];
        let mut sources_agree = true;
        for ((expression, _), declaration) in resolved.iter().zip(run.iter()) {
            if !ctx.equal(
                expression.source_entry.as_str(),
                declaration.source_entry.as_str(),
                "compare NX block dimension expression entries",
            )? || !ctx.equal(
                &expression.source_table,
                &resolved[0].0.source_table,
                "compare NX block dimension expression tables",
            )? {
                sources_agree = false;
                break;
            }
        }
        if !sources_agree {
            continue;
        }
        let id = if let Some((prefix, suffix)) = ctx.split_once(
            &construction.id,
            "block-construction",
            "split NX block dimensions identity",
        )? {
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
        for binding in ctx.admit_iter(operation_bindings, "copy NX block dimension bindings")? {
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
    for (data_block, (bytes, source_offset)) in
        ctx.admit_iter(&*blocks, "scan NX offset-store blocks")?
    {
        for (ordinal, frame) in ctx
            .admit_iter(
                crate::om::data_block_object_frames(ctx, bytes)?,
                "visit NX data-block object frames",
            )?
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

    let Some((section, block)) = ctx
        .strip_prefix(
            data_block,
            "nx:om-data-blocks-",
            "split NX data block object frame source",
        )?
        .map(|rest| ctx.split_once(rest, ":block#", "split NX data block object frame source"))
        .transpose()?
        .flatten()
    else {
        return Err(CodecError::malformed(
            "invalid NX data block object frame source",
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
    let Some(section_ordinal) = unique_offset_data_store(ctx, indexed, &[object_index])? else {
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
    ctx: &DecodeContext<'_>,
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    object_indices: &[u32],
) -> Result<Option<usize>, CodecError> {
    let mut largest = 0u32;
    if ctx.any_by(
        object_indices,
        |&object_index| {
            largest = largest.max(object_index);
            Ok(object_index == 0)
        },
        "validate NX offset store object indices",
    )? {
        return Ok(None);
    }
    let Ok(largest) = usize::try_from(largest) else {
        return Ok(None);
    };
    if object_indices.is_empty() {
        return Ok(None);
    }
    let holds_largest = |(_, candidate): &(_, crate::om::IndexedSection<'_>)| {
        Ok(candidate
            .as_offset_only()
            .is_some_and(|(_, _, records)| largest <= records.len()))
    };
    let operation = "find NX unique offset data store";
    let Some(first) = ctx.position_by(indexed, holds_largest, operation)? else {
        return Ok(None);
    };
    if ctx.any_by(&indexed[first + 1..], holds_largest, operation)? {
        return Ok(None);
    }
    Ok(Some(first))
}
