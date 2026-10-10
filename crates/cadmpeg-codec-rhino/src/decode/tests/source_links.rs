// SPDX-License-Identifier: Apache-2.0
//! Source-link fixture mutation for refusal controls.

pub(super) fn append_link_to_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
    links: &mut Vec<String>,
    link: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if ctx.equal(link, id, "Rhino source link equality")? {
        return Ok(false);
    }
    let Err(first) = ctx.binary_search_by(
        links,
        |existing| ctx.compare(existing.as_str(), link, "Rhino source link comparison"),
        "Rhino source link search",
    )?
    else {
        return Ok(true);
    };
    let copy = ctx.copy_retained_text(link, "Rhino unknown record link copy")?;
    ctx.insert_vec(links, first, copy, "Rhino unknown record links")?;
    Ok(true)
}

use super::*;

#[test]
fn decode_context_transitions_object_status_once_and_links_unknowns() {
    let archive = ArchiveVersion::V5;
    let object = object_record(archive, 1, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("required invariant");
    crate::decode::with_expand(&scan, |expand| {
        let mut context =
            crate::decode::DecodeContext::new(&scan, expand).expect("test transaction");
        assert!(context.object(0).is_some());
        assert!(context.unknown(0).is_some());
        assert_eq!(
            context.unit_binding(),
            crate::settings::UnitBinding::Unavailable
        );
        assert_eq!(context.archive(), archive);
        assert!(context
            .append_link(0, "rhino:curve#2")
            .expect("admitted link"));
        assert!(context
            .append_link(0, "rhino:curve#1")
            .expect("admitted link"));
        assert!(context
            .append_link(0, "rhino:curve#2")
            .expect("admitted link"));
        let mut links = context
            .unknown(0)
            .expect("required invariant")
            .links()
            .to_vec();
        links.sort();
        assert_eq!(links, ["rhino:curve#1", "rhino:curve#2"]);
        let own_id = context
            .unknown(0)
            .expect("required invariant")
            .id()
            .to_string();
        assert!(context
            .append_links(
                0,
                &[
                    "rhino:curve#3".to_string(),
                    "rhino:curve#1".to_string(),
                    own_id,
                    "rhino:curve#0".to_string(),
                ],
            )
            .expect("admitted links"));
        let mut links = context
            .unknown(0)
            .expect("required invariant")
            .links()
            .to_vec();
        links.sort();
        assert_eq!(
            links,
            [
                "rhino:curve#0",
                "rhino:curve#1",
                "rhino:curve#2",
                "rhino:curve#3"
            ]
        );
        assert!(context.mark_decoded(0));
        assert!(!context.mark_decoded(0));
        assert!(!context.mark_failed(0));
        assert_eq!(context.ir_mut().model.bodies.len(), 0);
        context
            .unknown_links_mut(0)
            .expect("required invariant")
            .clear();
        let result =
            crate::decode::seal_for_test(context.commit().expect("test decode commit"), false);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity == Severity::Info));
        assert_eq!(
            result
                .ir()
                .native_unknowns("rhino")
                .expect("required invariant")
                .len(),
            1
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert_eq!(validation.error_count(), 0);
    });
}

#[test]
fn seeded_link_flush_queue_deduplicates_rollback_rows() {
    let scan = scan_with_objects(&[object_record(
        ArchiveVersion::V5,
        1,
        POINT_CLASS,
    )]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        context
            .session
            .unknown_links_mut(0)
            .expect("seeded source row")
            .1
            .extend(["rhino:curve#a".into(), "rhino:curve#z".into()]);
        assert!(context
            .append_link(0, "rhino:curve#0")
            .expect("generated link is admitted"));
        assert_eq!(context.pending_seeded_link_rows, [0]);

        let journal =
            super::super::InstanceJournal::new(context.expand.ctx()).expect("instance journal");
        context.instance_journal = Some(journal);
        assert!(context
            .append_link(0, "rhino:curve#zz")
            .expect("journaled link is admitted"));
        context
            .rollback_instance_rows()
            .expect("journal rollback succeeds");
        assert_eq!(context.pending_seeded_link_rows, [0]);
        assert_eq!(
            context
                .unknown(0)
                .expect("retained source row")
                .links(),
            ["rhino:curve#a", "rhino:curve#z", "rhino:curve#0"]
        );

        context
            .flush_seeded_source_links()
            .expect("seeded links return to canonical order");
        assert_eq!(
            context
                .unknown(0)
                .expect("retained source row")
                .links(),
            ["rhino:curve#0", "rhino:curve#a", "rhino:curve#z"]
        );
    });
}

#[test]
fn seeded_malformed_links_keep_canonical_validation_order_after_append() {
    let scan = scan_with_objects(&[object_record(
        ArchiveVersion::V5,
        1,
        POINT_CLASS,
    )]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        context
            .unknown_links_mut(0)
            .expect("seeded source row")
            .extend(["bad-z".into(), "bad-a".into()]);

        assert!(context
            .append_link(0, "rhino:test:curve#generated")
            .expect("generated link is admitted"));
        assert_eq!(
            context.unknown(0).expect("retained source row").links(),
            ["bad-a", "bad-z", "rhino:test:curve#generated"]
        );

        let expected = NativeUnknownRecord::try_from(
            context.unknown(0).expect("retained source row"),
        )
        .expect_err("seeded malformed links remain invalid");
        let actual = cadmpeg_ir::validate::admit::validate_native_unknowns(
            context.expand.ctx(),
            context.session.unknowns(),
        )
        .expect("resource admission succeeds")
        .expect_err("seeded malformed links remain invalid");
        assert_eq!(actual.to_string(), expected.to_string());
    });
}

