// SPDX-License-Identifier: Apache-2.0
//! CATIA native ownership, alias, and wire projections.

use cadmpeg_core::decode::u64_from_index;

use crate::object_graph;

use super::edge_definition::CatiaConsolidatedEdgeDefinition;

use super::{
    catalog, container, design_object_id, entity_table, resolved_payload_references,
    resolved_storage_link, value_block, AliasLead, CatiaAliasRow, CatiaAllocationReferenceEncoding,
    CatiaCatalog, CatiaCatalogEntry, CatiaConsolidatedAnalyticCircleBinding,
    CatiaConsolidatedCircle, CatiaConsolidatedClass25Descriptor, CatiaConsolidatedEdgeNode,
    CatiaConsolidatedEdgeRun, CatiaConsolidatedEdgeUses, CatiaConsolidatedOwnerPacket,
    CatiaConsolidatedPcurve, CatiaConsolidatedSupportBinding, CatiaDesignClass, CatiaEntityRecord,
    CatiaEntityRecordBody, CatiaEntityReference, CatiaExternalReference, CatiaFaceNodeRelation,
    CatiaFaceNodeTargetEncoding, CatiaFinjplSegment, CatiaObjectClass, CatiaObjectEntity,
    CatiaObjectGraph, CatiaObjectOwner, CatiaObjectRecord, CatiaObjectStorage,
    CatiaOuterContainerBinding, CatiaOwnerBoundaryCycle, CatiaOwnerBoundaryEdge,
    CatiaOwnerChartAddress, CatiaOwnerChartAliasBinding, CatiaOwnerChartBridge,
    CatiaOwnerChartBridgeReference, CatiaOwnerChartCarrier, CatiaOwnerChartRelation,
    CatiaOwnerIdentityEncoding, CatiaOwnerIdentityTarget, CatiaOwnerPacketPayload,
    CatiaOwnerReferenceEncoding, CatiaPreviewImage, CatiaReferenceSignature, CatiaValueBlock,
    CatiaValueSchemaSelection, CatiaValueSchemaSelectionKind, CatiaValueSchemaSelectionValue,
    CatiaZeroEntityRecord, CodecError, ConsolidatedRecord, DecodeContext, GraphRecordIndex,
    HashMap, HashSet,
};

