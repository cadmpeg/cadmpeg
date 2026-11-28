// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a Part Studio, read from its stored feature definitions.
//!
//! Every Part Studio whose history the reader admits is one scope (§5.1) at
//! its root definition, with one ordered `Timeline` container (§5.2) whose
//! members are the features in `features.json` order. Each model feature has
//! a design feature under its own id (§5.10). A feature takes its typed
//! operation (§6) from its stored controls and selections. A loss names
//! each active input whose design field could not be stated.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::json;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::features::parameters::DesignParameter as SourceParameter;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, Storage};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use serde_json::Value;

use super::parameter_usage::ParameterUsage;
use super::{expression, features, id, push, topology, RollbackBar};
use crate::loss::OnshapeLossCode;

pub(in crate::reader) mod assembly;
mod constraints;
mod operation;
mod parameters;
mod sketches;

/// The feature definition and its quantities evaluated in the preceding variable environment.
#[derive(Debug)]
pub(super) struct Source<'a> {
    pub(super) record: &'a Value,
    ordinal: usize,
    pub(super) usage: ParameterUsage<'a>,
    pub(super) thin: Option<Value>,
    quantities: BTreeMap<&'static str, f64>,
    pub(super) legacy_loss: Option<LossNote>,
    pub(super) parameters: Vec<SourceParameter>,
}

impl<'a> Source<'a> {
    pub(super) fn read(
        record: &'a Value,
        ordinal: usize,
        thin: Option<Value>,
        usage: ParameterUsage<'a>,
        model: &cadmpeg_ir::document::Model,
        unresolved: &BTreeSet<String>,
    ) -> Self {
        let mut source = Self {
            record,
            ordinal,
            thin,
            usage,
            quantities: BTreeMap::new(),
            legacy_loss: None,
            parameters: Vec::new(),
        };
        for &(key, _, dimension) in quantity_roles(record) {
            let Ok(row) = features::parameter(record, key) else {
                continue;
            };
            if !source.quantity_active(key) {
                continue;
            }
            let value = row
                .get("expression")
                .and_then(Value::as_str)
                .and_then(|text| expression::evaluate(text, dimension, model, unresolved).ok())
                .map(|evaluated| evaluated.value)
                .or_else(|| expression::stored(row, dimension));
            if let Some(value) = value {
                source.quantities.insert(key, value);
            }
        }
        source
    }

    fn kind(&self) -> Option<&str> {
        self.record.get("featureType").and_then(Value::as_str)
    }

    pub(super) fn independently_projected(record: &Value) -> bool {
        !features::is_custom(record)
            && matches!(
                record.get("featureType").and_then(Value::as_str),
                Some(
                    "extrude"
                        | "revolve"
                        | "sweep"
                        | "fillet"
                        | "chamfer"
                        | "shell"
                        | "hole"
                        | "booleanBodies"
                )
            )
    }

    fn restates_legacy_loss(&self, operation: &FeatureOperation, loss: &LossNote) -> bool {
        use cadmpeg_ir::design::ops::shared::{Limit, Span};
        match operation {
            FeatureOperation::Combine(combine) => {
                combine.target.is_none() && loss.message.contains("as peers")
            }
            FeatureOperation::Shell(_) => {
                loss.message.starts_with("shell: hollow shells")
                    || loss
                        .message
                        .starts_with("shell: the legacy shell form supports inward walls")
            }
            FeatureOperation::Extrude(extrude) => {
                loss.message.starts_with("hasOffset ")
                    && matches!(extrude.span.as_ref(), Some(Span::Between { end, .. })
                    if matches!(end.as_ref(), Limit::ToGeometry { offset: Some(_), .. }))
                    || loss.message.starts_with("hasDraft ")
                        && extrude
                            .draft
                            .as_ref()
                            .is_some_and(|draft| draft.end.is_some())
                    || (loss.message.starts_with("hasSecondDirection ")
                        || loss.message.starts_with("hasSecondDirectionDraft "))
                        && self.symmetric_extrude() == Ok(true)
            }
            FeatureOperation::Sweep(sweep) => {
                loss.message.starts_with("this sweep Boolean operation")
                    || loss.message.starts_with("this sweep profile control")
                        && sweep.controls.orientation.is_some()
                    || loss.message.starts_with("sweep hasTwist ") && sweep.controls.twist.is_some()
                    || loss.message.starts_with("sweep hasScale ")
                        && sweep.controls.scaling.is_some()
            }
            _ => false,
        }
    }

