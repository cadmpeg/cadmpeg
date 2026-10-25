// SPDX-License-Identifier: Apache-2.0
//! Classification reads remain owned by the census that uses their returned values.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn census_fillet_family_and_postprocess_use_their_own_actual_source_fields() {
    let document=b"<Document SchemaVersion='4'><Objects Count='2'><Object name='Base' type='Part::Feature'/><Object name='Fillet' type='Part::Fillet'/></Objects><ObjectData Count='2'><Object name='Base'><Properties Count='0'/></Object><Object name='Fillet'><Properties Count='4'><Property name='Base' type='App::PropertyLink'><Link value='Base'/></Property><Property name='Edges' type='Part::PropertyFilletEdges'><FilletEdges file='edges'/></Property><Property name='Refine' type='App::PropertyBool'><Bool value='false'/></Property><Property name='Unrelated' type='App::PropertyFloat'><Float value='91'/></Property></Properties></Object></ObjectData></Document>";
    for (radius, kind) in [(2.0_f64, "fillet"), (-1.0, "native")] {
        let mut edges = 3_u32.to_le_bytes().to_vec();
        for edge in 1_i32..=3 {
            edges.extend_from_slice(&edge.to_le_bytes());
            edges.extend_from_slice(&radius.to_le_bytes());
            edges.extend_from_slice(&radius.to_le_bytes());
        }
        let archive = crate::test_support::test_archive::archive_entries(&[
            ("Document.xml", document),
            ("edges", &edges),
        ]);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(archive), &DecodeOptions::default())
            .unwrap();
        let records = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::DesignCensusRecord>("design_census")
            .unwrap();
        let census = records
            .iter()
            .find(|record| record.object == crate::native::native_id("object", "Fillet"))
            .unwrap();
        assert_eq!(census.semantic_kind, kind);
        assert!(census.post_processed);
        let layout = &decoded.source_fidelity().record_layout()[&census.id];
        let entry_part = decoded
            .source_fidelity()
            .parts()
            .values()
            .find(|part| part.level().as_str() == "freecad.fillet_edges_entry")
            .unwrap();
        assert_eq!(
            layout.reads()[entry_part.stream()],
            [0..4, 8..24, 28..44, 48..64]
        );
        let xml_reads = &layout.reads()[layout.stream()];
        let xml = std::str::from_utf8(document).unwrap();
        let refine = xml.find("value='false'").unwrap() + "value='".len();
        assert!(xml_reads
            .iter()
            .any(|range| range.start <= u64::try_from(refine).unwrap()
                && range.end >= u64::try_from(refine + 5).unwrap()));
        let unrelated = xml.find("value='91'").unwrap() + "value='".len();
        let reads_unrelated = xml_reads.iter().any(|range| {
            range.start <= u64::try_from(unrelated).unwrap()
                && range.end >= u64::try_from(unrelated + 2).unwrap()
        });
        assert!(!reads_unrelated);
        assert!(decoded
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Unrelated"
                && attribute.values
                    == [cadmpeg_ir::attributes::AttributeValue::float(91.0).unwrap()]));
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

