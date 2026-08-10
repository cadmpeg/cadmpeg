// SPDX-License-Identifier: Apache-2.0
//! Inventor-native validation.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::{CadIr, Check, Finding, NativeUnknownRecord, Severity};

use crate::native::{
    ActiveCarrierRecord, ActiveCarrierRecordState, AssemblyOccurrenceRecord,
    AssemblyPlacementRecord, AssemblyRecordIssueRecord, DatabaseIssueRecord, DatabaseRecord,
    ExternalReferenceRecord, MetaSectionRecord, MetaTypeRecord, PropertyRecord,
    PropertySectionRecord, PropertySetIssueRecord, PropertySetRecord, ProteinAssetRecord,
    ProteinEntryRecord, ProteinRecord, ProteinRecordState, ProteinRejectionRecord, RevisionRecord,
    RseRecordRecord, SegmentBulkIssueRecord, SegmentBulkRecord, SegmentMetaIssueRecord,
    SegmentMetaRecord, SegmentPairRecord, SegmentRegistryRecord, StorageBandRecord,
    StructuralIssueRecord, UfrxModelStateRecord, UfrxRecord, UfrxRecordState,
    UnpairedSegmentRecord, INVENTOR_NATIVE_VERSION,
};

const ARENAS: &[&str] = &[
    "active_carrier",
    "assembly_occurrences",
    "assembly_placements",
    "assembly_record_issues",
    "body_native_keys",
    "database_issues",
    "databases",
    "external_references",
    "edge_continuities",
    "edge_ownerships",
    "face_sidedness",
    "meta_sections",
    "meta_types",
    "mesh_surface_sentinels",
    "properties",
    "property_sections",
    "property_set_issues",
    "property_sets",
    "protein",
    "protein_assets",
    "protein_entries",
    "protein_rejections",
    "revisions",
    "rse_records",
    "segment_bulk",
    "segment_bulk_issues",
    "segment_meta",
    "segment_meta_issues",
    "segment_pairs",
    "segment_registry",
    "storage_bands",
    "structural_issues",
    "tolerant_coedge_parameters",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "transform_hints",
    "ufrx",
    "ufrx_model_states",
    "unknowns",
    "unpaired_segments",
    "vertex_ownerships",
    "wire_topologies",
];

pub(crate) fn validate_native(ir: &CadIr) -> Vec<Finding> {
    let Some(namespace) = ir.native.namespace("inventor") else {
        return Vec::new();
    };
    if namespace.version != INVENTOR_NATIVE_VERSION {
        return vec![finding(
            Check::Version,
            format!(
                "unsupported Inventor native namespace version {}",
                namespace.version
            ),
            None,
        )];
    }
    let actual_arenas = namespace
        .arenas
        .keys()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let expected_arenas = ARENAS.iter().copied().collect::<HashSet<_>>();
    if actual_arenas != expected_arenas {
        let mut missing = expected_arenas
            .difference(&actual_arenas)
            .copied()
            .collect::<Vec<_>>();
        let mut unexpected = actual_arenas
            .difference(&expected_arenas)
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        unexpected.sort_unstable();
        return vec![finding(
            Check::NativeLinks,
            format!(
                "Inventor native namespace version {INVENTOR_NATIVE_VERSION} has missing arenas {missing:?} and unexpected arenas {unexpected:?}"
            ),
            None,
        )];
    }
    let data = match NativeData::load(namespace) {
        Ok(data) => data,
        Err(error) => {
            return vec![finding(
                Check::NativeLinks,
                format!(
                    "Inventor native arenas do not match namespace version {INVENTOR_NATIVE_VERSION}: {error}"
                ),
                None,
            )];
        }
    };

    let mut findings = Vec::new();
    validate_databases(&data, &mut findings);
    validate_segments(&data, &mut findings);
    validate_active_carrier(&data, &mut findings);
    unique(
        &mut findings,
        data.unknowns.iter().map(|record| record.id.0.as_str()),
        "ASM unknown-record id",
    );
    validate_properties(&data, &mut findings);
    validate_protein(&data, &mut findings);
    unique(
        &mut findings,
        data.protein_assets.iter().map(|record| record.id.as_str()),
        "Protein asset id",
    );
    validate_protein_assets(&data, &mut findings);
    validate_protein_rejections(&data, &mut findings);
    validate_protein_record_coverage(&data, &mut findings);
    validate_ufrx(&data, &mut findings);
    validate_assembly(&data, &mut findings);
    for issue in &data.structural_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor {}: {}", issue.scope, issue.detail),
            Some(issue.id.clone()),
        ));
    }
    for issue in &data.property_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor property set {:?}: {}", issue.path, issue.detail),
            Some(issue.id.clone()),
        ));
    }
    findings
}

