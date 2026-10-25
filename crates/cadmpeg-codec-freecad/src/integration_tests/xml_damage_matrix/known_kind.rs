// SPDX-License-Identifier: Apache-2.0
//! Whole-scope recovery uses the generic #2246376513563 Native-field rule.

use super::*;
use cadmpeg_ir::features::FeatureOperation;

#[cfg(test)]
mod tests {
    use super::*;

    fn oracle(bytes: &[u8], changed: &[u8], before: &DecodeResult, after: &DecodeResult) {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        let root = ctx.name_stream(root, "file").unwrap();
        let next_arena = DecodeArena::new();
        let (next, next_root) =
            DecodeContext::from_root_bytes(changed, &next_arena, &DecodePolicy::service()).unwrap();
        let next_root = next.name_stream(next_root, "file").unwrap();
        let allowances = Allowances::given_up_zip(
            DecodeSnapshot::new(before.ir(), before.source_fidelity()),
            DecodeSnapshot::new(after.ir(), after.source_fidelity()),
            after.report(),
            (&ctx, root),
            (&next, next_root),
        )
        .unwrap();
        assert_decodes_agree(
            &serde_json::to_value(before.ir()).unwrap(),
            &serde_json::to_value(after.ir()).unwrap(),
            &allowances,
            Mode::Same,
        )
    }

    #[test]
    fn whole_control_scope_fallback_requires_an_unchanged_source_kind_header() {
        let xml = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'>\
            <Object name='Box' type='Part::Box'/></Objects><ObjectData Count='1'><Object name='Box'>\
            <Properties Count='3'><Property name='Length' type='App::PropertyLength'><Float value='2'/></Property>\
            <Property name='Width' type='App::PropertyLength'><Float value='3'/></Property>\
            <Property name='Height' type='App::PropertyLength'><Float value='4'/></Property>\
            </Properties></Object></ObjectData></Document>";
        let bytes = crate::test_support::test_archive::archive(xml);
        let before = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let frame = selected(&bytes, false)
            .into_iter()
            .find(|frame| frame.level == "freecad.properties")
            .unwrap();
        let changed = damage(&bytes, &frame, 0);
        let after = crate::FcstdCodec
            .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
            .unwrap();
        assert!(
            matches!(after.ir().model.features[0].definition().operation(),
            FeatureOperation::Native{kind,parameters} if kind.as_str()=="Part::Box" && parameters.is_empty())
        );
        oracle(&bytes, &changed, &before, &after);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&changed, &arena, &DecodePolicy::service()).unwrap();
        let root = ctx.name_stream(root, "file").unwrap();
        let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
        let mut member = archive
            .open(&ctx, "Document.xml")
            .unwrap()
            .window()
            .to_vec();
        let at = xml.find("Part::Box").unwrap();
        member[at..at + 9].copy_from_slice(b"Part::Cut");
        let changed_kind = zip::rebuild_member(&ctx, root, b"Document.xml", &member).unwrap();
        let after_kind = crate::FcstdCodec
            .decode(&mut Cursor::new(&changed_kind), &DecodeOptions::default())
            .unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| oracle(
                &bytes,
                &changed_kind,
                &before,
                &after_kind
            )))
            .is_err()
        );
    }
}
