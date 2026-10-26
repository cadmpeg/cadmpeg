// SPDX-License-Identifier: Apache-2.0
//! External-reference (`XRef`) and document-type entries of a `.f3d` container
//! ([spec §1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#12-stored-property-and-configuration-entries),
//! [§1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
//!
//! [`decode`] parses the top-level `RedirectionsStream.dat` table into
//! [`XrefDesign`] and [`XrefReference`] records. [`docstruct`] parses the JSON
//! form of `Properties.dat`. [`is_assembly`] classifies a BREP-less document
//! whose model is the placement of its XREF targets.

use cadmpeg_core::container::ContainerRole;

use std::collections::HashSet;

use serde::Deserialize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference};

use crate::bytes::utf16::Utf16View;
use crate::bytes::{
    is_guid_relaxed, lp_ascii_filtered_view, lp_ascii_strict, lp_utf16_bounded,
    lp_utf16_bounded_view, take_owned_reference, take_reference, Reference,
};
use crate::container::ContainerScan;
use crate::design::decode::components::ROOT_INSTANCE_NAME;
use crate::layout::component_insert_grouped_identity_carrier as grouped_identity_layout;
use crate::layout::root_instance_prefix as root_instance;
use crate::records::{
    feature::{assembly_features::DesignComponentInsertConstruction, scope::DesignParameterScope},
    xref::{XrefDesign, XrefLocalOwner, XrefPlacementSource, XrefReference},
};

/// Top-level container entry holding the external-reference table.
const REDIRECTIONS_ENTRY: &str = "RedirectionsStream.dat";
/// Top-level container entry holding the document-properties slot.
const PROPERTIES_ENTRY: &str = "Properties.dat";
/// Top-level JSON document carrying component-reference extension data.
const COMPONENT_REFERENCE_ENTRY: &str = "ComponentReferenceData.json";

/// Stable type-table identity of a Design occurrence-placement record.
const OCCURRENCE_PLACEMENT_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

/// The parsed `RedirectionsStream.dat` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct XrefTable {
    /// Design entries in source order; entry 0 is the document itself.
    pub(crate) designs: Vec<XrefDesign>,
    /// Outgoing XREF placements in source order; empty for a leaf document.
    pub(crate) references: Vec<XrefReference>,
    /// Type-admitted placements that do not close under their generation grammar.
    pub(crate) placement_failures: Vec<OccurrencePlacementFailure>,
    /// `(XREF ordinal, placement-record count)` pairs whose structured
    /// placements were superseded by scope-bound Component Insert carriers.
    pub(crate) placement_overrides: Vec<PlacementOverride>,
    pub(crate) placement_metadata_missing: bool,
}

/// One XREF ordinal whose structured placements were superseded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacementOverride {
    pub(crate) ordinal: u32,
    pub(crate) count: usize,
}

/// The `docstruct` document-type declaration of a JSON `Properties.dat`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Docstruct {
    /// Document type: `assembly-design` or `part-design`.
    pub(crate) doc_type: String,
    /// Document subtype, e.g. `assembly-standard` or `part-sheetmetal`.
    pub(crate) subtype: Option<String>,
}

#[derive(Deserialize)]
struct RedirectionsJson {
    name: String,
    #[serde(rename = "schema-version")]
    schema_version: u32,
    designs: Vec<DesignJson>,
    /// `{}` in a leaf document, an array in a referencing document.
    references: ReferencesJson,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum ReferencesJson {
    List(Vec<ReferenceJson>),
    /// The only legal non-array form is the empty leaf object `{}`.
    Leaf {},
}

#[derive(Deserialize)]
struct DesignJson {
    #[serde(rename = "file-version")]
    file_version: i64,
    #[serde(rename = "targetFileName")]
    target_file_name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "lineageUrn")]
    lineage_urn: String,
    #[serde(rename = "versionUrn")]
    version_urn: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ReferenceJson {
    #[serde(rename = "XREF")]
    Xref {
        from: String,
        #[serde(rename = "relativePath")]
        relative_path: String,
        properties: Vec<PropertyJson>,
    },
}

#[derive(Deserialize)]
enum PropertyJson {
    #[serde(rename = "neutronRole")]
    Role(StringProperty),
    #[serde(rename = "neutronData")]
    Data(StringProperty),
}

#[derive(Deserialize)]
#[serde(tag = "dataType", deny_unknown_fields)]
enum StringProperty {
    #[serde(rename = "STRING")]
    String { value: String },
}

impl ReferenceJson {
    fn into_record(self, ordinal: usize) -> Result<XrefReference, CodecError> {
        let Self::Xref {
            from,
            relative_path,
            properties,
        } = self;
        let from = required_text(from, format_args!("references[{ordinal}].from"))?;
        let relative_path = required_text(
            relative_path,
            format_args!("references[{ordinal}].relativePath"),
        )?;
        let mut role = None;
        let mut data = None;
        for property in properties {
            let (name, value, slot) = match property {
                PropertyJson::Role(StringProperty::String { value }) => {
                    ("neutronRole", value, &mut role)
                }
                PropertyJson::Data(StringProperty::String { value }) => {
                    ("neutronData", value, &mut data)
                }
            };
            if slot.replace(value).is_some() {
                return Err(redirections_error(format_args!(
                    "references[{ordinal}].properties repeats {name}"
                )));
            }
        }
        let neutron_role = role.ok_or_else(|| {
            redirections_error(format_args!(
                "references[{ordinal}].properties is missing neutronRole"
            ))
        })?;
        let neutron_role = required_text(
            neutron_role,
            format_args!("references[{ordinal}].properties.neutronRole.value"),
        )?;
        let neutron_data = data.ok_or_else(|| {
            redirections_error(format_args!(
                "references[{ordinal}].properties is missing neutronData"
            ))
        })?;
        let id = format!("f3d:xref:reference#{ordinal}");
        Ok(XrefReference {
            id,
            ordinal: ordinal_at(ordinal)?,
            occurrence_ordinal: 0,
            placement_source: None,
            from,
            relative_path,
            neutron_role,
            neutron_data,
            transform: None,
        })
    }
}

fn redirections_error(message: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("{REDIRECTIONS_ENTRY}: {message}"))
}

fn required_text(
    value: String,
    field: impl std::fmt::Display,
) -> Result<crate::records::xref::RequiredXrefText, CodecError> {
    value
        .try_into()
        .map_err(|_| redirections_error(format_args!("{field} must be non-empty")))
}

