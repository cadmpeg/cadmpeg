//! Candidate rejection and interval backing lifetimes.

use super::super::*;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};

#[test]
fn legacy_rejected_relation_signatures_release_candidate_storage() {
    let long = format!("({} : #In {},bad) : Real", "#".repeat(512), "T".repeat(512));
    for source in [
        "(#1_ : #In Real,bad) : Real",
        "(#1_ : #In Real,#1_ : #In Real) : Real",
        "(#1_ : #In Real) : VoidType",
        "(#1_ : #Out Real) : Real",
        "(#1_ : #Out Real,#2_ : #Out Real) : VoidType",
        long.as_str(),
    ] {
        crate::test_support::with_retained_limit(0, |ctx| {
            for _ in 0..64 {
                assert!(parse_relation_signature(ctx, source)?.is_none(), "{source}");
            }
            assert_eq!(ctx.resource_refusal(), None);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("rejected owned clauses retain no storage");

        crate::test_support::with_materialized_limit(4096, |ctx| {
            let mut parent = ctx.reserve_scoped(0, "test rejected signature parent")?;
            parent.with_storage(|| {
                for _ in 0..64 {
                    assert!(parse_relation_signature(ctx, source)?.is_none(), "{source}");
                }
                // No rejected candidate or borrowed-name backing remains live.
                let marker = ctx.copy_slice(&[0_u8; 4096], "test after rejected signatures")?;
                assert_eq!(marker.len(), 4096);
                assert_eq!(ctx.resource_refusal(), None);
                Ok::<_, cadmpeg_core::CodecError>(())
            })
        })
        .expect("rejected clauses release storage inside a live parent");
    }
}

#[test]
fn legacy_relation_signatures_promote_only_accepted_storage() {
    for (source, child_bytes, output) in [
        ("(#1_ : #In Real) : Real", 11, None),
        (
            "(#2_ : #Out Real,#1_ : #In Real) : VoidType\n",
            14,
            Some("#2_"),
        ),
    ] {
        // One input allocates four slots. Only the accepted strings escape.
        let retained = 4 * std::mem::size_of::<LegacyRelationParameter>() + child_bytes;
        let assert_signature = |signature: &LegacyRelationSignature| {
            assert_eq!(signature.inputs.len(), 1);
            assert_eq!(signature.inputs[0].parameter, "#1_");
            assert_eq!(signature.inputs[0].value_type, "Real");
            assert_eq!(
                signature.output().map(|value| value.parameter.as_str()),
                output
            );
            assert_eq!(
                signature.result_type(),
                if output.is_some() { "VoidType" } else { "Real" }
            );
            if let Some(output) = signature.output() {
                assert_eq!(output.value_type, "Real");
            }
        };
        let signature = crate::test_support::with_retained_limit(
            u64::try_from(retained).expect("accepted signature bytes"),
            |ctx| parse_relation_signature(ctx, source),
        )
        .expect("exact retained storage excludes the name table")
        .expect("complete signature");
        assert_signature(&signature);

        crate::test_support::with_materialized_limit(4096, |ctx| {
            let mut parent = ctx.reserve_scoped(0, "test accepted signature parent")?;
            parent.with_storage(|| {
                let signature = parse_relation_signature(ctx, source)?.expect("complete signature");
                assert_signature(&signature);
                let marker = ctx.copy_slice(
                    &[0_u8; 4096][..4096 - retained],
                    "test after accepted signature",
                )?;
                assert_eq!(marker.len() + retained, 4096);
                assert_eq!(ctx.resource_refusal(), None);
                Ok::<_, cadmpeg_core::CodecError>(())
            })
        })
        .expect("the parent receives the candidate without its borrowed-name table");

        crate::test_support::with_retained_limit(0, |ctx| {
            let mut parent = ctx.reserve_scoped(0, "test signature handoff parent")?;
            let signature = parent.with_storage(|| parse_relation_signature(ctx, source))?;
            assert_signature(signature.as_ref().expect("complete signature"));
            assert_eq!(ctx.resource_refusal(), None);
            drop(signature);
            drop(parent);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("a live parent receives accepted storage without a retained charge");
    }
}

#[test]
fn legacy_text_role_index_waits_for_a_complete_text_query() {
    let roles = (0..4096)
        .map(|index| LegacyRoleSelector {
            offset: index * 7,
            entity_id: 1,
            name: LegacyRoleName::Literal("name".to_owned()),
            encoding: LegacyRoleSelectorEncoding::Paged,
            selector: 1,
            field_code: None,
        })
        .collect::<Vec<_>>();
    for bytes in [&[][..], TEXT_OPEN, &[0xe8, 0, 0x12, 1, 2, b'x'][..]] {
        let _probe =
            RefusalProbe::arm(ResourceDimension::WorkUnits, "catia_legacy_role_ends", None);
        crate::test_support::with_work_limit(u64::MAX, |ctx| {
            let (fields, storage) = parse_text_fields(ctx, bytes, 0, bytes.len(), 1, &roles)?;
            let fields = storage.commit_value(fields)?;
            assert!(fields.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("absent or incomplete text fields do not query role membership");
    }
}

#[test]
fn legacy_interval_handoffs_release_only_discarded_backing() {
    let mut bytes = Vec::new();
    super::identity(&mut bytes, 1);
    bytes.extend_from_slice(&[5, b'n', b'a', b'm', b'e', 0xd1, 0]);
    bytes.extend_from_slice(TEXT_OPEN);
    bytes.extend_from_slice(&[6, b'L', b'a', b'b', b'e', b'l', 0xfe]);
    bytes.extend_from_slice(TYPE_OPEN);
    bytes.extend_from_slice(&[5, b'R', b'e', b'a', b'l', 0x83]);
    bytes.extend_from_slice(NAMED_SCALAR_OPEN);
    bytes.push(0xe7);
    bytes.extend_from_slice(STRING_OPEN);
    bytes.extend_from_slice(&[2, b'x']);
    bytes.extend_from_slice(INTEGER_OPEN);
    bytes.push(0x8c);
    bytes.extend_from_slice(CATALOG_OPEN);

    // Each of the six output vectors and the run vector has four slots.
    // Strings: two role names (8), field (5), type (4), value (1),
    // and three bound names (15). Total child bytes: 33.
    let retained = 4
        * (std::mem::size_of::<LegacyEntityRun>()
            + std::mem::size_of::<LegacyRoleSelector>()
            + std::mem::size_of::<LegacyTextField>()
            + std::mem::size_of::<LegacyTypeDescriptor>()
            + std::mem::size_of::<LegacyScalarValue>()
            + std::mem::size_of::<LegacyStringValue>()
            + std::mem::size_of::<LegacyIntegerValue>())
        + 33;
    let runs = crate::test_support::with_retained_limit(
        u64::try_from(retained).expect("run storage"),
        |ctx| parse_runs(ctx, &bytes),
    )
    .expect("interval backing is temporary while record children survive");
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.role_selectors.len(), 1);
    assert_eq!(run.role_selectors[0].name.literal(), Some("name"));
    assert_eq!(run.text_fields.len(), 1);
    assert_eq!(run.text_fields[0].value, "Label");
    assert_eq!(run.type_descriptors.len(), 1);
    assert_eq!(
        run.type_descriptors[0].value,
        LegacyTypeValue::Name("Real".to_owned())
    );
    assert_eq!(run.scalar_values.len(), 1);
    assert_eq!(
        run.scalar_values[0].evaluation,
        LegacyScalarEvaluation::Unset
    );
    assert_eq!(run.scalar_values[0].name.as_deref(), Some("Label"));
    assert_eq!(run.string_values.len(), 1);
    assert_eq!(run.string_values[0].value, "x");
    assert_eq!(run.string_values[0].name.as_deref(), Some("Label"));
    assert_eq!(run.integer_values.len(), 1);
    assert_eq!(run.integer_values[0].value, 11);
    assert_eq!(run.integer_values[0].name.as_deref(), Some("Label"));
}
