// SPDX-License-Identifier: Apache-2.0
//! Native sheet-metal operands and child identities.

use cadmpeg_ir::report::check::{Check, Finding};

use super::{finding, unique, NativeData};

pub(super) fn records(data: &NativeData, findings: &mut Vec<Finding>) {
    let raw = cadmpeg_core::decode::unique_index(data.records.iter().map(|record| {
        (
            (record.token.as_str(), record.ordinal),
            record.type_id.as_str(),
        )
    }));
    unique(
        findings,
        data.pm_dc_sheet_metal_styles
            .iter()
            .map(|r| (r.identity.segment_token.as_str(), r.identity.record_ordinal)),
        "Inventor sheet-metal style",
    );
    unique(
        findings,
        data.pm_dc_sheet_metal_flanges
            .iter()
            .map(|r| (r.identity.segment_token.as_str(), r.identity.record_ordinal)),
        "Inventor sheet-metal flange",
    );
    for style in &data.pm_dc_sheet_metal_styles {
        if !crate::feature::sheet_metal::style::valid(style, |ordinal| {
            raw.get(&(style.identity.segment_token.as_str(), ordinal))
                .copied()
                .flatten()
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor sheet-metal style has an invalid prefix or operand type".into(),
                Some(style.id()),
            ));
        }
    }
    for flange in &data.pm_dc_sheet_metal_flanges {
        if !crate::feature::sheet_metal::flange::valid(flange, |ordinal| {
            raw.get(&(flange.identity.segment_token.as_str(), ordinal))
                .copied()
                .flatten()
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor sheet-metal flange has an invalid child or context reference".into(),
                Some(flange.id()),
            ));
        }
    }
}

