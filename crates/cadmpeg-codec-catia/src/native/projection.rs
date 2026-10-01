// SPDX-License-Identifier: Apache-2.0
//! CATIA native ownership, alias, and wire projections.

use cadmpeg_core::decode::u64_from_index;

use crate::object_graph;

use super::edge_definition::CatiaConsolidatedEdgeDefinition;

use super::{
    catalog, container, design_object_id, entity_table, resolved_payload_references,
    resolved_storage_link, terminal_null_entity_id, value_block, AliasLead, CatiaAliasRow,
    CatiaAllocationReferenceEncoding, CatiaCatalog, CatiaCatalogEntry,
    CatiaConsolidatedAnalyticCircleBinding, CatiaConsolidatedCircle,
    CatiaConsolidatedClass25Descriptor, CatiaConsolidatedEdgeNode, CatiaConsolidatedEdgeRun,
    CatiaConsolidatedEdgeUses, CatiaConsolidatedOwnerPacket, CatiaConsolidatedPcurve,
    CatiaConsolidatedSupportBinding, CatiaDesignClass, CatiaEntityRecord, CatiaEntityRecordBody,
    CatiaEntityReference, CatiaExternalReference, CatiaFaceNodeRelation,
    CatiaFaceNodeTargetEncoding, CatiaFinjplSegment, CatiaObjectClass, CatiaObjectEntity,
    CatiaObjectGraph, CatiaObjectOwner, CatiaObjectRecord, CatiaObjectStorage,
    CatiaOuterContainerBinding, CatiaOwnerBoundaryCycle, CatiaOwnerBoundaryEdge,
    CatiaOwnerChartAddress, CatiaOwnerChartAliasBinding, CatiaOwnerChartBridge,
    CatiaOwnerChartBridgeReference, CatiaOwnerChartCarrier, CatiaOwnerChartRelation,
    CatiaOwnerIdentityEncoding, CatiaOwnerIdentityTarget, CatiaOwnerPacketPayload,
    CatiaOwnerReferenceEncoding, CatiaPreviewImage, CatiaReferenceSignature, CatiaValueBlock,
    CatiaValueSchemaSelection, CatiaValueSchemaSelectionKind, CatiaValueSchemaSelectionValue,
    CatiaZeroEntityRecord, CodecError, ConsolidatedRecord, DecodeContext, HashMap, HashSet,
};