pub(crate) fn consolidated_owner_packets(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedOwnerPacket>, CodecError> {
    // Packet rows and the chart, target, cycle, face-node and position indexes
    // are dropped once the packets are built.
    let mut scratch = ctx.reserve_scoped(0, "catia_native_owner_packet_scratch")?;
    let fixed = scratch.with_storage(|| {
        ctx.collect_vec(
            crate::families::b2::records::b2_owner_packets_from_records(ctx, bytes, records)?,
            "catia_native_fixed_owner_packets",
        )
    })?;
    let mut owner_charts = HashMap::new();
    let owner_chart_rows = scratch.with_storage(|| {
        crate::families::b2::records::b2_owner_charts_from_records(ctx, bytes, records)
    })?;
    for chart in ctx.admit_iter(&owner_chart_rows, "catia_native_owner_chart_visits")? {
        let native_reference =
            |reference: crate::families::b2::records::B2OwnerChartBridgeReference| {
                CatiaOwnerChartBridgeReference::new(
                    reference.value,
                    native_allocation_reference_encoding(reference.encoding),
                )
            };
        let key = (chart.source_index, chart.owner_pos);
        let value = CatiaOwnerChartRelation {
            carrier_byte_offset: u64_from_index(chart.carrier_pos),
            carrier: match chart.carrier {
                crate::families::b2::records::B2OwnerChartCarrier::B28 => {
                    CatiaOwnerChartCarrier::B28
                }
                crate::families::b2::records::B2OwnerChartCarrier::B2b => {
                    CatiaOwnerChartCarrier::B2b
                }
                crate::families::b2::records::B2OwnerChartCarrier::A32 => {
                    CatiaOwnerChartCarrier::A32
                }
            },
            bridge: match &chart.bridge {
                crate::families::b2::records::B2OwnerChartBridge::SupportedSurface {
                    pos,
                    carrier_surface,
                    support_surfaces,
                    support_pcurves,
                    middle_controls,
                    terminal_control,
                    construction_radius,
                } => CatiaOwnerChartBridge::SupportedSurface {
                    byte_offset: u64_from_index(*pos),
                    carrier_surface: native_reference(*carrier_surface),
                    support_surfaces: (*support_surfaces).map(native_reference),
                    support_pcurves: (*support_pcurves).map(native_reference),
                    middle_controls: *middle_controls,
                    terminal_control: *terminal_control,
                    construction_radius: *construction_radius,
                },
                crate::families::b2::records::B2OwnerChartBridge::Extended { pos, references } => {
                    CatiaOwnerChartBridge::Extended {
                        byte_offset: u64_from_index(*pos),
                        references: (*references).map(native_reference),
                    }
                }
            },
            parameter_point_byte_offsets: chart.parameter_point_offsets().map(u64_from_index),
        };
        scratch.with_storage(|| {
            ctx.insert_hash_map(&mut owner_charts, key, value, "catia_native_owner_charts")
        })?;
    }
    let mut identity_targets = HashMap::<(usize, usize), Vec<_>>::new();
    let identity_target_rows = scratch.with_storage(|| {
        crate::families::b2::records::b2_owner_identity_targets_from_records(ctx, bytes, records)
    })?;
    for target in ctx.admit_iter(
        &identity_target_rows,
        "catia_native_owner_identity_target_visits",
    )? {
        let key = (target.source_index, target.owner_pos);
        scratch.with_storage(|| {
            ctx.push_hash_group(
                &mut identity_targets,
                key,
                target,
                "catia_native_owner_identity_target_groups",
                "catia_native_owner_identity_target_entries",
            )
        })?;
    }
    let mut boundary_cycles = HashMap::new();
    let boundary_cycle_rows = scratch.with_storage(|| {
        crate::families::consolidated::records::consolidated_owner_boundary_cycles_from_records(
            ctx, bytes, records,
        )
    })?;
    for cycle in ctx.admit_iter(
        &boundary_cycle_rows,
        "catia_native_owner_boundary_cycle_visits",
    )? {
        let key = (cycle.source_index, cycle.owner_pos);
        let value = CatiaOwnerBoundaryCycle {
                    face_node: cycle.face_node.and_then(|face_node| {
                        let byte_len = cycle.owner_pos.checked_sub(face_node.pos)?;
                        Some(CatiaFaceNodeRelation {
                            byte_offset: u64_from_index(face_node.pos),
                            byte_len: u64_from_index(byte_len),
                            header_token: face_node.header_token,
                            target_encoding: match face_node.target_encoding {
                                crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                                    CatiaFaceNodeTargetEncoding::Compact
                                }
                                crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                                    CatiaFaceNodeTargetEncoding::TaggedU16Strong
                                }
                            },
                            target: face_node.target,
                            terminal: face_node.terminal,
                        })
                    }),
                    edges: cycle.edges.map(|edge| CatiaOwnerBoundaryEdge {
                        slot: edge.slot,
                        byte_offset: u64_from_index(edge.target_pos),
                        endpoint_records: edge.endpoint_records.map(u64_from_index),
                    }),
                };
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut boundary_cycles,
                key,
                value,
                "catia_native_owner_boundary_cycles",
            )
        })?;
    }
    let adjacent_counted = scratch.with_storage(|| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            ctx, bytes, records,
        )
    })?;
    let mut face_nodes = HashMap::new();
    let adjacent_face_owners = scratch.with_storage(|| {
        crate::families::b2::records::b2_adjacent_face_owners_from_records(ctx, bytes, records)
    })?;
    for linked in ctx.admit_iter(
        &adjacent_face_owners,
        "catia_native_adjacent_face_owner_visits",
    )? {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut face_nodes,
                (linked.owner.source_index, linked.owner.pos),
                linked.face_node,
                "catia_native_owner_face_nodes",
            )
        })?;
    }
    for linked in ctx.admit_iter(
        &adjacent_counted,
        "catia_native_adjacent_counted_owner_visits",
    )? {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut face_nodes,
                (linked.owner.source_index, linked.owner.pos),
                linked.face_node,
                "catia_native_owner_face_nodes",
            )
        })?;
    }
    let mut fixed_positions = HashSet::new();
    for packet in ctx.admit_iter(&fixed, "catia_native_fixed_owner_position_visits")? {
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut fixed_positions,
                (packet.source_index, packet.pos),
                "catia_native_fixed_owner_positions",
            )
        })?;
    }
    let counted_owners = scratch.with_storage(|| {
        crate::families::b2::records::b2_counted_owners_from_records(ctx, bytes, records)
    })?;
    let row_count = fixed
        .len()
        .checked_add(counted_owners.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia_native_owner_packet_rows", u64::MAX, u64::MAX)
        })?;
    let mut packets =
        scratch.with_storage(|| ctx.collection_vec(row_count, "catia_native_owner_packet_rows"))?;
    for packet in ctx.admit_iter(fixed, "catia_native_fixed_owner_packet_rows")? {
        packets.push((
            packet.pos,
            packet.source_index,
            packet.header_token,
            CatiaOwnerPacketPayload::FixedNine {
                reference_encoding: match packet.reference_encoding {
                    crate::families::b2::records::B2OwnerReferenceEncoding::TaggedU16Strong => {
                        CatiaOwnerReferenceEncoding::TaggedU16Strong
                    }
                    crate::families::b2::records::B2OwnerReferenceEncoding::WidthCodedStrong => {
                        CatiaOwnerReferenceEncoding::WidthCodedStrong
                    }
                    crate::families::b2::records::B2OwnerReferenceEncoding::AllCompact => {
                        CatiaOwnerReferenceEncoding::AllCompact
                    }
                },
                references: packet.references,
                identity_encodings: packet.identity_encodings.map(|encoding| match encoding {
                    crate::families::b2::records::B2OwnerIdentityEncoding::Allocation(encoding) => {
                        CatiaOwnerIdentityEncoding::Allocation(
                            native_allocation_reference_encoding(encoding),
                        )
                    }
                    crate::families::b2::records::B2OwnerIdentityEncoding::RawU8 => {
                        CatiaOwnerIdentityEncoding::RawU8
                    }
                }),
                numeric_tail: packet.numeric_tail,
                identity_targets: Vec::new(),
                owner_chart: None,
                boundary_cycle: None,
            },
        ));
    }
    for packet in ctx.admit_iter(counted_owners, "catia_native_counted_owner_packet_rows")? {
        if ctx.contains_hash_set(
            &fixed_positions,
            &(packet.source_index, packet.pos),
            "catia_native_fixed_owner_positions",
        )? {
            continue;
        }
        packets.push((
            packet.pos,
            packet.source_index,
            packet.header_token,
            CatiaOwnerPacketPayload::Counted {
                references: ctx
                    .copy_slice(&packet.references, "catia_native_owner_counted_references")?,
                tail: crate::families::b2::counted_owner_tail::CountedOwnerTail::new(
                    ctx.copy_slice(packet.tail.as_slice(), "catia_native_owner_counted_tail")?,
                )
                .ok_or_else(|| CodecError::malformed("CATIA counted owner tail is empty"))?,
            },
        ));
    }
    ctx.stable_sort_by_key(
        &mut packets,
        |value| (value.0, value.1),
        Ord::cmp,
        "catia_native_owner_packet_sort",
    )?;
    ctx.try_collect_vec(
        packets
            .into_iter()
            .map(|(pos, source_index, header_token, mut payload)| -> Result<_, CodecError> {
                if let CatiaOwnerPacketPayload::FixedNine {
                    identity_targets: stored_targets,
                    owner_chart,
                    boundary_cycle,
                    ..
                } = &mut payload
                {
                    const LOOKUP: &str = "catia_native_owner_packet_lookups";
                    let key = (source_index, pos);
                    if let Some(targets) = ctx.remove_hash_map(&mut identity_targets, &key, LOOKUP)? {
                        *stored_targets = ctx.collect_vec(
                            targets.into_iter().map(|target| CatiaOwnerIdentityTarget {
                                slot: target.slot, distance: target.distance,
                                target_byte_offset: u64_from_index(target.target_pos), target_class: target.target_class,
                            }), "catia_native_owner_emitted_targets",
                        )?;
                    }
                    *owner_chart = ctx
                        .remove_hash_map(&mut owner_charts, &key, LOOKUP)?
                        .map(|value| -> Result<_, CodecError> {
                            ctx.charge_retained(u64_from_index(std::mem::size_of_val(&value)), "catia_native_owner_packet_box")?;
                            Ok(Box::new(value))
                        }).transpose()?;
                    *boundary_cycle = ctx
                        .get_hash_map(&boundary_cycles, &key, LOOKUP)?
                        .copied()
                        .map(|value| -> Result<_, CodecError> {
                            ctx.charge_retained(u64_from_index(std::mem::size_of_val(&value)), "catia_native_owner_packet_box")?;
                            Ok(Box::new(value))
                        }).transpose()?;
                }
                Ok(CatiaConsolidatedOwnerPacket {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:owner-packet#{pos:010}"),
                        "catia_native_owner_packet_id",
                    )?,
                    byte_offset: u64_from_index(pos),
                    source_index,
                    header_token,
                    payload,
                    face_node: ctx
                        .get_hash_map(
                            &face_nodes,
                            &(source_index, pos),
                            "catia_native_owner_packet_lookups",
                        )?
                        .and_then(|face_node| {
                            let byte_len = pos.checked_sub(face_node.pos)?;
                            Some(CatiaFaceNodeRelation {
                                byte_offset: u64_from_index(face_node.pos),
                                byte_len: u64_from_index(byte_len),
                                header_token: face_node.header_token,
                                target_encoding: match face_node.target_encoding {
                                    crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                                        CatiaFaceNodeTargetEncoding::Compact
                                    }
                                    crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                                        CatiaFaceNodeTargetEncoding::TaggedU16Strong
                                    }
                                },
                                target: face_node.target,
                                terminal: face_node.terminal,
                            })
                        }),
                })
            }),
        "catia_native_owner_packet_output",
    )
}

