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
    let fixed = crate::families::b2::records::b2_owner_packets_from_records(bytes, records)
        .collect::<Vec<_>>();
    let mut owner_charts = HashMap::new();
    for chart in crate::families::b2::records::b2_owner_charts_from_records(bytes, records) {
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
        owner_charts.insert(key, value);
    }
    let mut identity_targets = HashMap::<(usize, usize), Vec<CatiaOwnerIdentityTarget>>::new();
    for target in
        crate::families::b2::records::b2_owner_identity_targets_from_records(bytes, records)
    {
        let key = (target.source_index, target.owner_pos);
        let value = CatiaOwnerIdentityTarget {
            slot: target.slot,
            distance: target.distance,
            target_byte_offset: u64_from_index(target.target_pos),
            target_class: target.target_class,
        };
        if let Some(targets) = identity_targets.get_mut(&key) {
            targets.push(value);
        } else {
            let targets = vec![value];
            identity_targets.insert(key, targets);
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
                            terminal: face_node.terminal.into(),
                        })
                    }),
                    edges: cycle.edges.map(|edge| CatiaOwnerBoundaryEdge {
                        slot: edge.slot,
                        byte_offset: u64_from_index(edge.target_pos),
                        endpoint_records: edge.endpoint_records.map(u64_from_index),
                    }),
                };
        boundary_cycles.insert(key, value);
    }
    let adjacent_counted =
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(bytes, records);
    let mut face_nodes = HashMap::new();
    for linked in crate::families::b2::records::b2_adjacent_face_owners_from_records(bytes, records)
    {
        face_nodes.insert(
            (linked.owner.source_index, linked.owner.pos),
            linked.face_node,
        );
    }
    for linked in adjacent_counted {
        face_nodes.insert(
            (linked.owner.source_index, linked.owner.pos),
            linked.face_node,
        );
    }
    let mut fixed_positions = HashSet::new();
    for packet in &fixed {
        fixed_positions.insert((packet.source_index, packet.pos));
    }
    let counted_owners =
        crate::families::b2::records::b2_counted_owners_from_records(bytes, records);
    let mut packets = fixed
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
        ).collect::<Vec<_>>();
    packets.sort_by(
        |(left_pos, left_source, _, _), (right_pos, right_source, _, _)| {
            (left_pos, left_source).cmp(&(right_pos, right_source))
        },
    );
    let mut output = Vec::new();

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
                id: format!("catia:consolidated:owner-packet#{pos:010}"),
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
                            terminal: face_node.terminal.into(),
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
        let id = pcurve.id.clone();
        pcurve_ids.insert(pcurve.byte_offset, id);
    }
    let mut resolved = HashMap::new();
    for block in
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            ctx, bytes, records, refusal,
        )?
    {
        resolved.insert(block.block.pcurves[0].pos, block);
    }
    let mut nodes_by_offset = HashMap::new();
    for node in nodes {
        nodes_by_offset.insert(node.byte_offset, node);
    }
    let mut output = Vec::new();
    for (index, run) in
        crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
            bytes, records,
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
            .map(|points| points.iter().map(point_coordinates).collect::<Vec<_>>());
        let value = CatiaConsolidatedEdgeRun {
            id: format!("catia:consolidated:edge-run#{index:01}"),
            byte_offset: pcurve_offsets[0],
            pcurves: [first.to_owned(), second.to_owned()],
            parameter_range: run.edge.parameters.range,
            tolerance: run.edge.parameters.tolerance,
            node: node.id.clone(),
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
        output.push(value);
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
        circle_ids.insert(circle.byte_offset, circle.id.as_str());
    }
    let mut frames = HashMap::new();
    for record in records.iter().filter(|record| {
        record.family() == crate::wire::records::ConsolidatedFamily::B && record.class() == 0x5e
    }) {
        frames.insert(
            record.byte_offset(),
            (record.width(), record.flag(), record.source_index()),
        );
    }
    let mut owned_nodes = HashMap::new();
    for owned in crate::families::consolidated::records::consolidated_owned_edge_nodes_from_records(
        bytes, records,
    ) {
        owned_nodes.insert(owned.node.pos, (owned.owner_pos, owned.allocation_ordinal));
    }
    let mut compact_endpoints = HashMap::new();
    for binding in
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            ctx, bytes, records,
        )?
    {
        compact_endpoints.insert(
            binding.node.pos,
            binding.endpoint_records.map(u64_from_index),
        );
    }
    let mut use_runs = HashMap::new();
    for run in crate::families::consolidated::records::consolidated_edge_use_runs_from_records(
        bytes, records,
    )? {
        let Some(uses) = native_consolidated_edge_uses(&run.uses) else {
            continue;
        };
        let definition = run
            .definition
            .map(CatiaConsolidatedEdgeDefinition::from_source);
        use_runs.insert(run.node.pos, (uses, definition));
    }
    let mut analytic_circles = HashMap::new();
    for run in
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs_from_records(
            bytes, records,
        )?
    {
        let Some(circle) = circle_ids.get(&(u64_from_index(run.circle.pos))) else {
            continue;
        };
        let circle = (*circle).to_owned();
        analytic_circles.insert(
            run.node.pos,
            CatiaConsolidatedAnalyticCircleBinding {
                descriptor: run.descriptor.into(),
                circle,
            },
        );
    }
    let mut class25_descriptors = HashMap::new();
    for run in crate::families::consolidated::records::consolidated_class25_edge_runs_from_records(
        bytes, records,
    )? {
        class25_descriptors.insert(
            run.node.pos,
            CatiaConsolidatedClass25Descriptor {
                byte_offset: u64_from_index(run.descriptor.pos),
                record_id: run.descriptor.record_id,
                control: run.descriptor.control,
                values: run.descriptor.values,
            },
        );
    }
    let mut output = Vec::new();
    for (index, node) in
        crate::families::b2::records::b2_edge_nodes_from_records(bytes, records).enumerate()
    {
        let Some(&(width, flag, source_index)) = frames.get(&node.pos) else {
            continue;
        };

        let allocation = owned_nodes.get(&node.pos).map(|(pos, ordinal)| {
            (
                format!("catia:consolidated:owner-packet#{:010}", *pos),
                *ordinal,
            )
        });
        let (uses, definition) = match use_runs.remove(&node.pos) {
            Some((uses, definition)) => (Some(uses), definition),
            None => (None, None),
        };
        output.push(CatiaConsolidatedEdgeNode {
            id: format!("catia:consolidated:edge-node#{index:01}"),
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

pub(crate) fn preview_views(segments: &[CatiaFinjplSegment]) -> Vec<CatiaPreviewImage> {
    let mut views = Vec::new();
    for segment in segments {
        for preview in container::preview_images(&segment.data) {
            let Some(byte_offset) = segment
                .byte_offset
                .checked_add(u64_from_index(preview.range.start))
            else {
                continue;
            };
            let id = format!("catia:outer:preview#{}", views.len());
            let data = segment.data[preview.range.clone()].to_vec();
            views.push(CatiaPreviewImage {
                id,
                byte_offset,
                byte_len: u64_from_index(preview.range.end - preview.range.start),
                width: preview.width,
                height: preview.height,
                components: preview.components,
                data: data.into(),
            });
        }
    }
    views
}

pub(crate) fn external_reference_views(
    segments: &[CatiaFinjplSegment],
) -> Vec<CatiaExternalReference> {
    let mut views = Vec::new();
    for segment in segments {
        for reference in container::external_references(&segment.data) {
            let Some(byte_offset) = segment
                .byte_offset
                .checked_add(u64_from_index(reference.offset))
            else {
                continue;
            };
            let id = format!("catia:outer:external-reference#{}", views.len());
            let segment_id = segment.id.clone();
            views.push(CatiaExternalReference {
                id,
                byte_offset,
                target: reference.target,
                segment: segment_id,
            });
        }
    }
    views
}

pub(crate) fn resolve_alias_surface_tags(rows: &mut [CatiaAliasRow]) {
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
            stored_by_group.insert(key, Some(row.tag()));
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
}

pub(crate) fn resolve_owner_chart_support_aliases(
    packets: &mut [CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
) {
    let mut unique_by_tag = HashMap::<u32, Option<&CatiaAliasRow>>::new();
    for row in aliases {
        let key = row.tag();
        if let Some(stored) = unique_by_tag.get_mut(&key) {
            *stored = None;
        } else {
            unique_by_tag.insert(key, Some(row));
        }
    }
    let resolve = |reference: &mut CatiaOwnerChartBridgeReference| {
        if let CatiaOwnerChartAddress::WidthCoded { alias } = &mut reference.address {
            *alias = if let Some(row) = unique_by_tag.get(&reference.value).copied().flatten() {
                let id = row.id.clone();
                cadmpeg_core::text::NonBlankString::new(id)
                    .map(|id| CatiaOwnerChartAliasBinding::new(id, row.canonical_surface_tag))
            } else {
                None
            };
        }
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
            resolve(reference);
        }
    }
}

#[cfg(test)]
pub(crate) fn validate_alias_surface_tags(
    rows: &[CatiaAliasRow],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut expected = rows.to_vec();
    resolve_alias_surface_tags(&mut expected);
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
    resolve_owner_chart_support_aliases(&mut expected, aliases);
    if packets == expected {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "owner-chart support references have invalid alias links".to_string(),
        ))
    }
}

pub(crate) fn value_schema_selections(
    block_id: &str,
    block_byte_offset: u64,
    fields: &[value_block::ValueField],
    catalog: &CatiaCatalog,
) -> Vec<CatiaValueSchemaSelection> {
    let mut selector_indices = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
            continue;
        };
        if usize::try_from(*ordinal)
            .ok()
            .is_some_and(|ordinal| ordinal <= catalog.entries.len())
        {
            selector_indices.push(index);
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
            let encoded_value = fields[index + 1..value_end].to_vec();
            CatiaValueSchemaSelectionKind::Selected(CatiaValueSchemaSelectionValue {
                class: CatiaDesignClass {
                    entry: entry.id.clone(),
                    name: entry.value.clone(),
                },
                encoded_value,
            })
        } else {
            CatiaValueSchemaSelectionKind::Terminal
        };
        let id = format!("catia:outer:value-selection#{byte_offset:010}");
        let parent = block_id.to_owned();
        selections.push(CatiaValueSchemaSelection {
            id,
            parent,
            offset,
            ordinal: *ordinal,
            kind,
        });
    }
    selections
}