fn validate_active_carrier(data: &NativeData, findings: &mut Vec<Finding>) {
    if data.active_carrier.len() != 1 {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor native data has {} active-carrier state records",
                data.active_carrier.len()
            ),
            None,
        ));
        return;
    }
    let carrier = &data.active_carrier[0];
    let selected_fields = carrier.segment_token.is_some()
        && carrier.record_ordinal.is_some()
        && carrier.segment_version_major.is_some()
        && matches!(carrier.family.as_deref(), Some("asm" | "acis"))
        && carrier.header_state.is_some()
        && carrier.header_kind.is_some()
        && carrier.header_value.is_some()
        && carrier.schema.is_some()
        && carrier.carrier_len.is_some_and(|length| length != 0)
        && carrier.carrier_offset.is_some()
        && carrier.carrier_sha256.is_some()
        && carrier.selected_key.is_some()
        && carrier.enabled.is_some()
        && carrier.delta_state.is_some()
        && carrier.history_reference.is_some()
        && carrier.detail.is_none();
    let empty_fields = carrier.segment_token.is_none()
        && carrier.record_ordinal.is_none()
        && carrier.segment_version_major.is_none()
        && carrier.family.is_none()
        && carrier.header_state.is_none()
        && carrier.header_kind.is_none()
        && carrier.header_value.is_none()
        && carrier.schema.is_none()
        && carrier.carrier_len.is_none()
        && carrier.carrier_offset.is_none()
        && carrier.carrier_sha256.is_none()
        && carrier.selected_key.is_none()
        && carrier.enabled.is_none()
        && carrier.delta_state.is_none()
        && carrier.history_reference.is_none();
    let valid = match carrier.state {
        ActiveCarrierRecordState::Selected => selected_fields,
        ActiveCarrierRecordState::NotApplicable => empty_fields && carrier.detail.is_none(),
        ActiveCarrierRecordState::NotExpanded => empty_fields && carrier.detail.is_none(),
        ActiveCarrierRecordState::Unavailable => empty_fields && carrier.detail.is_some(),
    };
    if !valid {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor active-carrier state fields are inconsistent".into(),
            Some(carrier.id.clone()),
        ));
    }
    if carrier.state == ActiveCarrierRecordState::Selected {
        let resolves = data.records.iter().any(|record| {
            Some(record.token.as_str()) == carrier.segment_token.as_deref()
                && Some(record.ordinal) == carrier.record_ordinal
                && record.type_id == "5c5945f6d5113313100060a6bba647b5"
        });
        if !resolves {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor active carrier does not resolve to its typed RSe record".into(),
                Some(carrier.id.clone()),
            ));
        }
    }
}

struct NativeData {
    storage_bands: Vec<StorageBandRecord>,
    databases: Vec<DatabaseRecord>,
    database_issues: Vec<DatabaseIssueRecord>,
    registry: Vec<SegmentRegistryRecord>,
    revisions: Vec<RevisionRecord>,
    pairs: Vec<SegmentPairRecord>,
    metadata: Vec<SegmentMetaRecord>,
    meta_sections: Vec<MetaSectionRecord>,
    meta_types: Vec<MetaTypeRecord>,
    metadata_issues: Vec<SegmentMetaIssueRecord>,
    bulk: Vec<SegmentBulkRecord>,
    records: Vec<RseRecordRecord>,
    bulk_issues: Vec<SegmentBulkIssueRecord>,
    unpaired: Vec<UnpairedSegmentRecord>,
    structural_issues: Vec<StructuralIssueRecord>,
    property_sets: Vec<PropertySetRecord>,
    property_sections: Vec<PropertySectionRecord>,
    properties: Vec<PropertyRecord>,
    property_issues: Vec<PropertySetIssueRecord>,
    protein: Vec<ProteinRecord>,
    protein_assets: Vec<ProteinAssetRecord>,
    protein_entries: Vec<ProteinEntryRecord>,
    protein_rejections: Vec<ProteinRejectionRecord>,
    ufrx: Vec<UfrxRecord>,
    ufrx_model_states: Vec<UfrxModelStateRecord>,
    external_references: Vec<ExternalReferenceRecord>,
    assembly_occurrences: Vec<AssemblyOccurrenceRecord>,
    assembly_placements: Vec<AssemblyPlacementRecord>,
    assembly_record_issues: Vec<AssemblyRecordIssueRecord>,
    active_carrier: Vec<ActiveCarrierRecord>,
    unknowns: Vec<NativeUnknownRecord>,
}

