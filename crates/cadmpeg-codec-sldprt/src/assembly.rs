// SPDX-License-Identifier: Apache-2.0
//! Assembly product structure from the `swXmlContents/COMPINSTANCETREE` section
//! (`docs/formats/sldprt.md` §11).
//!
//! The section is a `swSolidWorks` XML document. `swHeader/swFile` lists the
//! assembly document and every document it references. `swModelList/swModel`
//! holds one model for each (document, configuration) pair; the
//! `swReference` children of an assembly model are its placed component
//! instances, and each names its component model through `swModelRef`. The
//! models form a directed acyclic graph, so one sub-assembly model can be
//! placed more than once; the occurrence tree expands every path from the
//! model of the active configuration.
//!
//! `swTransform` is a 4x4 matrix for row vectors (`p' = p M`) in metres, and
//! it places the instance relative to the assembly model that contains it.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::ids::{IdentityKey, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{Occurrence, ProductDefinition};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

use cadmpeg_core::bytes::contains;

use crate::container::{is_embedded_document, xml_text, ContainerScan};
use crate::loss::SldprtLossCode;

use self::native::{NativeAssembly, NativeDefinition, NativePlacement, NativePrototype};

mod native;

/// Section that holds the component instance tree.
const TREE_SECTION: &str = "swXmlContents/COMPINSTANCETREE";
/// Prefix and suffix of a configuration mate-list section, compared without
/// case and with `/` for the path separator.
const MATES_PREFIX: &str = "contents/config-";
const MATES_SUFFIX: &str = "-mateslist";
/// Class-name prefix of a mate record inside a mate-list section.
const MATE_CLASS_TOKEN: &[u8] = b"moMate";
/// `SolidWorks` stores assembly lengths in metres.
const METRES_TO_MILLIMETRES: f64 = 1000.0;
/// Largest admitted deviation of the matrix column that must be `(0, 0, 0, 1)`.
const EPS_HOMOGENEOUS_COLUMN: f64 = 1e-9;
/// Number of names that one loss message lists before it counts the rest.
const NAMED_PER_NOTE: usize = 8;

/// Product definitions, occurrences, and losses projected from one assembly.
#[derive(Debug)]
pub(crate) struct AssemblyProjection {
    definitions: Vec<ProductDefinition>,
    occurrences: Vec<Occurrence>,
    losses: Vec<LossNote>,
    /// Whether a section outside the embedded documents carries a Parasolid
    /// stream, or a capped probe left that open.
    carries_parasolid: bool,
}

/// Part losses that `apply` removes from an assembly without its own
/// Parasolid stream.
const PART_MODEL_ABSENT: [SldprtLossCode; 3] = [
    SldprtLossCode::GeometryParasolidNotTransferred,
    SldprtLossCode::TopologyGraphNotTransferred,
    SldprtLossCode::ContainerNoParasolidStream,
];

impl AssemblyProjection {
    /// Add the projection to a decoded document.
    pub(crate) fn apply(self, decoded: &mut Decoded) {
        decoded
            .ir
            .model
            .product_definitions
            .extend(self.definitions);
        if !self.carries_parasolid && !self.occurrences.is_empty() {
            let absent = PART_MODEL_ABSENT.map(SldprtLossCode::kind);
            decoded
                .body
                .losses
                .retain(|loss| !absent.contains(&loss.code));
        }
        decoded.ir.model.occurrences.extend(self.occurrences);
        decoded.body.losses.extend(self.losses);
        decoded.ir.model.finalize();
    }

    #[cfg(test)]
    pub(crate) fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    #[cfg(test)]
    pub(crate) fn definitions(&self) -> &[ProductDefinition] {
        &self.definitions
    }

    #[cfg(test)]
    pub(crate) fn losses(&self) -> &[LossNote] {
        &self.losses
    }
}

