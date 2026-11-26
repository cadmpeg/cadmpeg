// SPDX-License-Identifier: Apache-2.0
//! Pinned Part Studio references. Only neutral geometry crosses the writer seam.
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_core::decode::LimitScope;
use cadmpeg_core::CodecError;
use serde::Serialize;
use serde_json::{json, Value};

use super::{definition, features, id, rows, text, topology, Mapping, Report, Session};
use crate::error::Error;

/// The pinned design-reference payload schema.
const REFERENCE_SCHEMA: &str = "dutchman.design-reference/2";
/// Name of a referenced studio whose element row states no name.
const REFERENCED_STUDIO: &str = "Referenced studio";

/// A derived mapping failure. A local failure keeps the feature native; a fatal
/// failure is a caller resource refusal and stops the decode.
pub(super) enum Failure {
    Local(String),
    Fatal(Error),
}
impl From<String> for Failure {
    fn from(reason: String) -> Self {
        Self::Local(reason)
    }
}
impl From<&str> for Failure {
    fn from(reason: &str) -> Self {
        Self::Local(reason.to_owned())
    }
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        match error {
            Error::Codec(CodecError::ResourceLimit(limit))
                if limit.scope == LimitScope::Session =>
            {
                Self::Fatal(Error::Codec(CodecError::ResourceLimit(limit)))
            }
            other => Self::Local(other.to_string()),
        }
    }
}
impl From<CodecError> for Failure {
    fn from(error: CodecError) -> Self {
        Self::from(Error::Codec(error))
    }
}

/// The Onshape feature type that inserts the bodies of another Part Studio.
const DERIVED_TYPE: &str = "importDerived";
/// The Onshape feature type of a Boolean.
pub(super) const BOOLEAN_TYPE: &str = "booleanBodies";

pub(super) fn is_derived(feature: &Value) -> bool {
    feature.get("featureType").and_then(Value::as_str) == Some(DERIVED_TYPE)
}

/// The pinned document, version and element of the `partStudio` reference parameter.
/// It is a structured REST reference, never an evaluated `FeatureScript` query or a
/// guessed URL.
fn reference(feature: &Value) -> Mapping<(&str, &str, &str)> {
    let mut found = None;
    for parameter in rows(feature, "parameters")? {
        if parameter.get("parameterId").and_then(Value::as_str) != Some("partStudio") {
            continue;
        }
        if found.is_some() {
            return Err("the derived feature has several partStudio parameters".into());
        }
        let identity = |key| {
            parameter
                .get(key)
                .and_then(Value::as_str)
                .filter(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        found = Some(
            match (identity("documentId"), identity("versionId"), identity("elementId")) {
                (Some(d), Some(v), Some(e)) => (d, v, e),
                _ => return Err("the derived reference has no pinned document/version/element identity; fetch a new bundle".into()),
            },
        );
    }
    found.ok_or_else(|| {
        "the derived Part Studio reference is not resolved; fetch a new bundle".into()
    })
}

/// The wire payload of one captured neutral design. Fields stay in key order.
/// `ir` is the CADIR wire form, which leaves out the arenas of
/// `ArenaName::WIRE_OMITTED`; `omitted` states those arenas, feature history
/// among them, keyed by arena.
#[derive(Serialize)]
struct DesignReference<'a> {
    bodies: Vec<Value>,
    ir: &'a cadmpeg_ir::CadIr,
    name: &'a str,
    omitted: Value,
    schema: &'static str,
    step: &'a str,
}

/// A boolean parameter at the value its feature specification gives as default.
fn at_default_flag(parameter: &Value, key: &str, default: bool) -> Mapping<()> {
    if parameter.get("value").and_then(Value::as_bool) == Some(default) {
        Ok(())
    } else {
        Err(format!("derived parameter {key} is not at its default; the insert placement and content must be resolved"))
    }
}

/// A quantity parameter at the value its feature specification gives as default.
fn at_default_quantity(parameter: &Value, key: &str, default: f64) -> Mapping<()> {
    if parameter.get("value").and_then(Value::as_f64) == Some(default) {
        Ok(())
    } else {
        Err(format!(
            "derived parameter {key} is not at its default; the insert placement must be resolved"
        ))
    }
}

/// A query parameter with no stored query.
fn empty_query(parameter: &Value, key: &str) -> Mapping<()> {
    match parameter.get("queries") {
        None => Ok(()),
        Some(Value::Array(queries)) if queries.is_empty() => Ok(()),
        _ => Err(format!("derived parameter {key} is not at its default; the insert placement and content must be resolved")),
    }
}

/// Validate the `partStudio` reference parameter and collect the part identities of its
/// `partQuery`. An empty `partQuery` selects the whole referenced studio.
fn part_studio<'a>(parameter: &'a Value, selected: &mut BTreeSet<&'a str>) -> Mapping<()> {
    if !parameter
        .get("btType")
        .and_then(Value::as_str)
        .is_some_and(|name| name.starts_with("BTMParameterReferencePartStudio"))
    {
        return Err("the derived partStudio parameter is not a Part Studio reference".into());
    }
    for (key, value) in parameter.as_object().into_iter().flatten() {
        match key.as_str() {
            "btType" | "parameterId" | "parameterName" | "nodeId" | "libraryRelationType"
            | "documentId" | "versionId" | "elementId" => {}
            "namespace" | "standardContentParametersId" if value.as_str() == Some("") => {}
            "elementLibraryData" | "partIdentity" if value.is_null() => {}
            "configuration" if value.as_array().is_some_and(Vec::is_empty) => {}
            "partQuery" => part_query(value, selected)?,
            _ => return Err(format!("derived reference field {key} is not supported yet; its configuration and placement must be resolved")),
        }
    }
    Ok(())
}

