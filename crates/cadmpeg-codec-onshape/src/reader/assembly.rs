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
use serde_json::{json, Value};
use std::collections::BTreeMap;

mod mates;

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
    for element in rows(manifest, "elements").map_err(Error::Decode)? {
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
        let mut annotations = part.source_fidelity.annotations;
        annotations
            .map_ids(rescope)?
            .map_err(|e| Error::Decode(e.to_string()))?;
        for body in &ir.geometry.bodies {
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
        merge_design(&mut model.ir.design, ir.design)?;
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
    for element in rows(manifest, "elements").map_err(Error::Decode)? {
        if element.get("type").and_then(Value::as_str) != Some("ASSEMBLY") {
            continue;
        }
        if let Some(eid) = element.get("id").and_then(Value::as_str) {
            let path = format!("assemblies/{eid}/features.json");
            if let Ok(response) = bundle.json(&path) {
                if !optional_rows(response, "features").is_ok_and(<[Value]>::is_empty) {
                    report.push_coded(&path, OnshapeLossCode::AssemblyRecordUnsupported,
                        "native assembly feature parameters retain their bundle asset; evaluated definitions supply resolved joints");
                }
            }
        }
    }
    model.body.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(
        ctx.depth(),
        !model.ir.geometry.bodies.is_empty(),
    );
    let root_id = manifest
        .get("root_element")
        .and_then(Value::as_str)
        .or_else(|| manifest.pointer("/root_element/id").and_then(Value::as_str))
        .or_else(|| {
            manifest
                .get("elements")
                .and_then(Value::as_array)?
                .iter()
                .find(|e| e.get("type").and_then(Value::as_str) == Some("ASSEMBLY"))?
                .get("id")?
                .as_str()
        });
    let Some(root_id) = root_id else {
        return Ok(model);
    };
    if rows(manifest, "elements")
        .map_err(Error::Decode)?
        .iter()
        .any(|element| {
            element.get("id").and_then(Value::as_str) == Some(root_id)
                && element.get("type").and_then(Value::as_str) == Some("PARTSTUDIO")
        })
    {
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
    let mut structure = Structure {
        ctx,
        report,
        model: &mut model,
        definition,
        bodies: &bodies,
        document: manifest
            .pointer("/document/id")
            .and_then(Value::as_str)
            .unwrap_or(""),
        microversion: mid,
        placements: BTreeMap::new(),
        count: 0,
    };
    if let Some(root) = definition.get("rootAssembly") {
        let product = structure.level(root, &[], Transform::identity())?;
        if let Some(product) = structure
            .model
            .ir
            .model
            .product_definitions
            .iter_mut()
            .find(|p| p.id == product)
        {
            product.storage = Storage::File {
                document: ExternalDocument::default(),
                source: None,
            };
        }
    } else {
        structure.report.push_coded(
            root_id,
            OnshapeLossCode::AssemblyRecordUnsupported,
            "rootAssembly is missing",
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

fn merge_design(
    target: &mut cadmpeg_ir::document::Design,
    source: cadmpeg_ir::document::Design,
) -> Result<(), Error> {
    let mut target_value =
        serde_json::to_value(&*target).map_err(|e| Error::Decode(e.to_string()))?;
    let source_value = serde_json::to_value(source).map_err(|e| Error::Decode(e.to_string()))?;
    let (Some(target_rows), Some(source_rows)) =
        (target_value.as_object_mut(), source_value.as_object())
    else {
        return Err(Error::Decode("design homes must be objects".into()));
    };
    for (key, value) in source_rows {
        match (target_rows.get_mut(key), value) {
            (Some(Value::Array(a)), Value::Array(b)) => a.extend(b.iter().cloned()),
            (Some(Value::Object(a)), Value::Object(b)) => a.extend(b.clone()),
            _ => return Err(Error::Decode(format!("design arena {key} cannot merge"))),
        }
    }
    *target = serde_json::from_value(target_value).map_err(|e| Error::Decode(e.to_string()))?;
    Ok(())
}

struct Structure<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    report: &'a mut Report,
    model: &'a mut Model,
    definition: &'a Value,
    bodies: &'a BTreeMap<(String, String), Option<BodyId>>,
    document: &'a str,
    microversion: &'a str,
    placements: BTreeMap<Vec<String>, OccurrenceId>,
    count: usize,
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
    fn level(
        &mut self,
        assembly: &Value,
        path: &[String],
        parent: Transform,
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
            match self.instance(instance, path, parent) {
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
        self.model.ir.model.product_definitions.push(product);
        mates::project(self, assembly, path)?;
        Ok(pid)
    }

    fn instance(
        &mut self,
        instance: &Value,
        parent_path: &[String],
        parent: Transform,
    ) -> Result<OccurrenceId, Error> {
        let native = text(instance, "id").map_err(Error::Decode)?;
        let mut path = parent_path.to_vec();
        path.push(native.to_owned());
        if self.placements.contains_key(&path) {
            return Err(Error::Decode("instance path repeats".into()));
        }
        let occurrence = optional_rows(
            self.definition.get("rootAssembly").unwrap_or(&Value::Null),
            "occurrences",
        )
        .map_err(Error::Decode)?
        .iter()
        .find(|o| o.get("path") == Some(&json!(path)));
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
        let local = world
            .map(|world| {
                parent
                    .try_inverse_affine()
                    .and_then(|inverse| inverse.compose(world))
            })
            .transpose()
            .map_err(|e| Error::Decode(e.to_string()))?;
        let prototype = match text(instance, "type").map_err(Error::Decode)? {
            "Assembly" => {
                let sub = optional_rows(self.definition, "subAssemblies")
                    .map_err(Error::Decode)?
                    .iter()
                    .find(|sub| {
                        [
                            "documentId",
                            "elementId",
                            "documentMicroversion",
                            "configuration",
                        ]
                        .iter()
                        .all(|field| instance.get(field) == sub.get(field))
                    });
                match sub {
                    Some(sub) if path.len() < 64 => {
                        self.level(sub, &path, world.unwrap_or(parent))?
                    }
                    _ => self.unresolved(instance, &path)?,
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
        placed.suppressed = instance
            .get("suppressed")
            .and_then(Value::as_bool)
            .or_else(|| {
                occurrence
                    .and_then(|o| o.get("suppressed"))
                    .and_then(Value::as_bool)
            });
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
            self.model.ir.model.assembly_joints.push(grounded);
        }
        self.model.ir.model.occurrences.push(placed);
        self.placements.insert(path, oid.clone());
        Ok(oid)
    }

    fn part(&mut self, instance: &Value) -> Result<ProductDefinitionId, Error> {
        let native = key(&[
            instance
                .get("documentId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            text(instance, "elementId").map_err(Error::Decode)?.into(),
            text(instance, "partId").map_err(Error::Decode)?.into(),
            instance
                .get("documentMicroversion")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            instance
                .get("configuration")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
        ]);
        let pid = product_id(&format!("part:{native}"))?;
        if self
            .model
            .ir
            .model
            .product_definitions
            .iter()
            .any(|p| p.id == pid)
        {
            return Ok(pid);
        }
        let mut product = ProductDefinition::new(pid.clone(), Storage::Embedded);
        product.kind = Some(ProductDefinitionKind::Part);
        let part = optional_rows(self.definition, "parts")
            .map_err(Error::Decode)?
            .iter()
            .find(|p| {
                [
                    "documentId",
                    "elementId",
                    "partId",
                    "documentMicroversion",
                    "configuration",
                ]
                .iter()
                .all(|f| p.get(f) == instance.get(f))
            });
        let name = part.and_then(|p| p.get("name")).and_then(Value::as_str);
        let unique_name = name.is_some_and(|name| {
            optional_rows(self.definition, "parts").is_ok_and(|parts| {
                parts
                    .iter()
                    .filter(|part| {
                        part.get("elementId") == instance.get("elementId")
                            && part.get("documentId") == instance.get("documentId")
                            && part.get("name").and_then(Value::as_str) == Some(name)
                    })
                    .count()
                    == 1
            })
        });
        product.label = name.filter(|s| !s.trim().is_empty()).map(str::to_owned);
        let eid = text(instance, "elementId").map_err(Error::Decode)?;
        let local_document = instance
            .get("documentId")
            .and_then(Value::as_str)
            .is_none_or(|d| d == self.document);
        let pinned = instance
            .get("documentMicroversion")
            .and_then(Value::as_str)
            .is_none_or(|value| value == self.microversion);
        let default_configuration = instance
            .get("configuration")
            .and_then(Value::as_str)
            .is_none_or(|value| matches!(value, "" | "default"));
        let single_part = part.is_some()
            && optional_rows(self.definition, "parts")
                .map_err(Error::Decode)?
                .iter()
                .filter(|part| {
                    part.get("elementId") == instance.get("elementId")
                        && part.get("documentId") == instance.get("documentId")
                })
                .take(2)
                .count()
                == 1;
        let mut studio_bodies = self.bodies.iter().filter(|((studio, _), _)| studio == eid);
        let single_body = studio_bodies
            .next()
            .and_then(|(_, body)| body.as_ref())
            .filter(|_| studio_bodies.next().is_none());
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
                "part identity has no unique final STEP body name match at the pinned default configuration",
            );
            if !local_document {
                product.storage = Storage::External(
                    ExternalDocument::document_id(instance["documentId"].as_str().unwrap_or(""))
                        .with_object(eid),
                );
            }
        }
        self.model.ir.model.product_definitions.push(product);
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
        self.model
            .ir
            .model
            .product_definitions
            .push(ProductDefinition::new(
                pid.clone(),
                Storage::External(ExternalDocument::document_id(document).with_object(element)),
            ));
        self.report.push_coded(
            &key(path),
            OnshapeLossCode::AssemblyReferenceUnresolved,
            "instance retains an unresolved product reference",
        );
        Ok(pid)
    }
}
