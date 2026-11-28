// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn the_patch_writes_the_active_override_in_design_cell_and_test_row() {
    let (decoded, fixture) = fixture(true);
    assert_eq!(own(decoded.ir(), &fixture.width), ("2 mm".to_owned(), 2.0));
    let a = named(decoded.ir(), "A").id.clone();
    for (path, mut ir) in [
        ("decode", decoded.ir().clone()),
        ("CADIR read", saved(decoded.ir())),
    ] {
        named_mut(&mut ir, "A")
            .overrides
            .parameters
            .insert(fixture.width.clone(), length_override(9.0));
        let (round_trip, losses) = written_with_losses(&ir, decoded.source_fidelity());
        let back = round_trip.ir();
        assert_eq!(
            own(back, &fixture.width),
            ("9 mm".to_owned(), 9.0),
            "{path}"
        );
        let (text, value) = parameter_cell(back, &fixture.entry, "a", &fixture.width);
        assert_eq!(text, "9 mm", "{path}");
        assert!((value - 0.9).abs() < 1e-9, "{path}: {value}");
        let test = test_row(back, &fixture.entry, "W");
        assert_eq!(test["Expression"], "9 mm", "{path}: {test}");
        assert!(
            (test["Value"].as_f64().unwrap() - 0.9).abs() < 1e-9,
            "{path}: {test}"
        );
        for (name, stated) in [("A", 9.0), ("B", 7.0), ("C", 12.0)] {
            let read = resolved_millimetres(back, name, &fixture.width);
            assert!((read - stated).abs() < 1e-9, "{path}: {name} reads {read}");
        }
        assert!(
            pinned(&losses, &a, &fixture.width).is_empty(),
            "{path}: {losses:#?}"
        );
        assert!(
            unwritten_parameter(&losses, &fixture.width).is_empty(),
            "{path}: {losses:#?}"
        );
    }
}

fn unwritten_parameter<'a>(losses: &'a [LossNote], id: &ParameterId) -> Vec<&'a LossNote> {
    losses
        .iter()
        .filter(|loss| {
            loss.code.to_string() == "export/value.unwritable" && loss.message.contains(id.as_str())
        })
        .collect()
}

#[test]
fn a_nested_design_parameter_edit_survives_synthesis() {
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let archive =
        crate::test_support::assembly_test::f3z_archive("root.f3d", &[("root.f3d", SOURCE)]);
    let decoded = decode(&archive);
    let (id, _) = length(decoded.ir());
    let mut ir = saved(decoded.ir());
    let parameter = ir
        .design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .unwrap();
    let name = parameter.name.clone();
    let value = length_override(15.5);
    parameter.definition = ParameterDefinition::Expression(value.expression.unwrap());
    parameter.value = value.value;

    let (_, bytes) = crate::test_support::configuration_table::written_on(
        &ir,
        Some(decoded.source_fidelity()),
        &["synthesized"],
    );
    let read = decode(&bytes);
    let parameter = read
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .expect("the recovered parameter");
    assert_eq!(own(read.ir(), &parameter.id), ("15.5 mm".to_owned(), 15.5));
}

#[test]
fn the_patch_writes_owned_parameter_expression_and_cache_independently() {
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let (decoded, fixture) = fixture(false);
    let (before, value) = own(decoded.ir(), &fixture.width);
    let depth = length_override(15.5);
    for (definition, cache) in [(false, true), (true, false), (true, true)] {
        let mut ir = saved(decoded.ir());
        let parameter = ir
            .design
            .parameters
            .iter_mut()
            .find(|parameter| parameter.id == fixture.width)
            .unwrap();
        let owner = parameter.owner.clone();
        if definition {
            parameter.definition =
                ParameterDefinition::Expression(depth.expression.clone().unwrap());
        }
        if cache {
            parameter.value.clone_from(&depth.value);
        }
        let (written, losses) = written_with_losses(&ir, decoded.source_fidelity());
        let (text, read) = own(written.ir(), &fixture.width);
        assert_eq!(text, if definition { "15.5 mm" } else { &before });
        assert!((read - if cache { 15.5 } else { value }).abs() < 1e-9);
        assert_eq!(
            written
                .ir()
                .design
                .parameters
                .iter()
                .find(|parameter| parameter.id == fixture.width)
                .unwrap()
                .owner,
            owner
        );
        assert!(
            unwritten_parameter(&losses, &fixture.width).is_empty(),
            "{losses:#?}"
        );
    }
}

#[test]
fn the_patch_keeps_the_archive_and_charges_unwritable_parameters_by_id() {
    use cadmpeg_ir::design::expr::{ExpressionDialect, ExpressionToken};
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let (decoded, fixture) = fixture(false);
    let mut ir = saved(decoded.ir());
    let parameter = ir
        .design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == fixture.width)
        .unwrap();
    let mut added = parameter.clone();
    added.id = ParameterId::mint("test:parameter:unplaced#length").unwrap();
    added.native_ref = None;
    parameter.definition = ParameterDefinition::Expression(Expression::Source {
        dialect: ExpressionDialect::Inventor,
        tokens: vec![ExpressionToken::Text("sqrt(81 mm^2)".into())],
    });
    let added_id = added.id.clone();
    ir.design.parameters.push(added);
    let (written, losses) = written_with_losses(&ir, decoded.source_fidelity());
    assert_eq!(
        own(written.ir(), &fixture.width),
        own(decoded.ir(), &fixture.width)
    );
    assert!(!written
        .ir()
        .design
        .parameters
        .iter()
        .any(|parameter| parameter.id == added_id));
    for id in [&fixture.width, &added_id] {
        assert_eq!(unwritten_parameter(&losses, id).len(), 1, "{losses:#?}");
    }
}

