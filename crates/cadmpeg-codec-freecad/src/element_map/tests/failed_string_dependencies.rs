// SPDX-License-Identifier: Apache-2.0
//! #214 Rule 2 prunes actual failed producers without inventing peer errors.

use super::*;
use cadmpeg_ir::source_fidelity::parts::PartState;

#[test]
fn failed_string_rows_prune_relative_components_and_map_consumers_without_cascading_errors() {
    for (unknown, malformed_component) in [(false, false), (true, false), (false, true)] {
        let selector = if unknown { 999 } else { 2 };
        let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Shape' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='3'><Property name='Shape' type='Part::PropertyPartShape'><Part HasherIndex='0' ElementMap='1'/><StringHasher new='1'/><StringHasher2 file='strings.txt'/><ElementMap new='1'/><ElementMap2>7 PostfixCount 0 MapCount 1 ElementMap 1 7 1 Face ChildCount 0 NameCount 5 0 ;failed.0.{selector:x} 0 ;relative.0.3 0 ;component.0.4 0 ;last.0.5 0 EndMap</ElementMap2></Property><Property name='Legacy' type='Part::PropertyPartShape'><Part HasherIndex='0' ElementMap='1'/><ElementMap file='legacy.txt'/></Property><Property name='LegacyXml' type='Part::PropertyPartShape'><Part HasherIndex='0' ElementMap='1'/><ElementMap count='4'><Element key='failed' value='Face1' sid='{selector}'/><Element key='relative' value='Face2' sid='3'/><Element key='component' value='Face3' sid='4'/><Element key='last' value='Face4' sid='5'/></ElementMap></Property></Properties></Object></ObjectData></Document>");
        let table: &[u8] = if malformed_component {
            b"StringTableStart v1 5\n1.0 0:first\n2.0.zz 0:bad component\n-1.0 0:relative\n4.0.2 0:component\n5.0 0:last\n"
        } else {
            b"StringTableStart v1 5\n1.0 0:first\n2.0 0:\xff\n-1.0 0:relative\n4.0.2 0:component\n5.0 0:last\n"
        };
        let legacy = format!("4\nFace1 failed 1 {selector}\nFace2 relative 1 3\nFace3 component 1 4\nFace4 last 1 5\n");
        let source = archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("strings.txt", table),
            ("legacy.txt", legacy.as_bytes()),
        ]);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        let failures = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
            .collect::<Vec<_>>();
        let expected = if unknown { 4 } else { 1 };
        assert_eq!(failures.len(), expected, "{:?}", decoded.report().losses);
        assert_eq!(
            failures
                .iter()
                .filter(|part| part.level().as_str() == "freecad.string_table_record")
                .count(),
            1
        );
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "record.unreadable")
                .count(),
            expected
        );
        let native = decoded.ir().native.namespace("fcstd").unwrap();
        let tables = native
            .arena_as::<crate::native::StringTableRecord>("string_tables")
            .unwrap();
        let rows = tables[0].entries().unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].as_ref().unwrap().payload, "first");
        assert!(rows[1..4].iter().all(Option::is_none));
        assert_eq!(rows[4].as_ref().unwrap().payload, "last");
        let maps = native
            .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
            .unwrap();
        assert_eq!(maps.len(), 3);
        let modern = maps
            .iter()
            .find(|map| map.legacy_records.is_none())
            .unwrap();
        let names = &modern.maps.root().groups[0].as_ref().unwrap().names;
        assert_eq!(names.len(), 5);
        assert!(names[1..4].iter().all(Option::is_none));
        assert_eq!(names[4].as_ref().unwrap()[0].string_ids, [5]);
        let legacy = maps
            .iter()
            .find_map(|map| map.legacy_records.as_ref())
            .unwrap();
        assert!(legacy[..3].iter().all(Option::is_none));
        assert_eq!(legacy[3].as_ref().unwrap().indexed_name, "Face4");
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

#[test]
fn failed_dictionary_entries_prune_only_their_actual_name_chain_consumers() {
    let mut document = b"<Document SchemaVersion='4' FileVersion='2'><Objects Count='1'><Object name='Shape' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part ElementMap='1'/><ElementMap new='1'/><ElementMap2>7 PostfixCount 1 ".to_vec();
    document.push(0xff);
    document.extend_from_slice(b" MapCount 1 ElementMap 1 7 1 Face ChildCount 0 NameCount 4 0 :1.1.0 0 ;post.1 0 ;good.0 0 EndMap</ElementMap2></Property></Properties></Object></ObjectData></Document>");
    let source = archive_entries(&[("Document.xml", &document)]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let failures = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:?}", decoded.report().losses);
    assert_eq!(failures[0].level().as_str(), "freecad.inline_map_postfix");
    let maps = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .unwrap();
    assert_eq!(maps.len(), 1);
    let names = &maps[0].maps.root().groups[0].as_ref().unwrap().names;
    assert!(names[1].is_none());
    assert!(names[2].is_none());
    assert_eq!(
        names[3].as_ref().unwrap()[0].resolved.as_deref(),
        Some("good")
    );
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}