/// Read the assembly structure of a scanned document.
///
/// Returns `None` for a part document and for a container-only decode. A
/// document with a mate-list section or a component tree is an assembly. An
/// assembly with no readable component tree gives a projection that holds only
/// a blocking loss.
pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<AssemblyProjection>, CodecError> {
    if ctx.container_only() {
        return Ok(None);
    }
    let mut tree = None;
    let mut mate_lists = Vec::new();
    let mut is_assembly = false;
    let mut carries_parasolid = false;
    let mut embedded = Vec::new();
    for section in scan.sections() {
        let parasolid = !section.ps_streams().is_empty() || section.ps_probe_capped();
        let Some(name) = section.name() else {
            carries_parasolid |= parasolid;
            continue;
        };
        if is_embedded_document(name) {
            embedded.push(name.to_string());
            continue;
        }
        carries_parasolid |= parasolid;
        if name == TREE_SECTION {
            is_assembly = true;
            if tree.is_none() {
                tree = Some(section.payload());
            }
        } else if is_mate_list(name) {
            is_assembly = true;
            if contains(section.payload(), MATE_CLASS_TOKEN) {
                mate_lists.push(name.to_string());
            }
        }
    }
    if !is_assembly {
        return Ok(None);
    }
    let mut native = match tree.and_then(xml_text) {
        Some(text) => native_project(ctx, &text)?,
        None => NativeAssembly::structure_lost(format!(
            "The assembly document has no readable `{TREE_SECTION}` section. No occurrence, \
             placement, or component reference is transferred."
        )),
    };
    if !embedded.is_empty() {
        native.push_loss(
            SldprtLossCode::AssemblyEmbeddedDocumentsNotTransferred.note(format!(
                "{} embedded component document(s) are not decoded: {}.",
                embedded.len(),
                named(&embedded)
            )),
        );
    }
    if !mate_lists.is_empty() {
        native.push_loss(SldprtLossCode::AssemblyMatesNotTransferred.note(format!(
            "{} configuration mate list(s) hold mate records that are not transferred: {}.",
            mate_lists.len(),
            named(&mate_lists)
        )));
    }
    let mut projection = native.into_projection();
    projection.carries_parasolid = carries_parasolid;
    Ok(Some(projection))
}

/// Whether a section name is a `Contents/Config-N-MatesList` section.
fn is_mate_list(name: &str) -> bool {
    let name = name.replace('\\', "/").to_ascii_lowercase();
    name.strip_prefix(MATES_PREFIX)
        .and_then(|rest| rest.strip_suffix(MATES_SUFFIX))
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
}

struct Model<'a> {
    file: Option<&'a str>,
    configuration: Option<&'a str>,
    references: Vec<roxmltree::Node<'a, 'a>>,
}

struct File<'a> {
    path: Option<&'a str>,
    doc_type: Option<&'a str>,
}

/// Project the component tree text into product definitions and occurrences.
#[cfg(test)]
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<AssemblyProjection, CodecError> {
    Ok(native_project(ctx, text)?.into_projection())
}