fn part_query<'a>(parameter: &'a Value, selected: &mut BTreeSet<&'a str>) -> Mapping<()> {
    for (key, value) in parameter.as_object().into_iter().flatten() {
        match key.as_str() {
            "btType"
            | "parameterId"
            | "parameterName"
            | "nodeId"
            | "libraryRelationType"
            | "queries" => {}
            "filter" if value.is_null() => {}
            _ => {
                return Err(format!(
                    "derived part query field {key} is not supported yet"
                ))
            }
        }
    }
    if parameter.get("queries").is_none() {
        return Ok(());
    }
    for query in rows(parameter, "queries")? {
        let ids = rows(query, "deterministicIds")?;
        if ids.is_empty()
            || query.get("queryString").is_some()
            || query
                .get("namespace")
                .and_then(Value::as_str)
                .is_some_and(|namespace| !namespace.is_empty())
        {
            return Err("derived part queries are not fully resolved; select the whole referenced Part Studio".into());
        }
        for value in ids {
            let native = value
                .as_str()
                .filter(|id| !id.trim().is_empty())
                .ok_or("a selected derived part identity is missing")?;
            if !selected.insert(native) || selected.len() > super::ROWS {
                return Err("derived part identities repeat or exceed the row limit".into());
            }
        }
    }
    Ok(())
}

/// Validate every parameter of an `importDerived` feature against its feature
/// specification and return the part identities its `partQuery` selects. A parameter
/// that is absent has its default. A parameter at a non-default value places or
/// configures the insert and keeps the feature native.
fn selected_parts(feature: &Value) -> Mapping<BTreeSet<&str>> {
    let mut selected = BTreeSet::new();
    for parameter in rows(feature, "parameters")? {
        let key = text(parameter, "parameterId")?;
        match key {
            "newUI" | "includeMateConnectors" | "includeProperties" => {
                at_default_flag(parameter, key, true)?;
            }
            "preserveActiveSheetMetal" => at_default_flag(parameter, key, false)?,
            "placement" => {
                if parameter.get("value").and_then(Value::as_str) != Some("AT_ORIGIN") {
                    return Err("derived placement at a mate connector is not supported yet; the insert placement must be resolved".into());
                }
            }
            "mateConnectorIndex" | "mateConnectorIndexInFeature" => {
                at_default_quantity(parameter, key, -1.0)?;
            }
            "mateConnectorId" => at_default_quantity(parameter, key, 0.0)?,
            "location" | "parts" => empty_query(parameter, key)?,
            "buildFunction" => {
                let empty = |name| {
                    parameter
                        .get(name)
                        .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
                };
                if !empty("imports")
                    || !empty("configuration")
                    || parameter
                        .get("namespace")
                        .and_then(Value::as_str)
                        .is_some_and(|namespace| !namespace.is_empty())
                {
                    return Err("derived parameter buildFunction is not at its default".into());
                }
            }
            "partStudio" => part_studio(parameter, &mut selected)?,
            _ => {
                return Err(format!(
                    "derived parameter {key} is not in the importDerived feature specification"
                ));
            }
        }
    }
    Ok(selected)
}

