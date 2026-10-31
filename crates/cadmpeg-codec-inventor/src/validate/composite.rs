// SPDX-License-Identifier: Apache-2.0
//! Validation of stored-body composites against the shared body join.

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, FeatureResultTopology};
use cadmpeg_ir::ids::BodyId;

use super::NativeData;
use crate::feature::body_selection::Scope;
use crate::feature::composite::{
    definition, operands, resolve, unique_owner, Joins, Operands, CONTROL_PROPERTY,
    RESULT_SET_PROPERTY, SOURCE_SET_PROPERTY, SOURCE_TAG,
};
use crate::feature::PmDcFeature;

/// A composite's typed slot records and its ordered result links with bodies.
type Resolved<'a> = (Operands<'a>, Vec<(String, BodyId)>);

/// Resolve the composite that `raw` owns with the decoder's predicate.
pub(super) fn evaluate<'a>(
    ir: &cadmpeg_ir::CadIr,
    data: &'a NativeData,
    raw: &PmDcFeature,
) -> Result<Resolved<'a>, &'static str> {
    if !unique_owner(&data.pm_dc_feature_labels, raw) {
        return Err("label owner requires one feature label");
    }
    let operands = operands(raw, |ordinal| {
        let mut matches = data.pm_dc_feature_properties.iter().filter(|property| {
            property.identity.segment_token == raw.identity.segment_token
                && property.identity.record_ordinal == ordinal
        });
        let property = matches.next()?;
        matches.next().is_none().then_some(property)
    })?;
    let scope = Scope {
        metadata: &data.metadata,
        roots: &data.pm_dc_model_roots,
    };
    let joins = Joins::new(
        &data.pm_dc_kernel_entity_links,
        &data.composite_member_links,
        &data.body_native_keys,
        &data.active_carrier,
        &scope,
    );
    let carrier_type = crate::pmdc::type_id_string(crate::kernel::KERNEL_RECORD_TYPE_ID);
    let bodies = resolve(
        &operands,
        &joins,
        |token, ordinal| {
            data.records.iter().any(|record| {
                record.token == token.as_str()
                    && record.ordinal == ordinal
                    && record.type_id == carrier_type
            })
        },
        |body| {
            ir.model
                .bodies
                .iter()
                .any(|candidate| &candidate.id == body)
        },
    )?;
    Ok((operands, bodies))
}

/// Whether the feature carries the projection its raw record resolves to:
/// stored geometry with its result topology when the join resolves, otherwise
/// the native definition with no result.
pub(super) fn valid(
    ir: &cadmpeg_ir::CadIr,
    data: &NativeData,
    feature: &Feature,
    raw: &PmDcFeature,
) -> bool {
    let results = ir
        .model
        .feature_result_topologies
        .iter()
        .filter(|result| result.output_of == feature.id)
        .collect::<Vec<&FeatureResultTopology>>();
    let properties = [SOURCE_SET_PROPERTY, RESULT_SET_PROPERTY, CONTROL_PROPERTY];
    match evaluate(ir, data, raw) {
        Ok((operands, bodies)) => {
            let [result] = results.as_slice() else {
                return false;
            };
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
            ) && feature.source_tag.as_deref() == Some(SOURCE_TAG)
                && feature.source_properties.len() == properties.len()
                && [
                    (SOURCE_SET_PROPERTY, operands.source.id()),
                    (RESULT_SET_PROPERTY, operands.result.id()),
                    (CONTROL_PROPERTY, operands.control.id()),
                ]
                .iter()
                .all(|(key, id)| feature.source_properties.get(*key) == Some(id))
                && result.native_ref.as_deref() == Some(operands.result.id().as_str())
                && result.bodies().len() == bodies.len()
                && result
                    .bodies()
                    .iter()
                    .zip(&bodies)
                    .all(|(body, (link, _))| body.as_str() == link)
                && result.faces().is_empty()
                && result.edges().is_empty()
                && result.vertices().is_empty()
        }
        Err(_) => {
            results.is_empty()
                && feature.evaluation.definition()
                    == &definition(unique_owner(&data.pm_dc_feature_labels, raw))
                && properties
                    .iter()
                    .all(|key| !feature.source_properties.contains_key(*key))
        }
    }
}
