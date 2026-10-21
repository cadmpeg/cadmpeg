// SPDX-License-Identifier: Apache-2.0
//! Construction reductions preserve independent parameter and solved content.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{
    target::TargetRequest, ArenaCoverage, ArenaDisposition, EncodeInput, Encoder,
};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::Severity;

pub(in crate::writer::tests) fn source_less_operation(
    definition: FeatureDefinition,
    name: &str,
) -> cadmpeg_ir::CadIr {
    let geometry = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = crate::SldprtCodec
        .decode(&mut Cursor::new(geometry), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(ir.model.features.is_empty());
    assert!(ir.model.parameters.is_empty());
    ir.source = None;
    ir.native = cadmpeg_ir::native::Native::default();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:feature#fresh-operation")
            .unwrap(),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    });
    ir
}

pub(in crate::writer::tests) fn assert_independent_parameter_reduction(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
    feature_index: usize,
    reason: &str,
) -> DecodeResult {
    let parameters = ir.model.parameters.clone();
    let feature = &ir.model.features[feature_index];
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)
        .expect("a valid conflicting construction is reduced within synthesis");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "writer.design-reduced"
            && loss.severity == Severity::Error
            && loss.message.contains(feature.id.as_str())
            && loss.message.contains(reason)
    }));
    let ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("the definition reduction declares its coverage");
    };
    assert_eq!(coverage.features, ArenaDisposition::Reported);
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert_eq!(ir.model.parameters, parameters);
    let regenerated = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the held solved geometry and independent parameters remain readable");
    let output = regenerated.ir();
    let written_feature = output
        .model
        .features
        .iter()
        .find(|written| written.name == feature.name)
        .unwrap();
    assert!(matches!(
        written_feature.definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    ));
    let native = crate::test_support::native::sldprt_native(output);
    let native_feature = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|written| Some(written.name.as_str()) == feature.name.as_deref())
        .unwrap();
    assert_eq!(native_feature.xml_tag, "Feature");
    assert_eq!(native_feature.kind, "Feature");
    assert_eq!(output.model.parameters.len(), parameters.len());
    for expected in &parameters {
        let owner = ir
            .model
            .features
            .iter()
            .find(|owner| Some(&owner.id) == expected.owner.as_ref())
            .unwrap();
        let written_owner = output
            .model
            .features
            .iter()
            .find(|written| written.name == owner.name)
            .unwrap();
        let actual = output
            .model
            .parameters
            .iter()
            .find(|actual| {
                actual.owner.as_ref() == Some(&written_owner.id) && actual.name == expected.name
            })
            .unwrap();
        assert_eq!(actual.expression, expected.expression);
        assert_eq!(actual.value, expected.value);
        assert_eq!(actual.driving, expected.driving);
        assert_eq!(actual.display, expected.display);
    }
    macro_rules! same_counts { ($($arena:ident),* $(,)?) => {$(
        assert_eq!(output.model.$arena.len(), ir.model.$arena.len(), stringify!($arena));
    )*}; }
    same_counts!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves
    );
    let point_values = |document: &cadmpeg_ir::CadIr| {
        let mut points = document
            .model
            .points
            .iter()
            .map(|point| {
                let point = point.position().get();
                [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()]
            })
            .collect::<Vec<_>>();
        points.sort_unstable();
        points
    };
    assert_eq!(point_values(output), point_values(ir));
    regenerated
}