    fn peek_tag(&self, key: &str) -> Option<&str> {
        features::value(self.record, key)
    }

    fn tag(&self, key: &'static str) -> Option<&str> {
        let value = self.peek_tag(key)?;
        self.usage.account(&[key]);
        Some(value)
    }

    fn symmetric_extrude(&self) -> super::Mapping<bool> {
        Ok(self.peek_flag("symmetric")?
            && matches!(self.peek_tag("endBound"), Some("BLIND" | "THROUGH_ALL")))
    }

    fn peek_flag(&self, key: &str) -> Result<bool, String> {
        features::enabled(self.record, key)
    }

    fn flag(&self, key: &'static str) -> Result<bool, String> {
        let value = self.peek_flag(key)?;
        self.usage.account(&[key]);
        Ok(value)
    }

    fn quantity<Q>(
        &self,
        key: &'static str,
        convert: impl FnOnce(f64) -> Option<Q>,
    ) -> Result<Q, String> {
        let value = self.quantity_value(key, convert)?;
        self.usage.account(&[key]);
        Ok(value)
    }

    fn quantity_value<Q>(
        &self,
        key: &'static str,
        convert: impl FnOnce(f64) -> Option<Q>,
    ) -> Result<Q, String> {
        self.quantities
            .get(key)
            .copied()
            .and_then(convert)
            .ok_or_else(|| format!("{key} has no resolved quantity in range"))
    }
}

/// The source definitions of the admitted features, by feature id.
pub(super) type Sources<'a> = BTreeMap<String, Source<'a>>;

type QuantityRole = (&'static str, &'static str, expression::Dimension);

fn quantity_roles(record: &Value) -> &'static [QuantityRole] {
    use expression::Dimension::{Angle, Length, Scalar};
    match record.get("featureType").and_then(Value::as_str) {
        Some("extrude") => &[
            ("depth", "AlongDistance", Length),
            ("thickness", "MidplaneThickness", Length),
            ("thickness1", "FirstWallThickness", Length),
            ("thickness2", "SecondWallThickness", Length),
            ("offsetDistance", "LimitOffset", Length),
            ("startOffsetDistance", "StartOffset", Length),
            ("secondDirectionDepth", "AgainstDistance", Length),
            ("draftAngle", "DraftAngle", Angle),
            ("secondDirectionDraftAngle", "AgainstDraftAngle", Angle),
        ],
        Some("revolve") => &[
            ("angle", "AlongAngle", Angle),
            ("angleBack", "AgainstAngle", Angle),
        ],
        Some("fillet") => &[
            ("radius", "Radius", Length),
            ("otherRadius", "OtherRadius", Length),
            ("nonCircularRadius", "NonCircularRadius", Length),
            ("width", "Width", Length),
        ],
        Some("chamfer") => &[
            ("width", "Distance", Length),
            ("width1", "FirstDistance", Length),
            ("width2", "SecondDistance", Length),
            ("angle", "Angle", Angle),
        ],
        Some("hole") => &[
            ("majorDiameter", "MajorDiameter", Length),
            ("tappedDepth", "TappedDepth", Length),
            ("holeDiameter", "Diameter", Length),
            ("tapDrillDiameter", "Diameter", Length),
            ("cBoreDiameter", "CounterboreDiameter", Length),
            ("cBoreDepth", "CounterboreDepth", Length),
            ("cSinkDiameter", "CountersinkDiameter", Length),
            ("cSinkAngle", "CountersinkAngle", Angle),
            ("holeDepth", "Depth", Length),
            ("tipAngle", "TipAngle", Angle),
        ],
        Some("shell") => &[("thickness", "Thickness", Length)],
        Some("sweep") => &[
            ("angle", "TwistAngle", Angle),
            ("turns", "TwistTurns", Scalar),
            ("pitch", "TwistPitch", Length),
            ("scaleFactor", "ScaleFactor", Scalar),
        ],
        _ => &[],
    }
}