/// Read the referenced studio of a derived feature and capture it as one payload.
pub(super) fn map(
    session: &mut Session<'_, '_>,
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    folder: &str,
    depth: usize,
    report: &mut Report,
) -> Result<String, Failure> {
    if feature
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| !namespace.is_empty())
    {
        return Err("custom FeatureScript derived features are not supported yet".into());
    }
    if depth >= super::MAX_REFERENCE_DEPTH {
        return Err("derived Part Studios exceed the reference depth limit".into());
    }
    let selected = selected_parts(feature)?;
    let (d, v, e) = reference(feature)?;
    let bundle = session.bundle;
    let prefix = format!("{folder}references/{d}/{v}/{e}/");
    let manifest = bundle
        .json(&format!("{prefix}manifest.json"))
        .map_err(|_| "the referenced Part Studio is missing; fetch a new bundle")?;
    let document = manifest.get("document").unwrap_or(&Value::Null);
    if text(document, "id")? != d
        || text(document, "wvid")? != v
        || text(document, "workspace_or_version")? != "v"
        || text(manifest, "root_element")? != e
    {
        return Err("the captured Part Studio does not match the pinned derived reference; fetch a new bundle".into());
    }
    if manifest.get("consistent").and_then(Value::as_bool) == Some(false) {
        return Err("the referenced Part Studio is marked inconsistent: its files do not reflect one microversion; fetch a new bundle".into());
    }
    // The checks that need only the host rollback states and the final parts of the
    // referenced studio run before the referenced studio is read.
    let before = inputs.solid_parts(ordinal)?;
    let after = inputs.solid_parts(ordinal + 1)?;
    if before.iter().any(|(id, name)| after.get(id) != Some(name)) {
        return Err("the derived feature changes an existing body; only an unchanged-placement Part Studio insert is supported yet".into());
    }
    let made: BTreeMap<_, _> = after
        .iter()
        .filter(|(key, _)| !before.contains_key(*key))
        .collect();
    if made.is_empty() {
        return Err(
            "the derived bodies cannot be matched to the whole referenced Part Studio".into(),
        );
    }
    let source_prefix = format!("{prefix}partstudios/{e}");
    let source_features = rows(
        bundle
            .json(&format!("{source_prefix}/features.json"))
            .map_err(|e| e.to_string())?,
        "features",
    )?;
    let mut scratch = Report::default();
    let source_inputs = topology::Inputs::new(
        bundle,
        &source_prefix,
        text(document, "microversion")?,
        source_features,
        &mut scratch,
    );
    // The final parts snapshot is needed only for a part subset and for a sole unnamed
    // solid; its failure is raised there.
    let source_parts = source_inputs
        .solid_parts(source_features.len())
        .map_err(|reason| {
            format!(
                "the referenced final parts snapshot cannot be used ({reason}); fetch a new bundle"
            )
        });
    if !selected.is_empty() {
        let parts = source_parts.as_ref().map_err(Clone::clone)?;
        if selected.len() != parts.len() || !selected.iter().all(|id| parts.contains_key(*id)) {
            return Err(
                "derived part subsets are not supported yet; select the whole referenced Part Studio"
                    .into(),
            );
        }
    }
    let source = match session.studios.get(&prefix) {
        Some(cached) => cached.clone()?,
        None => match super::read_studio(session, manifest, &prefix, depth + 1, report) {
            Ok(read) => {
                let read = Rc::new(read);
                session.studios.insert(prefix.clone(), Ok(Rc::clone(&read)));
                read
            }
            Err(error) => match Failure::from(error) {
                Failure::Fatal(error) => return Err(Failure::Fatal(error)),
                Failure::Local(reason) => {
                    session.studios.insert(prefix.clone(), Err(reason.clone()));
                    return Err(Failure::Local(reason));
                }
            },
        },
    };
    let element_name = manifest
        .get("elements")
        .and_then(Value::as_array)
        .and_then(|elements| {
            elements
                .iter()
                .find(|element| element.get("id").and_then(Value::as_str) == Some(e))
        })
        .and_then(|element| element.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(REFERENCED_STUDIO);
    let step = source
        .source_step
        .as_deref()
        .ok_or("the derived Part Studio has no solid bodies")?;
    if made.len() != source.ir.geometry.bodies.len() {
        return Err(
            "the derived bodies cannot be matched to the whole referenced Part Studio".into(),
        );
    }
    // A STEP export can omit its sole solid's name. The name of the sole solid in the
    // final rollback state then names the body in `bodies` and in the captured neutral
    // design, which is authoritative for body names; the STEP text keeps what the export
    // states.
    let sole_name = match source.ir.geometry.bodies.as_slice() {
        [body] if body.name.as_ref().is_none_or(|name| name.trim().is_empty()) => {
            let parts = source_parts.as_ref().map_err(Clone::clone)?;
            let names: Vec<_> = parts.values().collect();
            match names.as_slice() {
                [Some(name)] => Some(name.as_str()),
                _ => None,
            }
        }
        _ => None,
    };
    let mut bodies = Vec::new();
    let mut names = BTreeSet::new();
    for body in &source.ir.geometry.bodies {
        let name = body
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or(sole_name)
            .ok_or("a referenced body has no unique name")?;
        let matches: Vec<_> = made
            .iter()
            .filter(|(_, n)| n.as_deref() == Some(name))
            .collect();
        let [(native, _)] = matches.as_slice() else {
            return Err("derived body names do not match the captured Part Studio uniquely".into());
        };
        if !names.insert(name) {
            return Err("referenced body names are not unique".into());
        }
        bodies.push(json!({"id": id("body", native), "name": name}));
    }
    let named;
    let ir = match (sole_name, source.ir.geometry.bodies.as_slice()) {
        (Some(name), [_]) => {
            let mut ir = source.ir.clone();
            if let Some(body) = ir.geometry.bodies.first_mut() {
                body.name = Some(name.to_owned());
            }
            named = ir;
            &named
        }
        _ => &source.ir,
    };
    let omitted = ir.model.omitted_arenas_value().map_err(|e| e.to_string())?;
    let payload = serde_json::to_string(&DesignReference {
        bodies,
        ir,
        name: element_name,
        omitted,
        schema: REFERENCE_SCHEMA,
        step,
    })
    .map_err(|e| e.to_string())?;
    session.ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(payload.len()),
        "retain Onshape design reference",
    )?;
    Ok(payload)
}