impl NativeData {
    fn load(
        namespace: &cadmpeg_ir::native::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::native::NativeConvertError> {
        Ok(Self {
            storage_bands: namespace.arena_as("storage_bands")?,
            databases: namespace.arena_as("databases")?,
            database_issues: namespace.arena_as("database_issues")?,
            registry: namespace.arena_as("segment_registry")?,
            revisions: namespace.arena_as("revisions")?,
            pairs: namespace.arena_as("segment_pairs")?,
            metadata: namespace.arena_as("segment_meta")?,
            meta_sections: namespace.arena_as("meta_sections")?,
            meta_types: namespace.arena_as("meta_types")?,
            metadata_issues: namespace.arena_as("segment_meta_issues")?,
            bulk: namespace.arena_as("segment_bulk")?,
            records: namespace.arena_as("rse_records")?,
            bulk_issues: namespace.arena_as("segment_bulk_issues")?,
            unpaired: namespace.arena_as("unpaired_segments")?,
            structural_issues: namespace.arena_as("structural_issues")?,
            property_sets: namespace.arena_as("property_sets")?,
            property_sections: namespace.arena_as("property_sections")?,
            properties: namespace.arena_as("properties")?,
            property_issues: namespace.arena_as("property_set_issues")?,
            protein: namespace.arena_as("protein")?,
            protein_assets: namespace.arena_as("protein_assets")?,
            protein_entries: namespace.arena_as("protein_entries")?,
            protein_rejections: namespace.arena_as("protein_rejections")?,
            ufrx: namespace.arena_as("ufrx")?,
            ufrx_model_states: namespace.arena_as("ufrx_model_states")?,
            external_references: namespace.arena_as("external_references")?,
            assembly_occurrences: namespace.arena_as("assembly_occurrences")?,
            assembly_placements: namespace.arena_as("assembly_placements")?,
            assembly_record_issues: namespace.arena_as("assembly_record_issues")?,
            active_carrier: namespace.arena_as("active_carrier")?,
            unknowns: namespace.arena_as("unknowns")?,
        })
    }
}

fn validate_databases(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.storage_bands.iter().map(|record| record.band),
        "storage band",
    );
    unique(
        findings,
        data.storage_bands
            .iter()
            .map(|record| record.database_directory_id),
        "database directory id",
    );
    let storage = data
        .storage_bands
        .iter()
        .map(|record| record.band)
        .collect::<HashSet<_>>();
    let states = data
        .databases
        .iter()
        .map(|record| record.band)
        .chain(data.database_issues.iter().map(|record| record.band))
        .collect::<Vec<_>>();
    unique(findings, states.iter().copied(), "database state band");
    if storage != states.into_iter().collect() {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor database states do not cover the storage bands exactly".into(),
            None,
        ));
    }
    for database in &data.databases {
        if database.schema != 31 {
            findings.push(finding(
                Check::Version,
                format!(
                    "Inventor database {} has schema {}",
                    database.id, database.schema
                ),
                Some(database.id.clone()),
            ));
        }
    }
    for issue in &data.database_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor database band {} is unavailable: {}",
                issue.band, issue.detail
            ),
            Some(issue.id.clone()),
        ));
    }
}