pub(crate) fn consolidated_edge_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<CatiaConsolidatedEdgeRun>, CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "CATIA native edge run lookup")?;
    let mut pcurve_ids = HashMap::new();
    for pcurve in ctx.admit_iter(pcurves, "catia_native_edge_run_pcurve_visits")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut pcurve_ids,
                pcurve.byte_offset,
                pcurve.id.as_str(),
                "catia_native_edge_run_pcurve_index",
            )
        })?;
    }
    let resolved_blocks = lookup_storage.with_storage(|| {
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            ctx, bytes, records, refusal,
        )
    })?;
    let mut resolved = HashMap::new();
    for block in ctx.admit_iter(&resolved_blocks, "catia_native_resolved_edge_block_visits")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut resolved,
                block.block.pcurves[0].pos,
                block,
                "catia_native_edge_run_resolved_index",
            )
        })?;
    }
    let mut nodes_by_offset = HashMap::new();
    for node in ctx.admit_iter(nodes, "catia_native_edge_run_node_visits")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut nodes_by_offset,
                node.byte_offset,
                node,
                "catia_native_edge_run_node_index",
            )
        })?;
    }
    let mut output = Vec::new();
    let topology_edge_runs = lookup_storage.with_storage(|| {
        crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
            ctx, bytes, records,
        )
    })?;
    for (index, run) in ctx
        .admit_iter(&topology_edge_runs, "catia_native_topology_edge_run_visits")?
        .enumerate()
    {
        let pcurve_offsets = run
            .edge
            .pcurves
            .each_ref()
            .map(|pcurve| u64_from_index(pcurve.pos));
        const LOOKUP: &str = "catia_native_edge_run_lookups";
        let resolved = ctx.get_hash_map(&resolved, &run.edge.pcurves[0].pos, LOOKUP)?;
        let Some(node) =
            ctx.get_hash_map(&nodes_by_offset, &u64_from_index(run.node.pos), LOOKUP)?
        else {
            continue;
        };
        if node.uses.is_none() {
            continue;
        }
        let (Some(first), Some(second)) = (
            ctx.get_hash_map(&pcurve_ids, &pcurve_offsets[0], LOOKUP)?,
            ctx.get_hash_map(&pcurve_ids, &pcurve_offsets[1], LOOKUP)?,
        ) else {
            continue;
        };
        let shared_loci = resolved
            .and_then(|resolved| resolved.shared_loci.as_ref())
            .map(|points| {
                ctx.collect_vec(
                    points.iter().map(point_coordinates),
                    "catia_native_edge_run_shared_loci",
                )
            })
            .transpose()?;
        let value = CatiaConsolidatedEdgeRun {
            id: ctx.format_retained(
                format_args!("catia:consolidated:edge-run#{index:01}"),
                "catia_native_edge_run_id",
            )?,
            byte_offset: pcurve_offsets[0],
            pcurves: [
                ctx.copy_retained_text(first, "catia_native_edge_run_first_pcurve_id")?,
                ctx.copy_retained_text(second, "catia_native_edge_run_second_pcurve_id")?,
            ],
            parameter_range: run.edge.parameters.range,
            tolerance: run.edge.parameters.tolerance,
            node: ctx.copy_retained_text(&node.id, "catia_native_edge_run_node_id")?,
            support_bindings: resolved.map_or([None, None], |resolved| {
                resolved
                    .supports
                    .each_ref()
                    .map(|binding| binding.as_ref().map(native_consolidated_support_binding))
            }),
            shared_loci,
            endpoint_loci: resolved
                .and_then(|resolved| resolved.endpoint_loci.as_ref())
                .map(|points| points.map(|point| point_coordinates(&point))),
        };
        ctx.push_vec(&mut output, value, "catia_native_edge_runs")?;
    }
    Ok(output)
}