/// Validate `ComponentReferenceData.json`, if present.
///
/// The stable grammar is an open top-level JSON object. Member names and values
/// are application-defined extension data: the codec validates the envelope,
/// preserves the original ZIP entry byte-for-byte, and performs no semantic
/// projection without a separately identified field contract.
fn validate_component_reference_data(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(), CodecError> {
    if !scan
        .entries
        .iter()
        .any(|entry| entry.name == COMPONENT_REFERENCE_ENTRY)
    {
        return Ok(());
    }
    parse_component_reference_data(ctx, scan.entry_bytes(COMPONENT_REFERENCE_ENTRY)?)?;
    Ok(())
}

fn parse_component_reference_data(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<serde_json::Value, CodecError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let value = ctx
        .parse_json_value(text, "parse F3D component reference JSON")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    if !value.is_object() {
        return Err(CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} must contain a top-level JSON object"
        )));
    }
    Ok(value)
}

/// Parse the top-level `RedirectionsStream.dat` table, if present.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<XrefTable>, CodecError> {
    decode_with_scopes(ctx, scan, &[])
}

/// Parse the external-reference table and bind its occurrences to exact
/// `Component Insert` constructions already decoded from the Design streams.
pub(crate) fn decode_with_scopes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Option<XrefTable>, CodecError> {
    // Validate the extension document independently of whether a redirections
    // table is present. Its members are application-defined and retained by
    // source fidelity, so no field-level semantics are guessed here.
    validate_component_reference_data(ctx, scan)?;
    let bytes = match scan.entry_bytes(REDIRECTIONS_ENTRY) {
        Ok(bytes) => bytes,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let mut table = parse(ctx, bytes)?;
    bind_occurrences(scan, &mut table, scopes)?;
    Ok(Some(table))
}

/// The `u32` ordinal for an enumerated position, or a malformed-input error.
fn ordinal_at(position: usize) -> Result<u32, CodecError> {
    u32::try_from(position).map_err(|_| {
        CodecError::malformed(format_args!(
            "F3D external-reference ordinal {position} exceeds u32"
        ))
    })
}

/// Parse `RedirectionsStream.dat` bytes into an [`XrefTable`].
fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<XrefTable, CodecError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let parsed: RedirectionsJson = ctx
        .parse_json(text, "parse F3D redirections JSON")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    if parsed.name != "RedirectionsStream" {
        return Err(redirections_error(format_args!(
            "name must be RedirectionsStream"
        )));
    }
    if parsed.schema_version != 0 {
        let message = format!(
            "F3D {REDIRECTIONS_ENTRY}: unsupported schema-version {}",
            parsed.schema_version
        );
        return Err(CodecError::NotImplemented(message));
    }
    if parsed.designs.is_empty() {
        return Err(redirections_error(
            "designs must contain the document entry",
        ));
    }
    let mut designs = Vec::new();
    for (ordinal, design) in parsed.designs.into_iter().enumerate() {
        let target_file_name = required_text(
            design.target_file_name,
            format_args!("designs[{ordinal}].targetFileName"),
        )?;
        let id = format!("f3d:xref:design#{ordinal}");

        designs.push(XrefDesign {
            id,
            ordinal: ordinal_at(ordinal)?,
            file_version: design.file_version,
            target_file_name,
            display_name: design.display_name,
            lineage_urn: design.lineage_urn,
            version_urn: design.version_urn,
        });
    }
    let references = match parsed.references {
        ReferencesJson::List(references) if !references.is_empty() => references,
        ReferencesJson::List(_) => {
            return Err(redirections_error(
                "references must be {} when the document has no outgoing XREF",
            ));
        }
        ReferencesJson::Leaf {} => Vec::new(),
    };
    let mut admitted_references = Vec::new();
    for (ordinal, reference) in references.into_iter().enumerate() {
        admitted_references.push(reference.into_record(ordinal)?);
    }
    Ok(XrefTable {
        designs,
        references: admitted_references,
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
        placement_metadata_missing: false,
    })
}

