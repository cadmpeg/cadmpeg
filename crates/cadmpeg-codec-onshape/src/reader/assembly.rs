// SPDX-License-Identifier: Apache-2.0
//! Bundle product structure projection.

use super::{empty, id, optional_rows, rows, studio, text, Mapping, Model, Report, ROWS};
use crate::archive::Bundle;
use crate::error::Error;
use crate::loss::OnshapeLossCode;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocument, JointConnector, JointOperand, Occurrence, ProductDefinition,
    ProductDefinitionKind, Storage,
};
use cadmpeg_ir::transform::Transform;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

mod index;
mod mates;

const ASSEMBLY_DEPTH: usize = 64;
const MATE_NOTES_PER_LEVEL: usize = 16;
const MATE_NOTE_LIMIT: usize = 256;

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    step_decoder: crate::StepDecoder,
    report: &mut Report,
    bundle: &Bundle<'_>,
    mid: &str,
    manifest: &Value,
) -> Result<Model, Error> {
    let mut model = Model::from_decoded(empty(ctx, root), None);
    let mut bodies = BTreeMap::new();
    let mut single_bodies = BTreeMap::new();
    let elements = rows(manifest, "elements").map_err(Error::Decode)?;
    let mut declared = BTreeMap::new();
    for (position, element) in elements.iter().enumerate() {
        if let Some(eid) = element.get("id").and_then(Value::as_str) {
            declared.entry(eid).or_insert(position);
        }
    }
    for (position, element) in elements.iter().enumerate() {
        if let Some(eid) = element.get("id").and_then(Value::as_str) {
            if declared.get(eid) != Some(&position) {
                report.push_coded(
                    eid,
                    OnshapeLossCode::AssemblyRecordUnsupported,
                    "element id repeats; retained the first declaration",
                );
                continue;
            }
        }
        if element.get("type").and_then(Value::as_str) == Some("ASSEMBLY")
            && !element
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(element_id)
        {
            report.push_coded(
                "assembly element",
                OnshapeLossCode::AssemblyRecordUnsupported,
                "assembly element id has an invalid folder character",
            );
        }
        if element.get("type").and_then(Value::as_str) != Some("PARTSTUDIO") {
            continue;
        }
        let eid = match text(element, "id") {
            Ok(eid) => eid,
            Err(reason) => {
                report.push_coded(
                    "element",
                    OnshapeLossCode::AssemblyReferenceUnresolved,
                    reason,
                );
                continue;
            }
        };
        if !element_id(eid) {
            report.push_coded(
                eid,
                OnshapeLossCode::AssemblyRecordUnsupported,
                "element id has an invalid folder character",
            );
            continue;
        }
        let part = match ctx.run_part(|| {
            studio(ctx, root, step_decoder, report, bundle, mid, eid).map_err(Into::into)
        })? {
            Ok(part) => part,
            Err(error) => {
                report.push_coded(
                    eid,
                    OnshapeLossCode::AssemblyReferenceUnresolved,
                    error.to_string(),
                );
                continue;
            }
        };
        let tail = cadmpeg_ir::ids::IdentityKeyTail::try_new(format!(
            "/{}",
            cadmpeg_ir::ids::IdentityKey::encode_key_text(eid)
        ))
        .map_err(|e| Error::Decode(e.to_string()))?;
        let rescope = |source: &str| {
            Ok(
                cadmpeg_ir::ids::Identity::new(source.to_owned()).map_or_else(
                    |_| source.to_owned(),
                    |id| id.with_key_tail(&tail).into_string(),
                ),
            )
        };
        let mut ir = rescope_home(ctx, &part.ir, &tail)?;
        if model.ir.source.is_none() {
            model.ir.source.clone_from(&ir.source);
        }
        let mut annotations = part.source_fidelity.annotations;
        annotations
            .map_ids(rescope)?
            .map_err(|e| Error::Decode(e.to_string()))?;
        for body in &ir.geometry.bodies {
            single_bodies
                .entry(eid.to_owned())
                .and_modify(|body| *body = None)
                .or_insert_with(|| Some(body.id.clone()));
            bodies
                .entry((eid.to_owned(), body.name.clone().unwrap_or_default()))
                .and_modify(|value| *value = None)
                .or_insert_with(|| Some(body.id.clone()));
        }
        for definition in &mut ir.model.product_definitions {
            if definition.is_root() {
                definition.storage =
                    ir.source
                        .clone()
                        .map_or(Storage::Embedded, |source| Storage::File {
                            document: ExternalDocument::path(format!(
                                "partstudios/{eid}/export.step"
                            )),
                            source: Some(source),
                        });
            }
        }
        model
            .source_fidelity
            .annotations
            .append(annotations)
            .map_err(|e| Error::Decode(e.to_string()))?;
        merge_design(&mut model.ir.design, ir.design);
        model
            .ir
            .try_append(ir.geometry, ir.model, ir.native, |_| {
                Ok(Ok::<(), Error>(()))
            })??;
        let ledger: cadmpeg_ir::report::decode::TransferLedger =
            rescope_home(ctx, &part.body.transfer_ledger, &tail)?;
        model.body.transfer_ledger.entries.extend(ledger.entries);
        model.body.losses.extend(part.body.losses);
        model.body.notes.extend(part.body.notes);
    }
    model.body.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(
        ctx.depth(),
        !model.ir.geometry.bodies.is_empty(),
    );
    let root_id = match manifest.get("root_element") {
        None | Some(Value::Null) => rows(manifest, "elements")
            .map_err(Error::Decode)?
            .iter()
            .find(|element| element.get("type").and_then(Value::as_str) == Some("ASSEMBLY"))
            .and_then(|element| element.get("id"))
            .and_then(Value::as_str),
        Some(Value::String(eid)) => Some(eid.as_str()),
        Some(Value::Object(root)) => root.get("id").and_then(Value::as_str),
        Some(_) => None,
    };
    if root_id.is_none()
        && manifest
            .get("root_element")
            .is_some_and(|root| !root.is_null())
    {
        report.push_coded(
            "root_element",
            OnshapeLossCode::AssemblyRecordUnsupported,
            "root_element requires a string or an object with a string id",
        );
    }
    let Some(root_id) = root_id else {
        return Ok(model);
    };
    if !element_id(root_id) {
        report.push_coded(
            root_id,
            OnshapeLossCode::AssemblyRecordUnsupported,
            "assembly root id has an invalid folder character",
        );
        return Ok(model);
    }
    if declared.get(root_id).is_some_and(|position| {
        elements
            .get(*position)
            .and_then(|element| element.get("type"))
            .and_then(Value::as_str)
            == Some("PARTSTUDIO")
    }) {
        return Ok(model);
    }
    let definition = match bundle.json(&format!("assemblies/{root_id}/definition.json")) {
        Ok(value) => value,
        Err(error) => {
            report.push_coded(
                root_id,
                OnshapeLossCode::AssemblyRecordUnsupported,
                error.to_string(),
            );
            return Ok(model);
        }
    };
    let index = index::Index::build(ctx, definition, manifest, bundle, mid, report)?;
    let mut products = BTreeMap::new();
    for (position, product) in model.ir.model.product_definitions.iter().enumerate() {
        ctx.charge_entities(1, "Onshape product identity index")?;
        products.insert(product.id.clone(), position);
    }
    let mut joint_ids = BTreeSet::new();
    for joint in &model.ir.model.assembly_joints {
        ctx.charge_entities(1, "Onshape joint identity index")?;
        joint_ids.insert(joint.id.clone());
    }
    let mut structure = Structure {
        ctx,
        report,
        model: &mut model,
        index,
        bodies: &bodies,
        single_bodies: &single_bodies,
        document: manifest
            .pointer("/document/id")
            .and_then(Value::as_str)
            .unwrap_or(""),
        microversion: mid,
        root_element: root_id,
        placements: BTreeMap::new(),
        products,
        joint_ids,
        count: 0,
        mate_work: 0,
        mate_notes: 0,
        omitted_mate_notes: 0,
        mate_exhausted: false,
    };
    if let Some(root) = definition
        .get("rootAssembly")
        .filter(|root| root.is_object())
    {
        let product = structure.level(root, &[], Some(Transform::identity()), false)?;
        if let Some(position) = structure.products.get(&product) {
            if let Some(product) = structure
                .model
                .ir
                .model
                .product_definitions
                .get_mut(*position)
            {
                product.storage = Storage::File {
                    document: ExternalDocument::default(),
                    source: None,
                };
            }
        }
    } else {
        structure.report.push_coded(
            root_id,
            OnshapeLossCode::AssemblyRecordUnsupported,
            "rootAssembly is missing or malformed",
        );
    }
    if structure.omitted_mate_notes > 0 {
        structure.report.push_coded(
            root_id,
            OnshapeLossCode::MateDiagnosticsAggregated,
            format!(
                "{} additional mate diagnostics aggregated; source feature responses remain assets",
                structure.omitted_mate_notes
            ),
        );
    }

    Ok(model)
}