pub(crate) fn consolidated_edge_nodes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    circles: &[CatiaConsolidatedCircle],
) -> Result<Vec<CatiaConsolidatedEdgeNode>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia native edge node workspace")?;
    let mut circle_ids = HashMap::new();
    for circle in ctx.admit_iter(circles, "catia_native_edge_circle_visits")? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut circle_ids,
                circle.byte_offset,
                circle.id.as_str(),
                "catia_native_edge_circle_ids",
            )
        })?;
    }
    let mut frames = HashMap::new();
    for record in ctx
        .admit_iter(records, "catia_native_edge_frame_record_visits")?
        .filter(|record| {
            record.family() == crate::wire::records::ConsolidatedFamily::B && record.class() == 0x5e
        })
    {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut frames,
                record.byte_offset(),
                (record.width(), record.flag(), record.source_index()),
                "catia_native_edge_frames",
            )
        })?;
    }
    let mut owned_nodes = HashMap::new();
    let owned_node_rows = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_owned_edge_nodes_from_records(
            ctx, bytes, records,
        )
    })?;
    for owned in ctx.admit_iter(&owned_node_rows, "catia_native_owned_edge_node_visits")? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut owned_nodes,
                owned.node.pos,
                (owned.owner_pos, owned.allocation_ordinal),
                "catia_native_owned_edge_nodes",
            )
        })?;
    }
    let mut compact_endpoints = HashMap::new();
    let compact_endpoint_bindings = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            ctx, bytes, records,
        )
    })?;
    for binding in ctx.admit_iter(
        &compact_endpoint_bindings,
        "catia_native_compact_endpoint_visits",
    )? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut compact_endpoints,
                binding.node.pos,
                binding.endpoint_records.map(u64_from_index),
                "catia_native_edge_compact_endpoints",
            )
        })?;
    }
    let mut use_runs = HashMap::new();
    let use_rows = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_edge_use_runs_from_records(
            ctx, bytes, records,
        )
    })?;
    for run in ctx.admit_iter(&use_rows, "catia_native_edge_use_run_visits")? {
        let Some(uses) = native_consolidated_edge_uses(&run.uses) else {
            continue;
        };
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut use_runs,
                run.node.pos,
                (uses, run.definition.as_ref()),
                "catia_native_edge_use_runs",
            )
        })?;
    }
    let mut analytic_circles = HashMap::new();
    let analytic_rows = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs_from_records(
            ctx, bytes, records,
        )
    })?;
    for run in ctx.admit_iter(&analytic_rows, "catia_native_analytic_edge_run_visits")? {
        let Some(circle) = ctx.get_hash_map(
            &circle_ids,
            &u64_from_index(run.circle.pos),
            "catia_native_edge_circle_ids",
        )?
        else {
            continue;
        };
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut analytic_circles,
                run.node.pos,
                (&run.descriptor, *circle),
                "catia_native_analytic_edge_bindings",
            )
        })?;
    }
    let mut class25_descriptors = temporary.with_storage(|| {
        ctx.collect_hash_map(
            crate::families::consolidated::records::consolidated_class25_edge_runs_from_records(
                ctx, bytes, records,
            )?
            .into_iter()
            .map(|run| {
                (
                    run.node.pos,
                    CatiaConsolidatedClass25Descriptor {
                        byte_offset: u64_from_index(run.descriptor.pos),
                        record_id: run.descriptor.record_id,
                        control: run.descriptor.control,
                        values: run.descriptor.values,
                    },
                )
            }),
            "catia_native_class25_edge_descriptors",
        )
    })?;
    let mut output = Vec::new();
    for (index, node) in
        crate::families::b2::records::b2_edge_nodes_from_records(ctx, bytes, records)?.enumerate()
    {
        const LOOKUP: &str = "catia_native_edge_node_lookups";
        let Some(&(width, flag, source_index)) = ctx.get_hash_map(&frames, &node.pos, LOOKUP)?
        else {
            continue;
        };
        ctx.reserve_vec(&mut output, 1, "catia_native_consolidated_edge_nodes")?;
        let allocation = ctx
            .get_hash_map(&owned_nodes, &node.pos, LOOKUP)?
            .map(|(pos, ordinal)| {
                Ok::<_, CodecError>((
                    ctx.format_retained(
                        format_args!("catia:consolidated:owner-packet#{:010}", *pos),
                        "catia_native_edge_owner_id",
                    )?,
                    *ordinal,
                ))
            })
            .transpose()?;
        let (uses, definition) = match ctx.remove_hash_map(&mut use_runs, &node.pos, LOOKUP)? {
            Some((uses, definition)) => (Some(uses), definition),
            None => (None, None),
        };
        let definition = definition
            .map(|source| -> Result<_, CodecError> {
                let frame = crate::wire::records::ConsolidatedRawFrame::new(
                    source.frame.pos,
                    source.frame.width(),
                    source.frame.flag,
                    source.frame.header_token(),
                    ctx.copy_slice(
                        &source.frame.payload,
                        "catia_native_edge_definition_payload",
                    )?,
                )
                .map_err(CodecError::malformed)?;
                CatiaConsolidatedEdgeDefinition::from_source(
                    ctx,
                    crate::families::consolidated::records::ConsolidatedEdgeDefinition {
                        frame,
                        class: source.class,
                    },
                )
            })
            .transpose()?;
        let analytic_circle = ctx
            .remove_hash_map(&mut analytic_circles, &node.pos, LOOKUP)?
            .map(|(descriptor, circle)| -> Result<_, CodecError> {
                Ok(CatiaConsolidatedAnalyticCircleBinding {
                    descriptor: crate::wire::records::ConsolidatedRawFrame::new(
                        u64_from_index(descriptor.pos),
                        descriptor.width(),
                        descriptor.flag,
                        descriptor.header_token(),
                        ctx.copy_slice(
                            &descriptor.payload,
                            "catia_native_analytic_edge_descriptor",
                        )?,
                    )
                    .map_err(CodecError::malformed)?,
                    circle: ctx
                        .copy_retained_text(circle, "catia_native_analytic_edge_circle_id")?,
                })
            })
            .transpose()?;
        output.push(CatiaConsolidatedEdgeNode {
            id: ctx.format_retained(
                format_args!("catia:consolidated:edge-node#{index:01}"),
                "catia_native_edge_node_id",
            )?,
            byte_offset: u64_from_index(node.pos),
            source_index,
            token: crate::wire::records::WidthCodedToken::new(width, node.header_token)
                .map_err(CodecError::malformed)?,
            flag,
            allocation,
            curve_ref: node.curve_ref,
            vertex_refs: [node.start_vertex_ref, node.end_vertex_ref],
            endpoint_records: ctx
                .get_hash_map(&compact_endpoints, &node.pos, LOOKUP)?
                .copied(),
            parameter_selectors: [node.start_parameter_ref, node.end_parameter_ref],
            reference_encodings: node
                .reference_encodings
                .map(native_allocation_reference_encoding),
            terminal_value: node.terminal_value,
            terminal_encoding: native_allocation_reference_encoding(node.terminal_encoding),
            tail: node.tail,
            definition,
            uses,
            analytic_circle,
            class25_descriptor: ctx.remove_hash_map(&mut class25_descriptors, &node.pos, LOOKUP)?,
        });
    }
    Ok(output)
}

pub(super) fn native_allocation_reference_encoding(
    encoding: crate::wire::bytes::AllocationReferenceEncoding,
) -> CatiaAllocationReferenceEncoding {
    match encoding {
        crate::wire::bytes::AllocationReferenceEncoding::BackwardDistance => {
            CatiaAllocationReferenceEncoding::BackwardDistance
        }
        crate::wire::bytes::AllocationReferenceEncoding::OwnedChild => {
            CatiaAllocationReferenceEncoding::OwnedChild
        }
        crate::wire::bytes::AllocationReferenceEncoding::WidthCoded => {
            CatiaAllocationReferenceEncoding::WidthCoded
        }
        crate::wire::bytes::AllocationReferenceEncoding::Selector2 => {
            CatiaAllocationReferenceEncoding::Selector2
        }
        crate::wire::bytes::AllocationReferenceEncoding::TaggedU8 => {
            CatiaAllocationReferenceEncoding::TaggedU8
        }
        crate::wire::bytes::AllocationReferenceEncoding::TaggedU16 => {
            CatiaAllocationReferenceEncoding::TaggedU16
        }
    }
}