pub(super) fn definitions(ir: &cadmpeg_ir::CadIr, data: &NativeData, findings: &mut Vec<Finding>) {
    use crate::feature::sheet_metal::{operands, projection};
    use cadmpeg_ir::features::{
        BooleanOp, EdgeSelection, FeatureDefinition, FeatureOperation, ParameterValue,
        PlanarProfileRef,
    };
    let properties =
        cadmpeg_core::decode::unique_index(data.pm_dc_feature_properties.iter().map(|p| {
            (
                (
                    p.identity.segment_token.as_str().to_owned(),
                    p.identity.record_ordinal,
                ),
                p,
            )
        }));
    let parameters = cadmpeg_core::decode::unique_index(data.pm_dc_parameters.iter().map(|p| {
        (
            (
                p.identity.segment_token.as_str().to_owned(),
                p.identity.record_ordinal,
            ),
            p,
        )
    }));
    let values = cadmpeg_core::decode::unique_index(
        ir.model
            .parameters
            .iter()
            .filter_map(|p| Some((p.native_ref.as_deref()?, p))),
    );
    let sources =
        cadmpeg_core::decode::unique_index(data.pm_dc_features.iter().map(|p| (p.id(), p)));
    let source_of = |feature: &cadmpeg_ir::features::Feature| {
        sources
            .get(feature.native_ref.as_deref()?)?
            .as_ref()
            .copied()
    };
    let property = |token: &str, reference: crate::pmdc::PmDcReference| {
        properties
            .get(&(token.to_owned(), reference.record_ordinal()?))?
            .as_ref()
            .copied()
    };
    let length = |token: &str, reference: crate::pmdc::PmDcReference| {
        let parameter = parameters
            .get(&(token.to_owned(), reference.record_ordinal()?))?
            .as_ref()?;
        match values
            .get(parameter.id().as_str())?
            .as_ref()?
            .value
            .as_ref()?
        {
            ParameterValue::Length(length) if length.get() >= 0.0 => Some(*length),
            _ => None,
        }
    };
    let attached = |feature: &cadmpeg_ir::features::Feature,
                    source: &crate::feature::PmDcFeature| {
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
            profile,
            thickness,
            side,
            op: BooleanOp::Join,
            attachment:
                selected_attachment @ EdgeSelection::Historical {
                    state,
                    edges,
                    native,
                },
            inside_radius,
            relief,
            transition,
        }) = feature.evaluation.definition()
        else {
            return None;
        };
        let token = source.identity.segment_token.as_str();
        let style = projection::style(&data.pm_dc_sheet_metal_styles, token)?;
        let expected =
            operands::plate(source, style, |r| property(token, r), |r| length(token, r))?;
        if expected.thickness != *thickness
            || expected.side != *side
            || expected.inside_radius != *inside_radius
            || expected.relief != *relief
            || expected.transition != *transition
        {
            return None;
        }
        let selection = property(token, *source.properties.references().get(12)?)?;
        let topology = ir
            .model
            .feature_input_topologies
            .iter()
            .find(|topology| topology.id == *state)?;
        let owner_matches = topology.input_of == feature.id || ir.model.features.iter().any(|bend| {
            bend.id == topology.input_of && matches!(bend.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { plate, attachment, .. })
                if *plate == feature.id && attachment == selected_attachment)
        });
        if !owner_matches
            || edges.iter().any(|edge| {
                !topology.edge_geometry.iter().any(|geometry| {
                    geometry.edge == *edge
                        && geometry.start.is_some()
                        && geometry.end.is_some()
                        && geometry.tangent.is_some()
                })
            })
        {
            return None;
        }
        if native.as_str() != selection.id()
            || edges.len() != operands::selection_count(source, |r| property(token, r))?
        {
            return None;
        }
        let mut parents = data.pm_dc_sheet_metal_flanges.iter().filter_map(|flange| {
            if flange.identity.segment_token.as_str() != token {
                return None;
            }
            let member = projection::members(
                flange,
                &data.pm_dc_features,
                &data.pm_dc_feature_labels,
                |ordinal| {
                    data.pm_dc_sketches
                        .iter()
                        .filter(|s| {
                            s.identity.segment_token.as_str() == token
                                && s.identity.record_ordinal == ordinal
                        })
                        .count()
                        == 1
                },
            )?;
            (member.plate == source.identity.record_ordinal).then_some(member)
        });
        let parent = parents.next()?;
        if parents.next().is_some() {
            return None;
        }
        let native_sketch = data.pm_dc_sketches.iter().find(|s| {
            s.identity.segment_token.as_str() == token && s.identity.record_ordinal == parent.sketch
        })?;
        let PlanarProfileRef::SketchRegions { sketch, .. } = profile else {
            return None;
        };
        if !ir.model.sketches.iter().any(|s| {
            s.id == *sketch && s.native_ref.as_deref() == Some(native_sketch.id().as_str())
        }) {
            return None;
        }
        Some(parent)
    };
    for feature in &ir.model.features {
        let valid = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate { .. }) => {
                source_of(feature)
                    .and_then(|source| attached(feature, source))
                    .is_some()
            }
            FeatureDefinition::Operation(
                actual @ FeatureOperation::SheetMetalBend { plate, .. },
            ) => (|| {
                let source = source_of(feature)?;
                let mut targets = ir.model.features.iter().filter(|f| f.id == *plate);
                let target = targets.next()?;
                if targets.next().is_some() || !feature.dependencies.contains(plate) {
                    return None;
                }
                let plate_source = source_of(target)?;
                let parent = attached(target, plate_source)?;
                if parent.bend != source.identity.record_ordinal {
                    return None;
                }
                let expected = projection::bend_definition(target, plate_source, source, |r| {
                    property(source.identity.segment_token.as_str(), r)
                })?;
                if let (
                    FeatureOperation::SheetMetalBend {
                        attachment: expected,
                        ..
                    },
                    FeatureOperation::SheetMetalBend {
                        attachment: actual, ..
                    },
                ) = (&expected, actual)
                {
                    let EdgeSelection::Historical {
                        state,
                        edges,
                        native,
                    } = actual
                    else {
                        return None;
                    };
                    let topology = ir.model.feature_input_topologies.iter().find(|topology| {
                        topology.id == *state && topology.input_of == feature.id
                    })?;
                    let EdgeSelection::Historical {
                        edges: plate_edges,
                        native: plate_native,
                        ..
                    } = expected
                    else {
                        return None;
                    };
                    if edges != plate_edges
                        || native != plate_native
                        || edges.iter().any(|edge| {
                            !topology
                                .edge_geometry
                                .iter()
                                .any(|geometry| geometry.edge == *edge)
                        })
                    {
                        return None;
                    }
                }
                (expected == *actual).then_some(())
            })()
            .is_some(),
            _ => continue,
        };
        if !valid {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral sheet-metal operands do not match their PmDc graph".into(),
                Some(feature.id.as_str().to_owned()),
            ));
        }
    }
}
