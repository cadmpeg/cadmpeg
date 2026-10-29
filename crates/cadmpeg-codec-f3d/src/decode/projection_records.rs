// SPDX-License-Identifier: Apache-2.0
//! Record identities in reports about missing neutral projections.

use cadmpeg_ir::document::CadIr;
use std::collections::HashSet;
use std::fmt;

pub(super) fn neutral_refs(ir: &CadIr) -> HashSet<&str> {
    ir.model
        .features
        .iter()
        .filter_map(|record| record.native_ref.as_deref())
        .chain(
            ir.model
                .parameters
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .sketches
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketches
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .sketch_constraints
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .collect()
}

pub(super) struct Names<'a, I> {
    ids: I,
    projected: Option<&'a HashSet<&'a str>>,
}

impl<'a, I> Names<'a, I> {
    pub(super) fn new(ids: I, projected: &'a HashSet<&'a str>) -> Self {
        Self {
            ids,
            projected: Some(projected),
        }
    }
    pub(super) fn all(ids: I) -> Self {
        Self {
            ids,
            projected: None,
        }
    }
}

impl<'a, I: Iterator<Item = &'a str> + Clone> fmt::Display for Names<'a, I> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut separator = "";
        for id in self.ids.clone().filter(|id| {
            self.projected
                .is_none_or(|projected| !projected.contains(id))
        }) {
            write!(out, "{separator}{id}")?;
            separator = ", ";
        }
        Ok(())
    }
}

pub(super) fn missing_feature_scopes<'a>(
    ir: &CadIr,
    native: &'a crate::native::F3dNative,
) -> Result<Vec<&'a str>, cadmpeg_core::CodecError> {
    let projected = ir
        .model
        .features
        .iter()
        .filter_map(|record| record.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let authored = match crate::design::feature_project::authored_scope_ordinals_per_stream(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(ordinals) => Some(ordinals),
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => None,
    };
    Ok(native.design_parameter_scopes.iter().filter(|scope| {
        let is_authored = authored.as_ref().map_or_else(
            || scope.assembly_alignment().and_then(crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index).is_none(),
            |ordinals| {
                let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                ordinals.contains_key(&(stream, scope.record_index))
            },
        );
        is_authored && !projected.contains(scope.id.as_str())
    }).map(|scope| scope.id.as_str()).collect())
}