pub(crate) fn consolidated_owner_packets(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedOwnerPacket>, CodecError> {
    let fixed = ctx.collect_vec(
        crate::families::b2::records::b2_owner_packets_from_records(bytes, records),
        "catia_native_fixed_owner_packets",
    )?;
    let mut owner_charts = HashMap::new();
    for chart in crate::families::b2::records::b2_owner_charts_from_records(ctx, bytes, records)? {
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
            bridge: match chart.bridge {
                crate::families::b2::records::B2OwnerChartBridge::SupportedSurface {
                    pos,
                    carrier_surface,
                    support_surfaces,
                    support_pcurves,
                    middle_controls,
                    terminal_control,
                    construction_radius,
                } => CatiaOwnerChartBridge::SupportedSurface {
                    byte_offset: u64_from_index(pos),
                    carrier_surface: native_reference(carrier_surface),
                    support_surfaces: support_surfaces.map(native_reference),
                    support_pcurves: support_pcurves.map(native_reference),
                    middle_controls,
                    terminal_control,
                    construction_radius,
                },
                crate::families::b2::records::B2OwnerChartBridge::Extended { pos, references } => {
                    CatiaOwnerChartBridge::Extended {
                        byte_offset: u64_from_index(pos),
                        references: references.map(native_reference),
                    }
                }
            },
            parameter_point_byte_offsets: chart.parameter_point_offsets().map(u64_from_index),
        };
        ctx.insert_hash_map(&mut owner_charts, key, value, "catia_native_owner_charts")?;
    }
    let mut identity_targets = HashMap::<(usize, usize), Vec<CatiaOwnerIdentityTarget>>::new();
    for target in
        crate::families::b2::records::b2_owner_identity_targets_from_records(ctx, bytes, records)?
    {
        let key = (target.source_index, target.owner_pos);
        let value = CatiaOwnerIdentityTarget {
            slot: target.slot,
            distance: target.distance,
            target_byte_offset: u64_from_index(target.target_pos),
            target_class: target.target_class,
        };
        if let Some(targets) = identity_targets.get_mut(&key) {
            ctx.push_vec(targets, value, "catia_native_owner_identity_target_entries")?;
        } else {
            let mut targets = Vec::new();
            ctx.push_vec(
                &mut targets,
                value,
                "catia_native_owner_identity_target_entries",
            )?;
            ctx.insert_hash_map(
                &mut identity_targets,
                key,
                targets,
                "catia_native_owner_identity_target_groups",
            )?;
        }
    }
    let mut boundary_cycles = HashMap::new();
    for cycle in
        crate::families::consolidated::records::consolidated_owner_boundary_cycles_from_records(
            ctx, bytes, records,
        )?
    {
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
        ctx.insert_hash_map(
            &mut boundary_cycles,
            key,
            value,
            "catia_native_owner_boundary_cycles",
        )?;
    }
    let adjacent_counted =
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            ctx, bytes, records,
        )?;
    let mut face_nodes = HashMap::new();
    for linked in
        crate::families::b2::records::b2_adjacent_face_owners_from_records(ctx, bytes, records)?
    {
        ctx.insert_hash_map(
            &mut face_nodes,
            (linked.owner.source_index, linked.owner.pos),
            linked.face_node,
            "catia_native_owner_face_nodes",
        )?;
    }
    for linked in adjacent_counted {
        ctx.insert_hash_map(
            &mut face_nodes,
            (linked.owner.source_index, linked.owner.pos),
            linked.face_node,
            "catia_native_owner_face_nodes",
        )?;
    }
    let mut fixed_positions = HashSet::new();
    for packet in &fixed {
        ctx.insert_hash_set(
            &mut fixed_positions,
            (packet.source_index, packet.pos),
            "catia_native_fixed_owner_positions",
        )?;
    }
    let counted_owners =
        crate::families::b2::records::b2_counted_owners_from_records(ctx, bytes, records)?;
    let mut packets = ctx.collect_vec(fixed
        .into_iter()
        .map(|packet| {
            (
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
                        crate::families::b2::records::B2OwnerIdentityEncoding::Allocation(
                            encoding,
                        ) => CatiaOwnerIdentityEncoding::Allocation(
                            native_allocation_reference_encoding(encoding),
                        ),
                        crate::families::b2::records::B2OwnerIdentityEncoding::RawU8 => {
                            CatiaOwnerIdentityEncoding::RawU8
                        }
                    }),
                    numeric_tail: packet.numeric_tail,
                    identity_targets: Vec::new(),
                    owner_chart: None,
                    boundary_cycle: None,
                },
            )
        })
        .chain(
            counted_owners
                .into_iter()
                .filter(|packet| !fixed_positions.contains(&(packet.source_index, packet.pos)))
                .map(|packet| {
                    (
                        packet.pos,
                        packet.source_index,
                        packet.header_token,
                        CatiaOwnerPacketPayload::Counted {
                            references: packet.references,
                            tail: packet.tail,
                        },
                    )
                }),
        ), "catia_native_owner_packet_rows")?;
    packets.sort_by_key(|(pos, source_index, _, _)| (*pos, *source_index));
    let mut output = Vec::new();
    ctx.reserve_vec(
        &mut output,
        packets.len(),
        "catia_native_owner_packet_output",
    )?;
    for (pos, source_index, header_token, mut payload) in packets {
        if let CatiaOwnerPacketPayload::FixedNine {
            identity_targets: stored_targets,
            owner_chart,
            boundary_cycle,
            ..
        } = &mut payload
        {
            *stored_targets = identity_targets
                .remove(&(source_index, pos))
                .unwrap_or_default();
            *owner_chart = owner_charts.remove(&(source_index, pos)).map(Box::new);
            *boundary_cycle = boundary_cycles
                .get(&(source_index, pos))
                .copied()
                .map(Box::new);
        }
        output.push(CatiaConsolidatedOwnerPacket {
                id: ctx.format_retained(format_args!("catia:consolidated:owner-packet#{pos:010}"), "catia_native_owner_packet_id")?,
                byte_offset: u64_from_index(pos),
                source_index,
                header_token,
                payload,
                face_node: face_nodes
                    .get(&(source_index, pos))
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
            });
    }
    Ok(output)
}

