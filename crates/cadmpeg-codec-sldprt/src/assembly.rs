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
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{Occurrence, ProductDefinition};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::unknown::part::GivenUpPart;

use cadmpeg_core::bytes::contains;

use crate::container::{
    is_embedded_document, unwrapped_name, virtual_component_key, xml_text, ContainerScan,
};
use crate::loss::{named, SldprtLossCode};

use self::native::{
    NativeAssembly, NativeDefinition, NativePlacement, NativePrototype, TreeSection,
};

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

/// Product definitions, occurrences, and losses projected from one assembly.
#[derive(Debug)]
pub(crate) struct AssemblyProjection {
    definitions: Vec<ProductDefinition>,
    occurrences: Vec<Occurrence>,
    losses: Vec<LossNote>,
    /// The component tree sections the decode gives up.
    given_up: Vec<GivenUpPart>,
    /// Whether a section outside the embedded documents carries a Parasolid
    /// stream, or a capped probe left that open.
    carries_parasolid: bool,
    /// Each virtual component definition, the site key of its section, and
    /// its name.
    virtual_sites: Vec<(ProductDefinitionId, String, String)>,
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
    ///
    /// # Errors
    ///
    /// A given-up tree section whose stub the document already holds, or a
    /// resource refusal for the stub.
    pub(crate) fn apply(
        mut self,
        ctx: &DecodeContext<'_>,
        decoded: &mut Decoded,
    ) -> Result<(), CodecError> {
        let mut bodiless = Vec::new();
        for (definition, site, name) in &self.virtual_sites {
            let tail = format!("@{site}");
            let bodies = decoded
                .ir
                .geometry
                .bodies
                .iter()
                .filter(|body| body.id.as_str().ends_with(&tail))
                .map(|body| body.id.clone())
                .collect::<Vec<_>>();
            if bodies.is_empty() {
                bodiless.push(name.clone());
            }
            if let Some(target) = self
                .definitions
                .iter_mut()
                .find(|candidate| &candidate.id == definition)
            {
                target.bodies = bodies;
            }
        }
        if !bodiless.is_empty() {
            self.losses.push(
                SldprtLossCode::AssemblyVirtualComponentsNotTransferred.note(format!(
                    "{} virtual component document(s) of this assembly give no decoded body: {}.",
                    bodiless.len(),
                    named(&bodiless)
                )),
            );
        }
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
        for part in self.given_up {
            decoded.body.losses.push(part.commit(
                ctx,
                &mut decoded.ir,
                &mut decoded.source_fidelity,
            )?);
        }
        decoded.ir.finalize();
        Ok(())
    }

