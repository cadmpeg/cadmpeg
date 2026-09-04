use super::test_consolidated::valid_consolidated_plane_geometry;
use super::*;

pub(super) fn validate_consolidated_owner_packets(
    packets: &[CatiaConsolidatedOwnerPacket],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, packet) in packets.iter().enumerate() {
        let valid_face_node = packet.face_node.is_none_or(|face_node| {
            face_node.byte_offset.checked_add(face_node.byte_len) == Some(packet.byte_offset)
                && face_node.target.checked_add(1) == packet.payload.final_reference()
        });
        let valid_payload = match &packet.payload {
            CatiaOwnerPacketPayload::FixedNine { numeric_tail, .. } => {
                numeric_tail.header[0] == 0x84
                    && matches!(numeric_tail.header[1], 0x41 | 0xc1)
                    && numeric_tail.header[4] == 0x0d
                    && numeric_tail.lower.iter().all(|value| value.is_finite())
                    && numeric_tail.upper.iter().all(|value| value.is_finite())
                    && numeric_tail.lower[0] < numeric_tail.upper[0]
                    && numeric_tail.lower[1] < numeric_tail.upper[1]
                    && numeric_tail.bounds.iter().all(|bounds| {
                        bounds[0].is_finite() && bounds[1].is_finite() && bounds[0] < bounds[1]
                    })
            }
            CatiaOwnerPacketPayload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty()
            }
        };
        if packet.id != format!("catia:consolidated:owner-packet#{:010}", packet.byte_offset)
            || !valid_payload
            || !valid_face_node
            || index > 0 && packets[index - 1].byte_offset >= packet.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated owner packet `{}` is structurally invalid",
                packet.id
            )));
        }
    }
    Ok(())
}

pub(super) struct ConsolidatedSupportArenas<'a> {
    pub(super) circles: &'a [CatiaConsolidatedCircle],
    pub(super) cones: &'a [CatiaConsolidatedCone],
    pub(super) cylinders: &'a [CatiaConsolidatedCylinder],
    pub(super) embedded_cylinders: &'a [CatiaConsolidatedEmbeddedCylinder],
    pub(super) groups: &'a [CatiaConsolidatedGroup],
    pub(super) planes: &'a [CatiaConsolidatedPlaneCarrier],
    pub(super) spheres: &'a [CatiaConsolidatedSphere],
    pub(super) tori: &'a [CatiaConsolidatedTorus],
}