fn validate_segments(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.registry.iter().map(|record| record.ordinal),
        "registry ordinal",
    );
    unique(
        findings,
        data.registry
            .iter()
            .map(|record| record.segment_id.as_str()),
        "registry segment id",
    );
    unique(
        findings,
        data.revisions.iter().map(|record| record.ordinal),
        "revision ordinal",
    );
    unique(
        findings,
        data.pairs.iter().map(|record| record.token.as_str()),
        "segment token",
    );
    unique(
        findings,
        data.pairs.iter().map(|record| record.metadata_directory_id),
        "metadata directory id",
    );
    unique(
        findings,
        data.pairs.iter().map(|record| record.bulk_directory_id),
        "bulk directory id",
    );
    let registry_ids = data
        .registry
        .iter()
        .map(|record| record.segment_id.as_str())
        .collect::<HashSet<_>>();
    for meta in &data.metadata {
        if !registry_ids.contains(meta.segment_id.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor segment metadata {} has no registry identity",
                    meta.id
                ),
                Some(meta.id.clone()),
            ));
        }
    }
    let pair_tokens = data
        .pairs
        .iter()
        .map(|record| record.token.as_str())
        .collect::<HashSet<_>>();
    validate_segment_states(
        findings,
        &pair_tokens,
        data.metadata.iter().map(|record| record.token.as_str()),
        data.metadata_issues
            .iter()
            .map(|record| record.token.as_str()),
        "metadata",
    );
    validate_segment_states(
        findings,
        &pair_tokens,
        data.bulk.iter().map(|record| record.token.as_str()),
        data.bulk_issues.iter().map(|record| record.token.as_str()),
        "bulk",
    );
    let metadata_by_token = data
        .metadata
        .iter()
        .map(|record| (record.token.as_str(), record))
        .collect::<HashMap<_, _>>();
    unique(
        findings,
        data.meta_sections
            .iter()
            .map(|record| (record.token.as_str(), record.number)),
        "metadata section number",
    );
    unique(
        findings,
        data.meta_types
            .iter()
            .map(|record| (record.token.as_str(), record.index)),
        "metadata type index",
    );
    let sections_by_token = data.meta_sections.iter().fold(
        HashMap::<&str, HashSet<u8>>::new(),
        |mut sections, record| {
            sections
                .entry(record.token.as_str())
                .or_default()
                .insert(record.number);
            sections
        },
    );
    let types_by_token =
        data.meta_types
            .iter()
            .fold(HashMap::<&str, HashSet<u8>>::new(), |mut types, record| {
                types
                    .entry(record.token.as_str())
                    .or_default()
                    .insert(record.index);
                types
            });
    for (token, meta) in metadata_by_token {
        let expected_sections = (1_u8..=11).collect::<HashSet<_>>();
        if sections_by_token.get(token) != Some(&expected_sections)
            || types_by_token.get(token).map_or(0, HashSet::len) as u64 != meta.type_count
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor metadata tables do not match their segment summary".into(),
                Some(meta.id.clone()),
            ));
        }
    }
    let type_keys = data
        .meta_types
        .iter()
        .map(|record| (record.token.as_str(), record.index, record.type_id.as_str()))
        .collect::<HashSet<_>>();
    let record_counts =
        data.records
            .iter()
            .fold(HashMap::<&str, u64>::new(), |mut counts, record| {
                *counts.entry(record.token.as_str()).or_default() += 1;
                if !type_keys.contains(&(
                    record.token.as_str(),
                    record.type_index,
                    record.type_id.as_str(),
                )) {
                    findings.push(finding(
                        Check::NativeLinks,
                        "Inventor RSe record type does not resolve in its metadata table".into(),
                        Some(record.id.clone()),
                    ));
                }
                counts
            });
    unique(
        findings,
        data.records
            .iter()
            .map(|record| (record.token.as_str(), record.ordinal)),
        "segment record ordinal",
    );
    for bulk in &data.bulk {
        if bulk.record_state == "framed" {
            if bulk.record_count != record_counts.get(bulk.token.as_str()).copied().unwrap_or(0)
                || bulk.stream_trailer_len.is_none()
                || bulk.stream_trailer_sha256.is_none()
                || bulk.record_detail.is_some()
                || bulk.expanded_len.is_none()
                || bulk.expanded_sha256.is_none()
            {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor bulk record summary does not match its record arena".into(),
                    Some(bulk.id.clone()),
                ));
            }
        } else if bulk.record_state == "unavailable" {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor bulk records are unavailable: {}",
                    bulk.record_detail.as_deref().unwrap_or("no detail")
                ),
                Some(bulk.id.clone()),
            ));
        } else if bulk.record_state == "not_expanded" {
            if bulk.expanded_len.is_some()
                || bulk.expanded_sha256.is_some()
                || bulk.record_count != 0
                || bulk.stream_trailer_len.is_some()
                || bulk.stream_trailer_sha256.is_some()
                || bulk.record_detail.is_some()
            {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor unexpanded bulk state fields are inconsistent".into(),
                    Some(bulk.id.clone()),
                ));
            }
        } else {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor bulk record state is invalid".into(),
                Some(bulk.id.clone()),
            ));
        }
    }
    let expanded_lengths = data
        .bulk
        .iter()
        .filter_map(|bulk| {
            bulk.expanded_len
                .map(|length| (bulk.token.as_str(), length))
        })
        .collect::<HashMap<_, _>>();
    for record in &data.records {
        let end = record.payload_offset.checked_add(record.payload_len);
        if end.is_none_or(|end| {
            end > expanded_lengths
                .get(record.token.as_str())
                .copied()
                .unwrap_or(0)
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor RSe record payload range exceeds its bulk stream".into(),
                Some(record.id.clone()),
            ));
        }
    }
    for record in &data.unpaired {
        if pair_tokens.contains(record.token.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor segment is both paired and unpaired".into(),
                Some(record.id.clone()),
            ));
        }
    }
}