/// Parse the `docstruct` declaration of a non-empty `Properties.dat`, if
/// present. The entry is a `u32` payload byte count followed by that many
/// JSON bytes; count 0 is the empty slot and carries no declaration.
pub(crate) fn docstruct(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<Docstruct>, CodecError> {
    let bytes = match scan.entry_bytes(PROPERTIES_ENTRY) {
        Ok(bytes) => bytes,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let mut view = View::over_retained(bytes);
    let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    let Some(payload) = view.take(count) else {
        return Ok(None);
    };

    let Ok(text) = std::str::from_utf8(payload) else {
        return Ok(None);
    };
    let value = match ctx.parse_json_value(text, "parse F3D properties JSON") {
        Ok(value) => value,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let Some(docstruct) = value.get("docstruct") else {
        return Ok(None);
    };
    let Some(doc_type) = docstruct.get("type").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let doc_type = doc_type.to_owned();
    let subtype = docstruct
        .get("subtype")
        .and_then(serde_json::Value::as_str)
        .map(std::borrow::ToOwned::to_owned);
    Ok(Some(Docstruct { doc_type, subtype }))
}

/// An outgoing XREF document with explicitly empty model-body maps is an
/// assembly. Without that map state, require the document-type declaration
/// and absence of BREP streams.
pub(crate) fn is_assembly(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: Option<&XrefTable>,
    model_body_maps_empty: bool,
) -> Result<bool, CodecError> {
    if table.is_none_or(|table| table.references.is_empty()) {
        return Ok(false);
    }
    if model_body_maps_empty {
        return Ok(true);
    }
    if crate::container::design_breps(scan).next().is_some() {
        return Ok(false);
    }
    Ok(docstruct(ctx, scan)?.is_some_and(|docstruct| docstruct.doc_type == "assembly-design"))
}

/// The lineage/version design entry for one reference: the entry whose
/// `target_file_name` equals the reference's `relative_path`.
pub(crate) fn design_for<'a>(
    table: &'a XrefTable,
    reference: &XrefReference,
) -> Option<&'a XrefDesign> {
    table
        .designs
        .iter()
        .find(|design| design.target_file_name == reference.relative_path)
}

/// External product structure projected into local component instance paths.
pub(crate) struct ExternalOccurrenceProjection {
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Repeat external placements below each instance of their local owner.
pub(crate) fn project_occurrences(
    table: &XrefTable,
    sources: &crate::design::components::LocalComponentSources<'_>,
    local: &[Occurrence],
) -> Result<ExternalOccurrenceProjection, CodecError> {
    use crate::loss::F3dLossCode;
    use std::collections::BTreeMap;

    let carriers = sources
        .occurrences
        .iter()
        .map(|carrier| (carrier.id.as_str(), carrier))
        .collect::<BTreeMap<_, _>>();
    let mut instances = BTreeMap::<(&str, u64), Vec<_>>::new();
    let mut ordinals = BTreeMap::new();
    for occurrence in local {
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            let next = u64::from(occurrence.ordinal) + 1;
            let ordinal = ordinals.entry(parent).or_insert(0);
            *ordinal = (*ordinal).max(next);
        }
        let Some(carrier) = occurrence
            .native_ref
            .as_deref()
            .and_then(|id| carriers.get(id))
        else {
            continue;
        };
        let Some(stream) = crate::ids::native_stream(&carrier.id) else {
            continue;
        };
        let Some(definition) = sources
            .components
            .get(stream)
            .and_then(|components| components.roots.get(&carrier.component_record_index))
        else {
            continue;
        };
        instances
            .entry((stream, definition.root_entity))
            .or_default()
            .push(&occurrence.id);
    }
    let mut projection = ExternalOccurrenceProjection {
        occurrences: Vec::new(),
        losses: Vec::new(),
    };
    for (reference, occurrence) in table.references.iter().zip(root_occurrences(table)?) {
        let owner = reference
            .placement_source
            .as_ref()
            .and_then(|source| source.owner.as_ref().map(|owner| (source, owner)));
        let Some((source, owner)) = owner else {
            projection.occurrences.push(occurrence);
            continue;
        };
        let components = sources.components.get(&source.stream);
        let root = components.and_then(|components| {
            components
                .root_component
                .and_then(|record| components.roots.get(&record))
        });
        if root.is_some_and(|root| root.root_entity == owner.target) {
            projection.occurrences.push(occurrence);
            continue;
        }
        let unique = components.is_some_and(|components| {
            components
                .roots
                .values()
                .filter(|definition| definition.root_entity == owner.target)
                .count()
                == 1
        });
        let parents = instances
            .get(&(source.stream.as_str(), owner.target))
            .filter(|_| unique);
        let Some(parents) = parents else {
            projection.losses.push(F3dLossCode::XrefOwnerUnresolved.note(format!(
                "external occurrence carrier {} in {}: local owner {} has no unique instantiated component; placement remains at the document root",
                source.carrier_record_index, source.stream, owner.target
            )));
            projection.occurrences.push(occurrence);
            continue;
        };
        for parent in parents {
            let used = cadmpeg_core::decode::u64_from_index(local.len())
                + cadmpeg_core::decode::u64_from_index(projection.occurrences.len());
            if used >= sources.limits.max_entities {
                return Err(cadmpeg_core::decode::ResourceLimit {
                    dimension: cadmpeg_core::decode::ResourceDimension::Entities,
                    reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
                    limit: sources.limits.max_entities,
                    used,
                    additional: 1,
                    operation: "expand F3D external occurrences",
                }
                .into());
            }
            let mut child = occurrence.clone();
            child.id = crate::ids::nested_xref_occurrence_id(
                parent,
                reference.ordinal,
                reference.occurrence_ordinal,
            );
            child.parent = OccurrenceParent::Occurrence {
                occurrence: (*parent).clone(),
            };
            let ordinal = ordinals.entry(parent).or_insert(0);
            child.ordinal = u32::try_from(*ordinal).map_err(|_| {
                CodecError::malformed("external occurrence sibling ordinal exceeds u32::MAX")
            })?;
            *ordinal += 1;
            projection.occurrences.push(child);
        }
    }
    Ok(projection)
}

/// Construct document-frame placements before local owner expansion.
fn root_occurrences(table: &XrefTable) -> Result<Vec<Occurrence>, cadmpeg_core::CodecError> {
    let mut occurrences = Vec::new();
    for (ordinal, reference) in table.references.iter().enumerate() {
        let path = reference.relative_path.as_str().to_owned();
        let transform = reference.transform.map_or(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            crate::records::xref::XrefPlacementTransform::rows,
        );
        occurrences.push(Occurrence {
            id: crate::ids::neutral_xref_occurrence_id(
                reference.ordinal,
                reference.occurrence_ordinal,
            ),
            prototype: PrototypeReference::External {
                document: ExternalDocument::path(path),
                object: None,
            },
            parent: OccurrenceParent::Root {},
            ordinal: ordinal_at(ordinal)?,
            transform: crate::design::components::neutral_transform(transform)?,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(reference.id.as_str().to_owned()),
        });
    }
    Ok(occurrences)
}

/// Return the occurrence bound to a construction carrier in its Design stream.
pub(crate) fn component_insert_reference<'a>(
    references: &'a [XrefReference],
    stream: &str,
    construction: &DesignComponentInsertConstruction,
) -> Option<&'a XrefReference> {
    let mut matches = references.iter().filter(|reference| {
        reference.neutron_role.as_str() == construction.neutron_role
            && reference.placement_source.as_ref().is_some_and(|source| {
                source.stream == stream
                    && source.carrier_record_index == u64::from(construction.carrier_record_index)
            })
            && reference
                .transform
                .map(crate::records::xref::XrefPlacementTransform::rows)
                == Some((*construction.transform()).into())
    });
    let reference = matches.next()?;
    matches.next().is_none().then_some(reference)
}

/// Return the unique projected instance of one native external placement.
pub(crate) fn unique_occurrence<'a>(
    reference: &XrefReference,
    occurrences: &'a [Occurrence],
) -> Option<&'a Occurrence> {
    let mut matches = occurrences.iter().filter(|occurrence| {
        matches!(occurrence.prototype, PrototypeReference::External { .. })
            && occurrence.native_ref.as_deref() == Some(reference.id.as_str())
    });
    let occurrence = matches.next()?;
    matches.next().is_none().then_some(occurrence)
}

/// Resolve exact `Component Insert` history scopes to their placed occurrences.
pub(crate) fn bind_component_insert_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    table: &XrefTable,
    occurrences: &[Occurrence],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let Some(construction) = scope.component_insert_construction() else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(reference) = component_insert_reference(&table.references, stream, construction)
        else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            let Some(occurrence) = unique_occurrence(reference, occurrences) else {
                losses.push(
                    crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved.note(
                        "an external component feature has no unique expanded occurrence path",
                    ),
                );
                continue;
            };
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    FeatureOperation::InsertComponent {
                        occurrence: occurrence.id.clone(),
                    },
                ));
        }
    }
    losses
}