/// Native operand records whose owning feature has no delivered input topology.
/// A typed operand remains document content, but its references must not be
/// mistaken for a completed neutral historical-topology projection.
pub(super) fn missing_input_topology_records<'a>(
    ir: &CadIr,
    native: &'a crate::native::F3dNative,
) -> Vec<(&'static str, &'a str)> {
    let input_features = ir
        .model
        .feature_input_topologies
        .iter()
        .map(|topology| &topology.input_of)
        .collect::<HashSet<_>>();
    let held_scopes = ir
        .model
        .features
        .iter()
        .filter(|feature| input_features.contains(&feature.id))
        .filter_map(|feature| feature.native_ref.as_deref())
        .filter_map(|id| {
            native
                .design_parameter_scopes
                .iter()
                .find(|scope| scope.id == id)
        })
        .filter_map(|scope| Some((crate::ids::native_stream(&scope.id)?, scope.record_index)))
        .collect::<HashSet<_>>();
    let missing = |id: &str, scope_record_index| {
        crate::ids::native_stream(id)
            .is_none_or(|stream| !held_scopes.contains(&(stream, scope_record_index)))
    };
    let mut records = Vec::new();
    macro_rules! direct {
        ($($arena:ident),* $(,)?) => {$({
            for record in &native.$arena {
                if missing(&record.id, record.scope_record_index) {
                    records.push((stringify!($arena), record.id.as_str()));
                }
            }
        })*};
    }
    direct!(
        design_construction_operand_groups,
        design_edge_operands,
        design_edge_identity_operands,
        design_edge_treatment_vertex_operands,
        design_entity_selection_operands,
        design_face_operands,
        design_body_recipe_operands,
        design_extrude_selection_groups,
        design_loft_legacy_body_carriers,
    );
    let groups = native
        .design_construction_operand_groups
        .iter()
        .filter_map(|group| {
            Some((
                (crate::ids::native_stream(&group.id)?, group.record_index),
                group.scope_record_index,
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    for identity in &native.design_construction_operand_identities {
        let scope = crate::ids::native_stream(&identity.id)
            .and_then(|stream| groups.get(&(stream, identity.group_record_index)));
        if scope.is_none_or(|scope| missing(&identity.id, *scope)) {
            records.push((
                "design_construction_operand_identities",
                identity.id.as_str(),
            ));
        }
    }
    let selection_groups = native
        .design_extrude_selection_groups
        .iter()
        .filter_map(|group| {
            Some((
                (crate::ids::native_stream(&group.id)?, group.record_index),
                group.scope_record_index,
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    for member in &native.design_extrude_selection_members {
        let scope = crate::ids::native_stream(&member.id)
            .and_then(|stream| selection_groups.get(&(stream, member.group_record_index)));
        if scope.is_none_or(|scope| missing(&member.id, *scope)) {
            records.push(("design_extrude_selection_members", member.id.as_str()));
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omission_names_only_missing_records_in_source_order() {
        let projected = HashSet::from(["native:held"]);
        let source = ["native:missing-first", "native:held", "native:missing-last"];
        assert_eq!(
            Names::new(source.into_iter(), &projected).to_string(),
            "native:missing-first, native:missing-last"
        );
        assert_eq!(
            Names::all(source.into_iter()).to_string(),
            source.join(", ")
        );
        assert_eq!(
            Names::new(["native:held"].into_iter(), &projected).to_string(),
            ""
        );
    }

    #[test]
    fn input_topology_omission_report_names_the_typed_operand_record() {
        let mut native = crate::native::F3dNative::default();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#7",
            crate::records::feature::scope::DesignFeatureKind::WorkPoint,
            7,
        );
        native.design_parameter_scopes.push(scope);
        let id = "f3d:Design/BulkStream.dat:construction-group#9";
        native.design_construction_operand_groups.push(
            serde_json::from_value(serde_json::json!({
                "id": id,
                "scope_record_index": 7,
                "scope_reference_ordinal": 0,
                "record_index": 9,
                "class_tag": "300",
                "members": [10],
                "frame": {"opaque_index": 1, "opaque_scalar": 1.0, "variant": false},
                "role": 0x10_0000_0000u64,
                "paired_class_tag": "258"
            }))
            .unwrap(),
        );
        let ir = CadIr::empty();
        assert_eq!(
            missing_input_topology_records(&ir, &native),
            vec![("design_construction_operand_groups", id)]
        );
        let mut report =
            cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer {
                scope: ir.scope,
                geometry_transferred: false,
            });
        super::super::report_design_projection_gaps(
            &mut report,
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            &native,
        )
        .unwrap();
        assert!(report.losses.iter().any(|loss| {
            loss.code == crate::loss::F3dLossCode::FeatureDefinitionIncomplete.kind()
                && loss.message.contains(id)
                && loss.message.contains("feature-input topology")
        }));
    }

    #[test]
    fn unprojected_parameter_report_names_the_typed_source_record() {
        use crate::records::parameters::{
            DesignParameter, DesignParameterContent, DesignParameterDiscriminator,
            DesignParameterSource,
        };
        let id = "f3d:design:design-parameter#7";
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(
            DesignParameter::authored(DesignParameterContent {
                id: id.into(),
                class_tag: "305".to_owned().try_into().unwrap(),
                record_index: 7,
                source_ordinal: 0,
                source: DesignParameterSource::authored(
                    "User Parameter".into(),
                    None,
                    Some(DesignParameterDiscriminator::Code0),
                )
                .unwrap(),
                expression: "1 mm".into(),
                unit: Some("mm".into()),
                name: "d1".into(),
                evaluated_value: 0.1,
            })
            .unwrap(),
        );
        let ir = CadIr::empty();
        let mut report =
            cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer {
                scope: ir.scope,
                geometry_transferred: false,
            });
        super::super::report_design_projection_gaps(
            &mut report,
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            &native,
        )
        .unwrap();
        let loss = report
            .losses
            .iter()
            .find(|loss| loss.code == crate::loss::F3dLossCode::ParameterUnprojected.kind())
            .unwrap();
        assert!(loss.message.contains(id), "{}", loss.message);
    }
}
