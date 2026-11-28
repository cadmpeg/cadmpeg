// SPDX-License-Identifier: Apache-2.0
//! Configuration documents bound to the records of a generated timeline.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterOwner};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{Feature, FeatureContent, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::CadIr;

use super::frames::DesignRecords;
use super::parameters::ParameterPlan;
use super::roster::Context;
use crate::design::configurations::{ConfigurationBindings, ConfigurationTargets};
use crate::design_v11::configurations::rebase::Rebased;
use crate::design_v11::configurations::ActiveRows;
use crate::native::F3dNative;
use crate::records::configuration::table::ColumnTarget;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::records::parameters::DesignParameterOwner;
use crate::writer::generate::configuration_table::{self, read_back::Destination};
use crate::writer::generate::{configuration_entry_names, DESIGN_FOLDER};
use crate::writer::primitives::decoded_configurations;

pub(super) const DESIGN_BULK: &str =
    "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";

#[derive(Default)]
pub(in crate::writer::generate) struct Source {
    native: F3dNative,
    features: Vec<Feature>,
}

impl Source {
    pub(in crate::writer::generate) fn new(
        ctx: &DecodeContext<'_>,
        target: &CadIr,
    ) -> Result<Self, CodecError> {
        let Some(namespace) = target.native.namespace("f3d") else {
            return Ok(Self::default());
        };
        let documents: Vec<DesignConfiguration> =
            namespace.arena_as_for_decode(ctx, "design_configurations")?;
        if documents.is_empty() {
            return Ok(Self::default());
        }
        let mut direct = BTreeSet::new();
        let mut owned = BTreeSet::new();
        let mut scopes = BTreeSet::new();
        for column in documents
            .iter()
            .flat_map(DesignConfiguration::tables)
            .flat_map(|table| &table.columns)
        {
            match column.target {
                ColumnTarget::Parameter(index) => {
                    direct.insert(index);
                }
                ColumnTarget::OwnedParameter(index) => {
                    owned.insert(index);
                }
                ColumnTarget::Feature(index) => {
                    scopes.insert(index);
                }
                ColumnTarget::Property(_) => {}
            }
        }
        let mut native = if target.model.features.is_empty() && !scopes.is_empty() {
            F3dNative::load_inner(ctx, namespace)?
        } else {
            F3dNative {
                design_parameters: if direct.is_empty() && owned.is_empty() {
                    Vec::new()
                } else {
                    namespace.arena_as_for_decode(ctx, "design_parameters")?
                },
                design_parameter_owners: if owned.is_empty() {
                    Vec::new()
                } else {
                    namespace.arena_as_for_decode(ctx, "design_parameter_owners")?
                },
                design_parameter_scopes: if scopes.is_empty() {
                    Vec::new()
                } else {
                    namespace.arena_as_for_decode(ctx, "design_parameter_scopes")?
                },
                ..Default::default()
            }
        };
        let mut features = if target.model.features.is_empty() && !scopes.is_empty() {
            match super::parameters::native_projection(ctx, &native) {
                Ok(projected) => projected.features,
                Err(error) if crate::design::decode::record::is_fatal(&error) => return Err(error),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };
        native.design_parameters.retain(|record| {
            direct.contains(&record.record_index)
                || record
                    .owner_record_index()
                    .is_some_and(|index| owned.contains(&index))
        });
        native
            .design_parameter_owners
            .retain(|record| owned.contains(&record.record_index()));
        native
            .design_parameter_scopes
            .retain(|record| scopes.contains(&record.record_index));
        let feature_ids = native
            .design_parameter_scopes
            .iter()
            .map(crate::ids::neutral_feature_id)
            .collect::<BTreeSet<_>>();
        features.retain(|feature| feature_ids.contains(&feature.id));
        Ok(Self {
            native: F3dNative {
                design_configurations: documents,
                design_parameters: native.design_parameters,
                design_parameter_owners: native.design_parameter_owners,
                design_parameter_scopes: native.design_parameter_scopes,
                ..Default::default()
            },
            features,
        })
    }
}

pub(in crate::writer::generate) struct Configurations {
    pub(in crate::writer::generate) members: Vec<(String, Vec<u8>)>,
    pub(in crate::writer::generate) destination: Option<Destination<'static>>,
    pub(in crate::writer::generate) expected: CadIr,
}

pub(super) fn write(
    decode: &DecodeContext<'_>,
    target: &CadIr,
    (source, rebased): (&Source, Option<&Rebased>),
    ctx: &mut Context<'_>,
    parameters: &ParameterPlan,
    records: &mut DesignRecords,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Configurations, CodecError> {
    let source_features = &source.features;
    let source = &source.native;
    if target.design.configurations.is_empty()
        && target.model.configurations.is_empty()
        && source.design_configurations.is_empty()
        && target
            .design
            .scopes
            .iter()
            .all(|scope| scope.active_configuration.is_none())
    {
        return Ok(Configurations {
            members: Vec::new(),
            destination: None,
            expected: CadIr::empty(),
        });
    }
    let destinations = ctx.configuration_parameters(parameters);
    let features = ctx.configuration_features();
    let stream = crate::ids::native_scope(DESIGN_BULK);
    let mut occupied = source
        .design_parameters
        .iter()
        .map(|record| record.record_index)
        .chain(
            source
                .design_parameter_owners
                .iter()
                .map(DesignParameterOwner::record_index),
        )
        .chain(
            source
                .design_parameter_scopes
                .iter()
                .map(|record| record.record_index),
        )
        .chain(
            source
                .design_configurations
                .iter()
                .flat_map(DesignConfiguration::tables)
                .flat_map(|table| &table.columns)
                .filter_map(|column| match column.target {
                    ColumnTarget::Parameter(record)
                    | ColumnTarget::OwnedParameter(record)
                    | ColumnTarget::Feature(record) => Some(record),
                    ColumnTarget::Property(_) => None,
                }),
        )
        .chain(ctx.roster.rows().iter().map(|(index, _)| *index))
        .chain([u32::MAX])
        .collect::<BTreeSet<_>>();
    let mut next = 1u32;
    let mut temporary = || -> Result<u32, CodecError> {
        while occupied.contains(&next) {
            next = next.checked_add(1).ok_or_else(|| {
                CodecError::NotImplemented("configuration target index space is full".into())
            })?;
        }
        let index = next;
        occupied.insert(index);
        Ok(index)
    };
    let mut identities = BTreeMap::new();
    let mut bindings = ConfigurationBindings::default();
    let parameter_ids = target
        .design
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .chain(
            target
                .model
                .parameters
                .iter()
                .map(|parameter| parameter.id.clone()),
        )
        .chain(
            source
                .design_parameters
                .iter()
                .map(crate::ids::neutral_parameter_id),
        )
        .collect::<BTreeSet<_>>();
    for id in parameter_ids {
        let placeholder = temporary()?;
        let record = destinations
            .get(&id)
            .map_or(placeholder, |destination| destination.index);
        let placed = crate::ids::neutral_parameter_id_parts(&stream, record);
        identities.insert(id.to_string(), placed.to_string());
        bindings.parameters.insert(placeholder, placed);
    }
    let feature_ids = target
        .design
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .chain(
            target
                .model
                .features
                .iter()
                .map(|feature| feature.id.clone()),
        )
        .chain(
            source
                .design_parameter_scopes
                .iter()
                .map(crate::ids::neutral_feature_id),
        )
        .collect::<BTreeSet<_>>();
    for id in feature_ids {
        let placeholder = temporary()?;
        let placed = features.get(&id).map_or_else(
            || crate::ids::neutral_feature_id_parts(&stream, "Unwritten", 1, placeholder),
            |(_, id)| id.clone(),
        );
        identities.insert(id.to_string(), placed.to_string());
        bindings.features.insert(placeholder, placed);
    }
    let mut map =
        IdentityMap::new(|id| Ok(identities.get(id).cloned().unwrap_or_else(|| id.to_owned())));
    let mut expected = CadIr::empty();
    expected.tolerances = target.tolerances;
    expected.design.scopes.clone_from(&target.design.scopes);
    expected
        .design
        .parameters
        .clone_from(&target.design.parameters);
    for parameter in &mut expected.design.parameters {
        parameter.id = parameter.id.clone().rewrite_identities(decode, &mut map)?;
        parameter.owner = match parameter.owner.take() {
            Some(ParameterOwner::Feature(feature)) => Some(ParameterOwner::Feature(
                feature.rewrite_identities(decode, &mut map)?,
            )),
            owner => owner,
        };
        parameter.definition = match parameter.definition.clone() {
            ParameterDefinition::Expression(expression) => {
                ParameterDefinition::Expression(expression.rewrite_identities(decode, &mut map)?)
            }
            definition => definition,
        };
    }
    expected.design.features.clone_from(&target.design.features);
    for feature in &mut expected.design.features {
        feature.id = feature.id.clone().rewrite_identities(decode, &mut map)?;
        feature.suppressed = feature
            .suppressed
            .take()
            .rewrite_identities(decode, &mut map)?;
    }
    expected
        .design
        .configurations
        .clone_from(&target.design.configurations);
    for configuration in &mut expected.design.configurations {
        configuration.overrides.parameters =
            std::mem::take(&mut configuration.overrides.parameters)
                .into_iter()
                .map(|(id, mut fields)| {
                    fields.expression = fields
                        .expression
                        .take()
                        .rewrite_identities(decode, &mut map)?;
                    Ok((id.rewrite_identities(decode, &mut map)?, fields))
                })
                .collect::<Result<_, CodecError>>()?;
        configuration.overrides.features = std::mem::take(&mut configuration.overrides.features)
            .into_iter()
            .map(|(id, fields)| Ok((id.rewrite_identities(decode, &mut map)?, fields)))
            .collect::<Result<_, CodecError>>()?;
    }
    if !target.model.omitted_arenas_held() {
        expected.model.drop_omitted_arenas();
    }
    expected
        .model
        .product_definitions
        .clone_from(&target.model.product_definitions);
    expected
        .model
        .occurrences
        .clone_from(&target.model.occurrences);
    expected.model.configurations = target
        .model
        .configurations
        .clone()
        .rewrite_identities(decode, &mut map)?;
    expected.model.parameters = parameters
        .configuration_parameters(target)
        .rewrite_identities(decode, &mut map)?;
    let mut own_features = if target.model.features.is_empty() {
        source_features.clone()
    } else {
        target.model.features.clone()
    };
    for feature in &target.design.features {
        let suppressed = feature.suppressed.as_ref().and_then(|value| match value {
            cadmpeg_ir::design::dimension::Dimension::Literal(value) => Some(*value),
            cadmpeg_ir::design::dimension::Dimension::Parameter(_)
            | cadmpeg_ir::design::dimension::Dimension::Negated(_) => None,
        });
        if let Some(base) = own_features.iter_mut().find(|base| base.id == feature.id) {
            base.suppressed = suppressed;
            continue;
        }
        let operation = if ctx
            .sketches
            .iter()
            .any(|sketch| sketch.feature == &feature.id)
        {
            FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
            }
        } else {
            let kind = match &feature.definition.operation {
                cadmpeg_ir::design::feature::FeatureOperation::Extrude(_) => "Extrude",
                cadmpeg_ir::design::feature::FeatureOperation::Revolve(_) => "Revolve",
                _ => "Generated",
            };
            FeatureOperation::Native {
                kind: kind.into(),
                parameters: BTreeMap::new(),
            }
        };
        own_features.push(Feature {
            product_definition: None,
            id: feature.id.clone(),
            ordinal: 0,
            name: feature.name.clone(),
            suppressed,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
            native_ref: feature.native_ref.clone(),
        });
    }
    expected.model.features = own_features.rewrite_identities(decode, &mut map)?;
    map.finish()?;
    let emitted_parameters = expected
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.as_str(), &parameter.id))
        .collect::<BTreeMap<_, _>>();
    let emitted_features = expected
        .model
        .features
        .iter()
        .map(|feature| (feature.id.as_str(), &feature.id))
        .collect::<BTreeMap<_, _>>();
    let mut source_parameters = BTreeMap::<_, BTreeSet<_>>::new();
    for parameter in &source.design_parameters {
        source_parameters
            .entry(parameter.record_index)
            .or_default()
            .insert(crate::ids::neutral_parameter_id(parameter));
    }
    for parameter in &source.design_parameters {
        let id = crate::ids::neutral_parameter_id(parameter);
        if let Some(&placed) = identities
            .get(id.as_str())
            .and_then(|id| emitted_parameters.get(id.as_str()))
        {
            if source_parameters
                .get(&parameter.record_index)
                .is_some_and(|ids| ids.len() == 1)
            {
                bindings
                    .parameters
                    .insert(parameter.record_index, placed.clone());
            }
            if let Some(stream) = crate::ids::native_stream(&parameter.id) {
                bindings
                    .scoped_parameters
                    .insert((stream.to_owned(), parameter.record_index), placed.clone());
            }
        }
    }
    let mut source_features = BTreeMap::<_, BTreeSet<_>>::new();
    for scope in &source.design_parameter_scopes {
        let id = crate::ids::neutral_feature_id(scope);
        source_features
            .entry(scope.record_index)
            .or_default()
            .insert(id);
    }
    for (record, ids) in source_features {
        if ids.len() != 1 {
            continue;
        }
        if let Some(&placed) = ids
            .first()
            .and_then(|id| identities.get(id.as_str()))
            .and_then(|id| emitted_features.get(id.as_str()))
        {
            bindings.features.insert(record, placed.clone());
        }
    }
    let mut documents = source.design_configurations.clone();
    let [table, rule] = ctx.configuration_basenames();
    for (basename, kind) in [
        (table, DesignConfigurationKind::Table),
        (rule, DesignConfigurationKind::Rule),
    ] {
        if !documents.iter().any(|document| document.kind() == kind) {
            documents.push(DesignConfiguration::try_new(
                format!("{DESIGN_FOLDER}/DesignConfigurationTable.BlobParts/{basename}"),
                kind,
                serde_json::Map::new(),
            )?);
        }
    }
    let targets = ConfigurationTargets {
        parameters: &source.design_parameters,
        owners: &source.design_parameter_owners,
        scopes: &source.design_parameter_scopes,
        neutral_parameters: &expected.model.parameters,
        neutral_features: &expected.model.features,
        bindings: Some(&bindings),
    };
    let decoded = decoded_configurations(&documents, &targets)?;
    let rebased = rebased
        .into_iter()
        .flat_map(|rebased| rebased.parameters.keys())
        .filter_map(|id| identities.get(id.as_str()))
        .filter_map(|id| {
            expected
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id.as_str() == id)
                .map(|parameter| parameter.id.clone())
        })
        .collect::<BTreeSet<_>>();
    let destinations = destinations
        .into_iter()
        .filter_map(|(id, place)| {
            let id = identities.get(id.as_str())?;
            let parameter = emitted_parameters.get(id.as_str())?;
            Some(((*parameter).clone(), place))
        })
        .collect();
    let mut stated = configuration_table::stated_rows(
        &expected,
        &decoded.configurations,
        &targets,
        &rebased,
        ActiveRows::Design,
        losses,
    );
    stated.place_expressions(&expected, targets.neutral_parameters, &destinations, losses);
    let part_numbers = std::mem::take(&mut stated.part_numbers);
    let feature_destinations = features
        .values()
        .map(|(record, id)| (id.clone(), *record))
        .collect();
    let (mut written, plan) = configuration_table::generated(
        &expected,
        &documents,
        &decoded,
        &stated,
        &targets,
        Some(&destinations),
        losses,
    )?;
    written.bind_features(&targets, &feature_destinations, losses)?;
    let names = configuration_entry_names(&written.documents);
    let mut placed = Vec::with_capacity(names.len());
    for (index, (document, (name, fault))) in written.documents.iter().zip(names).enumerate() {
        if let Some(fault) = fault {
            losses.extend(cadmpeg_ir::codec::write::value_unwritable(
                "F3D",
                &fault.reason(),
                [document.id()],
            ));
        }
        let payload = plan.payload(index, document);
        placed.push(DesignConfiguration::try_new(
            name,
            document.kind(),
            payload.into_owned(),
        )?);
    }
    let destination = Destination::synthesized(
        written,
        placed,
        &plan,
        part_numbers,
        &destinations,
        super::root_scope(&expected).map(|scope| scope.definition.clone()),
        DESIGN_BULK,
    )?
    .into_owned();
    let mut members = Vec::with_capacity(destination.documents.len());
    let mut table_name = None;
    let mut rule_name = None;
    for document in &*destination.documents {
        let name = document.entry_name();
        let bytes =
            crate::records::configuration::encode_configuration_payload(name, document.payload())?;
        let relative = name
            .strip_prefix(DESIGN_FOLDER)
            .and_then(|name| name.strip_prefix('/'))
            .ok_or_else(|| {
                CodecError::Malformed(
                    "configuration entry is outside the generated design asset".into(),
                )
            })?;
        members.push((relative.to_owned(), bytes));
        let basename = name.rsplit('/').next().unwrap_or_default().to_owned();
        match document.kind() {
            DesignConfigurationKind::Table if table_name.is_none() => table_name = Some(basename),
            DesignConfigurationKind::Rule if rule_name.is_none() => rule_name = Some(basename),
            _ => {}
        }
    }
    ctx.configuration_names = table_name.zip(rule_name).map(|(table, rule)| [table, rule]);
    losses.extend(ctx.table.write_configuration_root(plan, records)?);
    Ok(Configurations {
        members,
        destination: Some(destination),
        expected,
    })
}