fn validate_segment_states<'a>(
    findings: &mut Vec<Finding>,
    pairs: &HashSet<&'a str>,
    parsed: impl IntoIterator<Item = &'a str>,
    issues: impl IntoIterator<Item = &'a str>,
    member: &str,
) {
    let states = parsed.into_iter().chain(issues).collect::<Vec<_>>();
    unique(
        findings,
        states.iter().copied(),
        &format!("segment {member} state"),
    );
    if states.into_iter().collect::<HashSet<_>>() != *pairs {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor segment {member} states do not cover paired segments exactly"),
            None,
        ));
    }
}

fn validate_properties(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.property_sets.iter().map(|record| record.path.as_str()),
        "property-set path",
    );
    let mut sections_by_set = HashMap::<&str, u64>::new();
    let mut properties_by_section = HashMap::<(&str, u32), u64>::new();
    for section in &data.property_sections {
        *sections_by_set.entry(&section.set_path).or_default() += 1;
    }
    for property in &data.properties {
        *properties_by_section
            .entry((&property.set_path, property.section_ordinal))
            .or_default() += 1;
    }
    for set in &data.property_sets {
        if sections_by_set.get(set.path.as_str()).copied().unwrap_or(0) != set.section_count {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor property-set section count does not match its section arena".into(),
                Some(set.id.clone()),
            ));
        }
    }
    let set_paths = data
        .property_sets
        .iter()
        .map(|record| record.path.as_str())
        .collect::<HashSet<_>>();
    for section in &data.property_sections {
        if !set_paths.contains(section.set_path.as_str())
            || properties_by_section
                .get(&(section.set_path.as_str(), section.ordinal))
                .copied()
                .unwrap_or(0)
                != section.property_count
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor property section does not match its set or property arena".into(),
                Some(section.id.clone()),
            ));
        }
    }
}

fn validate_protein(data: &NativeData, findings: &mut Vec<Finding>) {
    if data.protein.len() != 1 {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor native data has {} Protein state records",
                data.protein.len()
            ),
            None,
        ));
        return;
    }
    unique(
        findings,
        data.protein_entries.iter().map(|record| record.ordinal),
        "Protein entry ordinal",
    );
    let record = &data.protein[0];
    let valid = match record.state {
        ProteinRecordState::Absent => {
            record.directory_id.is_none()
                && record.declared_len.is_none()
                && record.entry_count == 0
                && record.detail.is_none()
                && data.protein_entries.is_empty()
                && data.protein_assets.is_empty()
                && data.protein_rejections.is_empty()
        }
        ProteinRecordState::Empty => {
            record.directory_id.is_some()
                && record.declared_len == Some(0)
                && record.entry_count == 0
                && record.detail.is_none()
                && data.protein_entries.is_empty()
                && data.protein_assets.is_empty()
                && data.protein_rejections.is_empty()
        }
        ProteinRecordState::Package => {
            record.directory_id.is_some()
                && record.declared_len.is_some_and(|length| length != 0)
                && record.entry_count == data.protein_entries.len() as u64
                && record.detail.is_none()
        }
        ProteinRecordState::Malformed => {
            record.directory_id.is_some()
                && record.entry_count == 0
                && record.detail.is_some()
                && data.protein_entries.is_empty()
                && data.protein_assets.is_empty()
                && data.protein_rejections.is_empty()
        }
    };
    if !valid {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor Protein state fields are inconsistent".into(),
            Some(record.id.clone()),
        ));
    }
    if record.state == ProteinRecordState::Malformed {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor Protein stream is malformed: {}",
                record.detail.as_deref().unwrap_or("no detail")
            ),
            Some(record.id.clone()),
        ));
    }
}