pub(super) fn native_consolidated_edge_uses(
    uses: &[crate::families::b2::records::B2UseMetadata; 2],
) -> Option<CatiaConsolidatedEdgeUses> {
    let [Some(first), Some(second)] = uses
        .each_ref()
        .map(|use_| use_.references()?.try_into().ok())
    else {
        return None;
    };
    let references = [first, second];
    let [Some(first_sense), Some(second_sense)] =
        uses.each_ref().map(|use_| match use_.sense()? {
            crate::families::b2::records::B2UseSense::Sense84 => Some(0x84),
            crate::families::b2::records::B2UseSense::Sense88 => Some(0x88),
        })
    else {
        return None;
    };
    ([first_sense, second_sense] == [0x88, 0x84])
        .then_some(CatiaConsolidatedEdgeUses { references })
}

pub(crate) fn point_coordinates(point: &cadmpeg_ir::math::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

pub(crate) fn native_consolidated_support_binding(
    binding: &crate::families::consolidated::records::ConsolidatedSupportBinding,
) -> CatiaConsolidatedSupportBinding {
    match binding {
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cylinder { pos } => {
            CatiaConsolidatedSupportBinding::Cylinder {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::EmbeddedCylinder {
            pos,
            wrapper_pos,
        } => CatiaConsolidatedSupportBinding::EmbeddedCylinder {
            byte_offset: u64_from_index(*pos),
            wrapper_byte_offset: u64_from_index(*wrapper_pos),
        },
        crate::families::consolidated::records::ConsolidatedSupportBinding::Circle { pos } => {
            CatiaConsolidatedSupportBinding::Circle {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cone { pos } => {
            CatiaConsolidatedSupportBinding::Cone {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Sphere { pos } => {
            CatiaConsolidatedSupportBinding::Sphere {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Torus { pos } => {
            CatiaConsolidatedSupportBinding::Torus {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Plane { pos } => {
            CatiaConsolidatedSupportBinding::Plane {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier {
            pos,
            offset,
        } => CatiaConsolidatedSupportBinding::NurbsCarrier {
            byte_offset: u64_from_index(*pos),
            offset: *offset,
        },
    }
}

pub(crate) fn zero_entity_record(
    records: &[CatiaZeroEntityRecord],
    ordinal: u32,
) -> Option<&CatiaZeroEntityRecord> {
    let index = usize::try_from(ordinal.checked_sub(1)?).ok()?;
    records.get(index)
}

pub(crate) fn zero_entity_vertex_owner(
    records: &[CatiaZeroEntityRecord],
    incidence_ordinal: u32,
) -> Option<&CatiaZeroEntityRecord> {
    let incidence = zero_entity_record(records, incidence_ordinal)?;
    let owner = zero_entity_record(records, incidence_ordinal.checked_add(1)?)?;
    (incidence.logical_end == owner.byte_offset && owner.tag == [0x5d, 0x06]).then_some(owner)
}

pub(crate) fn finjpl_family(kind: container::FinjplKind) -> &'static str {
    match kind {
        container::FinjplKind::Storage => "storage",
        container::FinjplKind::ProjectFlags => "project-flags",
        container::FinjplKind::Other => "other",
    }
}

/// The one segment containing an extent. Segments are disjoint and in offset
/// order, each running from one marker to the next, so only the last segment
/// starting at or before the extent can contain it, together with its
/// predecessor when the extent is empty and on their shared boundary.
pub(crate) fn containing_finjpl_segment<'s>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    byte_offset: u64,
    byte_len: u64,
    segments: &'s [CatiaFinjplSegment],
) -> Result<Option<&'s str>, CodecError> {
    let Some(byte_end) = byte_offset.checked_add(byte_len) else {
        return Ok(None);
    };
    let contains = |segment: &CatiaFinjplSegment| {
        segment.byte_offset <= byte_offset
            && segment
                .byte_offset
                .checked_add(segment.byte_len)
                .is_some_and(|segment_end| byte_end <= segment_end)
    };
    let after = ctx.partition_point(
        segments,
        |segment| Ok(segment.byte_offset <= byte_offset),
        "catia_native_finjpl_segment_search",
    )?;
    let Some(last) = after.checked_sub(1) else {
        return Ok(None);
    };
    let previous = last
        .checked_sub(1)
        .and_then(|index| segments.get(index))
        .filter(|segment| contains(segment));
    Ok(
        match (
            segments.get(last).filter(|segment| contains(segment)),
            previous,
        ) {
            (Some(segment), None) | (None, Some(segment)) => Some(segment.id.as_str()),
            (Some(_), Some(_)) | (None, None) => None,
        },
    )
}

pub(crate) fn preview_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segments: &[CatiaFinjplSegment],
) -> Result<Vec<CatiaPreviewImage>, cadmpeg_core::CodecError> {
    let mut views = Vec::new();
    for segment in ctx.admit_iter(segments, "catia_native_preview_segment_visits")? {
        let previews = container::preview_images(ctx, &segment.data)?;
        for preview in ctx.admit_iter(&previews, "catia_native_preview_item_visits")? {
            let Some(byte_offset) = segment
                .byte_offset
                .checked_add(u64_from_index(preview.range.start))
            else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("catia:outer:preview#{}", views.len()),
                "catia_native_preview_id",
            )?;
            let data = ctx.copy_slice(
                &segment.data[preview.range.clone()],
                "catia_native_preview_bytes",
            )?;
            ctx.push_vec(
                &mut views,
                CatiaPreviewImage {
                    id,
                    byte_offset,
                    byte_len: u64_from_index(preview.range.end - preview.range.start),
                    width: preview.width,
                    height: preview.height,
                    components: preview.components,
                    data,
                },
                "catia_native_preview_views",
            )?;
        }
    }
    Ok(views)
}

pub(crate) fn external_reference_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segments: &[CatiaFinjplSegment],
) -> Result<Vec<CatiaExternalReference>, cadmpeg_core::CodecError> {
    let mut views = Vec::new();
    for segment in ctx.admit_iter(segments, "catia_native_external_reference_segment_visits")? {
        for reference in ctx.admit_iter(
            container::external_references(ctx, &segment.data)?,
            "catia_native_external_reference_visits",
        )? {
            let Some(byte_offset) = segment
                .byte_offset
                .checked_add(u64_from_index(reference.offset))
            else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("catia:outer:external-reference#{}", views.len()),
                "catia_native_external_reference_id",
            )?;
            let segment_id =
                ctx.copy_retained_text(&segment.id, "catia_native_external_reference_segment")?;
            ctx.push_vec(
                &mut views,
                CatiaExternalReference {
                    id,
                    byte_offset,
                    target: reference.target,
                    segment: segment_id,
                },
                "catia_native_external_reference_views",
            )?;
        }
    }
    Ok(views)
}

pub(crate) fn resolve_alias_surface_tags(
    ctx: &DecodeContext<'_>,
    rows: &mut [CatiaAliasRow],
) -> Result<(), CodecError> {
    // The stored tag of each group, or `None` when several rows store one.
    let (stored_by_group, _storage) = ctx.unique_index(
        ctx.admit_iter(&*rows, "catia_native_alias_tag_read_visits")?
            .filter_map(|row| {
                let group = row.group.as_ref()?;
                (row.lead() == AliasLead::SurfaceSupportStorage)
                    .then_some(((group.prototype, group.group_id), row.tag()))
            }),
        "catia_alias_group_index",
    )?;
    for row in ctx.admit_iter(rows, "catia_native_alias_tag_write_visits")? {
        row.canonical_surface_tag = match row.lead() {
            AliasLead::SurfaceSupportStorage => Some(row.tag()),
            AliasLead::NonSurfaceAlias => match row.group.as_ref() {
                Some(group) => ctx
                    .get_hash_map(
                        &stored_by_group,
                        &(group.prototype, group.group_id),
                        "catia_alias_group_index",
                    )?
                    .copied()
                    .flatten(),
                None => None,
            },
            _ => None,
        };
    }
    Ok(())
}

pub(crate) fn resolve_owner_chart_support_aliases(
    ctx: &DecodeContext<'_>,
    packets: &mut [CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
) -> Result<(), CodecError> {
    let (unique_by_tag, _storage) = ctx.unique_index(
        aliases.iter().map(|row| (row.tag(), row)),
        "catia_owner_alias_index",
    )?;
    let resolve = |reference: &mut CatiaOwnerChartBridgeReference| -> Result<(), CodecError> {
        if let CatiaOwnerChartAddress::WidthCoded { alias } = &mut reference.address {
            *alias = if let Some(row) = ctx
                .get_hash_map(&unique_by_tag, &reference.value, "catia_owner_alias_index")?
                .copied()
                .flatten()
            {
                let id = ctx.copy_retained_text(&row.id, "catia_owner_alias_binding_id")?;
                cadmpeg_core::text::NonBlankString::for_decode(ctx, id, "validate nonblank text")?
                    .map(|id| CatiaOwnerChartAliasBinding::new(id, row.canonical_surface_tag))
            } else {
                None
            };
        }
        Ok(())
    };
    for packet in ctx.admit_iter(packets, "catia_native_owner_alias_packet_visits")? {
        let Some(chart) = packet.owner_chart_mut() else {
            continue;
        };
        let CatiaOwnerChartBridge::SupportedSurface {
            support_surfaces,
            support_pcurves,
            ..
        } = &mut chart.bridge
        else {
            continue;
        };
        for reference in support_surfaces.iter_mut().chain(support_pcurves) {
            resolve(reference)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_alias_surface_tags(
    rows: &[CatiaAliasRow],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut expected = rows.to_vec();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("validation fixture fits service input limit");
    resolve_alias_surface_tags(&ctx, &mut expected)
        .expect("validation fixture fits service resource limits");
    if rows
        .iter()
        .zip(expected)
        .all(|(row, expected)| row.canonical_surface_tag == expected.canonical_surface_tag)
    {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "alias rows have invalid canonical surface tags".to_string(),
        ))
    }
}

#[cfg(test)]
pub(crate) fn validate_owner_chart_support_aliases(
    packets: &[CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut expected = packets.to_vec();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("validation fixture fits service input limit");
    resolve_owner_chart_support_aliases(&ctx, &mut expected, aliases)
        .expect("validation fixture fits service resource limits");
    if packets == expected {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "owner-chart support references have invalid alias links".to_string(),
        ))
    }
}

pub(crate) fn value_schema_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block_id: &str,
    block_byte_offset: u64,
    fields: &[value_block::ValueField],
    catalog: &CatiaCatalog,
) -> Result<Vec<CatiaValueSchemaSelection>, cadmpeg_core::CodecError> {
    let mut selector_storage = ctx.reserve_scoped(0, "catia_value_selector_indices")?;
    let mut selector_indices = Vec::new();
    for (index, field) in ctx
        .admit_iter(fields, "catia_native_value_selector_field_visits")?
        .enumerate()
    {
        let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
            continue;
        };
        if usize::try_from(*ordinal)
            .ok()
            .is_some_and(|ordinal| ordinal <= catalog.entries.len())
        {
            selector_storage.with_storage(|| {
                ctx.push_vec(&mut selector_indices, index, "catia_value_selector_indices")
            })?;
        }
    }
    let mut selections = Vec::new();
    for (selector_rank, &index) in ctx
        .admit_iter(
            &selector_indices,
            "catia_native_value_selector_index_visits",
        )?
        .enumerate()
    {
        let value_block::ValueField::SchemaSelector { ordinal, offset } = &fields[index] else {
            continue;
        };
        let Some(ordinal_index) = usize::try_from(*ordinal).ok() else {
            continue;
        };
        if ordinal_index > catalog.entries.len() {
            continue;
        }
        let offset = u64_from_index(*offset);
        let Some(byte_offset) = block_byte_offset
            .checked_add(6)
            .and_then(|base| base.checked_add(offset))
        else {
            continue;
        };
        let value_end = selector_indices
            .get(selector_rank + 1)
            .copied()
            .unwrap_or(fields.len());
        let kind = if let Some(entry) = catalog.entries.get(ordinal_index) {
            let mut encoded_value = Vec::new();
            for field in ctx.admit_iter(
                &fields[index + 1..value_end],
                "catia_native_value_selection_field_visits",
            )? {
                let copy = value_block::copy_field_charged(ctx, field)?;
                ctx.push_vec(&mut encoded_value, copy, "catia_value_selection_fields")?;
            }
            CatiaValueSchemaSelectionKind::Selected(CatiaValueSchemaSelectionValue {
                class: CatiaDesignClass {
                    entry: ctx.copy_retained_text(&entry.id, "catia_value_selection_class_id")?,
                    name: ctx
                        .copy_retained_text(&entry.value, "catia_value_selection_class_name")?,
                },
                encoded_value,
            })
        } else {
            CatiaValueSchemaSelectionKind::Terminal
        };
        let id = ctx.format_retained(
            format_args!("catia:outer:value-selection#{byte_offset:010}"),
            "catia_value_selection_id",
        )?;
        let parent = ctx.copy_retained_text(block_id, "catia_value_selection_parent")?;
        ctx.push_vec(
            &mut selections,
            CatiaValueSchemaSelection {
                id,
                parent,
                offset,
                ordinal: *ordinal,
                kind,
            },
            "catia_value_selections",
        )?;
    }
    Ok(selections)
}

impl CatiaValueBlock {
    pub(super) fn from_parts(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        block: value_block::ValueBlock,
        catalog: &CatiaCatalog,
        object_graph: Option<&CatiaObjectGraph>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let id = ctx.format_retained(
            format_args!("catia:outer:value-block#{:010}", block.pos),
            "catia_value_block_id",
        )?;
        // The tokenized fields are dropped once the selections are built.
        let (fields, _fields_storage) = ctx
            .with_scoped_storage("catia_value_block_fields", || {
                value_block::tokenize_charged(ctx, &block.payload)
            })?;
        let byte_offset = u64_from_index(block.pos);
        let schema_selections = value_schema_selections(ctx, &id, byte_offset, &fields, catalog)?;
        Ok(Self {
            id,
            byte_offset,
            object_graph: object_graph
                .map(|graph| ctx.copy_retained_text(&graph.id, "catia_value_block_graph_id"))
                .transpose()?,
            catalog: ctx.copy_retained_text(&catalog.id, "catia_value_block_catalog_id")?,
            payload: block.payload,
            schema_selections,
        })
    }
}

impl CatiaAliasRow {
    pub(super) fn from_source(
        ctx: &DecodeContext<'_>,
        row: object_graph::SurfaceAlias,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: ctx.format_retained(
                format_args!("catia:outer:alias-row#{:010}", row.pos),
                "catia_native_alias_row_id",
            )?,
            byte_offset: u64_from_index(row.pos),
            lead_raw: row.lead_raw,
            tag_raw: row.tag_raw,
            flag: row.flag,
            f1: row.f1,
            object_graph: None,
            object_record: None,
            design_object: None,
            f2: row.f2,
            f3: row.f3,
            group: row.group,
            canonical_surface_tag: None,
        })
    }
}

impl CatiaCatalog {
    pub(super) fn from_source(
        ctx: &DecodeContext<'_>,
        catalog: catalog::Catalog,
    ) -> Result<Self, CodecError> {
        let id = ctx.format_retained(
            format_args!("catia:outer:catalog#{:010}", catalog.pos),
            "catia_native_catalog_id",
        )?;
        let entries = ctx.try_collect_vec(
            catalog
                .entries
                .into_iter()
                .map(|entry| -> Result<_, CodecError> {
                    Ok(CatiaCatalogEntry {
                        id: ctx.format_retained(
                            format_args!("catia:outer:catalog-entry#{:010}", entry.pos),
                            "catia_native_catalog_entry_id",
                        )?,
                        parent: ctx.copy_retained_text(&id, "catia_native_catalog_entry_parent")?,
                        ordinal: entry.ordinal,
                        byte_offset: u64_from_index(entry.pos),
                        value: entry.value,
                    })
                }),
            "catia_native_catalog_entries",
        )?;
        Ok(Self {
            id,
            byte_offset: u64_from_index(catalog.pos),
            byte_len: u64_from_index(catalog.total_len),
            entries,
        })
    }
}

/// Projects one parsed graph and its entity records. The returned record
/// index lives in the caller's scoped storage.
pub(crate) fn native_object_graph(
    ctx: &DecodeContext<'_>,
    index_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    graph: &object_graph::ObjectGraph,
    entity_records: Vec<entity_table::EntityRecord>,
    finjpl_segment: Option<String>,
    outer_container: Option<CatiaOuterContainerBinding>,
) -> Result<(CatiaObjectGraph, Vec<CatiaEntityRecord>, GraphRecordIndex), CodecError> {
    let id = ctx.format_retained(
        format_args!("catia:outer:object-graph#{:010}", graph.pos),
        "catia_native_graph_id",
    )?;
    let mut records = Vec::new();
    for (ordinal, record) in ctx
        .admit_iter(&graph.records, "catia_native_graph_record_visits")?
        .enumerate()
    {
        let entity = entity_records.get(ordinal);
        let roles = record.roles();
        let row = CatiaObjectRecord {
            id: ctx.format_retained(
                format_args!("catia:outer:object-record#{:010}", record.pos),
                "catia_native_record_id",
            )?,
            parent: ctx.copy_retained_text(&id, "catia_native_record_parent")?,
            design_object: None,
            entity: entity
                .map(|entity| -> Result<CatiaObjectEntity, CodecError> {
                    Ok(CatiaObjectEntity {
                        record: ctx.format_retained(
                            format_args!("catia:outer:entity-record#{:010}", entity.pos),
                            "catia_native_record_entity_id",
                        )?,
                        id: entity.entity_id,
                    })
                })
                .transpose()?,
            ordinal: u64_from_index(ordinal),
            byte_offset: u64_from_index(record.pos),
            byte_len: u64_from_index(record.total_len),
            lead: record.lead,
            head: ctx.copy_slice(record.head(), "catia_native_record_head")?,
            inline_body: record
                .inline_body()
                .map(|bytes| ctx.copy_slice(bytes, "catia_native_record_inline_body"))
                .transpose()?,
            owner: roles.owner.map(CatiaObjectOwner::from),
            class: roles.class_ref.map(|class_ref| CatiaObjectClass {
                ordinal: class_ref,
                name: None,
                entry: None,
            }),
            storage: roles.storage_ref.map(|storage_ref| CatiaObjectStorage {
                reference: storage_ref,
                record: None,
                design_object: None,
            }),
            payload: record.payload().copy_charged(ctx)?,
            repeated_reference_schema_selection: None,
            references: Vec::new(),
        };
        ctx.push_vec(&mut records, row, "catia_native_graph_records")?;
    }
    for record in ctx.admit_iter(&mut records, "catia_native_graph_design_object_ids")? {
        record.design_object = record
            .owner_entity_id()
            .map(|owner| design_object_id(ctx, u64_from_index(graph.pos), owner))
            .transpose()?;
    }
    let record_index = GraphRecordIndex::new(ctx, index_storage, &records)?;
    for index in ctx.admit_iter(
        &(0..records.len()),
        "catia_native_graph_record_link_updates",
    )? {
        let storage_ref = records[index]
            .storage
            .as_ref()
            .map(|storage| storage.reference);
        let (storage_record, storage_design_object) =
            resolved_storage_link(ctx, storage_ref, &records, &record_index)?;
        let references =
            resolved_payload_references(ctx, &records[index].payload, &records, &record_index)?;
        let record = &mut records[index];
        if let Some(storage) = &mut record.storage {
            storage.record = storage_record;
            storage.design_object = storage_design_object;
        }
        record.references = references;
    }
    let mut entities = Vec::new();
    for (ordinal, entity) in ctx
        .admit_iter(entity_records, "catia_native_graph_entity_visits")?
        .enumerate()
    {
        let Some(object_record) = records.get(ordinal) else {
            continue;
        };
        let reference_signature = entity.reference_signature(ctx)?;
        let body = match entity.body {
            entity_table::EntityBody::Inline(bytes) => CatiaEntityRecordBody::Inline(bytes),
            entity_table::EntityBody::Nested {
                prefix,
                suffix,
                value_payload,
                record_suffix,
                ..
            } => CatiaEntityRecordBody::Nested {
                definition_prefix: prefix,
                definition_suffix: suffix,
                value_payload,
                record_suffix,
            },
        };
        let row = CatiaEntityRecord {
            id: ctx.format_retained(
                format_args!("catia:outer:entity-record#{:010}", entity.pos),
                "catia_native_entity_id",
            )?,
            object_graph: ctx.copy_retained_text(&id, "catia_native_entity_graph")?,
            object_record: ctx
                .copy_retained_text(&object_record.id, "catia_native_entity_object")?,
            ordinal: u64_from_index(ordinal),
            byte_offset: u64_from_index(entity.pos),
            lead: entity.lead,
            body,
            definition_schema_selections: Vec::new(),
            entity_id: entity.entity_id,
            value_schema_selections: Vec::new(),
            object_production: None,
            value_production: None,
            range_interval: None,
            reference_signature: reference_signature.map(|production| CatiaReferenceSignature {
                production,
                first_entity: CatiaEntityReference::Unresolved { entity_id: 0 },
                second_entity: CatiaEntityReference::Unresolved { entity_id: 0 },
            }),
            suffix: None,
            suffix_schema_selection: None,
        };
        index_storage
            .with_storage(|| ctx.push_vec(&mut entities, row, "catia_native_graph_entities"))?;
    }
    Ok((
        CatiaObjectGraph {
            id,
            byte_offset: u64_from_index(graph.pos),
            byte_len: u64_from_index(graph.total_len),
            finjpl_segment,
            outer_container,
            catalog_byte_offset: graph.catalog_pos.map(u64_from_index),
            catalog: None,
            records,
        },
        entities,
        record_index,
    ))
}

impl CatiaOuterContainerBinding {
    pub(super) fn from_source(
        ctx: &DecodeContext<'_>,
        declaration: &container::OuterContainerDeclaration,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            data_offset: u64_from_index(declaration.data_offset),
            ordinal: declaration.ordinal,
            class_name: ctx
                .copy_retained_text(&declaration.class_name, "catia_native_outer_class")?,
            base_class: ctx
                .copy_retained_text(&declaration.base_class, "catia_native_outer_base_class")?,
            stream_name: ctx
                .copy_retained_text(&declaration.stream_name, "catia_native_outer_stream_name")?,
        })
    }
}

#[cfg(test)]
mod consolidated_edge_run_limit_tests {
    use std::collections::HashSet;

    #[test]
    fn native_edge_runs_propagate_a5_surface_limit() {
        let bytes = crate::test_support::test_a5a8::a5_surface_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            super::consolidated_edge_runs(
                ctx,
                &bytes,
                &records,
                &[],
                &[],
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_a5_distinct_knots"));
        let runs = crate::test_support::with_service_context(|ctx| {
            super::consolidated_edge_runs(
                ctx,
                &bytes,
                &records,
                &[],
                &[],
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service collection budget");
        assert!(runs.is_empty());
    }

    #[test]
    fn native_edge_run_indexes_loci_and_ids_refuse_limits() {
        let mut supported = crate::test_support::test_b2::b2_cylinder_stream();
        for point in [
            [1.0f32, 4.0, 3.0],
            [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
        ] {
            supported.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in point {
                supported.extend_from_slice(&value.to_le_bytes());
            }
        }
        supported.extend_from_slice(
            &crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142),
        );
        let fixtures = [
            crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142),
            supported,
        ];
        let mut collection_refusals = HashSet::new();
        let mut retained_refusals = HashSet::new();
        let mut scoped_refusals = HashSet::new();
        for bytes in fixtures {
            let native = super::super::CatiaNative::decode(&bytes);
            assert_eq!(native.consolidated_edge_runs.len(), 1);
            let records = crate::wire::records::consolidated_records(&bytes);
            for limit in 0..1024 {
                let result = crate::test_support::with_collection_limit(limit, |ctx| {
                    super::consolidated_edge_runs(
                        ctx,
                        &bytes,
                        &records,
                        &native.consolidated_pcurves,
                        &native.consolidated_edge_nodes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                });
                if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) = result {
                    collection_refusals.insert(error.operation);
                }
            }
            let mut limit = 0;
            for _ in 0..4096 {
                let result = crate::test_support::with_retained_limit(limit, |ctx| {
                    super::consolidated_edge_runs(
                        ctx,
                        &bytes,
                        &records,
                        &native.consolidated_pcurves,
                        &native.consolidated_edge_nodes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                });
                if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) = result {
                    retained_refusals.insert(error.operation);
                    assert!(error.used + error.additional > limit);
                    limit = error.used + error.additional;
                } else {
                    assert!(result.is_ok());
                    break;
                }
            }
            let mut limit = 0;
            for _ in 0..4096 {
                let result = crate::test_support::with_materialized_limit(limit, |ctx| {
                    super::consolidated_edge_runs(
                        ctx,
                        &bytes,
                        &records,
                        &native.consolidated_pcurves,
                        &native.consolidated_edge_nodes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                });
                if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) = result {
                    scoped_refusals.insert(error.operation);
                    assert!(error.used + error.additional > limit);
                    limit = error.used + error.additional;
                } else {
                    assert!(result.is_ok());
                    break;
                }
            }
        }
        for operation in [
            "catia_native_edge_run_pcurve_index",
            "catia_native_edge_run_resolved_index",
            "catia_native_edge_run_node_index",
            "catia_native_edge_run_shared_loci",
            "catia_native_edge_runs",
        ] {
            assert!(
                collection_refusals.contains(operation),
                "{operation} did not refuse"
            );
        }
        for operation in [
            "catia_native_edge_run_id",
            "catia_native_edge_run_first_pcurve_id",
            "catia_native_edge_run_second_pcurve_id",
            "catia_native_edge_run_node_id",
        ] {
            assert!(
                retained_refusals.contains(operation),
                "{operation} did not refuse"
            );
        }
        assert!(scoped_refusals.contains("catia_native_edge_run_pcurve_index"));
    }
}
