// SPDX-License-Identifier: Apache-2.0
//! Validation of stored-body Base features and current-body identities.

use cadmpeg_ir::features::{Feature, FeatureResultTopology};

use super::NativeData;
use crate::feature::{base, PmDcFeature};
use crate::pmdc::PmDcReference;

pub(super) fn valid(
    ir: &cadmpeg_ir::CadIr,
    data: &NativeData,
    feature: &Feature,
    source: &PmDcFeature,
    result: Option<&FeatureResultTopology>,
) -> bool {
    let mut labels = data.pm_dc_feature_labels.iter().filter(|label| {
        label.identity.segment_token == source.identity.segment_token
            && label.header.owner.record_ordinal() == Some(source.identity.record_ordinal)
    });
    let Some(label) = labels.next() else {
        return false;
    };
    if labels.next().is_some() {
        return false;
    }
    let property_at = |ordinal| {
        let mut matches = data.pm_dc_feature_properties.iter().filter(|property| {
            property.identity.segment_token == source.identity.segment_token
                && property.identity.record_ordinal == ordinal
        });
        let property = matches.next()?;
        matches.next().is_none().then_some(property)
    };
    let Some((input, output)) = base::wrappers(source, label.class_id(), property_at) else {
        return false;
    };
    if label.class_id() == crate::feature::derived::CLASS_ID {
        if !label.participants.references().is_empty() {
            return false;
        }
        let Some(operands) = crate::feature::derived::operands(
            source,
            property_at,
            |ordinal| {
                let mut records = data.records.iter().filter(|record| {
                    record.token == source.identity.segment_token.as_str()
                        && record.ordinal == ordinal
                });
                let record = records.next()?;
                records
                    .next()
                    .is_none()
                    .then_some((record.type_id.clone(), record.payload_len))
            },
            &data.pm_dc_transforms,
        ) else {
            return false;
        };
        if feature.source_properties.get("derived_source") != Some(&operands.source.id())
            || feature.source_properties.get("derived_transform") != Some(&operands.transform.id())
        {
            return false;
        }
    } else if feature.source_properties.contains_key("derived_source")
        || feature.source_properties.contains_key("derived_transform")
    {
        return false;
    }
    let Some(output_reference) = base::surface_reference(output) else {
        return false;
    };
    let mut external_sections = data.meta_sections.iter().filter(|section| {
        section.token == source.identity.segment_token.as_str()
            && section.number == crate::records::MetaSectionNumber::Ten
    });
    let Some(external_section) = external_sections.next() else {
        return false;
    };
    if external_sections.next().is_some()
        || output_reference.index() > external_section.discriminator
        || u64::from(external_section.discriminator).checked_mul(
            cadmpeg_core::decode::u64_from_index(crate::layout::meta_external_reference::LEN),
        ) != Some(external_section.payload_len)
    {
        return false;
    }
    let Some(reference) = base::surface_reference(input) else {
        return false;
    };
    if reference.index() != 0 {
        if !reference.qualified() {
            return false;
        }
        let mut links = data.pm_dc_kernel_entity_links.iter().filter(|link| {
            link.identity.segment_token == input.identity.segment_token
                && Some(link.identity.record_ordinal) == reference.record_ordinal()
        });
        let Some(link) = links.next() else {
            return false;
        };
        if links.next().is_some()
            || link.state != 0
            || link.selection_source != input.header.source_index
            || !link.body.qualified()
            || !data.records.iter().any(|record| {
                record.token == input.identity.segment_token.as_str()
                    && Some(record.ordinal) == link.body.record_ordinal()
                    && record.type_id
                        == crate::pmdc::type_id_string(crate::kernel::KERNEL_RECORD_TYPE_ID)
            })
        {
            return false;
        }
    } else if reference.qualified() {
        return false;
    }
    let Some(result) = result else { return false };
    if ir
        .model
        .feature_result_topologies
        .iter()
        .filter(|candidate| candidate.output_of == feature.id)
        .count()
        != 1
        || result.native_ref.as_deref() != Some(output.id().as_str())
        || result.bodies().len() != 1
        || result.bodies()[0].as_str() != output.id()
        || !result.faces().is_empty()
        || !result.edges().is_empty()
        || !result.vertices().is_empty()
    {
        return false;
    }
    let Some(link_id) = feature.source_properties.get(base::RESULT_LINK_PROPERTY) else {
        return false;
    };
    let mut links = data
        .pm_dc_kernel_entity_links
        .iter()
        .filter(|link| link.id() == *link_id);
    let Some(link) = links.next() else {
        return false;
    };
    if links.next().is_some() || !base::unique_link_header(link, &data.pm_dc_kernel_entity_links) {
        return false;
    }
    let crate::native::ActiveCarrierRecord::Selected {
        segment_token,
        record_ordinal,
        selected_key,
        ..
    } = &data.active_carrier
    else {
        return false;
    };
    let mut metadata = data
        .metadata
        .iter()
        .filter(|meta| meta.token == *segment_token && meta.kind == "pm_brep");
    let Some(target) = metadata.next() else {
        return false;
    };
    if metadata.next().is_some()
        || data
            .metadata
            .iter()
            .filter(|meta| meta.segment_id == target.segment_id)
            .count()
            != 1
        || data
            .metadata
            .iter()
            .filter(|meta| {
                matches!(meta.kind.as_str(), "pm_brep" | "mb_brep")
                    && meta.display_name == target.display_name
            })
            .count()
            != 1
        || data
            .pm_dc_model_roots
            .iter()
            .filter(|root| {
                root.identity.segment_token == source.identity.segment_token
                    && root.brep_segment == target.display_name
            })
            .count()
            != 1
        || data
            .pm_dc_model_roots
            .iter()
            .filter(|root| {
                root.identity.segment_token == source.identity.segment_token
                    && data.metadata.iter().any(|meta| {
                        meta.kind == "pm_brep" && meta.display_name == root.brep_segment
                    })
            })
            .count()
            != 1
    {
        return false;
    }
    if PmDcReference::from_packed(*selected_key) != link.body {
        return false;
    }
    let Some(body) = base::current_body(
        output,
        link,
        segment_token,
        *record_ordinal,
        &data.body_native_keys,
    ) else {
        return false;
    };
    ir.model
        .bodies
        .iter()
        .any(|candidate| candidate.id == body.body)
}