fn validate_protein_assets(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.protein_assets
            .iter()
            .map(|record| (record.entry_name.as_str(), record.ordinal)),
        "Protein decoded-record position",
    );
    let entry_names = data
        .protein_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<HashSet<_>>();
    for asset in &data.protein_assets {
        if asset.ordinal != asset.asset.ordinal
            || !asset.entry_name.ends_with("InstanceProperties.bin")
            || !entry_names.contains(asset.entry_name.as_str())
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor Protein asset position is inconsistent or does not resolve to a package entry"
                    .into(),
                Some(asset.id.clone()),
            ));
        }
    }
}

fn validate_protein_rejections(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.protein_rejections
            .iter()
            .map(|record| record.id.as_str()),
        "Protein rejection id",
    );
    unique(
        findings,
        data.protein_rejections
            .iter()
            .map(|record| (record.entry_name.as_str(), record.ordinal)),
        "Protein rejected-record position",
    );
    let entry_names = data
        .protein_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<HashSet<_>>();
    let accepted_positions = data
        .protein_assets
        .iter()
        .map(|record| (record.entry_name.as_str(), record.ordinal))
        .collect::<HashSet<_>>();
    for rejection in &data.protein_rejections {
        if rejection.detail.is_empty()
            || !rejection.entry_name.ends_with("InstanceProperties.bin")
            || !entry_names.contains(rejection.entry_name.as_str())
            || accepted_positions.contains(&(rejection.entry_name.as_str(), rejection.ordinal))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor Protein rejection position overlaps an asset or does not resolve to a package entry and detail"
                    .into(),
                Some(rejection.id.clone()),
            ));
        }
    }
}

fn validate_protein_record_coverage(data: &NativeData, findings: &mut Vec<Finding>) {
    let mut positions = HashMap::<&str, HashSet<u64>>::new();
    for (entry_name, ordinal) in data
        .protein_assets
        .iter()
        .map(|record| (record.entry_name.as_str(), record.ordinal))
        .chain(
            data.protein_rejections
                .iter()
                .map(|record| (record.entry_name.as_str(), record.ordinal)),
        )
    {
        positions.entry(entry_name).or_default().insert(ordinal);
    }
    for (entry_name, ordinals) in positions {
        let contiguous = ordinals
            .iter()
            .copied()
            .max()
            .and_then(|maximum| maximum.checked_add(1))
            .and_then(|count| usize::try_from(count).ok())
            == Some(ordinals.len());
        if !contiguous {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor Protein logical-record positions are not contiguous for {entry_name:?}"
                ),
                None,
            ));
        }
    }
}