impl Source<'_> {
    fn quantity_active(&self, key: &str) -> bool {
        let tag = |key| self.peek_tag(key);
        let flag = |key| self.peek_flag(key) == Ok(true);
        match (self.kind(), key) {
            (Some("extrude"), "depth") => tag("endBound") == Some("BLIND"),
            (Some("extrude"), "thickness") => tag("bodyType") == Some("THIN") && flag("midplane"),
            (Some("extrude"), "thickness1" | "thickness2") => {
                tag("bodyType") == Some("THIN") && !flag("midplane")
            }
            (Some("extrude"), "secondDirectionDepth" | "secondDirectionDraftAngle") => {
                self.symmetric_extrude() != Ok(true)
                    && flag("hasSecondDirection")
                    && if key == "secondDirectionDepth" {
                        tag("secondDirectionBound") == Some("BLIND")
                    } else {
                        flag("hasSecondDirectionDraft")
                    }
            }
            (Some("extrude"), "draftAngle") => flag("hasDraft"),
            (Some("extrude"), "offsetDistance") => {
                flag("hasOffset") && !matches!(tag("endBound"), Some("BLIND" | "THROUGH_ALL"))
            }
            (Some("extrude"), "startOffsetDistance") => {
                flag("startOffset") && tag("startOffsetBound") == Some("BLIND")
            }
            (Some("revolve"), "angle") => {
                !flag("fullRevolve") && tag("revolveType") != Some("FULL")
            }
            (Some("revolve"), "angleBack") => flag("hasStartBound"),
            (Some("fillet"), "radius") => tag("blendControlType") != Some("WIDTH"),
            (Some("fillet"), "width") => tag("blendControlType") == Some("WIDTH"),
            (Some("fillet"), "otherRadius") => {
                flag("isAsymmetric") && tag("blendControlType") != Some("WIDTH")
            }
            (Some("chamfer"), "width1" | "width2") => tag("chamferType") == Some("TWO_OFFSETS"),
            (Some("chamfer"), "width") => tag("chamferType") != Some("TWO_OFFSETS"),
            (Some("chamfer"), "angle") => {
                matches!(tag("chamferType"), Some("OFFSET_ANGLE" | "DISTANCE_ANGLE"))
            }
            (Some("hole"), "cBoreDiameter" | "cBoreDepth") => tag("style") == Some("C_BORE"),
            (Some("hole"), "cSinkDiameter" | "cSinkAngle") => tag("style") == Some("C_SINK"),
            (Some("hole"), "holeDepth") => tag("endStyle") == Some("BLIND"),
            (Some("hole"), "tipAngle") => {
                tag("endStyle") == Some("BLIND")
                    && features::tip_angle_preset(self.record).is_none()
            }
            (Some("hole"), "majorDiameter" | "tapDrillDiameter") => {
                features::tapped_standard(self.record)
            }
            (Some("hole"), "holeDiameter") => {
                !features::tapped_standard(self.record)
                    || features::parameter(self.record, "tapDrillDiameter").is_err()
            }
            (Some("hole"), "tappedDepth") => {
                features::tapped_standard(self.record)
                    && features::tap_through(self.record) != Ok(true)
            }
            (Some("sweep"), "scaleFactor") => flag("hasScale"),
            (Some("sweep"), "turns" | "angle" | "pitch") => {
                flag("hasTwist")
                    && !matches!(tag("profileControl"), Some("LOCK_FACES" | "LOCK_DIRECTION"))
                    && key
                        == match tag("twistType") {
                            Some("ANGLE") => "angle",
                            Some("PITCH") => "pitch",
                            _ => "turns",
                        }
            }
            _ => true,
        }
    }

    pub(super) fn prepare_quantities(
        &mut self,
        model: &cadmpeg_ir::document::Model,
        unresolved: &BTreeSet<String>,
    ) -> super::Mapping<()> {
        let native = super::text(self.record, "featureId")?;
        for &(key, role, dimension) in quantity_roles(self.record) {
            if features::parameter(self.record, key).is_err() || !self.quantity_active(key) {
                continue;
            }
            let parameter_id = id("parameter", &format!("{native}/{key}"));
            if model
                .parameters
                .iter()
                .any(|parameter| parameter.id.as_str() == parameter_id)
            {
                continue;
            }
            let role =
                if self.kind() == Some("extrude") && key == "depth" && self.symmetric_extrude()? {
                    "TotalDistance"
                } else {
                    role
                };
            if let Some(parameter) = features::formula(
                self.record,
                self.ordinal,
                key,
                role,
                dimension,
                model,
                unresolved,
            )? {
                self.parameters.push(parameter);
            }
        }
        Ok(())
    }
}