pub(super) fn validate_consolidated_edge_runs(
    runs: &[CatiaConsolidatedEdgeRun],
    pcurves: &[CatiaConsolidatedPcurve],
    supports: &ConsolidatedSupportArenas<'_>,
    nodes: &[CatiaConsolidatedEdgeNode],
    vertex_identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let pcurves = pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let circles = supports
        .circles
        .iter()
        .map(|circle| (circle.id.as_str(), circle))
        .collect::<HashMap<_, _>>();
    let circle_offsets = circles
        .values()
        .map(|circle| circle.byte_offset)
        .collect::<HashSet<_>>();
    let cone_offsets = supports
        .cones
        .iter()
        .map(|cone| cone.byte_offset)
        .collect::<HashSet<_>>();
    let sphere_offsets = supports
        .spheres
        .iter()
        .map(|sphere| sphere.byte_offset)
        .collect::<HashSet<_>>();
    let torus_offsets = supports
        .tori
        .iter()
        .map(|torus| torus.byte_offset)
        .collect::<HashSet<_>>();
    let cylinder_offsets = supports
        .cylinders
        .iter()
        .map(|cylinder| cylinder.byte_offset)
        .collect::<HashSet<_>>();
    let group_offsets = supports
        .groups
        .iter()
        .map(|group| (group.id.as_str(), group.byte_offset))
        .collect::<HashMap<_, _>>();
    let embedded_cylinder_offsets = supports
        .embedded_cylinders
        .iter()
        .filter_map(|cylinder| {
            Some((
                cylinder.byte_offset,
                *group_offsets.get(cylinder.group.as_str())?,
            ))
        })
        .collect::<HashSet<_>>();
    let plane_offsets = supports
        .planes
        .iter()
        .filter(|plane| valid_consolidated_plane_geometry(&plane.payload))
        .map(|plane| plane.byte_offset)
        .collect::<HashSet<_>>();
    let mut run_nodes = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let token_limit = 1u32.checked_shl(u32::from(node.width) * 8);
        let uses_valid = node.uses.as_ref().is_none_or(|uses| {
            node.curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses.references == [[first, second], [second, node.curve_ref]]
                })
                && node.parameter_selectors == [2, 1]
        });
        let definition_valid = node.definition.as_ref().is_none_or(|definition| {
            let token_limit = 1u32.checked_shl(u32::from(definition.width) * 8);
            let expected_data =
                crate::families::consolidated::records::consolidated_edge_definition_data(
                    definition.class,
                    &definition.payload,
                );
            node.uses.is_some()
                && matches!(definition.width, 1..=3)
                && matches!(definition.flag, 0x03 | 0x13 | 0x83)
                && matches!(definition.class, 0x23..=0x25)
                && token_limit.is_some_and(|limit| definition.header_token < limit)
                && !definition.payload.is_empty()
                && definition.byte_offset < node.byte_offset
                && definition.data == expected_data
        });
        let analytic_circle_valid = node.analytic_circle.as_ref().is_none_or(|binding| {
            let definition = node.definition.as_ref();
            let circle = circles.get(binding.circle.as_str());
            node.uses.is_some()
                && definition.is_some_and(|definition| {
                    definition.class == 0x23
                        && matches!(
                            definition.data,
                            Some(ConsolidatedEdgeDefinitionData::Scalar {
                                ref values,
                                ..
                            }) if values.len() == 8
                        )
                        && circle.is_some_and(|circle| {
                            binding.descriptor.byte_offset < circle.byte_offset
                                && circle.byte_offset < definition.byte_offset
                        })
                })
                && matches!(binding.descriptor.width, 1..=3)
                && matches!(binding.descriptor.flag, 0x03 | 0x13 | 0x83)
                && 1u32
                    .checked_shl(u32::from(binding.descriptor.width) * 8)
                    .is_some_and(|limit| binding.descriptor.header_token < limit)
                && !binding.descriptor.payload.is_empty()
        });
        let class25_descriptor_valid = node.class25_descriptor.as_ref().is_none_or(|descriptor| {
            node.uses.is_some()
                && node.definition.as_ref().is_some_and(|definition| {
                    definition.class == 0x25
                        && matches!(
                            definition.data,
                            Some(
                                ConsolidatedEdgeDefinitionData::Scalar25 { .. }
                                    | ConsolidatedEdgeDefinitionData::SegmentedScalar25 { .. }
                            )
                        )
                        && descriptor.byte_offset < definition.byte_offset
                })
                && matches!(descriptor.control, 0x02 | 0x0a)
                && matches!(descriptor.values.len(), 2 | 3)
                && descriptor.values.iter().all(|value| value.is_finite())
        });
        if node.id != format!("catia:consolidated:edge-node#{index}")
            || !matches!(node.width, 1..=3)
            || !matches!(node.flag, 0x03 | 0x13 | 0x83)
            || token_limit.is_some_and(|limit| node.header_token >= limit)
            || !uses_valid
            || !definition_valid
            || !analytic_circle_valid
            || !class25_descriptor_valid
            || index > 0 && nodes[index - 1].byte_offset >= node.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` is structurally invalid",
                node.id
            )));
        }
    }
    for (index, run) in runs.iter().enumerate() {
        let expected_id = format!("catia:consolidated:edge-run#{index}");
        let pcurve_offsets = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.byte_offset));
        let pcurve_ranges = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.range));
        let Some(node) = nodes_by_id.get(run.node.as_str()) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` references missing node `{}`",
                run.id, run.node
            )));
        };
        if !run_nodes.insert(run.node.as_str()) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` belongs to multiple runs",
                run.node
            )));
        }
        let loci_valid = run.shared_loci.as_ref().map_or_else(
            || run.endpoint_loci.is_none(),
            |loci| {
                loci.len() >= 2
                    && loci.iter().flatten().all(|value| value.is_finite())
                    && run.endpoint_loci
                        == loci
                            .first()
                            .copied()
                            .zip(loci.last().copied())
                            .map(|(first, last)| [first, last])
            },
        );
        let bindings_valid = run
            .support_bindings
            .iter()
            .flatten()
            .all(|binding| match binding {
                CatiaConsolidatedSupportBinding::Cylinder { byte_offset } => {
                    cylinder_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::EmbeddedCylinder {
                    byte_offset,
                    wrapper_byte_offset,
                } => embedded_cylinder_offsets.contains(&(*byte_offset, *wrapper_byte_offset)),
                CatiaConsolidatedSupportBinding::Circle { byte_offset } => {
                    circle_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Cone { byte_offset } => {
                    cone_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Sphere { byte_offset } => {
                    sphere_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Torus { byte_offset } => {
                    torus_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Plane { byte_offset } => {
                    plane_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::NurbsCarrier { offset, .. } => offset.is_finite(),
            });
        if run.id != expected_id
            || pcurve_offsets[0] != Some(run.byte_offset)
            || pcurve_offsets[1].is_none()
            || pcurve_offsets[0] >= pcurve_offsets[1]
            || pcurve_offsets[1].is_some_and(|offset| offset >= node.byte_offset)
            || pcurve_ranges != [Some(run.parameter_range), Some(run.parameter_range)]
            || run.parameter_range[0] >= run.parameter_range[1]
            || !run.parameter_range.iter().all(|value| value.is_finite())
            || !run.tolerance.is_finite()
            || run.tolerance < 0.0
            || node.uses.is_none()
            || !matches!(node.tail, 0x01 | 0x21)
            || !bindings_valid
            || !loci_valid
            || index > 0 && runs[index - 1].byte_offset >= run.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    let mut expected_nodes = nodes.to_vec();
    let expected_identities = consolidated_vertex_identities(&mut expected_nodes);
    if expected_nodes != nodes || expected_identities != vertex_identities {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "consolidated vertex identities disagree with edge incidence".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_native_links(
    aliases: &[CatiaAliasRow],
    catalogs: &[CatiaCatalog],
    graphs: &[CatiaObjectGraph],
    segments: &[CatiaFinjplSegment],
    value_blocks: &[CatiaValueBlock],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for catalog in catalogs {
        let count_width = if catalog.declared_count <= 0x50 { 1 } else { 2 };
        let Some(mut expected_offset) = catalog.byte_offset.checked_add(6 + count_width) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an overflowing extent",
                catalog.id
            )));
        };
        let catalog_end = catalog.byte_offset.checked_add(catalog.byte_len);
        if catalog.id != format!("catia:outer:catalog#{:010}", catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an invalid source identity",
                catalog.id
            )));
        }
        for (index, entry) in catalog.entries.iter().enumerate() {
            let next_offset = catalog
                .entries
                .get(index + 1)
                .map(|next| next.byte_offset)
                .or(catalog_end);
            let encoded_len = next_offset.and_then(|next| next.checked_sub(entry.byte_offset));
            let value_len = u64::try_from(entry.value.len()).ok();
            if entry.byte_offset != expected_offset
                || entry.id != format!("catia:outer:catalog-entry#{:010}", entry.byte_offset)
                || !encoded_len.zip(value_len).is_some_and(|(encoded, value)| {
                    matches!(encoded.checked_sub(value), Some(1 | 5))
                })
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog entry `{}` has an invalid source extent",
                    entry.id
                )));
            }
            expected_offset = next_offset.expect("validated catalog end");
        }
        if Some(expected_offset) != catalog_end {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` entries do not cover its frame",
                catalog.id
            )));
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let parsed = container::finjpl_segments(&segment.data, 0, segment.data.len());
        let expected_id = format!("catia:outer:finjpl#{index}");
        if segment.id != expected_id
            || u64::try_from(segment.data.len()).ok() != Some(segment.byte_len)
            || segment.byte_offset.checked_add(segment.byte_len).is_none()
            || !matches!(parsed.as_slice(), [parsed]
                if parsed.range == (0..segment.data.len())
                    && parsed.type_word == segment.type_word
                    && finjpl_family(parsed.kind) == segment.family
                    && parsed.name == segment.name)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "FINJPL segment `{}` has an invalid retained view",
                segment.id
            )));
        }
    }
    if segments
        .windows(2)
        .any(|pair| pair[0].byte_offset.checked_add(pair[0].byte_len) != Some(pair[1].byte_offset))
    {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "CATIA FINJPL segment extents are not contiguous".to_string(),
        ));
    }
    for block in value_blocks {
        if block.id != format!("catia:outer:value-block#{:010}", block.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid source identity",
                block.id
            )));
        }
        let Some(catalog) = catalogs.iter().find(|catalog| catalog.id == block.catalog) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` references missing catalog `{}`",
                block.id, block.catalog
            )));
        };
        if block.byte_offset.checked_add(block.byte_len) != Some(catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` is not adjacent to catalog `{}`",
                block.id, block.catalog
            )));
        }
        let payload_len = u64::try_from(block.payload.len()).ok();
        if block.declared_len.checked_add(1) != Some(block.byte_len)
            || payload_len.and_then(|len| len.checked_add(6)) != Some(block.declared_len)
            || value_block::tokenize(&block.payload) != block.fields
            || value_schema_selections(&block.id, block.byte_offset, &block.fields, catalog)
                != block.schema_selections
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid derived view",
                block.id
            )));
        }
        let mut adjacent_graphs = graphs.iter().filter(|graph| {
            graph.byte_offset.checked_add(graph.byte_len) == Some(block.byte_offset)
        });
        let adjacent_graph = adjacent_graphs.next();
        if adjacent_graphs.next().is_some()
            || block.object_graph.as_deref() != adjacent_graph.map(|graph| graph.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid adjacent graph link",
                block.id
            )));
        }
    }
    for graph in graphs {
        let Some(graph_end) = graph.byte_offset.checked_add(graph.byte_len) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an overflowing extent",
                graph.id
            )));
        };
        let mut expected_record_offset = graph.byte_offset.checked_add(6);
        if graph.id != format!("catia:outer:object-graph#{:010}", graph.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid source identity",
                graph.id
            )));
        }
        if graph.finjpl_segment.as_deref()
            != containing_finjpl_segment(graph.byte_offset, graph.byte_len, segments)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid FINJPL segment link",
                graph.id
            )));
        }
        for record in &graph.records {
            if Some(record.byte_offset) != expected_record_offset
                || record.id != format!("catia:outer:object-record#{:010}", record.byte_offset)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid source extent",
                    record.id
                )));
            }
            expected_record_offset = record.byte_offset.checked_add(record.byte_len);
        }
        if expected_record_offset != Some(graph_end) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` records do not cover its frame",
                graph.id
            )));
        }
        let mut candidates = catalogs
            .iter()
            .filter(|catalog| catalog.byte_offset == graph_end)
            .chain(
                value_blocks
                    .iter()
                    .filter(|block| block.byte_offset == graph_end)
                    .filter_map(|block| {
                        catalogs.iter().find(|catalog| catalog.id == block.catalog)
                    }),
            );
        let catalog = candidates.next();
        if candidates.next().is_some()
            || graph.catalog_byte_offset != catalog.map(|catalog| catalog.byte_offset)
            || graph.catalog.as_deref() != catalog.map(|catalog| catalog.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid schema-catalog link",
                graph.id
            )));
        }
        for record in &graph.records {
            let expected_class = catalog.and_then(|catalog| {
                usize::try_from(record.class_ref?).ok().and_then(|ordinal| {
                    catalog
                        .entries
                        .get(ordinal)
                        .map(|entry| (entry.id.as_str(), entry.value.as_str()))
                })
            });
            if record.class_entry.as_deref() != expected_class.map(|(entry, _)| entry)
                || record.class_name.as_deref() != expected_class.map(|(_, value)| value)
                || record.repeated_reference_schema_selection
                    != repeated_reference_schema_selection(
                        record.repeated_reference_suffix.as_ref(),
                        catalog,
                    )
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid schema class",
                    record.id
                )));
            }
        }
    }
    let mut primary_graphs = graphs.iter().filter(|graph| {
        graph
            .outer_container
            .as_ref()
            .is_some_and(|container| container.class_name == "CATPrtCont")
    });
    let primary_graph = match (primary_graphs.next(), primary_graphs.next()) {
        (Some(graph), None) => Some(graph),
        _ => None,
    };
    for alias in aliases {
        if alias.id != format!("catia:outer:alias-row#{:010}", alias.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has an invalid source identity",
                alias.id
            )));
        }
        let expected = usize::from(alias.entity_record_ordinal)
            .checked_sub(1)
            .and_then(|index| {
                let graph = primary_graph?;
                let record = graph.records.get(index)?;
                Some((
                    graph.id.as_str(),
                    record.id.as_str(),
                    record.design_object.as_deref(),
                ))
            });
        let valid = expected.map_or_else(
            || {
                alias.object_graph.is_none()
                    && alias.object_record.is_none()
                    && alias.design_object.is_none()
            },
            |(graph, record, object)| {
                alias.object_graph.as_deref() == Some(graph)
                    && alias.object_record.as_deref() == Some(record)
                    && alias.design_object.as_deref() == object
            },
        );
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has invalid graph, record, or design-object links",
                alias.id
            )));
        }
        if let Some(group) = &alias.group {
            if group.target_slot != (u32::from(alias.f1[2]) | ((alias.f2 & 0x00ff_ffff) << 8))
                || !object_graph::is_alias_group_storage_prefix(&group.storage_prefix)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "alias row `{}` has invalid group storage",
                    alias.id
                )));
            }
        }
    }
    Ok(())
}