/// Expand container references through their occurrence records in the active
/// Design `BulkStream` and retain each occurrence-local placement matrix.
fn bind_occurrences(
    scan: &ContainerScan,
    table: &mut XrefTable,
    scopes: &[DesignParameterScope],
) -> Result<(), CodecError> {
    let mut streams = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let meta_entry = entry
            .name
            .strip_suffix("BulkStream.dat")
            .and_then(|prefix| {
                scan.entries
                    .iter()
                    .find(|candidate| candidate.name.strip_prefix(prefix) == Some("MetaStream.dat"))
            });
        let (serializer_magic, headers) = if let Some(meta_entry) = meta_entry {
            let meta = scan.parsed_metastream(&meta_entry.name)?;
            let meta_bytes = scan.entry_bytes(&meta_entry.name)?;
            (
                Some(crate::metastream::serializer_magic(
                    meta_bytes,
                    &meta_entry.name,
                )?),
                typed_occurrence_placement_records(bytes, &meta)?,
            )
        } else {
            table.placement_metadata_missing = true;
            (None, Vec::new())
        };
        let (placements, failures) =
            occurrence_placements_with_failures(bytes, &headers, serializer_magic, None)?;

        streams.push((placements, failures, crate::ids::native_scope(&entry.name)));
    }
    let mut expanded = Vec::new();
    let placement_failures = streams
        .iter()
        .flat_map(|(_, failures, _)| failures.iter().cloned())
        .collect();
    let mut placement_overrides = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (placements, _, stream) in &streams {
            let direct = select_component_insert_transforms(
                scopes.iter().filter_map(|scope| {
                    let stream = crate::ids::native_stream(&scope.id)?;
                    let construction = scope.component_insert_construction()?;
                    Some((stream, construction))
                }),
                stream,
                &reference.neutron_role,
            );
            let structured_count =
                superseded_placement_count(&direct, placements, &reference.neutron_role);
            if !direct.is_empty() && structured_count != 0 {
                placement_overrides.push(PlacementOverride {
                    ordinal: reference.ordinal,
                    count: structured_count,
                });
            }
            let selected =
                occurrence_transforms_with_precedence(direct, placements, &reference.neutron_role);

            occurrences.extend(
                selected
                    .into_iter()
                    .map(|placement| (placement, stream.clone())),
            );
        }
        if occurrences.is_empty() {
            expanded.push(reference.clone());
            continue;
        }
        for (occurrence_ordinal, (placement, stream)) in occurrences.into_iter().enumerate() {
            let occurrence_id = format!(
                "f3d:xref:reference#{}-occurrence-{occurrence_ordinal}",
                reference.ordinal
            );
            expanded.push(XrefReference {
                id: occurrence_id,
                occurrence_ordinal: ordinal_at(occurrence_ordinal)?,
                placement_source: Some(XrefPlacementSource {
                    stream,
                    carrier_record_index: placement.carrier_record_index,
                    owner: placement.owner,
                }),
                transform: placement
                    .transform
                    .map(crate::records::xref::XrefPlacementTransform::try_from)
                    .transpose()
                    .map_err(CodecError::NotImplemented)?,
                ..reference.clone()
            });
        }
    }
    table.references = expanded;
    table.placement_failures = placement_failures;
    table.placement_overrides = placement_overrides;
    Ok(())
}

/// Select the exact `Component Insert` constructions for one Design stream
/// and external-reference role. The construction parser has already joined
/// each role to its scope-owned relation record and verified its carrier
/// transform, so the class tag is not an admission discriminator here.
fn select_component_insert_transforms<'a, I>(
    constructions: I,
    stream: &str,
    role: &str,
) -> Vec<(u64, [[f64; 4]; 4])>
where
    I: IntoIterator<Item = (&'a str, &'a DesignComponentInsertConstruction)>,
{
    constructions
        .into_iter()
        .filter(|(construction_stream, construction)| {
            *construction_stream == stream && construction.neutron_role == role
        })
        .map(|(_, construction)| {
            (
                u64::from(construction.carrier_record_index),
                construction.transform().rows(),
            )
        })
        .collect::<Vec<_>>()
}

/// Apply a scope transform once to its carrier and keep the other placements.
fn occurrence_transforms_with_precedence(
    direct: Vec<(u64, [[f64; 4]; 4])>,
    placements: &[OccurrencePlacement],
    role: &str,
) -> Vec<OccurrencePlacement> {
    let mut selected = Vec::new();
    let mut overridden = HashSet::new();
    for placement in placements
        .iter()
        .filter(|placement| placement.link_names.iter().any(|name| name == role))
    {
        let mut placement = placement.clone();
        if let Some((carrier, transform)) = direct
            .iter()
            .find(|(carrier, _)| *carrier == placement.carrier_record_index)
        {
            if !overridden.insert(*carrier) {
                continue;
            }
            placement.transform = Some(*transform);
        }
        selected.push(placement);
    }
    for (carrier_record_index, transform) in direct {
        if overridden.insert(carrier_record_index) {
            selected.push(OccurrencePlacement {
                carrier_record_index,
                owner: None,
                link_names: vec![role.to_owned()],
                transform: Some(transform),
            });
        }
    }
    selected
}

/// Count placements whose carrier also has a scope-bound transform.
fn superseded_placement_count(
    direct: &[(u64, [[f64; 4]; 4])],
    placements: &[OccurrencePlacement],
    role: &str,
) -> usize {
    placements
        .iter()
        .filter(|placement| {
            placement.link_names.iter().any(|name| name == role)
                && direct
                    .iter()
                    .any(|(carrier, _)| *carrier == placement.carrier_record_index)
        })
        .count()
}