#[test]
fn sketch_and_census_admission_use_separate_actual_source_producers() {
    let document="<Document SchemaVersion='4'><Objects Count='1'><Object name='Sketch' type='Sketcher::SketchObject'/></Objects><ObjectData Count='1'><Object name='Sketch'><Properties Count='3'><Property name='Geometry' type='Part::PropertyGeometryList'><GeometryList count='1'><Geometry type='Part::GeomLineSegment'><LineSegment StartX='0' StartY='0' EndX='2' EndY='1'/></Geometry></GeometryList></Property><Property name='Constraints' type='Sketcher::PropertyConstraintList'><ConstraintList count='0'/></Property><Property name='Unrelated' type='App::PropertyFloat'><Float value='91'/></Property></Properties></Object></ObjectData></Document>";
    let bytes = crate::test_support::test_archive::archive(&document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded.ir().model.sketches.first().unwrap();
    let records = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .unwrap();
    let census = records.first().unwrap();
    assert_eq!(census.semantic_kind, "sketch");
    let end = document.find("EndX='2'").unwrap() + "EndX='".len();
    let unrelated = document.find("value='91'").unwrap() + "value='".len();
    for id in [sketch.id.as_str(), census.id.as_str()] {
        let layout = &decoded.source_fidelity().record_layout()[id];
        assert!(layout.reads()[layout.stream()]
            .iter()
            .any(|range| range.start <= u64::try_from(end).unwrap()
                && range.end > u64::try_from(end).unwrap()));
        assert!(layout.reads()[layout.stream()]
            .iter()
            .all(|range| range.end <= u64::try_from(unrelated).unwrap()
                || range.start >= u64::try_from(unrelated + 2).unwrap()));
    }
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}

#[test]
fn link_and_placement_selectors_supply_feature_and_census_facts_separately() {
    for (runtime, name, value, changed, kind, token) in [
        (
            "PartDesign::Body",
            "Tip",
            "<Link value='A'/>",
            "<Link value='B'/>",
            "tree_node",
            "value='A'",
        ),
        (
            "PartDesign::FeatureBase",
            "BaseFeature",
            "<Link value='A'/>",
            "<Link value=''/>",
            "derived_geometry",
            "value='A'",
        ),
        (
            "PartDesign::Plane",
            "Placement",
            "<PropertyPlacement Px='2' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='1'/>",
            "<PropertyPlacement Px='2' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='0'/>",
            "datum_plane",
            "Px='2'",
        ),
    ] {
        let group = if runtime == "PartDesign::Body" {
            "<Property name='Group' type='App::PropertyLinkList'><LinkList count='1'><Link value='A'/></LinkList></Property>"
        } else {
            ""
        };
        let count = if group.is_empty() { 2 } else { 3 };
        let value_type = if name == "Placement" {
            "App::PropertyPlacement"
        } else {
            "App::PropertyLink"
        };
        let document = format!("<Document SchemaVersion='4'><Objects Count='3'><Object name='A' type='Part::Feature'/><Object name='B' type='Part::Feature'/><Object name='Selected' type='{runtime}'/></Objects><ObjectData Count='3'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object><Object name='Selected'><Properties Count='{count}'>{group}<Property name='{name}' type='{value_type}'>{value}</Property><Property name='Unrelated' type='App::PropertyFloat'><Float value='91'/></Property></Properties></Object></ObjectData></Document>");
        let decode = |document: &str| {
            crate::FcstdCodec
                .decode(
                    &mut Cursor::new(crate::test_support::test_archive::archive(document)),
                    &DecodeOptions::default(),
                )
                .unwrap()
        };
        let before = decode(&document);
        let feature = before
            .ir()
            .model
            .features
            .iter()
            .find(|feature| {
                feature.native_ref.as_deref()
                    == Some(crate::native::native_id("object", "Selected").as_str())
            })
            .unwrap();
        let census_records = before
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::DesignCensusRecord>("design_census")
            .unwrap();
        let census = census_records
            .iter()
            .find(|record| record.object == crate::native::native_id("object", "Selected"))
            .unwrap();
        assert_eq!(census.semantic_kind, kind, "{runtime}");
        let field = document.rfind(token).unwrap() + token.find('\'').unwrap() + 1;
        let unrelated = document.find("value='91'").unwrap() + "value='".len();
        for id in [feature.id.as_str(), census.id.as_str()] {
            let layout = &before.source_fidelity().record_layout()[id];
            let reads = &layout.reads()[layout.stream()];
            assert!(
                reads
                    .iter()
                    .any(|range| range.contains(&u64::try_from(field).unwrap())),
                "{runtime} {id}"
            );
            assert!(
                reads
                    .iter()
                    .all(|range| !range.contains(&u64::try_from(unrelated).unwrap())),
                "{runtime} {id}"
            );
        }
        let after = decode(&document.replacen(value, changed, 1));
        let records = after
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::DesignCensusRecord>("design_census")
            .unwrap();
        let changed = records
            .iter()
            .find(|record| record.object == census.object)
            .unwrap();
        assert_eq!(
            changed.semantic_kind,
            "native",
            "{runtime}: {:?}",
            after.report().losses
        );
        assert!(crate::test_support::validate_native(after.ir()).is_empty());
    }
}

#[test]
fn missing_sketch_geometry_row_uses_its_child_home_without_fabricating_a_shorter_sketch() {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let document = "<Document SchemaVersion='4'><Objects Count='2'><Object name='Sketch' type='Sketcher::SketchObject'/><Object name='Peer' type='Part::Feature'/></Objects><ObjectData Count='2'><Object name='Sketch'><Properties Count='2'><Property name='Geometry' type='Part::PropertyGeometryList'><GeometryList count='2'><Geometry type='Part::GeomLineSegment'><LineSegment StartX='0' StartY='0' EndX='2' EndY='1'/></Geometry><Geometry type='Part::GeomPoint'><GeomPoint X='3' Y='4'/></Geometry></GeometryList></Property><Property name='Constraints' type='Sketcher::PropertyConstraintList'><ConstraintList count='0'/></Property></Properties></Object><Object name='Peer'><Properties Count='1'><Property name='Extra' type='App::PropertyFloat'><Float value='99'/></Property></Properties></Object></ObjectData></Document>";
    let decode = |document: &str| {
        crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(document)),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let before = decode(document);
    assert_eq!(before.ir().model.sketches.len(), 1);
    assert_eq!(before.ir().model.sketch_entities.len(), 2);
    let sketch_id = before.ir().model.sketches[0].id.to_string();
    let after = decode(&document.replacen("type='Part::GeomLineSegment'", "type='&missing;'", 1));
    assert!(after.ir().model.sketches.is_empty());
    assert!(after.ir().model.sketch_entities.is_empty());
    let peer = before
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Extra")
        .unwrap();
    assert_eq!(
        peer.values,
        [cadmpeg_ir::attributes::AttributeValue::float(99.0).unwrap()]
    );
    assert_eq!(
        after
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.id == peer.id),
        Some(peer)
    );
    let feature = after
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some(crate::native::native_id("object", "Sketch").as_str())
        })
        .unwrap();
    assert!(matches!(
        feature.definition().operation(),
        cadmpeg_ir::features::FeatureOperation::Native { .. }
    ));
    let records = after
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .unwrap();
    assert_eq!(
        records
            .iter()
            .find(|record| record.object == crate::native::native_id("object", "Sketch"))
            .unwrap()
            .semantic_kind,
        "native"
    );
    let failed = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.value_element");
    let errors = after
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains(&sketch_id));
    assert!(crate::test_support::validate_native(after.ir()).is_empty());
}