#[test]
fn the_patch_writes_a_parameter_reference_and_relocates_the_next_record() {
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let mut width = crate::design::test_support::parameter_record(
        None,
        "2 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        0.2,
    );
    let mut depth = crate::design::test_support::parameter_record(
        None,
        "9 mm",
        "User Parameter",
        Some("mm"),
        "RegisteredDepth",
        0.9,
    );
    width[4..7].copy_from_slice(b"256");
    depth[4..7].copy_from_slice(b"256");
    depth[7..11].copy_from_slice(&72u32.to_le_bytes());
    let depth_start = u64::try_from(width.len()).unwrap();
    let mut bulk = width;
    bulk.extend(depth);
    let collection_start = u64::try_from(bulk.len()).unwrap();
    crate::test_support::indexed_header(&mut bulk, *b"257", 19);
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "7A6A3D31-BE74-4E19-9115-20642944C2E4",
                "",
                8,
                "CommonData",
                &[71, 72],
            ),
            (
                "B0556F58-EE09-46A7-B18F-E296F48E5B13",
                "",
                4,
                "CommonData",
                &[19],
            ),
        ],
        &[(71, 0), (72, depth_start), (19, collection_start)],
    );
    let mut source = zip::ZipArchive::new(Cursor::new(SOURCE)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        zip.raw_copy_file(source.by_index_raw(index).unwrap())
            .unwrap();
    }
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    for (name, bytes) in [("MetaStream.dat", meta), ("BulkStream.dat", bulk)] {
        zip.start_file(
            format!("FusionAssetName[Active]/FusionDesignSegmentType3/{name}"),
            stored,
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let decoded = decode(&zip.finish().unwrap().into_inner());
    let mut ir = saved(decoded.ir());
    let id = |name: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| parameter.name.as_ref().is_some_and(|n| n.as_str() == name))
            .unwrap_or_else(|| panic!("parameter {name}: {:#?}", ir.design.parameters))
            .id
            .clone()
    };
    let (width, depth) = (id("Width"), id("RegisteredDepth"));
    let reference = Expression::Neutral(Neutral(Node::Reference(ParameterRef {
        target: RefTarget::Parameter(depth.clone()),
        through: None,
    })));
    let parameter = ir
        .design
        .parameters
        .iter_mut()
        .find(|p| p.id == width)
        .unwrap();
    parameter.definition = ParameterDefinition::Expression(reference.clone());
    parameter.value = length_override(9.0).value;
    let collection_type = crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        let collection = native
            .design_types
            .iter_mut()
            .find(|row| {
                row.table_position()
                    .0
                    .contains("FusionDesignSegmentType3/MetaStream.dat")
                    && row.entities.values().copied().eq([19])
            })
            .expect("the registered collection type");
        collection.set_version(5);
        collection.id().clone()
    });
    let (written, losses) = written_with_losses(&ir, decoded.source_fidelity());
    assert_eq!(
        own(written.ir(), &width),
        ("RegisteredDepth".to_owned(), 9.0)
    );
    assert_eq!(own(written.ir(), &depth), own(decoded.ir(), &depth));
    let native = f3d_native(written.ir());
    let collection = native
        .design_types
        .iter()
        .find(|row| row.id() == &collection_type)
        .expect("the type row survives the parameter edit");
    assert_eq!(collection.version, 5);
    let read = written
        .ir()
        .design
        .parameters
        .iter()
        .find(|p| p.id == width)
        .unwrap();
    assert_eq!(read.definition, ParameterDefinition::Expression(reference));
    assert!(
        unwritten_parameter(&losses, &width).is_empty(),
        "{losses:#?}"
    );
}

#[test]
fn a_fusion_expression_with_an_unstated_cache_still_reads_back() {
    use cadmpeg_ir::design::expr::{ExpressionDialect, ExpressionToken};
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let (decoded, fixture) = fixture(false);
    let mut ir = saved(decoded.ir());
    let expression = Expression::Source {
        dialect: ExpressionDialect::Fusion,
        tokens: vec![ExpressionToken::Text("2 mm + 3 mm".into())],
    };
    let parameter = ir
        .design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == fixture.width)
        .unwrap();
    parameter.definition = ParameterDefinition::Expression(expression.clone());
    parameter.value = None;
    let (written, losses) = written_with_losses(&ir, decoded.source_fidelity());
    let read = written
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == fixture.width)
        .unwrap();
    assert_eq!(read.definition, ParameterDefinition::Expression(expression));
    assert!(
        unwritten_parameter(&losses, &fixture.width).is_empty(),
        "{losses:#?}"
    );
}

#[test]
fn a_held_active_cell_edit_writes_the_same_design_record() {
    let (decoded, fixture) = fixture(true);
    let mut ir = decoded.ir().clone();
    let active = named(&ir, "A").id.clone();
    let row = ir
        .model
        .configurations
        .iter_mut()
        .find(|row| row.id == active)
        .unwrap();
    row.parameter_overrides
        .insert(fixture.width.clone(), "9 mm".into());
    row.parameter_values.insert(
        fixture.width.clone(),
        LegacyValue::Length(cadmpeg_ir::scalar::Length::new(9.0).unwrap()),
    );
    let (written, losses) = written_with_losses(&ir, decoded.source_fidelity());
    assert_eq!(own(written.ir(), &fixture.width), ("9 mm".to_owned(), 9.0));
    let (expression, cache) = parameter_cell(written.ir(), &fixture.entry, "a", &fixture.width);
    assert_eq!(expression, "9 mm");
    assert!((cache - 0.9).abs() < 1e-9);
    assert_eq!(
        test_row(written.ir(), &fixture.entry, "W")["Expression"],
        "9 mm"
    );
    assert!(
        unwritten_parameter(&losses, &fixture.width).is_empty(),
        "{losses:#?}"
    );
}