fn rescope_home<T: serde::Serialize + serde::de::DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    home: &T,
    tail: &cadmpeg_ir::ids::IdentityKeyTail,
) -> Result<T, Error> {
    fn visit(
        ctx: &DecodeContext<'_>,
        value: &mut Value,
        tail: &cadmpeg_ir::ids::IdentityKeyTail,
    ) -> Result<(), Error> {
        let _depth = ctx.enter_nested("Onshape nested home identities")?;
        let rescope = |text: &str| {
            cadmpeg_ir::ids::Identity::new(text.to_owned()).map_or_else(
                |_| text.to_owned(),
                |id| id.with_key_tail(tail).into_string(),
            )
        };
        match value {
            Value::String(text) => *text = rescope(text),
            Value::Array(values) => {
                for value in values {
                    visit(ctx, value, tail)?;
                }
            }
            Value::Object(values) => {
                for (key, mut value) in std::mem::take(values) {
                    if !matches!(
                        key.as_str(),
                        "name" | "label" | "source_name" | "description" | "expression"
                    ) {
                        visit(ctx, &mut value, tail)?;
                    }
                    values.insert(rescope(&key), value);
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut value = cadmpeg_ir::schema::structural::project(ctx, home, "Onshape home projection")?;
    visit(ctx, &mut value, tail)?;
    serde_json::from_value(value).map_err(|e| Error::Decode(e.to_string()))
}

fn merge_design(target: &mut cadmpeg_ir::document::Design, source: cadmpeg_ir::document::Design) {
    macro_rules! append {
        ($($field:ident),* $(,)?) => {{
            let cadmpeg_ir::document::Design { $(mut $field,)* names } = source;
            $(target.$field.append(&mut $field);)*
            let mut combined = target.names.entries().clone();
            combined.extend(names.entries().iter().map(|(entity, name)| (entity.clone(), name.clone())));
            target.names = combined.into();
        }};
    }
    append!(
        scopes,
        containers,
        features,
        sketches,
        sketch_entities,
        sketch_constraints,
        spatial_sketches,
        spatial_sketch_entities,
        spatial_sketch_constraints,
        parameters,
        spreadsheets,
        sheet_metal_rules,
        programs,
        configurations,
        arrangements,
        publications,
        evaluations
    );
}

struct Placement {
    id: OccurrenceId,
    world: Option<Transform>,
}
struct Structure<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    report: &'a mut Report,
    model: &'a mut Model,
    index: index::Index<'a>,
    bodies: &'a BTreeMap<(String, String), Option<BodyId>>,
    single_bodies: &'a BTreeMap<String, Option<BodyId>>,
    document: &'a str,
    microversion: &'a str,
    root_element: &'a str,
    placements: BTreeMap<Vec<String>, Placement>,
    products: BTreeMap<ProductDefinitionId, usize>,
    joint_ids: BTreeSet<cadmpeg_ir::products::JointId>,
    count: usize,
    mate_work: usize,
    mate_notes: usize,
    omitted_mate_notes: usize,
    mate_exhausted: bool,
}
fn element_id(eid: &str) -> bool {
    !eid.is_empty()
        && eid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn key(path: &[String]) -> String {
    serde_json::to_string(path).unwrap_or_default()
}
fn product_id(native: &str) -> Result<ProductDefinitionId, Error> {
    id("product", native)
        .try_into()
        .map_err(|e| Error::Decode(format!("{e}")))
}
fn transform(value: &Value) -> Mapping<Transform> {
    let values = value
        .as_array()
        .filter(|v| v.len() == 16)
        .ok_or("transform requires 16 coefficients")?;
    let mut coefficients = [0.0; 16];
    for (out, value) in coefficients.iter_mut().zip(values) {
        *out = value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or("transform coefficient must be finite")?;
    }
    let [r00, r01, r02, tx, r10, r11, r12, ty, r20, r21, r22, tz, r30, r31, r32, r33] =
        coefficients;
    if [r30, r31, r32, r33] != [0.0, 0.0, 0.0, 1.0] {
        return Err("transform bottom row is invalid".into());
    }
    Transform::affine([
        [r00, r01, r02, tx * 1000.0],
        [r10, r11, r12, ty * 1000.0],
        [r20, r21, r22, tz * 1000.0],
    ])
    .ok_or_else(|| "transform exceeds finite coordinates".into())
}

impl Structure<'_, '_> {
    fn insert_product(&mut self, product: ProductDefinition) {
        self.products.insert(
            product.id.clone(),
            self.model.ir.model.product_definitions.len(),
        );
        self.model.ir.model.product_definitions.push(product);
    }
    fn admit_mate_work(&mut self) -> Result<bool, Error> {
        if self.mate_work >= ROWS {
            if !self.mate_exhausted {
                self.report.push_coded(self.root_element, OnshapeLossCode::MateWorkLimited,
                    "expanded assembly mate and parameter work exceeds the row limit; remaining features retain their assets");
                self.mate_exhausted = true;
            }
            return Ok(false);
        }
        self.ctx
            .charge_entities(1, "Onshape assembly mate and parameter work")?;
        self.mate_work += 1;
        Ok(true)
    }
    fn mate_loss(
        &mut self,
        level_notes: &mut usize,
        subject: &str,
        code: OnshapeLossCode,
        detail: impl Into<String>,
    ) -> Result<(), Error> {
        if *level_notes >= MATE_NOTES_PER_LEVEL || self.mate_notes >= MATE_NOTE_LIMIT {
            self.omitted_mate_notes += 1;
        } else {
            self.ctx
                .charge_entities(1, "Onshape assembly mate diagnostic")?;
            self.report.push_coded(subject, code, detail);
            *level_notes += 1;
            self.mate_notes += 1;
        }
        Ok(())
    }
    fn level(
        &mut self,
        assembly: &Value,
        path: &[String],
        parent: Option<Transform>,
        suppressed: bool,
    ) -> Result<ProductDefinitionId, Error> {
        let _depth = self.ctx.enter_nested("Onshape assembly structure")?;
        let pid = product_id(&format!("assembly:{}", key(path)))?;
        let mut product = ProductDefinition::new(pid.clone(), Storage::Embedded);
        product.kind = Some(ProductDefinitionKind::Assembly);
        let instances = match optional_rows(assembly, "instances") {
            Ok(rows) => rows,
            Err(reason) => {
                self.report.push_coded(
                    &key(path),
                    OnshapeLossCode::AssemblyRecordUnsupported,
                    reason,
                );
                &[]
            }
        };
        for instance in instances {
            if self.count >= ROWS {
                self.report.push_coded(
                    &key(path),
                    OnshapeLossCode::AssemblyRecordUnsupported,
                    "expanded assembly exceeds the occurrence limit",
                );
                break;
            }
            self.count += 1;
            self.ctx.charge_entities(3, "Onshape assembly instance")?;
            match self.instance(instance, path, parent, suppressed) {
                Ok(occurrence) => product.occurrences.push(occurrence),
                Err(Error::Decode(reason)) => self.report.push_coded(
                    &key(path),
                    OnshapeLossCode::AssemblyRecordUnsupported,
                    reason,
                ),
                Err(error) => return Err(error),
            }
        }
        self.ctx.charge_entities(1, "Onshape assembly definition")?;
        self.insert_product(product);
        mates::project(self, assembly, path, suppressed)?;
        Ok(pid)
    }

    fn instance(
        &mut self,
        instance: &Value,
        parent_path: &[String],
        parent: Option<Transform>,
        inherited_suppressed: bool,
    ) -> Result<OccurrenceId, Error> {
        let native = text(instance, "id").map_err(Error::Decode)?;
        let mut path = parent_path.to_vec();
        path.push(native.to_owned());
        if self.placements.contains_key(&path) {
            return Err(Error::Decode("instance path repeats".into()));
        }
        let occurrence = self.index.occurrences.get(&path).copied();
        let world = occurrence
            .and_then(|o| o.get("transform"))
            .map(transform)
            .transpose();
        let world = match world {
            Ok(value) => value,
            Err(reason) => {
                self.report
                    .push_coded(native, OnshapeLossCode::AssemblyRecordUnsupported, reason);
                None
            }
        };
        if world.is_none() {
            self.report.push_coded(
                native,
                OnshapeLossCode::AssemblyRecordUnsupported,
                "occurrence has no resolved transform; placement remains unstated",
            );
        }
        let local = match (parent, world) {
            (Some(parent), Some(world)) => parent
                .try_inverse_affine()
                .and_then(|inverse| inverse.compose(world))
                .map_or_else(
                    |reason| {
                        self.report.push_coded(
                            native,
                            OnshapeLossCode::AssemblyRecordUnsupported,
                            reason.to_string(),
                        );
                        None
                    },
                    Some,
                ),
            _ => None,
        };
        if parent.is_none() {
            self.report.push_coded(
                native,
                OnshapeLossCode::AssemblyRecordUnsupported,
                "ancestor placement is unresolved; local placement remains unstated",
            );
        }
        let trusted_world = if local.is_some() { world } else { None };
        let own_suppression = instance.get("suppressed").and_then(Value::as_bool);
        let occurrence_suppression = occurrence
            .and_then(|row| row.get("suppressed"))
            .and_then(Value::as_bool);
        let suppressed = inherited_suppressed
            || own_suppression == Some(true)
            || occurrence_suppression == Some(true);
        let prototype = match text(instance, "type").map_err(Error::Decode)? {
            "Assembly" => {
                let sub = self
                    .index
                    .subassemblies
                    .get(&self.index.definition_key(instance))
                    .copied();
                match sub {
                    Some(sub) if path.len() < ASSEMBLY_DEPTH => {
                        self.level(sub, &path, trusted_world, suppressed)?
                    }
                    Some(_) => {
                        self.report.push_coded(
                            native,
                            OnshapeLossCode::AssemblyRecordUnsupported,
                            "subassembly expansion reaches the supported depth limit",
                        );
                        self.unresolved(instance, &path)?
                    }
                    None => self.unresolved(instance, &path)?,
                }
            }
            "Part" => match self.part(instance) {
                Ok(product) => product,
                Err(Error::Decode(reason)) => {
                    self.report.push_coded(
                        native,
                        OnshapeLossCode::AssemblyReferenceUnresolved,
                        reason,
                    );
                    self.unresolved(instance, &path)?
                }
                Err(error) => return Err(error),
            },
            _ => self.unresolved(instance, &path)?,
        };
        let oid: OccurrenceId = id("occurrence", &key(&path))
            .try_into()
            .map_err(|e| Error::Decode(format!("{e}")))?;
        let mut placed = Occurrence::new(oid.clone(), prototype, local);
        placed.name = instance
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);
        placed.suppressed = if suppressed {
            Some(true)
        } else {
            own_suppression.or(occurrence_suppression)
        };
        placed.visible = occurrence
            .and_then(|row| row.get("hidden"))
            .or_else(|| instance.get("hidden"))
            .and_then(Value::as_bool)
            .map(|hidden| !hidden);
        if occurrence
            .and_then(|o| o.get("fixed"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            let mut grounded = AssemblyJoint::grounded(
                id("joint", &format!("fixed:{}", key(&path)))
                    .try_into()
                    .map_err(|e| Error::Decode(format!("{e}")))?,
                JointConnector {
                    operand: JointOperand::occurrence(oid.clone(), native, Vec::new()),
                    frame: Transform::identity(),
                    detached: false,
                },
                None,
            );
            grounded.suppressed = placed.suppressed == Some(true);
            self.joint_ids.insert(grounded.id.clone());
            self.model.ir.model.assembly_joints.push(grounded);
        }
        self.model.ir.model.occurrences.push(placed);
        self.placements.insert(
            path,
            Placement {
                id: oid.clone(),
                world: trusted_world,
            },
        );
        Ok(oid)
    }

    fn part(&mut self, instance: &Value) -> Result<ProductDefinitionId, Error> {
        let reference = self.index.reference(instance);
        let native = key(&reference);
        let pid = product_id(&format!("part:{native}"))?;
        if self.products.contains_key(&pid) {
            return Ok(pid);
        }
        let mut product = ProductDefinition::new(pid.clone(), Storage::Embedded);
        product.kind = Some(ProductDefinitionKind::Part);
        let part = self.index.parts.get(&reference).copied();
        let name = part
            .and_then(|part| part.get("name"))
            .and_then(Value::as_str);
        let (document, element) = index::studio_key(instance);
        let unique_name = name.is_some_and(|name| {
            self.index
                .names
                .get(&(document.clone(), element.clone(), name.into()))
                == Some(&1)
        });
        product.label = name.filter(|s| !s.trim().is_empty()).map(str::to_owned);
        let eid = text(instance, "elementId").map_err(Error::Decode)?;
        let local_document = instance
            .get("documentId")
            .and_then(Value::as_str)
            .is_none_or(|d| d == self.document);
        let pinned = self
            .index
            .microversion(instance)
            .is_none_or(|value| value == self.microversion);
        let default_configuration = instance
            .get("configuration")
            .and_then(Value::as_str)
            .is_none_or(|value| matches!(value, "" | "default"));
        let single_part =
            part.is_some() && self.index.studios.get(&(document, element)) == Some(&1);
        let single_body = self.single_bodies.get(eid).and_then(Option::as_ref);
        let matched = if local_document && pinned && default_configuration {
            name.filter(|_| unique_name)
                .and_then(|name| {
                    self.bodies
                        .get(&(eid.into(), name.into()))
                        .and_then(Option::as_ref)
                })
                .or(if single_part { single_body } else { None })
        } else {
            None
        };
        if let Some(body) = matched {
            product.bodies.push(body.clone());
        } else {
            self.report.push_coded(
                &native,
                OnshapeLossCode::AssemblyReferenceUnresolved,
                format!(
                    "part identity remains unbound at the pinned default configuration; {}",
                    self.index.description(instance)
                ),
            );
            if !local_document {
                product.storage = Storage::External(
                    ExternalDocument::document_id(instance["documentId"].as_str().unwrap_or(""))
                        .with_object(eid),
                );
            }
        }
        self.insert_product(product);
        Ok(pid)
    }

    fn unresolved(
        &mut self,
        instance: &Value,
        path: &[String],
    ) -> Result<ProductDefinitionId, Error> {
        let pid = product_id(&format!("unresolved:{}", key(path)))?;
        let document = instance
            .get("documentId")
            .and_then(Value::as_str)
            .unwrap_or("");
        let element = instance
            .get("elementId")
            .and_then(Value::as_str)
            .unwrap_or("");
        self.insert_product(ProductDefinition::new(
            pid.clone(),
            Storage::External(ExternalDocument::document_id(document).with_object(element)),
        ));
        self.report.push_coded(
            &key(path),
            OnshapeLossCode::AssemblyReferenceUnresolved,
            format!(
                "instance retains an unresolved product reference; {}",
                self.index.description(instance)
            ),
        );
        Ok(pid)
    }
}

#[cfg(test)]
mod tests;