#[test]
fn unknown_selected_control_header_supplies_native_classification_without_reading_its_body() {
    let document = "<Document SchemaVersion='4'><Objects Count='1'><Object name='Body' type='PartDesign::Body'/></Objects><ObjectData Count='1'><Object name='Body'><Properties Count='1'><Property name='Group' type='Vendor::Uninterpreted'><Opaque value='opaque-payload'/></Property></Properties></Object></ObjectData></Document>";
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let feature = &decoded.ir().model.features[0];
    assert!(matches!(
        feature.definition().operation(),
        cadmpeg_ir::features::FeatureOperation::Native { .. }
    ));
    let records = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .unwrap();
    assert_eq!(records[0].semantic_kind, "native");
    let kind = document.find("Vendor::Uninterpreted").unwrap();
    let payload = document.find("opaque-payload").unwrap();
    for id in [feature.id.as_str(), records[0].id.as_str()] {
        let layout = &decoded.source_fidelity().record_layout()[id];
        let reads = &layout.reads()[layout.stream()];
        assert!(reads
            .iter()
            .any(|range| range.contains(&u64::try_from(kind).unwrap())));
        assert!(
            reads
                .iter()
                .all(|range| !range.contains(&u64::try_from(payload).unwrap())),
            "{id}: {:?}",
            layout.reads()
        );
    }
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}