fn note(notes: &mut Vec<LossNote>, code: OnshapeLossCode, subject: &str, detail: &str) {
    notes.push(code.note(format!("{detail}; subject: {subject}")));
}

/// Project the design layer of the Part Studio `ir`, whose model features
/// were decoded from the sources `sources`, with `bar` its rollback bar. The
/// result names the thin extrudes whose design feature fell back to their
/// family, with the reason.
///
/// # Errors
///
/// A resource refusal, or a design record the IR refuses.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    notes: &mut Vec<LossNote>,
    bar: &RollbackBar,
    sources: &Sources<'_>,
    inputs: &topology::Inputs<'_>,
) -> Result<Vec<(String, String)>, CodecError> {
    let mut thin_refused = Vec::new();
    if ir.model.features.is_empty() {
        return Ok(thin_refused);
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.features.len()) + 2,
        "Onshape design timeline",
    )?;
    let malformed = |reason: String| CodecError::malformed(reason);
    super::reserve_history(&mut ir.design.features, ir.model.features.len())
        .map_err(CodecError::from)?;
    let sketched = sketches::project(ctx, ir)?;
    for (constraint, reason) in &sketched.native {
        note(
            notes,
            OnshapeLossCode::ConstraintNotSupported,
            constraint.as_str(),
            &format!("{reason}; the design states the constraint native"),
        );
    }
    let sketch_failures = sketched.failures;
    let definition = owner(ctx, ir)?;
    let derived_parameters = std::cell::RefCell::new(Vec::new());
    let stated_sketches: BTreeSet<&str> = ir
        .design
        .sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect();
    let stated_entities: BTreeMap<&str, &str> = ir
        .design
        .sketch_entities
        .iter()
        .map(|entity| (entity.id.as_str(), entity.sketch.as_str()))
        .collect();
    let owned: BTreeMap<(&str, &str), &cadmpeg_ir::features::ParameterId> = ir
        .model
        .parameters
        .iter()
        .chain(sources.values().flat_map(|source| &source.parameters))
        .filter_map(|parameter| {
            let role = parameter
                .properties
                .iter()
                .find(|(key, _)| key.as_str() == "source_kind")?
                .1;
            Some((
                (parameter.owner.as_ref()?.as_str(), role.as_str()),
                &parameter.id,
            ))
        })
        .collect();
    let (mut parameters, texts) = parameters::project(&ir.model.parameters, &definition, sources);
    let arrangements = cadmpeg_ir::design::regions::Arrangements::default();
    let mut designed = Vec::with_capacity(ir.model.features.len());
    let mut preceding = BTreeSet::new();
    let mut timeline: Vec<_> = ir.model.features.iter().collect();
    timeline.sort_by_key(|feature| feature.ordinal);
    for model in timeline {
        let source = sources.get(model.id.as_str());
        let name = model.name.as_deref().unwrap_or(model.id.as_str());
        let context = operation::Context {
            sketches: &stated_sketches,
            entities: &stated_entities,
            preceding: &preceding,
            owned: &owned,
            inputs: &ir.model.feature_input_topologies,
            model_entities: &ir.model.sketch_entities,
            arrangements: &arrangements,
            topology: inputs,
            model: &ir.model,
            scope: &definition,
            derived_parameters: &derived_parameters,
        };
        let thin = source.and_then(|source| Some((source.thin.as_ref()?, source)));
        let typed = match (thin, sketch_failures.get(&model.id)) {
            (Some((thin, source)), _) => match operation::thin(thin, source, &context) {
                Ok(operation) => Ok((operation, None)),
                Err(reason) => {
                    thin_refused.push((name.to_owned(), reason));
                    Err(None)
                }
            },
            (None, Some(reason)) => Err(Some((
                FeatureOperation::Unresolved {
                    family: OperationFamily::Sketch,
                },
                OnshapeLossCode::DesignOperationUnresolved,
                format!("{reason}; the model record carries the decoded sketch"),
            ))),
            (None, None) => operation::project(model, source, &context)
                .map_err(|fallback| Some((fallback.operation, fallback.code, fallback.reason))),
        };
        let legacy_loss = source.and_then(|source| source.legacy_loss.as_ref());
        let restated = match (&typed, source, legacy_loss) {
            (Ok((operation, _)), Some(source), Some(loss)) => {
                source.restates_legacy_loss(operation, loss)
            }
            _ => false,
        };
        if let Some(loss) = legacy_loss.filter(|_| !restated) {
            notes.push(loss.clone());
        }
        let projected = typed.is_ok();
        let operation = match typed {
            Ok((operation, assumed)) => {
                if let Some(assumed) = assumed {
                    note(notes, OnshapeLossCode::ChamferSideAssumed, name, assumed);
                }
                operation
            }
            Err(None) => FeatureOperation::Unresolved {
                family: OperationFamily::Extrude,
            },
            Err(Some((operation, code, reason))) => {
                if !reason.is_empty()
                    && !legacy_loss.is_some_and(|loss| {
                        loss.message.contains(
                            reason
                                .strip_suffix("; the model record carries the decoded operands")
                                .unwrap_or(&reason),
                        )
                    })
                {
                    note(notes, code, name, &reason);
                }
                operation
            }
        };
        if let FeatureOperation::Sweep(sweep) = &operation {
            if sweep.controls.scaling.is_some() {
                note(
                    notes,
                    OnshapeLossCode::SweepScaleAssumed,
                    name,
                    "the sweep uses a linear ramp from unity to the stored end scale",
                );
            }
        }
        if let Some(source) = source.filter(|source| !features::is_custom(source.record)) {
            let assignment = source.kind() == Some("assignVariable")
                && source.record["featureId"].as_str().is_some_and(|native| {
                    ir.model
                        .parameters
                        .iter()
                        .any(|p| p.id.as_str() == id("parameter", native))
                });
            if projected || assignment {
                source.usage.report(name, notes);
            }
        }
        if let FeatureOperation::Sketch(sketch) = &operation {
            preceding.insert(sketch.as_str().to_owned());
        }
        designed.push(Feature {
            id: model.id.clone(),
            name: model.name.clone(),
            suppressed: model.suppressed.map(Dimension::Literal),
            definition: cadmpeg_ir::design::feature::FeatureDefinition {
                operation,
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
    }
    ir.design.features.extend(designed);
    parameters.extend(derived_parameters.into_inner());
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(parameters.len()),
        "Onshape design parameters",
    )?;
    ir.design.parameters.extend(parameters);
    for parameter in texts {
        note(
            notes,
            OnshapeLossCode::ParameterExpressionText,
            parameter.as_str(),
            "the parameter's expression stays Onshape source text",
        );
    }
    for source in sources.values().filter(|source| {
        source.kind() == Some("importDerived") && !features::is_custom(source.record)
    }) {
        let Ok((document, version, element)) = super::derived::reference(source.record) else {
            continue;
        };
        let id = external_definition_id(document, version, element).map_err(malformed)?;
        if ir
            .model
            .product_definitions
            .iter()
            .any(|definition| definition.id == id)
        {
            continue;
        }
        ctx.charge_entities(1, "Onshape linked definition")?;
        ir.model.product_definitions.push(ProductDefinition::new(
            id,
            Storage::External(ExternalDocument {
                document_id: NonBlankString::new(document),
                version: NonBlankString::new(version),
                object: NonBlankString::new(element),
                ..ExternalDocument::default()
            }),
        ));
    }
    let current = install_scope(ir, bar, &definition).map_err(malformed)?;
    name_bodies(ir, current);
    Ok(thin_refused)
}

fn external_definition_id(
    document: &str,
    version: &str,
    element: &str,
) -> super::Mapping<cadmpeg_ir::ids::ProductDefinitionId> {
    cadmpeg_ir::ids::ProductDefinitionId::mint(id(
        "definition",
        &format!("{document}/{version}/{element}"),
    ))
    .map_err(|_| "the pinned Part Studio identity is invalid".into())
}

/// Name each body that one feature of the first `current` timeline members
/// makes by that feature (§5.7).
fn name_bodies(ir: &mut CadIr, current: usize) {
    let current: BTreeSet<_> = ir
        .design
        .features
        .iter()
        .take(current)
        .map(|feature| &feature.id)
        .collect();
    let mut producers = BTreeMap::new();
    for feature in ir
        .model
        .features
        .iter()
        .filter(|feature| current.contains(&feature.id))
    {
        for body in feature.evaluation.outputs() {
            producers
                .entry(body.clone())
                .and_modify(|producer: &mut Option<_>| *producer = None)
                .or_insert_with(|| Some(feature.id.clone()));
        }
    }
    let mut entries = ir.design.names.entries().clone();
    for (body, producer) in producers {
        let Some(feature) = producer else { continue };
        entries.insert(
            cadmpeg_ir::design::refs::EntityRef::Body(body),
            cadmpeg_ir::design::names::NameEntry {
                name: Some(cadmpeg_ir::design::names::TopoName::Generated {
                    kind: cadmpeg_ir::design::names::NameKind::Body,
                    feature,
                    role: Some(cadmpeg_ir::design::names::GenerationRole::Body),
                    generator: None,
                    index: None,
                }),
                native: None,
            },
        );
    }
    ir.design.names = entries.into();
}

/// State the Part Studio's timeline and scope over its design features, and
/// give the number of members before the rollback bar.
fn install_scope(
    ir: &mut CadIr,
    bar: &RollbackBar,
    definition: &cadmpeg_ir::ids::ProductDefinitionId,
) -> Result<usize, String> {
    let container = id("container", "Part Studio");
    let bodies: Vec<_> = ir.geometry.bodies.iter().map(|body| &body.id).collect();
    let members: Vec<_> = ir
        .design
        .features
        .iter()
        .map(|feature| json!({"kind":"feature","value":feature.id}))
        .collect();
    let count = members.len();
    // The bar stands before the first kept member at or after the stored
    // rollback feature. The current state is the state before the bar.
    let barred = bar.0.iter().find_map(|native| {
        let member = id("feature", native);
        ir.design
            .features
            .iter()
            .position(|feature| feature.id.as_str() == member)
    });
    let (rollback, at) = match barred {
        Some(0) => (
            json!({"kind":"before","value":members[0]}),
            json!({"kind":"start","value":container}),
        ),
        Some(index) => (
            json!({"kind":"before","value":members[index]}),
            json!({"kind":"after","value":members[index - 1]}),
        ),
        None => (
            json!({"kind":"end"}),
            json!({"kind":"after","value":members[members.len() - 1]}),
        ),
    };
    push(
        &mut ir.design.containers,
        json!({"kind":"ordered","value":{"id":container,"kind":"timeline","members":members,"folders":[],"rollback":rollback}}),
    )?;
    push(
        &mut ir.design.scopes,
        json!({"definition":definition,"roots":[container],"active_arrangements":[],"occurrences":{},"unproduced":[],
        "current":[{"at":at,"bodies":bodies,"poses":[],"status":"current"}]}),
    )?;
    Ok(barred.unwrap_or(count))
}

/// The Part Studio's root definition, stated where the STEP export states
/// none.
fn owner(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<cadmpeg_ir::ids::ProductDefinitionId, CodecError> {
    let placed: BTreeSet<_> = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| occurrence.prototype.clone())
        .collect();
    if let Some(root) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root() && !placed.contains(&definition.id))
    {
        return Ok(root.id.clone());
    }
    let stated = cadmpeg_ir::ids::ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("onshape", "model", "product-definition"),
        cadmpeg_ir::identity_key!("part-studio"),
    );
    ir.state_root_definition(
        ctx,
        stated.clone(),
        cadmpeg_ir::products::ExternalDocument::default(),
    )?;
    Ok(stated)
}