impl CatiaValueBlock {
    pub(super) fn from_parts(
        block: value_block::ValueBlock,
        catalog: &CatiaCatalog,
        object_graph: Option<&CatiaObjectGraph>,
    ) -> Self {
        let id = format!("catia:outer:value-block#{:010}", block.pos);
        let fields = value_block::tokenize(&block.payload);
        let byte_offset = u64_from_index(block.pos);
        let schema_selections = value_schema_selections(&id, byte_offset, &fields, catalog);
        Self {
            id,
            byte_offset,
            object_graph: object_graph.map(|graph| graph.id.clone()),
            catalog: catalog.id.clone(),
            payload: block.payload,
            schema_selections,
        }
    }
}

impl CatiaAliasRow {
    pub(super) fn from_source(row: object_graph::SurfaceAlias) -> Self {
        Self {
            id: format!("catia:outer:alias-row#{:010}", row.pos),
            byte_offset: u64_from_index(row.pos),
            lead_raw: row.lead_raw,
            tag_raw: row.tag_raw,
            flag: row.flag,
            f1: row.f1.into(),
            object_graph: None,
            object_record: None,
            design_object: None,
            f2: row.f2,
            f3: row.f3,
            group: row.group,
            canonical_surface_tag: None,
        }
    }
}

impl CatiaCatalog {
    pub(super) fn from_source(catalog: catalog::Catalog) -> Self {
        let id = format!("catia:outer:catalog#{:010}", catalog.pos);
        let mut entries = Vec::new();
        for entry in catalog.entries {
            let native_entry = CatiaCatalogEntry {
                id: format!("catia:outer:catalog-entry#{:010}", entry.pos),
                parent: id.clone(),
                ordinal: entry.ordinal,
                byte_offset: u64_from_index(entry.pos),
                value: entry.value,
            };
            entries.push(native_entry);
        }
        Self {
            id,
            byte_offset: u64_from_index(catalog.pos),
            byte_len: u64_from_index(catalog.total_len),
            entries,
        }
    }
}

