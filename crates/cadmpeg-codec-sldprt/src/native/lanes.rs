// SPDX-License-Identifier: Apache-2.0
use super::SldprtNative;
use crate::records::FeatureInputLane;
use crate::resolved_features::assembly::is_supplemental_config_lane;
use crate::resolved_features::bindings::finalize_lane_bindings;
use crate::resolved_features::class_sources::ClassSources;
use cadmpeg_core::decode::DecodeContext;

pub(crate) fn expected_lanes<'a>(
    ctx: &DecodeContext<'_>,
    native: &'a SldprtNative,
    sources: &mut crate::resolved_features::body_sources::BodySources<'_>,
) -> Result<Vec<(&'a FeatureInputLane, FeatureInputLane)>, cadmpeg_ir::NativeConvertError> {
    sources.capture_classes(ctx, &native.feature_input_lanes)?;
    let class_sources = sources.class_sources();

    let mut expected_primary_lanes: Vec<FeatureInputLane> = native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .cloned()
        .collect();
    let mut expected_supplemental_lanes: Vec<FeatureInputLane> = native
        .feature_input_lanes
        .iter()
        .filter(|lane| is_supplemental_config_lane(lane))
        .cloned()
        .collect();
    for lane in expected_primary_lanes
        .iter_mut()
        .chain(&mut expected_supplemental_lanes)
    {
        if lane.native_payload.is_empty() {
            continue;
        }
        let scalars = crate::resolved_features::scalars::named_scalars(
            &lane.native_payload,
            &lane.id,
            &lane.names,
        )?
        .records;
        rebuild_scalar_relations(&class_sources, lane, scalars)?;
    }
    Ok(expected_lane_pairs_impl(
        native,
        expected_primary_lanes,
        expected_supplemental_lanes,
        ctx,
        sources,
    )?
    .collect())
}

fn expected_lane_pairs_impl<'a>(
    native: &'a SldprtNative,
    mut expected_primary_lanes: Vec<FeatureInputLane>,
    mut expected_supplemental_lanes: Vec<FeatureInputLane>,
    ctx: &DecodeContext<'_>,
    sources: &mut crate::resolved_features::body_sources::BodySources<'_>,
) -> Result<
    impl Iterator<Item = (&'a FeatureInputLane, FeatureInputLane)> + 'a,
    cadmpeg_ir::NativeConvertError,
> {
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &native.feature_histories,
        &mut expected_primary_lanes,
        sources,
    )?;
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &native.feature_histories,
        &mut expected_supplemental_lanes,
        sources,
    )?;
    for (expected_lane, actual_lane) in expected_supplemental_lanes.iter_mut().zip(
        native
            .feature_input_lanes
            .iter()
            .filter(|lane| is_supplemental_config_lane(lane)),
    ) {
        // Detached supplemental objects acquire owners before later projection
        // can replace an unresolved sketch definition. The final model does not
        // retain that intermediate state. Treat the stored owner partition as
        // derived provenance, then re-derive every byte-backed local link from it.
        for (expected, actual) in expected_lane
            .sketch_entities
            .iter_mut()
            .zip(&actual_lane.sketch_entities)
        {
            expected.feature_ref.clone_from(&actual.feature_ref);
            expected.links = None;
        }
        for (expected, actual) in expected_lane
            .references
            .iter_mut()
            .zip(&actual_lane.references)
        {
            expected.feature_ref.clone_from(&actual.feature_ref);
        }
        for (expected, actual) in expected_lane.scalars.iter_mut().zip(&actual_lane.scalars) {
            expected.feature_ref.clone_from(&actual.feature_ref);
        }
        finalize_lane_bindings(ctx, &native.feature_histories, expected_lane, sources)?;
    }
    Ok(native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .zip(expected_primary_lanes)
        .chain(
            native
                .feature_input_lanes
                .iter()
                .filter(|lane| is_supplemental_config_lane(lane))
                .zip(expected_supplemental_lanes),
        ))
}

fn rebuild_scalar_relations(
    class_sources: &ClassSources,
    lane: &mut FeatureInputLane,
    scalars: Vec<crate::records::FeatureInputScalar>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    lane.scalars = scalars;
    lane.relation_bindings = crate::resolved_features::markers::relation_bindings(
        class_sources,
        &lane.id,
        &lane.classes,
        &lane.scalars,
        &[],
    )?;
    lane.references = crate::resolved_features::markers::reference_cells(
        class_sources,
        &lane.scalars,
        &lane.classes,
    )?
    .records;
    Ok(())
}