/// Build the native structure of the component tree text.
fn native_project(ctx: &DecodeContext<'_>, text: &str) -> Result<NativeAssembly, CodecError> {
    let document = match ctx.parse_xml(text, "parse SolidWorks component instance tree") {
        Ok(document) => document,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(NativeAssembly::structure_lost(format!(
                "The `{TREE_SECTION}` section is not well-formed XML. No occurrence is \
                 transferred."
            )))
        }
    };
    let root = document.root_element();
    if root.tag_name().name() != "swSolidWorks" {
        return Ok(NativeAssembly::structure_lost(format!(
            "The `{TREE_SECTION}` section root is `{}`. No occurrence is transferred.",
            root.tag_name().name()
        )));
    }
    let mut files = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut configurations = Vec::new();
    for node in root.descendants().filter(roxmltree::Node::is_element) {
        match node.tag_name().name() {
            "swFile" => {
                if let Some(id) = node.attribute("id") {
                    files.insert(
                        id,
                        File {
                            path: node.attribute("swPath"),
                            doc_type: node.attribute("swDocType"),
                        },
                    );
                }
            }
            "swModel" => {
                if let Some(id) = node.attribute("id") {
                    let references = node
                        .children()
                        .filter(|child| child.tag_name().name() == "swReference")
                        .collect();
                    models.insert(
                        id,
                        Model {
                            file: node.attribute("swFileRef"),
                            configuration: node.attribute("swConfigurationName"),
                            references,
                        },
                    );
                }
            }
            "swConfiguration" => configurations.push(node),
            _ => {}
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(files.len() + models.len()),
        "admit SolidWorks component tree models",
    )?;

    let active = configurations
        .iter()
        .filter(|node| node.attribute("swMostRecentConfiguration") == Some("YES"))
        .filter_map(|node| node.attribute("swModelRef"))
        .collect::<Vec<_>>();
    let [root_model] = active.as_slice() else {
        return Ok(NativeAssembly::structure_lost(format!(
            "The component tree names {} active configurations. No occurrence is transferred.",
            active.len()
        )));
    };
    let root_model = *root_model;
    if !models.contains_key(root_model) {
        return Ok(NativeAssembly::structure_lost(format!(
            "The active configuration names model `{root_model}`, which the component tree \
             does not define. No occurrence is transferred."
        )));
    }

    let mut native = NativeAssembly::default();
    let mut tally = Tally::default();
    let other_configurations = configurations
        .iter()
        .filter(|node| node.attribute("swModelRef") != Some(root_model))
        .filter_map(|node| node.attribute("swName"))
        .collect::<Vec<_>>();
    if !other_configurations.is_empty() {
        native.push_loss(
            SldprtLossCode::AssemblyConfigurationsNotTransferred.note(format!(
                "The component trees of {} other configuration(s) are not transferred: {}.",
                other_configurations.len(),
                named(&other_configurations)
            )),
        );
    }

    let mut walk = Walk {
        ctx,
        files: &files,
        models: &models,
        definitions: BTreeSet::new(),
        occurrences: BTreeSet::new(),
        native: &mut native,
        tally: &mut tally,
    };
    let Some(root_definition) = walk.definition(root_model)? else {
        return Ok(NativeAssembly::structure_lost(format!(
            "The active configuration's model `{root_model}` has no valid identity. No \
             occurrence is transferred."
        )));
    };
    let root_name = walk.model_name(root_model);
    let Some(root_occurrence) = NativePlacement::tree_root(root_model, root_definition, root_name)
    else {
        return Ok(NativeAssembly::structure_lost(format!(
            "The active configuration's model `{root_model}` has no valid identity. No \
             occurrence is transferred."
        )));
    };
    let root_id = root_occurrence.id().clone();
    ctx.charge_entities(1, "project SolidWorks root occurrence")?;
    walk.occurrences.insert(root_id.clone());
    walk.native.push_placement(root_occurrence);
    let mut path = BTreeSet::from([root_model]);
    walk.children(root_model, &root_id, None, &mut path)?;
    tally.report(&mut native);
    Ok(native)
}

/// Counts and names for the occurrence-level losses.
#[derive(Default)]
struct Tally {
    external: usize,
    external_documents: BTreeSet<String>,
    virtual_components: Vec<String>,
    suppressed: Vec<String>,
    unresolved: Vec<String>,
    excluded_from_bom: Vec<String>,
}