pub(crate) fn native_object_graph(
    graph: object_graph::ObjectGraph,
    entity_records: Vec<entity_table::EntityRecord>,
    finjpl_segment: Option<String>,
    outer_container: Option<CatiaOuterContainerBinding>,
) -> (CatiaObjectGraph, Vec<CatiaEntityRecord>) {
    let id = format!("catia:outer:object-graph#{:010}", graph.pos);
    let mut records = Vec::new();
    for (ordinal, record) in graph.records.into_iter().enumerate() {
        let entity = entity_records.get(ordinal);
        let roles = record.roles();
        let row = CatiaObjectRecord {
            id: format!("catia:outer:object-record#{:010}", record.pos),
            parent: id.clone(),
            design_object: None,
            entity: entity.map(|entity| CatiaObjectEntity {
                record: format!("catia:outer:entity-record#{:010}", entity.pos),
                id: entity.entity_id,
            }),
            ordinal: u64_from_index(ordinal),
            byte_offset: u64_from_index(record.pos),
            byte_len: u64_from_index(record.total_len),
            lead: record.lead,
            head: record.head().to_vec(),
            inline_body: record.inline_body().map(<[u8]>::to_vec),
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
            payload: record.payload().clone(),
            repeated_reference_schema_selection: None,
            references: Vec::new(),
        };
        records.push(row);
    }
    for record in &mut records {
        record.design_object = record
            .owner_entity_id()
            .map(|owner| design_object_id(u64_from_index(graph.pos), owner));
    }
    let record_indices = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((record.entity_id()?, index)))
        .collect::<std::collections::HashMap<_, _>>();
    let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
    for index in 0..records.len() {
        let storage_ref = records[index]
            .storage
            .as_ref()
            .map(|storage| storage.reference);
        let (storage_record, storage_design_object) =
            resolved_storage_link(storage_ref, &records, &record_indices);
        let references = resolved_payload_references(
            &records[index].payload,
            &records,
            &record_indices,
            terminal_null_entity_id,
        );
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
        let reference_signature = entity.reference_signature();
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
            id: format!("catia:outer:entity-record#{:010}", entity.pos),
            object_graph: id.clone(),
            object_record: object_record.id.clone(),
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
        entities.push(row);
    }
    (
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
    )
}

impl CatiaOuterContainerBinding {
    pub(super) fn from_source(declaration: &container::OuterContainerDeclaration) -> Self {
        Self {
            data_offset: u64_from_index(declaration.data_offset),
            ordinal: declaration.ordinal,
            class_name: declaration.class_name.clone(),
            base_class: declaration.base_class.clone(),
            stream_name: declaration.stream_name.clone(),
        }
    }
}

#[cfg(test)]
mod consolidated_edge_run_tests {

    #[test]
    fn surface_only_stream_has_no_edge_runs() {
        let bytes = crate::test_support::test_a5a8::a5_surface_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
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
}
