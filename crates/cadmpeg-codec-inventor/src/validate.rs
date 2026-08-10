// SPDX-License-Identifier: Apache-2.0
//! Inventor-native validation.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::{CadIr, Check, Finding, Severity};

use crate::native::{
    DatabaseIssueRecord, DatabaseRecord, ExternalReferenceRecord, PropertyRecord,
    PropertySectionRecord, PropertySetIssueRecord, PropertySetRecord, ProteinEntryRecord,
    ProteinRecord, ProteinRecordState, RevisionRecord, SegmentBulkIssueRecord, SegmentBulkRecord,
    SegmentMetaIssueRecord, SegmentMetaRecord, SegmentPairRecord, SegmentRegistryRecord,
    StorageBandRecord, StructuralIssueRecord, UfrxRecord, UfrxRecordState, UnpairedSegmentRecord,
    INVENTOR_NATIVE_VERSION,
};

const ARENAS: &[&str] = &[
    "database_issues",
    "databases",
    "external_references",
    "properties",
    "property_sections",
    "property_set_issues",
    "property_sets",
    "protein",
    "protein_entries",
    "revisions",
    "segment_bulk",
    "segment_bulk_issues",
    "segment_meta",
    "segment_meta_issues",
    "segment_pairs",
    "segment_registry",
    "storage_bands",
    "structural_issues",
    "ufrx",
    "unpaired_segments",
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
    validate_properties(&data, &mut findings);
    validate_protein(&data, &mut findings);
    validate_ufrx(&data, &mut findings);
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

struct NativeData {
    storage_bands: Vec<StorageBandRecord>,
    databases: Vec<DatabaseRecord>,
    database_issues: Vec<DatabaseIssueRecord>,
    registry: Vec<SegmentRegistryRecord>,
    revisions: Vec<RevisionRecord>,
    pairs: Vec<SegmentPairRecord>,
    metadata: Vec<SegmentMetaRecord>,
    metadata_issues: Vec<SegmentMetaIssueRecord>,
    bulk: Vec<SegmentBulkRecord>,
    bulk_issues: Vec<SegmentBulkIssueRecord>,
    unpaired: Vec<UnpairedSegmentRecord>,
    structural_issues: Vec<StructuralIssueRecord>,
    property_sets: Vec<PropertySetRecord>,
    property_sections: Vec<PropertySectionRecord>,
    properties: Vec<PropertyRecord>,
    property_issues: Vec<PropertySetIssueRecord>,
    protein: Vec<ProteinRecord>,
    protein_entries: Vec<ProteinEntryRecord>,
    ufrx: Vec<UfrxRecord>,
    external_references: Vec<ExternalReferenceRecord>,
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
            metadata_issues: namespace.arena_as("segment_meta_issues")?,
            bulk: namespace.arena_as("segment_bulk")?,
            bulk_issues: namespace.arena_as("segment_bulk_issues")?,
            unpaired: namespace.arena_as("unpaired_segments")?,
            structural_issues: namespace.arena_as("structural_issues")?,
            property_sets: namespace.arena_as("property_sets")?,
            property_sections: namespace.arena_as("property_sections")?,
            properties: namespace.arena_as("properties")?,
            property_issues: namespace.arena_as("property_set_issues")?,
            protein: namespace.arena_as("protein")?,
            protein_entries: namespace.arena_as("protein_entries")?,
            ufrx: namespace.arena_as("ufrx")?,
            external_references: namespace.arena_as("external_references")?,
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
        }
        ProteinRecordState::Empty => {
            record.directory_id.is_some()
                && record.declared_len == Some(0)
                && record.entry_count == 0
                && record.detail.is_none()
                && data.protein_entries.is_empty()
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
                && record.reference_count == 0
                && record.detail.is_none()
                && data.external_references.is_empty()
        }
        UfrxRecordState::ParsedPrefix => {
            record.directory_id.is_some()
                && record.schema == Some(11)
                && record.section_versions.len() >= 5
                && record.original_file_name.is_some()
                && record.caption.is_some()
                && record.reference_count == data.external_references.len() as u64
                && record.tail_sha256.is_some()
                && record.detail.is_none()
        }
        UfrxRecordState::Malformed => {
            record.directory_id.is_some()
                && record.schema.is_none()
                && record.reference_count == 0
                && record.detail.is_some()
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