impl Tally {
    fn report(self, native: &mut NativeAssembly) {
        if self.external > 0 {
            native.push_loss(SldprtLossCode::AssemblyComponentsExternal.note(format!(
                "{} component occurrence(s) reference {} external document(s). The referenced \
                 documents are not opened.",
                self.external,
                self.external_documents.len()
            )));
        }
        if !self.virtual_components.is_empty() {
            native.push_loss(
                SldprtLossCode::AssemblyVirtualComponentsNotTransferred.note(format!(
                    "{} occurrence(s) of virtual components have an unresolved prototype: {}.",
                    self.virtual_components.len(),
                    named(&self.virtual_components)
                )),
            );
        }
        if !self.suppressed.is_empty() {
            native.push_loss(SldprtLossCode::AssemblySuppressedComponentsOmitted.note(format!(
                "{} suppressed component instance(s) are omitted with the instances below them: {}.",
                self.suppressed.len(),
                named(&self.suppressed)
            )));
        }
        if !self.unresolved.is_empty() {
            native.push_loss(SldprtLossCode::AssemblyPlacementUnresolved.note(format!(
                "{} component instance(s) have no valid identity, placement, or component \
                 model and are omitted with the instances below them: {}.",
                self.unresolved.len(),
                named(&self.unresolved)
            )));
        }
        if !self.excluded_from_bom.is_empty() {
            native.push_loss(
                SldprtLossCode::AssemblyBomExclusionNotTransferred.note(format!(
                    "{} occurrence(s) are excluded from the bill of materials; the exclusion is \
                     not transferred: {}.",
                    self.excluded_from_bom.len(),
                    named(&self.excluded_from_bom)
                )),
            );
        }
    }
}

fn named<S: AsRef<str>>(names: &[S]) -> String {
    let mut text = names
        .iter()
        .take(NAMED_PER_NOTE)
        .map(|name| format!("`{}`", name.as_ref()))
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > NAMED_PER_NOTE {
        text = format!("{text}, and {} more", names.len() - NAMED_PER_NOTE);
    }
    text
}

struct Walk<'w, 'a> {
    ctx: &'w DecodeContext<'w>,
    files: &'w BTreeMap<&'a str, File<'a>>,
    models: &'w BTreeMap<&'a str, Model<'a>>,
    definitions: BTreeSet<&'a str>,
    /// Occurrence identities already placed.
    occurrences: BTreeSet<OccurrenceId>,
    native: &'w mut NativeAssembly,
    tally: &'w mut Tally,
}