fn validate_ufrx(data: &NativeData, findings: &mut Vec<Finding>) {
    if data.ufrx.len() != 1 {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor native data has {} UFRxDoc state records",
                data.ufrx.len()
            ),
            None,
        ));
        return;
    }
    unique(
        findings,
        data.external_references.iter().map(|record| record.ordinal),
        "external reference ordinal",
    );
    unique(
        findings,
        data.ufrx_model_states.iter().map(|record| record.ordinal),
        "UFRxDoc model-state ordinal",
    );
    unique(
        findings,
        data.external_references
            .iter()
            .map(|record| record.reference_id),
        "external reference id",
    );
    let record = &data.ufrx[0];
    let valid = match record.state {
        UfrxRecordState::Absent => {
            record.directory_id.is_none()
                && record.schema.is_none()
                && record.representation.is_none()
                && record.model_state_count == 0
                && record.reference_count == 0
                && record.detail.is_none()
                && data.ufrx_model_states.is_empty()
                && data.external_references.is_empty()
        }
        UfrxRecordState::ParsedPrefix => {
            record.directory_id.is_some()
                && record
                    .schema
                    .is_some_and(|schema| (11..=15).contains(&schema))
                && record.section_versions.len() >= 5
                && record.original_file_name.is_some()
                && record.caption.is_some()
                && record.model_state_count == data.ufrx_model_states.len() as u64
                && (record.schema == Some(15)) == record.representation.is_some()
                && record.reference_count == data.external_references.len() as u64
                && record.tail_sha256.is_some()
                && record.detail.is_none()
        }
        UfrxRecordState::Unsupported => {
            record.directory_id.is_some()
                && record.schema.is_some()
                && !record.section_versions.is_empty()
                && record.original_file_name.is_none()
                && record.caption.is_none()
                && record.representation.is_none()
                && record.model_state_count == 0
                && record.reference_count == 0
                && record.tail_sha256.is_some()
                && record.detail.is_some()
                && data.ufrx_model_states.is_empty()
                && data.external_references.is_empty()
        }
        UfrxRecordState::Malformed => {
            record.directory_id.is_some()
                && record.schema.is_none()
                && record.representation.is_none()
                && record.model_state_count == 0
                && record.reference_count == 0
                && record.detail.is_some()
                && data.ufrx_model_states.is_empty()
                && data.external_references.is_empty()
        }
    };
    if !valid {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor UFRxDoc state fields are inconsistent".into(),
            Some(record.id.clone()),
        ));
    }
    if record.state == UfrxRecordState::Malformed {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor UFRxDoc stream is malformed: {}",
                record.detail.as_deref().unwrap_or("no detail")
            ),
            Some(record.id.clone()),
        ));
    }
    for (expected, state) in data.ufrx_model_states.iter().enumerate() {
        if state.ordinal as usize != expected
            || state.name.is_empty()
            || state.suffix_len != 77
            || state.suffix_sha256.len() != 64
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor UFRxDoc model-state framing is inconsistent".into(),
                Some(state.id.clone()),
            ));
        }
    }
    if let Some(representation) = &record.representation {
        if representation.active_representation.is_empty()
            || representation.active_representation_kind.is_empty()
            || representation.active_model_state.is_empty()
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor UFRxDoc representation state is inconsistent".into(),
                Some(record.id.clone()),
            ));
        }
    }
    for reference in &data.external_references {
        if reference.path.is_empty()
            && reference
                .document_id
                .chars()
                .all(|character| character == '0')
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor external reference has neither a path nor a document id".into(),
                Some(reference.id.clone()),
            ));
        }
    }
}

fn validate_assembly(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.assembly_occurrences
            .iter()
            .map(|record| record.occurrence_id),
        "assembly occurrence id",
    );
    unique(
        findings,
        data.assembly_placements
            .iter()
            .map(|record| record.occurrence_id),
        "assembly placement occurrence id",
    );
    let occurrence_ids = data
        .assembly_occurrences
        .iter()
        .map(|record| record.occurrence_id)
        .collect::<HashSet<_>>();
    for placement in &data.assembly_placements {
        if !occurrence_ids.contains(&placement.occurrence_id)
            || placement.suffix_sha256.len() != 64
            || placement
                .transform
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor assembly placement does not resolve to a finite occurrence".into(),
                Some(placement.id.clone()),
            ));
        }
    }
    if !data.external_references.is_empty() {
        let declared = data
            .external_references
            .iter()
            .map(|reference| u64::from(reference.occurrence_count))
            .sum::<u64>();
        if declared != data.assembly_occurrences.len() as u64 {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor external references declare {declared} occurrences, but the typed assembly table contains {}",
                    data.assembly_occurrences.len()
                ),
                None,
            ));
        }
    }
    for issue in &data.assembly_record_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor assembly record {}:{} is unavailable: {}",
                issue.segment_token, issue.record_ordinal, issue.detail
            ),
            Some(issue.id.clone()),
        ));
    }
}

fn unique<T: Eq + std::hash::Hash>(
    findings: &mut Vec<Finding>,
    values: impl IntoIterator<Item = T>,
    field: &str,
) {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            findings.push(finding(
                Check::NativeLinks,
                format!("Inventor native data repeats a {field}"),
                None,
            ));
        }
    }
}

fn finding(check: Check, message: String, entity: Option<String>) -> Finding {
    Finding {
        check,
        severity: Severity::Error,
        message,
        entity,
    }
}