/// A Boolean whose operands are bodies made by earlier features of a studio with a
/// derived insert. The operands name the stored result topology of their producers.
pub(super) fn boolean(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &cadmpeg_ir::document::Model,
) -> Mapping<Value> {
    if feature
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| !namespace.is_empty())
    {
        return Err("custom FeatureScript Boolean features are not supported yet".into());
    }
    let parameters = rows(feature, "parameters")?;
    for parameter in parameters {
        let key = text(parameter, "parameterId")?;
        if !matches!(
            key,
            "targets"
                | "tools"
                | "operationType"
                | "keepTools"
                | "offset"
                | "offsetAll"
                | "entitiesToOffset"
                | "offsetDistance"
                | "oppositeDirection"
                | "reFillet"
        ) {
            return Err(format!("Boolean parameter {key} is not supported yet"));
        }
    }
    // The enumeration is exactly UNION, SUBTRACTION and INTERSECTION; UNION is its default.
    let operation = match features::value(feature, "operationType") {
        None | Some("UNION") => "UNION",
        Some("SUBTRACTION") => "SUBTRACTION",
        Some("INTERSECTION") => "INTERSECTION",
        Some(_) => return Err("this Boolean operation is not supported yet".into()),
    };
    // The offset family is visible only for SUBTRACTION, and only `offset` set to true
    // makes the rest operative. A hidden stale value is ignored.
    if operation == "SUBTRACTION"
        && features::parameter(feature, "offset")
            .ok()
            .is_some_and(|offset| offset.get("value").and_then(Value::as_bool) != Some(false))
    {
        return Err("Boolean parameter offset is not supported yet".into());
    }
    let keep = features::enabled(feature, "keepTools")?;
    let parts = inputs.solid_parts(ordinal)?;
    let selected = |key| -> Mapping<Vec<(String, String)>> {
        let mut picked = Vec::new();
        for query in features::queries(feature, key)? {
            let ids = rows(query, "deterministicIds")?;
            if query.get("queryString").is_some()
                || ids.is_empty()
                || query
                    .get("namespace")
                    .is_some_and(|namespace| namespace.as_str() != Some(""))
            {
                return Err("Boolean body queries are not resolved".into());
            }
            for value in ids {
                let native = value.as_str().ok_or("a Boolean body identity is missing")?;
                if !parts.contains_key(native)
                    || picked.iter().any(|(local, _)| local == &id("body", native))
                {
                    return Err(
                        "a Boolean body is missing or repeats in its input rollback state".into(),
                    );
                }
                let producer = inputs.producer(ordinal, native, topology::OperandKind::Body)?;
                inputs.unique_feature(&producer, ordinal)?;
                picked.push((id("body", native), id("feature", &producer)));
            }
        }
        Ok(picked)
    };
    // `targets` is visible only for SUBTRACTION. For UNION and INTERSECTION only `tools`
    // is operative and a stored `targets` is stale. The combine form names one target
    // body, so peer tools cannot be stated and the feature stays native.
    if operation != "SUBTRACTION" {
        let tools = selected("tools")?;
        return Err(format!(
            "a {operation} Boolean combines its {} tool bodies as peers; the combine form names one target body, so the operands cannot be stated",
            tools.len()
        ));
    }
    let target = selected("targets")?;
    let tools = selected("tools")?;
    if target.len() != 1
        || tools.is_empty()
        || target
            .iter()
            .any(|(local, _)| tools.iter().any(|(tool, _)| tool == local))
    {
        return Err("this Boolean needs one target and disjoint tool bodies".into());
    }
    let generated = |selected: Vec<(String, String)>| -> Mapping<Value> {
        let mut references = Vec::new();
        for (local, producer) in selected {
            let owners: Vec<_> = model
                .feature_result_topologies
                .iter()
                .filter(|state| state.bodies().iter().any(|body| body.as_str() == local))
                .collect();
            let [owner] = owners.as_slice() else {
                return Err(
                    "a Boolean body cannot be tied uniquely to an earlier feature result".into(),
                );
            };
            if owner.output_of.as_str() != producer {
                return Err(
                    "the Boolean body producer disagrees with its stored rollback transition"
                        .into(),
                );
            }
            references.push(json!({"feature":owner.output_of,"local_id":local}));
        }
        Ok(
            json!({"kind":"generated","value":{"bodies":references,"native":serde_json::to_string(feature).map_err(|e|e.to_string())?}}),
        )
    };
    Ok(definition(
        "combine",
        json!({"operands": {"target": generated(target)?, "tools":generated(tools)?}, "op":"cut", "keep_tools":keep, "destination":"target_component"}),
    ))
}