impl<'a> Walk<'_, 'a> {
    fn file_path(&self, model: &str) -> Option<&'a str> {
        let file = self.models.get(model)?.file?;
        self.files.get(file)?.path
    }

    /// The document stem of a model's file.
    fn model_name(&self, model: &str) -> Option<String> {
        let path = self.file_path(model)?;
        self::native::document_stem(path)
    }

    /// Whether a model is an assembly model: its document is an assembly, or,
    /// without a document type, it places instances.
    fn is_assembly_model(&self, data: &Model<'_>) -> bool {
        let doc_type = data
            .file
            .and_then(|file| self.files.get(file))
            .and_then(|file| file.doc_type);
        match doc_type {
            Some(doc_type) => doc_type.eq_ignore_ascii_case("ASSEMBLY"),
            None => !data.references.is_empty(),
        }
    }

    /// The product definition of an assembly model, added once, or `None` for
    /// a model identity that is not a valid identity key.
    fn definition(&mut self, model: &'a str) -> Result<Option<ProductDefinitionId>, CodecError> {
        let Some(definition) = NativeDefinition::tree_model(model, self.model_name(model)) else {
            return Ok(None);
        };
        let id = definition.id().clone();
        if self.definitions.insert(model) {
            self.ctx
                .charge_entities(1, "project SolidWorks assembly model")?;
            self.native.push_definition(definition);
        }
        Ok(Some(id))
    }

    /// Project the component instances of `model`, placed under `parent`.
    fn children(
        &mut self,
        model: &'a str,
        parent: &OccurrenceId,
        parent_key: Option<&IdentityKey>,
        path: &mut BTreeSet<&'a str>,
    ) -> Result<(), CodecError> {
        let _depth = self.ctx.enter_nested("project SolidWorks component tree")?;
        let references = self
            .models
            .get(model)
            .map(|model| model.references.clone())
            .unwrap_or_default();
        for (ordinal, reference) in references.into_iter().enumerate() {
            self.ctx
                .charge_entities(1, "visit SolidWorks component instance")?;
            let name = instance_name(reference);
            if reference.attribute("swSuppressed") == Some("YES") {
                self.tally.suppressed.push(name);
                continue;
            }
            let (Some(id), Some(child)) =
                (reference.attribute("id"), reference.attribute("swModelRef"))
            else {
                self.tally.unresolved.push(name);
                continue;
            };
            let Some(child_model) = self.models.get(child) else {
                self.tally.unresolved.push(name);
                continue;
            };
            let Some(transform) = reference.attribute("swTransform").and_then(placement) else {
                self.tally.unresolved.push(name);
                continue;
            };
            if path.contains(child) {
                self.tally.unresolved.push(name);
                continue;
            }
            let Some((key, occurrence)) = NativePlacement::tree_occurrence_id(parent_key, id)
                .filter(|(_, occurrence)| !self.occurrences.contains(occurrence))
            else {
                self.tally.unresolved.push(name);
                continue;
            };
            let expands = self.is_assembly_model(child_model);
            let prototype = if expands {
                let Some(definition) = self.definition(child)? else {
                    self.tally.unresolved.push(name);
                    continue;
                };
                NativePrototype::Local(definition)
            } else if reference.attribute("swIsVirtualComponent") == Some("YES") {
                self.tally.virtual_components.push(name.clone());
                NativePrototype::Unresolved
            } else {
                let path = self.file_path(child).unwrap_or_default();
                self.tally.external += 1;
                self.tally.external_documents.insert(path.to_string());
                NativePrototype::external(path, child_model.configuration)
            };
            self.ctx
                .charge_entities(1, "project SolidWorks component occurrence")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("SolidWorks component ordinal exceeds u32"))?;
            if reference.attribute("swExcludeFromBOM") == Some("YES") {
                self.tally.excluded_from_bom.push(name.clone());
            }
            let visible = match reference.attribute("swHidden") {
                Some("YES") => Some(false),
                Some("NO") => Some(true),
                _ => None,
            };
            self.occurrences.insert(occurrence.clone());
            self.native.push_placement(
                NativePlacement::tree_child(occurrence.clone(), prototype, parent.clone(), id)
                    .placed(ordinal, transform, name, visible),
            );
            if expands {
                path.insert(child);
                self.children(child, &occurrence, Some(&key), path)?;
                path.remove(child);
            }
        }
        Ok(())
    }
}

/// The instance name `SolidWorks` shows: `<swName>-<swReferenceNumber>`.
fn instance_name(reference: roxmltree::Node<'_, '_>) -> String {
    let name = reference.attribute("swName").unwrap_or_default();
    match reference.attribute("swReferenceNumber") {
        Some(number) => format!("{name}-{number}"),
        None => name.to_string(),
    }
}

/// Convert an `swTransform` value to a placement in millimetres.
///
/// The value is a row-vector 4x4 matrix: rows 0-2 hold the rotation rows of
/// `M` with a zero fourth column, and row 3 holds the translation and `1`. The
/// neutral transform is the column-vector form, so the rotation is transposed.
pub(crate) fn placement(text: &str) -> Option<Transform> {
    let mut values = [0.0_f64; 16];
    let mut count = 0;
    for token in text.split_ascii_whitespace() {
        *values.get_mut(count)? = token
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())?;
        count += 1;
    }
    if count != 16 {
        return None;
    }
    let homogeneous = [values[3], values[7], values[11], values[15] - 1.0];
    if homogeneous
        .iter()
        .any(|value| value.abs() > EPS_HOMOGENEOUS_COLUMN)
    {
        return None;
    }
    let mut rows = [[0.0_f64; 4]; 3];
    for (i, row) in rows.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().take(3).enumerate() {
            *cell = values[j * 4 + i];
        }
        row[3] = values[12 + i] * METRES_TO_MILLIMETRES;
    }
    Transform::affine(rows).filter(Transform::is_proper_rigid)
}

#[cfg(test)]
mod tests;
