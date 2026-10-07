// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::transform::Transform;
use std::collections::{BTreeMap, BTreeSet};

use super::super::{OccurrenceDefinition, OccurrenceExpansion, OccurrenceOutput, RealPrecision};
use crate::parameter::{ParameterRecord, Token, TokenValue};

#[test]
fn occurrence_expansion_reports_a_missing_instance_directory_entry() {
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        b"408,3,0,0,0,1;".to_vec(),
        6,
        [
            (408, 0..3),
            (3, 4..5),
            (0, 6..7),
            (0, 8..9),
            (0, 10..11),
            (1, 12..13),
        ]
        .into_iter()
        .map(|(value, span)| Token {
            value: TokenValue::Integer(value),
            span,
        })
        .collect(),
        Vec::new(),
    );
    let entries = BTreeMap::new();
    let parameters = [record];
    let records = BTreeMap::from([(1, &parameters[0])]);
    let definitions = BTreeMap::from([(
        3,
        OccurrenceDefinition {
            members: Vec::new(),
            transform: Transform::identity(),
        },
    )]);
    let neutral_links = BTreeMap::new();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let expansion = OccurrenceExpansion {
        directory: &[],
        parameters: &parameters,
        entries: &entries,
        records: &records,
        definitions: &definitions,
        neutral_links: &neutral_links,
        length_factor: 1.0,
        precision: RealPrecision {
            single_significance: 7,
            double_significance: 15,
        },
        output_limit: 10,
        depth_limit: 10,
        ctx: &ctx,
    };
    let mut path = Vec::new();
    let mut occurrences = Vec::new();
    let mut malformed = BTreeSet::new();
    let mut path_storage = ctx.reserve_scoped(0, "test occurrence path").unwrap();
    let mut placement_storage = ctx.reserve_scoped(0, "test occurrence placement").unwrap();
    expansion
        .expand(
            1,
            Transform::identity(),
            &mut OccurrenceOutput {
                path: &mut path,
                path_storage: &mut path_storage,
                occurrences: &mut occurrences,
                malformed: &mut malformed,
                placement_storage: &mut placement_storage,
            },
        )
        .unwrap();
    assert_eq!(malformed, BTreeSet::from([1]));
    assert!(occurrences.is_empty());
    assert!(path.is_empty());
}

#[test]
fn occurrence_expansion_path_refuses_scoped_storage_before_output() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let directory = [
        crate::test_support::directory_target(1, 408),
        crate::test_support::directory_target(3, 308),
    ];
    let parameters = [ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        6,
        [408, 3, 0, 0, 0, 1]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    )];
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect();
    let records = BTreeMap::from([(1, &parameters[0])]);
    let definitions = BTreeMap::from([(
        3,
        OccurrenceDefinition {
            members: Vec::new(),
            transform: Transform::identity(),
        },
    )]);
    let neutral_links = BTreeMap::new();
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let expansion = OccurrenceExpansion {
            directory: &directory,
            parameters: &parameters,
            entries: &entries,
            records: &records,
            definitions: &definitions,
            neutral_links: &neutral_links,
            length_factor: 1.0,
            precision: RealPrecision {
                single_significance: 7,
                double_significance: 15,
            },
            output_limit: 10,
            depth_limit: 10,
            ctx: &ctx,
        };
        let mut path = Vec::new();
        let mut occurrences = Vec::new();
        let mut malformed = BTreeSet::new();
        let mut path_storage = ctx.reserve_scoped(0, "test occurrence path")?;
        let mut placement_storage = ctx.reserve_scoped(0, "test occurrence placement")?;
        let result = expansion.expand(
            1,
            Transform::identity(),
            &mut OccurrenceOutput {
                path: &mut path,
                path_storage: &mut path_storage,
                occurrences: &mut occurrences,
                malformed: &mut malformed,
                placement_storage: &mut placement_storage,
            },
        );
        assert!(malformed.is_empty());
        assert!(path.is_empty());
        result.map(|()| occurrences)
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges occurrence expansion path slots",
        run,
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "iges occurrence expansion path slots"));
    let occurrences = run(u64::MAX).unwrap();
    assert_eq!(occurrences.len(), 1);
    let wire = serde_json::to_value(&occurrences[0]).unwrap();
    assert_eq!(wire["id"], "iges:product:occurrence#1");
    assert_eq!(wire["definition"], "iges:entity:directory#3");
    assert_eq!(wire["root"], true);
}