/// Return the `BulkStream` offsets whose type-table identity is the stable
/// occurrence-placement type. Dynamic class tags and record shape are not
/// sufficient because unrelated component records can share that shape.
fn typed_occurrence_placement_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<IndexedRecord>, CodecError> {
    let mut placement_entities = std::collections::HashMap::new();
    for (ordinal, design_type) in meta.types.iter().enumerate().filter(|(_, design_type)| {
        design_type.module == "Component"
            && design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)
    }) {
        for &entity in design_type.entities.values() {
            placement_entities.insert(entity, ordinal);
        }
    }
    let mut records = Vec::new();
    for frame in crate::metastream::primary_record_frames(meta, bytes.len())? {
        let Some(&ordinal) = placement_entities.get(&frame.entity_id) else {
            continue;
        };
        let header = bytes.get(frame.start..frame.member_end).ok_or_else(|| {
            CodecError::malformed("F3D occurrence-placement index is outside its BulkStream")
        })?;
        let class_matches = lp_ascii_strict(header, 0, 3..=3)
            .and_then(|(tag, _)| tag.parse::<usize>().ok())
            .and_then(|tag| tag.checked_sub(256))
            == Some(ordinal);
        if !class_matches || View::u64_le_at(header, 7) != Some(frame.entity_id) {
            return Err(CodecError::malformed(
                "F3D occurrence-placement header disagrees with its registered primary index",
            ));
        }
        records.push(IndexedRecord {
            offset: frame.start,
            end: frame.member_end,
        });
    }
    Ok(records)
}

#[derive(Debug)]
struct IndexedRecord {
    offset: usize,
    end: usize,
}

/// The transforms of every placement whose target path carries `role`, in
/// record order. One occurrence-placement record is one occurrence.
#[cfg(test)]
fn occurrence_transforms(
    placements: &[OccurrencePlacement],
    role: &str,
) -> Vec<Option<[[f64; 4]; 4]>> {
    placements
        .iter()
        .filter(|placement| placement.link_names.iter().any(|name| name == role))
        .map(|placement| placement.transform)
        .collect::<Vec<_>>()
}

#[cfg(test)]
fn indexed_records(bytes: &[u8]) -> Vec<IndexedRecord> {
    let mut records: Vec<IndexedRecord> = Vec::new();
    for at in bytes
        .len()
        .checked_sub(11)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        if View::u32_le_at(bytes, at) != Some(3) {
            continue;
        }
        let Some(tag) = bytes.get(at + 4..at + 7) else {
            continue;
        };
        if !tag.iter().all(u8::is_ascii_digit) || bytes.get(at + 7..at + 15).is_none() {
            continue;
        }
        if let Some(previous) = records.last_mut() {
            previous.end = at;
        }

        records.push(IndexedRecord {
            offset: at,
            end: bytes.len(),
        });
    }
    records
}