    /// The site keys of the virtual component sections that the assembly
    /// places.
    pub(crate) fn virtual_site_keys(&self) -> BTreeSet<String> {
        self.virtual_sites
            .iter()
            .map(|(_, site, _)| site.clone())
            .collect()
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

    #[cfg(test)]
    pub(crate) fn given_up(&self) -> &[GivenUpPart] {
        &self.given_up
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
    let mut trees = Vec::new();
    let mut mate_lists = Vec::new();
    let mut is_assembly = false;
    let mut carries_parasolid = false;
    let mut embedded = Vec::new();
    let mut virtual_sections = BTreeMap::new();
    let mut shared_virtual_names = Vec::new();
    let mut trees_differ = false;
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
        if let Some(key) = virtual_component_key(name) {
            match virtual_sections.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(section.site_key()));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                    shared_virtual_names.push(name.to_string());
                }
            }
            continue;
        }
        carries_parasolid |= parasolid;
        let unwrapped = unwrapped_name(name);
        if unwrapped == TREE_SECTION {
            is_assembly = true;
            match tree {
                None => tree = Some(section.payload()),
                Some(first) if first != section.payload() => trees_differ = true,
                Some(_) => {}
            }
            trees.push(TreeSection {
                stream: section.source_stream().clone(),
                stored: section.stored_payload(),
            });
        } else if is_mate_list(unwrapped) {
            is_assembly = true;
            if contains(section.payload(), MATE_CLASS_TOKEN) {
                mate_lists.push(name.to_string());
            }
        }
    }
    if !is_assembly {
        return Ok(None);
    }
    let unreadable = format!(
        "The assembly document has no readable `{TREE_SECTION}` section. No occurrence, \
         placement, or component reference is transferred."
    );
    let mut native = match tree.map(xml_text) {
        Some(Some(_)) if trees_differ => NativeAssembly::structure_given_up(
            ctx,
            &trees,
            format!(
                "The document holds different `{TREE_SECTION}` sections. No occurrence is \
                 transferred."
            ),
        )?,
        Some(Some(text)) => native_project(ctx, &text, &trees, &virtual_sections)?,
        Some(None) => NativeAssembly::structure_given_up(ctx, &trees, unreadable)?,
        None => NativeAssembly::structure_lost(unreadable),
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
    if !shared_virtual_names.is_empty() {
        native.push_loss(
            SldprtLossCode::AssemblyVirtualComponentsNotTransferred.note(format!(
                "{} virtual component section(s) share a name with another and are not \
                 transferred: {}.",
                shared_virtual_names.len(),
                named(&shared_virtual_names)
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
    let name = unwrapped_name(name).replace('\\', "/").to_ascii_lowercase();
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
    Ok(native_project(ctx, text, &[test_tree(text)], &BTreeMap::new())?.into_projection())
}

/// The section a test projection reads the component tree `text` from.
#[cfg(test)]
pub(crate) fn test_tree(text: &str) -> TreeSection<'_> {
    TreeSection {
        stream: cadmpeg_ir::stream_name!("swXmlContents/COMPINSTANCETREE"),
        stored: text.as_bytes(),
    }
}

/// Build the native structure of the component tree text, which the
/// sections `trees` hold. `virtual_sections` maps the key of each virtual
/// component section to its site key, or to `None` when sections share the
/// key.
fn native_project(
    ctx: &DecodeContext<'_>,
    text: &str,
    trees: &[TreeSection<'_>],
    virtual_sections: &BTreeMap<String, Option<String>>,
) -> Result<NativeAssembly, CodecError> {
    let document = match ctx.parse_xml(text, "parse SolidWorks component instance tree") {
        Ok(document) => document,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return NativeAssembly::structure_given_up(
                ctx,
                trees,
                format!(
                    "The `{TREE_SECTION}` section is not well-formed XML. No occurrence is \
                     transferred."
                ),
            );
        }
    };
    let root = document.root_element();
    if root.tag_name().name() != "swSolidWorks" {
        return NativeAssembly::structure_given_up(
            ctx,
            trees,
            format!(
                "The `{TREE_SECTION}` section root is `{}`. No occurrence is transferred.",
                root.tag_name().name()
            ),
        );
    }
    let mut files = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut repeated = None;
    let mut configurations = Vec::new();
    for node in root.descendants().filter(roxmltree::Node::is_element) {
        match node.tag_name().name() {
            "swFile" => {
                if let Some(id) = node.attribute("id") {
                    let file = File {
                        path: node.attribute("swPath"),
                        doc_type: node.attribute("swDocType"),
                    };
                    if files.insert(id, file).is_some() {
                        repeated.get_or_insert(("swFile", id));
                    }
                }
            }
            "swModel" => {
                if let Some(id) = node.attribute("id") {
                    let references = node
                        .children()
                        .filter(|child| child.tag_name().name() == "swReference")
                        .collect();
                    let model = Model {
                        file: node.attribute("swFileRef"),
                        configuration: node.attribute("swConfigurationName"),
                        references,
                    };
                    if models.insert(id, model).is_some() {
                        repeated.get_or_insert(("swModel", id));
                    }
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
    if let Some((element, id)) = repeated {
        return NativeAssembly::structure_given_up(
            ctx,
            trees,
            format!(
                "The component tree repeats the `{element}` id `{id}`. No occurrence is \
                 transferred."
            ),
        );
    }

    let active = configurations
        .iter()
        .filter(|node| node.attribute("swMostRecentConfiguration") == Some("YES"))
        .filter_map(|node| node.attribute("swModelRef"))
        .collect::<Vec<_>>();
    let [root_model] = active.as_slice() else {
        return NativeAssembly::structure_given_up(
            ctx,
            trees,
            format!(
                "The component tree names {} active configurations. No occurrence is \
                 transferred.",
                active.len()
            ),
        );
    };
    let root_model = *root_model;
    if !models.contains_key(root_model) {
        return NativeAssembly::structure_given_up(
            ctx,
            trees,
            format!(
                "The active configuration names model `{root_model}`, which the component \
                 tree does not define. No occurrence is transferred."
            ),
        );
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
        virtual_sections,
        holder_stem: None,
        definitions: BTreeSet::new(),
        expanded: BTreeSet::new(),
        occurrences: BTreeSet::new(),
        claimed: BTreeSet::new(),
        given_up: Vec::new(),
        native: &mut native,
        tally: &mut tally,
    };
    walk.holder_stem = walk.model_name(root_model);
    let Some(root_definition) = walk.root_definition(root_model)? else {
        return NativeAssembly::structure_given_up(
            ctx,
            trees,
            format!(
                "The active configuration's model `{root_model}` has no valid identity. No \
                 occurrence is transferred."
            ),
        );
    };
    let mut path = BTreeSet::from([root_model]);
    walk.expanded.insert(root_model);
    walk.children(root_model, &root_definition, &mut path)?;
    walk.report_given_up()?;
    tally.report(&mut native);
    Ok(native)
}

/// A component instance the walk gives up: its occurrence, its name, why,
/// and the occurrences below it that have no owner without it.
struct GivenUpInstance {
    occurrence: OccurrenceId,
    name: String,
    why: String,
    below: Vec<OccurrenceId>,
}

/// Counts and names for the occurrence-level losses.
#[derive(Default)]
struct Tally {
    external: usize,
    external_documents: BTreeSet<String>,
    virtual_components: Vec<String>,
    suppressed: Vec<String>,
    /// Instances without an identity that names an occurrence.
    unnamed: Vec<String>,
    /// Instances whose identity an earlier instance holds.
    repeated: Vec<String>,
    mirrored: Vec<String>,
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
        if !self.unnamed.is_empty() {
            native.push_loss(SldprtLossCode::AssemblyPlacementUnresolved.note(format!(
                "{} component instance(s) have no valid identity and are omitted: {}.",
                self.unnamed.len(),
                named(&self.unnamed)
            )));
        }
        if !self.repeated.is_empty() {
            native.push_loss(SldprtLossCode::AssemblyPlacementUnresolved.note(format!(
                "{} component instance(s) repeat the identity of an earlier instance and are \
                 omitted: {}.",
                self.repeated.len(),
                named(&self.repeated)
            )));
        }
        if !self.mirrored.is_empty() {
            native.push_loss(SldprtLossCode::AssemblyPlacementUnresolved.note(format!(
                "{} component instance(s) have a mirrored placement and are omitted: {}.",
                self.mirrored.len(),
                named(&self.mirrored)
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

struct Walk<'w, 'a> {
    ctx: &'w DecodeContext<'w>,
    files: &'w BTreeMap<&'a str, File<'a>>,
    models: &'w BTreeMap<&'a str, Model<'a>>,
    virtual_sections: &'w BTreeMap<String, Option<String>>,
    /// The stem of the document being decoded, which holds the
    /// `VirtualComp` sections.
    holder_stem: Option<String>,
    definitions: BTreeSet<&'a str>,
    /// Models whose component instances are already projected.
    expanded: BTreeSet<&'a str>,
    /// Occurrence identities already placed.
    occurrences: BTreeSet<OccurrenceId>,
    /// Occurrence identities placed or given up: a later instance with one
    /// of them repeats it.
    claimed: BTreeSet<OccurrenceId>,
    /// The instances given up, in walk order.
    given_up: Vec<GivenUpInstance>,
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

    /// The site key of the section that holds a virtual component model's
    /// document (`sldprt.md` §11.5). The document stem is
    /// `<name>^<assembly>`: `<assembly>` must be the stem of the document
    /// being decoded, and the section key is `<name>` in ASCII lower case
    /// followed by `-sldprt`. Each `^` of the stem is tried.
    fn virtual_site(&self, model: &str, reference: roxmltree::Node<'_, '_>) -> Option<String> {
        let holder = self.holder_stem.as_deref()?;
        let stem = self
            .model_name(model)
            .or_else(|| reference.attribute("swName").map(str::to_string))?;
        stem.match_indices('^').find_map(|(at, _)| {
            let name = stem.get(..at).filter(|name| !name.is_empty())?;
            let assembly = stem.get(at + 1..)?;
            if !assembly.eq_ignore_ascii_case(holder) {
                return None;
            }
            self.virtual_sections
                .get(&format!("{}-sldprt", name.to_ascii_lowercase()))?
                .clone()
        })
    }

    /// The product definition of a virtual component model whose document
    /// section has site key `site`, added once.
    fn virtual_definition(
        &mut self,
        model: &'a str,
        site: &str,
    ) -> Result<Option<ProductDefinitionId>, CodecError> {
        let Some(definition) = NativeDefinition::tree_model(model, self.model_name(model))
            .map(|definition| definition.with_bodies_from(site.to_string()))
        else {
            return Ok(None);
        };
        let id = definition.id().clone();
        if self.definitions.insert(model) {
            self.ctx
                .charge_entities(1, "project SolidWorks virtual component")?;
            self.native.push_definition(definition);
        }
        Ok(Some(id))
    }

    /// The root definition: the model of the active configuration, added
    /// first, or `None` for a model identity that is not a valid identity key.
    fn root_definition(
        &mut self,
        model: &'a str,
    ) -> Result<Option<ProductDefinitionId>, CodecError> {
        let Some(definition) = NativeDefinition::tree_model(model, self.model_name(model))
            .map(NativeDefinition::into_root)
        else {
            return Ok(None);
        };
        let id = definition.id().clone();
        self.definitions.insert(model);
        self.ctx
            .charge_entities(1, "project SolidWorks assembly model")?;
        self.native.push_definition(definition);
        Ok(Some(id))
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

    fn give_up(&mut self, occurrence: OccurrenceId, name: String, why: String) {
        self.given_up.push(GivenUpInstance {
            occurrence,
            name,
            why,
            below: Vec::new(),
        });
    }

    /// Project the instances of the assembly model `child`, which an omitted
    /// instance places, into its definition, which no instance then places.
    /// The instances of a model without a valid identity have no owner; they
    /// are added to the list `below` of the omitted instance.
    fn adopt(
        &mut self,
        child: Option<&'a str>,
        path: &mut BTreeSet<&'a str>,
    ) -> Result<(), CodecError> {
        let Some(child) = child else {
            return Ok(());
        };
        if !self
            .models
            .get(child)
            .is_some_and(|model| self.is_assembly_model(model))
            || path.contains(child)
            || self.expanded.contains(child)
        {
            return Ok(());
        }
        let Some(definition) = self.definition(child)? else {
            return Ok(());
        };
        self.expanded.insert(child);
        path.insert(child);
        self.children(child, &definition, path)?;
        path.remove(child);
        Ok(())
    }

    /// Collect in `below` the occurrences of the assembly model `model`,
    /// which has no valid identity and so no definition to own them, and
    /// project the assembly models they place as unplaced definitions.
    fn orphan(
        &mut self,
        model: &'a str,
        path: &mut BTreeSet<&'a str>,
        below: &mut Vec<OccurrenceId>,
    ) -> Result<(), CodecError> {
        if !self.expanded.insert(model) {
            return Ok(());
        }
        let _depth = self.ctx.enter_nested("project SolidWorks component tree")?;
        let references = self
            .models
            .get(model)
            .map(|model| model.references.clone())
            .unwrap_or_default();
        path.insert(model);
        for reference in references {
            self.ctx
                .charge_entities(1, "visit SolidWorks component instance")?;
            if reference.attribute("swSuppressed") == Some("YES") {
                continue;
            }
            if let Some((_, occurrence)) = reference
                .attribute("id")
                .and_then(NativePlacement::tree_occurrence_id)
            {
                below.push(occurrence);
            }
            let child = reference.attribute("swModelRef");
            match child.and_then(|child| self.models.get(child).map(|data| (child, data))) {
                Some((child, data)) if self.is_assembly_model(data) && !path.contains(child) => {
                    if NativeDefinition::tree_model(child, None).is_some() {
                        self.adopt(Some(child), path)?;
                    } else {
                        self.orphan(child, path, below)?;
                    }
                }
                _ => {}
            }
        }
        path.remove(model);
        Ok(())
    }

    /// Charge each instance the walk gave up. An occurrence listed below a
    /// given-up instance that the walk placed or gave up itself is not
    /// dropped with it.
    fn report_given_up(&mut self) -> Result<(), CodecError> {
        for instance in std::mem::take(&mut self.given_up) {
            let note = SldprtLossCode::AssemblyPlacementUnresolved.note(format!(
                "Component instance `{}` {} and is omitted.",
                instance.name, instance.why
            ));
            let below = instance
                .below
                .into_iter()
                .filter(|occurrence| !self.claimed.contains(occurrence))
                .collect::<BTreeSet<_>>();
            let unreadable =
                cadmpeg_ir::unknown::part::GivenUpRecord::caused_by(&instance.occurrence, &note)
                    .dropping(&below)
                    .loss();
            self.ctx
                .charge_entities(1, "report SolidWorks component instance")?;
            self.native.push_loss(note);
            self.native.push_loss(unreadable);
        }
        Ok(())
    }

    /// Project the component instances of `model`, which its definition
    /// `owner` places, and those of each assembly model they place, once.
    fn children(
        &mut self,
        model: &'a str,
        owner: &ProductDefinitionId,
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
            let child = reference.attribute("swModelRef");
            let Some((id, instance)) = reference
                .attribute("id")
                .and_then(|id| Some((id, NativePlacement::tree_occurrence_id(id)?)))
            else {
                self.tally.unnamed.push(name);
                self.adopt(child, path)?;
                continue;
            };
            if !self.claimed.insert(instance.1.clone()) {
                self.tally.repeated.push(name);
                self.adopt(child, path)?;
                continue;
            }
            let Some(child) = child else {
                self.give_up(instance.1, name, "names no component model".into());
                continue;
            };
            let Some(child_model) = self.models.get(child) else {
                self.give_up(
                    instance.1,
                    name,
                    format!("names model `{child}`, which the component tree does not define"),
                );
                continue;
            };
            let text = reference.attribute("swTransform");
            let Some(transform) = text.and_then(placement) else {
                if text.is_some_and(is_mirrored) {
                    self.tally.mirrored.push(name);
                } else {
                    self.give_up(instance.1, name, "has no proper rigid placement".into());
                }
                self.adopt(Some(child), path)?;
                continue;
            };
            if path.contains(child) {
                self.give_up(
                    instance.1,
                    name,
                    format!("places model `{child}`, which contains the instance"),
                );
                continue;
            }
            let expands = self.is_assembly_model(child_model);
            let prototype = if expands {
                let Some(definition) = self.definition(child)? else {
                    let mut below = Vec::new();
                    self.orphan(child, path, &mut below)?;
                    self.given_up.push(GivenUpInstance {
                        occurrence: instance.1,
                        name,
                        why: format!("places model `{child}`, which has no valid identity"),
                        below,
                    });
                    continue;
                };
                NativePrototype::Local(definition)
            } else if reference.attribute("swIsVirtualComponent") == Some("YES") {
                let definition = match self.virtual_site(child, reference) {
                    Some(site) => self.virtual_definition(child, &site)?,
                    None => None,
                };
                match definition {
                    Some(definition) => NativePrototype::Local(definition),
                    None => {
                        self.tally.virtual_components.push(name.clone());
                        NativePrototype::Unresolved
                    }
                }
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
            self.occurrences.insert(instance.1.clone());
            let child_definition = match &prototype {
                NativePrototype::Local(definition) => Some(definition.clone()),
                NativePrototype::External { .. } | NativePrototype::Unresolved => None,
            };
            self.native.push_placement(
                NativePlacement::tree_child(instance, prototype, owner.clone(), id)
                    .placed(ordinal, transform, name, visible),
            );
            if let (true, Some(child_definition)) = (expands, child_definition) {
                if self.expanded.insert(child) {
                    path.insert(child);
                    self.children(child, &child_definition, path)?;
                    path.remove(child);
                }
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
    Transform::affine(placement_rows(text)?).filter(Transform::is_proper_rigid)
}

/// Whether an `swTransform` value is a rigid motion with a reflection: one
/// rotation row negated gives a proper rigid motion.
fn is_mirrored(text: &str) -> bool {
    placement_rows(text).is_some_and(|mut rows| {
        for cell in rows[0].iter_mut().take(3) {
            *cell = -*cell;
        }
        Transform::affine(rows).is_some_and(|transform| transform.is_proper_rigid())
    })
}

/// The affine rows of an `swTransform` value, in millimetres.
fn placement_rows(text: &str) -> Option<[[f64; 4]; 3]> {
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
    Some(rows)
}

#[cfg(test)]
mod tests;