pub(crate) fn consolidated_edge_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<CatiaConsolidatedEdgeRun>, CodecError> {
    let mut pcurve_ids = HashMap::new();
    for pcurve in pcurves {
        let id = ctx.copy_retained_text(&pcurve.id, "catia_native_edge_run_pcurve_index_id")?;
        ctx.insert_hash_map(
            &mut pcurve_ids,
            pcurve.byte_offset,
            id,
            "catia_native_edge_run_pcurve_index",
        )?;
    }
    let mut resolved = HashMap::new();
    for block in
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            ctx, bytes, records, refusal,
        )?
    {
        ctx.insert_hash_map(
            &mut resolved,
            block.block.pcurves[0].pos,
            block,
            "catia_native_edge_run_resolved_index",
        )?;
    }
    let mut nodes_by_offset = HashMap::new();
    for node in nodes {
        ctx.insert_hash_map(
            &mut nodes_by_offset,
            node.byte_offset,
            node,
            "catia_native_edge_run_node_index",
        )?;
    }
    let mut output = Vec::new();
    for (index, run) in
        crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
            ctx, bytes, records,
        )?
        .into_iter()
        .enumerate()
    {
        let pcurve_offsets = run
            .edge
            .pcurves
            .each_ref()
            .map(|pcurve| u64_from_index(pcurve.pos));
        let resolved = resolved.get(&run.edge.pcurves[0].pos);
        let Some(node) = nodes_by_offset.get(&(u64_from_index(run.node.pos))) else {
            continue;
        };
        if node.uses.is_none() {
            continue;
        }
        let (Some(first), Some(second)) = (
            pcurve_ids.get(&pcurve_offsets[0]),
            pcurve_ids.get(&pcurve_offsets[1]),
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
    let mut circle_ids = HashMap::new();
    for circle in circles {
        ctx.insert_hash_map(
            &mut circle_ids,
            circle.byte_offset,
            circle.id.as_str(),
            "catia_native_edge_circle_ids",
        )?;
    }
    let mut frames = HashMap::new();
    for record in records.iter().filter(|record| {
        record.family == crate::wire::records::ConsolidatedFamily::B && record.class == 0x5e
    }) {
        ctx.insert_hash_map(
            &mut frames,
            record.byte_offset(),
            (record.width, record.flag, record.source_index),
            "catia_native_edge_frames",
        )?;
    }
    let mut owned_nodes = HashMap::new();
    for owned in crate::families::consolidated::records::consolidated_owned_edge_nodes_from_records(
        ctx, bytes, records,
    )? {
        ctx.insert_hash_map(
            &mut owned_nodes,
            owned.node.pos,
            (owned.owner_pos, owned.allocation_ordinal),
            "catia_native_owned_edge_nodes",
        )?;
    }
    let mut compact_endpoints = HashMap::new();
    for binding in
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            ctx, bytes, records,
        )?
    {
        ctx.insert_hash_map(
            &mut compact_endpoints,
            binding.node.pos,
            binding.endpoint_records.map(u64_from_index),
            "catia_native_edge_compact_endpoints",
        )?;
    }
    let mut use_runs = HashMap::new();
    for run in crate::families::consolidated::records::consolidated_edge_use_runs_from_records(
        ctx, bytes, records,
    )? {
        let Some(uses) = native_consolidated_edge_uses(&run.uses) else {
            continue;
        };
        ctx.insert_hash_map(
            &mut use_runs,
            run.node.pos,
            (
                uses,
                run.definition
                    .map(|definition| CatiaConsolidatedEdgeDefinition::from_source(ctx, definition))
                    .transpose()?,
            ),
            "catia_native_edge_use_runs",
        )?;
    }
    let mut analytic_circles = HashMap::new();
    for run in
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs_from_records(
            ctx, bytes, records,
        )?
    {
        let Some(circle) = circle_ids.get(&(u64_from_index(run.circle.pos))) else {
            continue;
        };
        let circle = ctx.copy_retained_text(circle, "catia_native_analytic_edge_circle_id")?;
        ctx.insert_hash_map(
            &mut analytic_circles,
            run.node.pos,
            CatiaConsolidatedAnalyticCircleBinding {
                descriptor: run.descriptor.into(),
                circle,
            },
            "catia_native_analytic_edge_bindings",
        )?;
    }
    let mut class25_descriptors = HashMap::new();
    for run in crate::families::consolidated::records::consolidated_class25_edge_runs_from_records(
        ctx, bytes, records,
    )? {
        ctx.insert_hash_map(
            &mut class25_descriptors,
            run.node.pos,
            CatiaConsolidatedClass25Descriptor {
                byte_offset: u64_from_index(run.descriptor.pos),
                record_id: run.descriptor.record_id,
                control: run.descriptor.control,
                values: run.descriptor.values,
            },
            "catia_native_class25_edge_descriptors",
        )?;
    }
    let mut output = Vec::new();
    for (index, node) in
        crate::families::b2::records::b2_edge_nodes_from_records(bytes, records).enumerate()
    {
        let Some(&(width, flag, source_index)) = frames.get(&node.pos) else {
            continue;
        };
        ctx.reserve_vec(&mut output, 1, "catia_native_consolidated_edge_nodes")?;
        let allocation = owned_nodes
            .get(&node.pos)
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
        let (uses, definition) = match use_runs.remove(&node.pos) {
            Some((uses, definition)) => (Some(uses), definition),
            None => (None, None),
        };
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
            endpoint_records: compact_endpoints.get(&node.pos).copied(),
            parameter_selectors: [node.start_parameter_ref, node.end_parameter_ref],
            reference_encodings: node
                .reference_encodings
                .map(native_allocation_reference_encoding),
            terminal_value: node.terminal_value,
            terminal_encoding: native_allocation_reference_encoding(node.terminal_encoding),
            tail: node.tail,
            definition,
            uses,
            analytic_circle: analytic_circles.remove(&node.pos),
            class25_descriptor: class25_descriptors.remove(&node.pos),
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

pub(crate) fn containing_finjpl_segment(
    byte_offset: u64,
    byte_len: u64,
    segments: &[CatiaFinjplSegment],
) -> Option<&str> {
    let byte_end = byte_offset.checked_add(byte_len)?;
    let mut containing = segments.iter().filter(|segment| {
        segment.byte_offset <= byte_offset
            && segment
                .byte_offset
                .checked_add(segment.byte_len)
                .is_some_and(|segment_end| byte_end <= segment_end)
    });
    let segment = containing.next()?;
    containing.next().is_none().then_some(segment.id.as_str())
}

pub(crate) fn preview_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segments: &[CatiaFinjplSegment],
) -> Result<Vec<CatiaPreviewImage>, cadmpeg_core::CodecError> {
    let mut views = Vec::new();
    for segment in segments {
        for preview in container::preview_images(ctx, &segment.data)? {
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
            let data = ctx.copy_retained_slice(
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
    for segment in segments {
        for reference in container::external_references(ctx, &segment.data)? {
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
    let mut stored_by_group = HashMap::<(u32, u32), Option<u32>>::new();
    for row in rows.iter() {
        let Some(group) = row.group.as_ref() else {
            continue;
        };
        if row.lead() != AliasLead::SurfaceSupportStorage {
            continue;
        }
        let key = (group.prototype, group.group_id);
        if let Some(stored) = stored_by_group.get_mut(&key) {
            *stored = None;
        } else {
            ctx.insert_hash_map(
                &mut stored_by_group,
                key,
                Some(row.tag()),
                "catia_alias_group_index",
            )?;
        }
    }
    for row in rows {
        row.canonical_surface_tag = match row.lead() {
            AliasLead::SurfaceSupportStorage => Some(row.tag()),
            AliasLead::NonSurfaceAlias => row.group.as_ref().and_then(|group| {
                stored_by_group
                    .get(&(group.prototype, group.group_id))
                    .copied()
                    .flatten()
            }),
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
    let mut unique_by_tag = HashMap::<u32, Option<&CatiaAliasRow>>::new();
    for row in aliases {
        let key = row.tag();
        if let Some(stored) = unique_by_tag.get_mut(&key) {
            *stored = None;
        } else {
            ctx.insert_hash_map(
                &mut unique_by_tag,
                key,
                Some(row),
                "catia_owner_alias_index",
            )?;
        }
    }
    let resolve = |reference: &mut CatiaOwnerChartBridgeReference| -> Result<(), CodecError> {
        if let CatiaOwnerChartAddress::WidthCoded { alias } = &mut reference.address {
            *alias = if let Some(row) = unique_by_tag.get(&reference.value).copied().flatten() {
                let id = ctx.copy_retained_text(&row.id, "catia_owner_alias_binding_id")?;
                cadmpeg_core::text::NonBlankString::new(id)
                    .map(|id| CatiaOwnerChartAliasBinding::new(id, row.canonical_surface_tag))
            } else {
                None
            };
        }
        Ok(())
    };
    for packet in packets {
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
    let mut selector_indices = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
            continue;
        };
        if usize::try_from(*ordinal)
            .ok()
            .is_some_and(|ordinal| ordinal <= catalog.entries.len())
        {
            ctx.push_vec(&mut selector_indices, index, "catia_value_selector_indices")?;
        }
    }
    let mut selections = Vec::new();
    for (selector_rank, &index) in selector_indices.iter().enumerate() {
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
            for field in &fields[index + 1..value_end] {
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
        let fields = value_block::tokenize_charged(ctx, &block.payload)?;
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
        let mut entries = Vec::new();
        for entry in catalog.entries {
            let native_entry = CatiaCatalogEntry {
                id: ctx.format_retained(
                    format_args!("catia:outer:catalog-entry#{:010}", entry.pos),
                    "catia_native_catalog_entry_id",
                )?,
                parent: ctx.copy_retained_text(&id, "catia_native_catalog_entry_parent")?,
                ordinal: entry.ordinal,
                byte_offset: u64_from_index(entry.pos),
                value: entry.value,
            };
            ctx.push_vec(&mut entries, native_entry, "catia_native_catalog_entries")?;
        }
        Ok(Self {
            id,
            byte_offset: u64_from_index(catalog.pos),
            byte_len: u64_from_index(catalog.total_len),
            entries,
        })
    }
}

pub(crate) fn native_object_graph(
    ctx: &DecodeContext<'_>,
    graph: object_graph::ObjectGraph,
    entity_records: Vec<entity_table::EntityRecord>,
    finjpl_segment: Option<String>,
    outer_container: Option<CatiaOuterContainerBinding>,
) -> Result<(CatiaObjectGraph, Vec<CatiaEntityRecord>), CodecError> {
    let id = ctx.format_retained(
        format_args!("catia:outer:object-graph#{:010}", graph.pos),
        "catia_native_graph_id",
    )?;
    let mut records = Vec::new();
    for (ordinal, record) in graph.records.into_iter().enumerate() {
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
            head: ctx.copy_retained_slice(record.head(), "catia_native_record_head")?,
            inline_body: record
                .inline_body()
                .map(|bytes| ctx.copy_retained_slice(bytes, "catia_native_record_inline_body"))
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
    for record in &mut records {
        record.design_object = record
            .owner_entity_id()
            .map(|owner| design_object_id(ctx, u64_from_index(graph.pos), owner))
            .transpose()?;
    }
    let record_indices = ctx.collect_hash_map(
        records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| Some((record.entity_id()?, index))),
        "catia_native_graph_record_indices",
    )?;
    let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
    for index in 0..records.len() {
        let storage_ref = records[index]
            .storage
            .as_ref()
            .map(|storage| storage.reference);
        let (storage_record, storage_design_object) =
            resolved_storage_link(ctx, storage_ref, &records, &record_indices)?;
        let references = resolved_payload_references(
            ctx,
            &records[index].payload,
            &records,
            &record_indices,
            terminal_null_entity_id,
        )?;
        let record = &mut records[index];
        if let Some(storage) = &mut record.storage {
            storage.record = storage_record;
            storage.design_object = storage_design_object;
        }
        record.references = references;
    }
    let mut entities = Vec::new();
    for (ordinal, entity) in entity_records.into_iter().enumerate() {
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
        ctx.push_vec(&mut entities, row, "catia_native_graph_entities")?;
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
            "catia_native_edge_run_pcurve_index_id",
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
    }
}