/// One occurrence-placement record: the target path it names and the transform
/// it places that path at
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)
/// "**Placement.**").
#[derive(Debug, Clone, PartialEq)]
struct OccurrencePlacement {
    carrier_record_index: u64,
    owner: Option<XrefLocalOwner>,
    /// Cross-document link names carried by the path elements, in path order.
    /// The role-bearing element is not necessarily the first.
    link_names: Vec<String>,
    /// `None` is the stored identity form, which carries no matrix.
    transform: Option<[[f64; 4]; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OccurrencePlacementFailure {
    pub(crate) carrier_record_index: Option<u64>,
    /// Link names recovered from the valid target-path prefix.
    pub(crate) link_names: Vec<String>,
}

/// Parse every indexed record that closes exactly under the occurrence-placement
/// grammar, in record order.
#[cfg(test)]
fn occurrence_placements(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_filtered(bytes, records, serializer_magic, None)
}

/// Parse occurrence-placement records, optionally restricted by the
/// `MetaStream` type-table admission set.
#[cfg(test)]
fn occurrence_placements_filtered(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_with_failures(bytes, records, serializer_magic, typed_offsets)
        .expect("test placement parse")
        .0
}

/// Parse admitted placement records and retain role names from records whose
/// target path is valid but whose remaining generation-specific payload is not.
fn occurrence_placements_with_failures(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Result<(Vec<OccurrencePlacement>, Vec<OccurrencePlacementFailure>), CodecError> {
    let mut placements = Vec::new();
    let mut failures = Vec::new();
    for record in records
        .iter()
        .filter(|record| typed_offsets.is_none_or(|offsets| offsets.contains(&record.offset)))
    {
        let Some(body) = bytes.get(record.offset..record.end) else {
            continue;
        };
        if lp_ascii_strict(body, root_instance::INSTANCE_NAME_LENGTH, 0..=256)
            .is_some_and(|(name, _)| name.as_bytes() == ROOT_INSTANCE_NAME)
            || occurrence_path_elements(body).is_some_and(|(names, _, _)| names.is_empty())
        {
            continue;
        }
        if let Some(placement) = occurrence_placement(body, serializer_magic)? {
            placements.push(placement);
        } else {
            let link_names = occurrence_path(body)
                .map(|(names, _)| names)
                .or_else(|| legacy_occurrence_role(body).map(|role| vec![role.to_string()]))
                .unwrap_or_default();
            failures.push(OccurrencePlacementFailure {
                carrier_record_index: View::u64_le_at(body, 7),
                link_names,
            });
        }
    }
    Ok((placements, failures))
}

/// Parse one record body, header included, requiring the member sequence to end
/// exactly at the record end.
fn occurrence_placement(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Result<Option<OccurrencePlacement>, CodecError> {
    let Some(carrier_record_index) = View::u64_le_at(body, 7) else {
        return Ok(None);
    };
    if let Some((role, transform)) = legacy_occurrence_placement(body) {
        let role = role.to_string();
        let link_names = [role].into_iter().collect::<Vec<_>>();
        return Ok(Some(OccurrencePlacement {
            carrier_record_index,
            owner: None,
            link_names,
            transform,
        }));
    }
    if let Some(placement) = repeated_target_occurrence_placement(body)? {
        return Ok(Some(placement));
    }
    if let Some(placement) = modern_occurrence_placement(body, serializer_magic) {
        return Ok(Some(placement));
    }
    let Some(record_index) = View::u32_le_at(body, 7) else {
        return Ok(None);
    };
    let Some((link_name, _)) = grouped_component_insert_identity(body, 0, body.len(), record_index)
    else {
        return Ok(None);
    };
    let link_names = vec![link_name.to_string()];
    Ok(Some(OccurrencePlacement {
        carrier_record_index: u64::from(record_index),
        owner: None,
        link_names,
        transform: None,
    }))
}

/// Parse the placement generation that repeats the target identity after the
/// standard path and stores the identity flag beside that repeated target.
fn repeated_target_occurrence_placement(
    body: &[u8],
) -> Result<Option<OccurrencePlacement>, CodecError> {
    Ok(
        repeated_target_occurrence_placement_details(body)?.map(|details| OccurrencePlacement {
            carrier_record_index: details.carrier_record_index,
            owner: Some(details.owner),
            link_names: details.link_names,
            transform: details.transform.map(|(_, matrix)| matrix),
        }),
    )
}

struct RepeatedTargetPlacementDetails {
    carrier_record_index: u64,
    owner: XrefLocalOwner,
    link_names: Vec<String>,
    role: String,
    role_offset: usize,
    transform: Option<(usize, [[f64; 4]; 4])>,
}

macro_rules! xref_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn repeated_target_occurrence_placement_details(
    body: &[u8],
) -> Result<Option<RepeatedTargetPlacementDetails>, CodecError> {
    let (link_names, path_target, mut at) = xref_some!(occurrence_target_path(body));
    if xref_some!(View::u32_le_at(body, at)) == 0 {
        return Ok(None);
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = xref_some!(lp_utf16_bounded(body, at, 36..=36));
        if !is_guid_relaxed(&guid) {
            return Ok(None);
        }
        at = next;
    }
    if body.get(at) == Some(&0) {
        at += 1;
    }
    let repeated_start = at;
    let repeated = xref_some!(take_owned_reference(body, &mut at));
    if !same_external_target(&path_target, &repeated) {
        return Ok(None);
    }
    let role = xref_some!(repeated.link_name()).to_owned();
    let role_offset = xref_some!(reference_role_offset(body, repeated_start));
    let transform = match *xref_some!(body.get(at)) {
        1 => {
            at += 1;
            None
        }
        0 => {
            at += 1;
            let offset = at;
            let matrix = xref_some!(decode_rigid_matrix(body, at));
            at = xref_some!(at.checked_add(128));
            Some((offset, matrix))
        }
        _ => return Ok(None),
    };
    if xref_some!(View::u32_le_at(body, at)) != 0 {
        return Ok(None);
    }
    at += 4;
    let (final_role, next) = xref_some!(lp_utf16_bounded(body, at, 0..=256));
    if final_role != role {
        return Ok(None);
    }
    at = next;
    if body.get(at) != Some(&0) {
        return Ok(None);
    }
    at += 1;
    let (target, type_guid) =
        xref_some!(xref_some!(take_owned_reference(body, &mut at)).into_local());
    Ok(
        (at == body.len()).then_some(RepeatedTargetPlacementDetails {
            carrier_record_index: xref_some!(View::u64_le_at(body, 7)),
            owner: XrefLocalOwner { target, type_guid },
            link_names,
            transform,
            role,
            role_offset,
        }),
    )
}

fn same_external_target(
    left: &Reference<String, String>,
    right: &Reference<String, String>,
) -> bool {
    match (left, right) {
        (
            Reference::CrossDocument {
                target: a,
                segment: sa,
                asset_guid: aa,
                target_type_guid: ta,
                link_name: ra,
                version: va,
                ..
            },
            Reference::CrossDocument {
                target: b,
                segment: sb,
                asset_guid: ab,
                target_type_guid: tb,
                link_name: rb,
                version: vb,
                ..
            },
        ) => {
            a == b
                && sa == sb
                && aa.eq_ignore_ascii_case(ab)
                && ta.eq_ignore_ascii_case(tb)
                && ra == rb
                && match (va, vb) {
                    (None, None) => true,
                    (Some((ka, ua)), Some((kb, ub))) => ka.eq_ignore_ascii_case(kb) && ua == ub,
                    _ => false,
                }
        }
        _ => false,
    }
}

fn reference_role_offset(body: &[u8], start: usize) -> Option<usize> {
    let mut at = start.checked_add(9)?;
    if View::u32_le_at(body, at) == Some(36) {
        at = lp_ascii_strict(body, at, 36..=36)?.1;
    }
    at = at.checked_add(5)?;
    at = lp_utf16_bounded_view(body, at, 0..=64)?.1.checked_add(1)?;
    if let Some((guid, end)) = lp_ascii_strict(body, at, 36..=36) {
        if crate::bytes::is_guid_hyphenated(guid) {
            at = end;
        }
    }
    at.checked_add(4)
}

/// Bind a repeated-target occurrence carrier to a Component Insert scope
/// through its relation record.
pub(crate) fn repeated_target_component_insert(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_transform: [[f64; 4]; 4],
) -> Result<Option<(String, usize, Option<usize>)>, CodecError> {
    let body = xref_some!(bytes.get(carrier_at..relation_at));
    if xref_some!(View::u64_le_at(body, 7)) != u64::from(carrier_record_index) {
        return Ok(None);
    }
    let details = xref_some!(repeated_target_occurrence_placement_details(body)?);
    let transform = details.transform.map_or(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        |(_, matrix)| matrix,
    );
    if transform != expected_transform {
        return Ok(None);
    }
    Ok(Some((
        details.role,
        carrier_at + details.role_offset,
        details.transform.map(|(offset, _)| carrier_at + offset),
    )))
}

/// Parse the grouped identity carrier used by the compact `Component Insert`
/// generation. The carrier has no matrix; its placement is the stored
/// identity transform. The repeated GUID and role fields are part of the
/// carrier grammar, not an occurrence-count signal.
pub(crate) fn grouped_component_insert_identity(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "382",
    )
}

/// Parse the class-380 grouped identity carrier used by the class-410/class-261
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class380(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "380",
    )
}

/// Parse the class-369 grouped identity carrier used by the class-426/class-258
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class369(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "369",
    )
}

/// Parse the variable-role grouped identity carrier used by the class-434/
/// class-266 `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class341(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "341",
    )
}

fn grouped_component_insert_identity_with_layout<'a>(
    bytes: &'a [u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_class_tag: &str,
) -> Option<(Utf16View<'a>, usize)> {
    const MARKER_AFTER_ROLE: &[u8] = &[0, 1, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_GUID_ROLE_MARKER: &[u8] = &[0, 3, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_EXTERNAL_ROLE_MARKER: &[u8] = &[0, 4, 0, 0, 0, 0, 1, 0, 0, 0];
    const MARKER_AFTER_METADATA: &[u8] = &[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    const MARKER_AFTER_PLACEMENT: &[u8] = &[0, 1, 0, 0, 0, 0];
    const CLASS_341_REPEAT_MARKER: &[u8] = &[1, 0, 0, 0, 0];
    const CLOSURE: &[u8] = &[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_span = relation_at.checked_sub(carrier_at)?;
    if class_tag != expected_class_tag
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
        || (expected_class_tag != "369"
            && expected_class_tag != "341"
            && carrier_span != grouped_identity_layout::LEN)
        || ((expected_class_tag == "369" || expected_class_tag == "341")
            && carrier_span < grouped_identity_layout::LEN)
        || bytes.get(carrier_at + 11..carrier_at + 19)? != [0; 8]
        || bytes.get(carrier_at + 19) != Some(&1)
        || bytes.get(carrier_at + 20..carrier_at + 24)? != [1, 0, 0, 0]
        || bytes.get(carrier_at + 24) != Some(&1)
        || bytes.get(carrier_at + 33) != Some(&1)
        || bytes.get(carrier_at + 34..carrier_at + 38)? != [0; 4]
    {
        return None;
    }
    let occurrence_identity = View::u64_le_at(
        bytes,
        carrier_at + grouped_identity_layout::OCCURRENCE_IDENTITY,
    )?;
    let (component_guid, mut at) = lp_utf16_bounded_view(
        bytes,
        carrier_at + grouped_identity_layout::FIRST_COMPONENT_GUID,
        36..=36,
    )?;
    if !component_guid.is_guid_relaxed() {
        return None;
    }
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(type_guid) {
        return None;
    }
    at = next;
    let first_role_at = at;
    let variable_role = matches!(expected_class_tag, "341" | "369");
    let role_bounds = if variable_role { 36..=256 } else { 36..=36 };
    let (role, next) = lp_utf16_bounded_view(bytes, at, role_bounds.clone())?;
    let valid_role = if variable_role {
        role.is_guid_relaxed() || role.is_guid_urn_role()
    } else {
        role.is_guid_relaxed()
    };
    if !valid_role {
        return None;
    }
    at = next;
    let marker_after_role = if expected_class_tag == "369" {
        if role.len() == 36 {
            CLASS_369_GUID_ROLE_MARKER
        } else {
            CLASS_369_EXTERNAL_ROLE_MARKER
        }
    } else {
        MARKER_AFTER_ROLE
    };
    if bytes.get(at..at + marker_after_role.len())? != marker_after_role {
        return None;
    }
    at += marker_after_role.len();

    let (metadata_guid_a, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !metadata_guid_a.is_guid_relaxed() {
        return None;
    }
    at = next;
    let (metadata_guid_b, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !metadata_guid_b.is_guid_relaxed() {
        return None;
    }
    at = next;
    if expected_class_tag == "341" {
        if bytes.get(at..at + 2)? != [0, 1]
            || View::u64_le_at(bytes, at + 2)? != occurrence_identity
        {
            return None;
        }
        at += 10;
        if bytes.get(at..at + CLASS_341_REPEAT_MARKER.len())? != CLASS_341_REPEAT_MARKER {
            return None;
        }
        at += CLASS_341_REPEAT_MARKER.len();
    } else {
        if bytes.get(at..at + MARKER_AFTER_METADATA.len())? != MARKER_AFTER_METADATA {
            return None;
        }
        at += MARKER_AFTER_METADATA.len();
    }

    let (repeated_component_guid, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !repeated_component_guid.is_guid_relaxed()
        || !repeated_component_guid.eq_ignore_ascii_case(component_guid)
    {
        return None;
    }
    at = next;
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (repeated_type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(repeated_type_guid) || !repeated_type_guid.eq_ignore_ascii_case(type_guid) {
        return None;
    }
    at = next;
    let (repeated_role, next) = lp_utf16_bounded_view(bytes, at, role_bounds.clone())?;
    if !repeated_role.eq_ignore_ascii_case(role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + MARKER_AFTER_PLACEMENT.len())? != MARKER_AFTER_PLACEMENT {
        return None;
    }
    at += MARKER_AFTER_PLACEMENT.len();

    let (final_role, next) = lp_utf16_bounded_view(bytes, at, role_bounds)?;
    if !final_role.eq_ignore_ascii_case(role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + CLOSURE.len())? != CLOSURE || at + CLOSURE.len() != relation_at {
        return None;
    }
    Some((role, first_role_at + 4))
}

/// Parse the current placement envelope: a standard target path, an optional
/// rigid matrix, and the generation-selected reference runs.
fn modern_occurrence_placement(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<OccurrencePlacement> {
    let (link_names, at) = occurrence_path(body)?;
    // The identity marker is absent in the oldest container generation, which
    // always stores the matrix. Both readings start with a zero byte when the
    // marker is present and the matrix follows, so the record end decides.
    for identity_marker in [true, false] {
        let mut cursor = at;
        let mut transform = None;
        if identity_marker {
            match body.get(cursor) {
                Some(1) => cursor += 1,
                Some(0) => {
                    let Some(matrix) = decode_rigid_matrix(body, cursor + 1) else {
                        continue;
                    };
                    transform = Some(matrix);
                    cursor += 129;
                }
                _ => continue,
            }
        } else {
            let Some(matrix) = decode_rigid_matrix(body, cursor) else {
                continue;
            };
            transform = Some(matrix);
            cursor += 128;
        }
        if placement_tail(body, cursor, serializer_magic).is_some() {
            return Some(OccurrencePlacement {
                carrier_record_index: View::u64_le_at(body, 7)?,
                owner: None,
                link_names,

                transform,
            });
        }
    }
    None
}

pub(crate) fn identity_component_insert_carrier(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    let body = bytes.get(carrier_at..relation_at)?;
    if View::u64_le_at(body, 7)? != u64::from(carrier_record_index) {
        return None;
    }
    let (mut at, targets_match) = legacy_occurrence_prefix(body, true)?;
    if !targets_match || body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let role_at = at + 4;
    let (role, next) = lp_utf16_bounded_view(body, at, 36..=36)?;
    if !role.is_guid_relaxed() || body.get(next..)? != [0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return None;
    }
    Some((role.to_string(), carrier_at + role_at))
}

/// Parse the legacy typed placement envelope.
///
/// This form keeps the same stable type-table identity as the current
/// placement, but its target-reference carrier is wider and its matrix is
/// after the repeated target envelope. The dynamic class tag is deliberately
/// not an admission key: the type-table identity and exact member framing are
/// the stable discriminators.
fn legacy_occurrence_placement(body: &[u8]) -> Option<(Utf16View<'_>, Option<[[f64; 4]; 4]>)> {
    let (mut at, targets_match) = legacy_occurrence_prefix(body, false)?;
    if !targets_match {
        return None;
    }
    let identity_marker = *body.get(at)?;
    at += 1;
    let transform = match identity_marker {
        1 => None,
        0 => {
            let matrix = decode_rigid_matrix(body, at)?;
            at = at.checked_add(128)?;
            Some(matrix)
        }
        _ => return None,
    };
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, after_role) = lp_utf16_bounded_view(body, at, 36..=36)?;
    if !link_name.is_guid_relaxed() {
        return None;
    }
    at = after_role;
    if body.get(at..at + 12)? != [0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return None;
    }
    at += 12;
    (at == body.len()).then_some((link_name, transform))
}

/// Return the role from a structurally valid legacy placement prefix.
///
/// The role is recovered even when the transform or closing tail is damaged,
/// so the caller can report an undecoded typed placement against the correct
/// external reference instead of treating it as an unrelated record.
fn legacy_occurrence_role(body: &[u8]) -> Option<Utf16View<'_>> {
    let (mut at, _) = legacy_occurrence_prefix(body, false)?;
    match *body.get(at)? {
        1 => at += 1,
        0 => at = at.checked_add(129)?,
        _ => return None,
    }
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, _) = lp_utf16_bounded_view(body, at, 36..=36)?;
    link_name.is_guid_relaxed().then_some(link_name)
}

/// Parse the shared prefix of the legacy identity and matrix forms.
fn legacy_occurrence_prefix(body: &[u8], insert: bool) -> Option<(usize, bool)> {
    let (_class_tag, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    let (name, after_name) = lp_ascii_strict(body, at, 0..=256)?;
    if insert && !name.is_empty() {
        return None;
    }
    at = after_name;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    let first_target = take_legacy_occurrence_reference(body, &mut at)?;
    if insert
        && !first_target
            .type_guid
            .eq_ignore_ascii_case("E03784ED-5E19-4E14-B9F2-3B07017018CD")
    {
        return None;
    }
    View::u32_le_at(body, at)?;
    at += 4;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    if insert {
        if !matches!(body.get(at), Some(0 | 1)) || body.get(at + 1) != Some(&0) {
            return None;
        }
        at += 2;
    }
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = lp_utf16_bounded_view(body, at, 36..=36)?;
        if !guid.is_guid_relaxed() {
            return None;
        }
        at = next;
    }
    if insert {
        if body.get(at) != Some(&1)
            || View::u64_le_at(body, at + 1)? == 0
            || body.get(at + 9..at + 16)? != [0, 1, 1, 0, 0, 0, 0]
        {
            return None;
        }
        at += 16;
    } else {
        if body.get(at) != Some(&0) {
            return None;
        }
        at += 1;
    }
    let second_target = take_legacy_occurrence_reference(body, &mut at)?;
    let targets_match = first_target.entity == second_target.entity
        && first_target.source_occurrence_identity == second_target.source_occurrence_identity
        && first_target
            .type_guid
            .eq_ignore_ascii_case(second_target.type_guid);
    Some((at, targets_match))
}

struct LegacyOccurrenceTarget<'a> {
    entity: u64,
    source_occurrence_identity: u64,
    type_guid: &'a str,
}

/// Consume one legacy occurrence target reference.
fn take_legacy_occurrence_reference<'a>(
    body: &'a [u8],
    at: &mut usize,
) -> Option<LegacyOccurrenceTarget<'a>> {
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    let entity = View::u64_le_at(body, *at)?;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    let source_occurrence_identity = View::u64_le_at(body, *at)?;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    let (type_guid, next) = lp_ascii_strict(body, *at, 36..=36)?;
    if !is_guid_relaxed(type_guid) {
        return None;
    }
    *at = next;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    Some(LegacyOccurrenceTarget {
        entity,
        source_occurrence_identity,
        type_guid,
    })
}

/// Parse the target-path prefix shared by every occurrence-placement form.
fn occurrence_path(body: &[u8]) -> Option<(Vec<String>, usize)> {
    occurrence_target_path(body).map(|(names, _, end)| (names, end))
}

type OccurrenceTargetPath = (Vec<String>, Reference<String, String>, usize);

fn occurrence_target_path(body: &[u8]) -> Option<OccurrenceTargetPath> {
    let (names, target, at) = occurrence_path_elements(body)?;
    (body.get(at) == Some(&0)).then_some((names, target, at + 1))
}

fn occurrence_path_elements(body: &[u8]) -> Option<OccurrenceTargetPath> {
    let (_, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    at = lp_ascii_strict(body, at, 0..=256)?.1;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    let count = View::u32_le_at(body, at).and_then(|count| usize::try_from(count).ok())?;
    if count == 0 || count > 4096 {
        return None;
    }
    at += 4;
    let mut link_names = Vec::new();
    let mut target = None;
    for _ in 0..count {
        let element = take_owned_reference(body, &mut at)?;
        if let Some(link_name) = element.link_name() {
            link_names.push(link_name.to_owned());
        }
        target = Some(element);
        View::u32_le_at(body, at)?;
        at += 4;
    }
    Some((link_names, target?, at))
}

/// Consume the three reference runs that close a placement, returning `Some`
/// only when they end exactly at the record end.
fn placement_tail(body: &[u8], mut at: usize, serializer_magic: Option<u32>) -> Option<()> {
    let count = usize::try_from(View::u32_le_at(body, at)?).ok()?;
    if count > 256 {
        return None;
    }
    at += 4;
    for _ in 0..count {
        take_reference(body, &mut at)?;
    }
    // Only the modern MetaStream serializer magic admits the tagged run. Its
    // tag byte is neither reference presence value.
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        if matches!(body.get(at), Some(0 | 1)) {
            return None;
        }
        at += 1;
        let tagged = usize::try_from(View::u32_le_at(body, at)?).ok()?;
        if tagged > 256 {
            return None;
        }
        at = at.checked_add(4)?.checked_add(tagged.checked_mul(4)?)?;
        take_reference(body, &mut at)?;
    }
    take_reference(body, &mut at)?;
    take_reference(body, &mut at)?;
    (at == body.len()).then_some(())
}

fn decode_rigid_matrix(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = view.f64_le()?;
        }
    }
    crate::records::xref::XrefPlacementTransform::try_from(rows)
        .ok()
        .map(crate::records::xref::XrefPlacementTransform::rows)
}

#[cfg(test)]
mod tests;
