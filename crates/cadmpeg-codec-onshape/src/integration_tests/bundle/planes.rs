// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::serde_erase::json;

fn offset_bundle(flipped: bool, kind: &str) -> Vec<u8> {
    changed(&fixture("unsupported"), |path, value| {
        if path.ends_with("features.json") {
            let features = value["features"].as_array_mut().unwrap();
            features.truncate(2);
            features[0]["parameters"][0]["queries"] = json!([{
                "btType":"BTMIndividualCreatedByQuery-137", "featureId":"raised-datum"
            }]);
            features.insert(
                0,
                json!({
                    "btType":"BTMFeature-134", "featureId":"raised-datum", "name":"Raised datum",
                    "featureType":"cPlane", "parameters":[
                        {"parameterId":"cplaneType","value":kind},
                        {"parameterId":"entities","queries":[{
                            "btType":"BTMIndividualCreatedByQuery-137","featureId":"Top"
                        }]},
                        {"parameterId":"offset","expression":"7 mm"},
                        {"parameterId":"oppositeDirection","value":flipped}
                    ]
                }),
            );
        }
    })
}

#[test]
fn offset_datum_and_attached_sketch_have_resolved_frames() {
    let height = 7.0;
    let (model, report) = read(&offset_bundle(false, "OFFSET")).unwrap();
    assert_eq!(
        kinds(&model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let expected = json!({"origin":{"x":0.0,"y":0.0,"z":height},
        "normal":{"x":0.0,"y":0.0,"z":1.0}, "u_axis":{"x":1.0,"y":0.0,"z":0.0}});
    assert_eq!(
        wire["model"]["features"][0]["definition"]["value"]["value"]["distance"],
        height
    );
    assert_eq!(
        wire["model"]["features"][0]["definition"]["value"]["value"]["reference"]["kind"],
        "resolved_plane"
    );
    assert_eq!(wire["model"]["sketches"][0]["placement"]["frame"], expected);
    assert_eq!(
        wire["model"]["features"][1]["dependencies"],
        json!(["onshape:model:feature#raised-datum"])
    );
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| !loss.code.as_deref().is_some_and(
                |code| code.starts_with("onshape/") && code != "onshape/sketch.plane-assumed"
            )),
        "{:?}",
        report.losses()
    );
}

#[test]
fn flipped_offset_plane_keeps_native_with_named_loss() {
    let (model, report) = read(&offset_bundle(true, "OFFSET")).unwrap();
    assert_eq!(kinds(&model)[0], "native");
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/plane.not-supported")
        && loss.detail.contains("flipped offset plane")));
}

#[test]
fn unsupported_plane_type_keeps_native_with_named_loss() {
    let (model, report) = read(&offset_bundle(false, "THREE_POINT")).unwrap();
    assert_eq!(kinds(&model)[0], "native");
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/plane.not-supported")
        && loss.detail.contains("cplaneType")));
}

#[test]
fn offset_from_planar_face_requires_attachment_orientation_proof() {
    for orientation in [true, false] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("bodydetails/2.json") {
                value["bodies"][0]["faces"] = json!([{"id":"support-face","orientation":orientation,"loops":[],
                    "surface":{"type":"PLANE","origin":{"x":0.0,"y":0.0,"z":0.003},
                    "normal":{"x":0.0,"y":0.0,"z":1.0}}}]);
            }
            if path.ends_with("features.json") {
                value["features"][2] = json!({"btType":"BTMFeature-134", "featureId":"face-datum",
                    "featureType":"cPlane","parameters":[
                    {"parameterId":"cplaneType","value":"OFFSET"},
                    {"parameterId":"offset","expression":"7 mm"},
                    {"parameterId":"entities","queries":[{"deterministicIds":["support-face"]}]}]});
                let mut sketch = value["features"][0].clone();
                sketch["featureId"] = json!("upper-sketch");
                sketch["parameters"][0]["queries"] = json!([{"featureId":"face-datum"}]);
                value["features"].as_array_mut().unwrap().push(sketch);
                value["rollbackIndex"] = json!(4);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native", "native"]);
        assert_eq!(model.ir.model.sketches.len(), 1);
        let wire = timeline(&model);
        let frame = &wire["model"]["sketches"][0]["placement"]["frame"];
        assert_eq!(frame["normal"], json!({"x":0.0,"y":0.0,"z":1.0}));
        assert_eq!(frame["u_axis"], json!({"x":1.0,"y":0.0,"z":0.0}));
        assert!(report.losses().iter().any(|loss| loss.code.as_deref()
            == Some("onshape/plane.not-supported")
            && loss.detail.contains("ONSHAPE-009")));
    }
}

#[test]
fn compressed_created_plane_query_retains_stored_sketch_attachment() {
    let bytes = changed(&offset_bundle(false, "OFFSET"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][1]["parameters"][0]["queries"] = json!([{
                "btType":"BTMIndividualQuery-138", "deterministicIds":["datum-pick"],
                "queryString":"query=qCompressed(1.0,\"%M4Sa$entityTypeS4$FACESb$historyTypeS8$CREATIONSb$operationIdA1S14$raised-datum.planeOpS9$queryTypeS5$DUMMY\",id);"
            }]);
            value["features"][1]["plane"] = json!({
                "origin":{"x":0.002,"y":0.0,"z":0.007},
                "normal":{"x":0.0,"y":0.0,"z":1.0},
                "x":{"x":0.0,"y":1.0,"z":0.0}
            });
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"],
        json!({
            "origin":{"x":2.0,"y":0.0,"z":7.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":0.0,"y":1.0,"z":0.0}
        })
    );
    assert!(
        report.losses().iter().all(|loss| !loss
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("onshape/"))),
        "{:?}",
        report.losses()
    );
}

fn matrix_bundle(matrix: &Value) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(offset_bundle(false, "OFFSET"))).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let response = json!({"sketches":[{"featureId":"Base sketch", "sketchMatrix":matrix}]});
    zip.write_all(&json::to_vec(&response).unwrap()).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn sketch_response_matrix_places_sketch_on_created_datum() {
    let bytes = matrix_bundle(&json!([
        0.0, -1.0, 0.0, 0.002, 1.0, 0.0, 0.0, 0.003, 0.0, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0
    ]));
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"],
        json!({
        "origin":{"x":2.0,"y":3.0,"z":7.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0},
        "u_axis":{"x":0.0,"y":1.0,"z":0.0}})
    );
}

#[test]
fn malformed_optional_matrix_retains_the_support_frame_with_named_assumption() {
    let (model, report) = read(&matrix_bundle(&json!([1.0, 0.0, 0.0]))).unwrap();
    assert_eq!(kinds(&model), ["datum_offset_plane", "sketch", "extrude"]);
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"]["origin"]["z"],
        7.0
    );
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/sketch.plane-assumed")));
}

#[test]
fn top_sketch_ignores_contradictory_optional_matrix() {
    let bytes = changed(
        &matrix_bundle(&json!([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0
        ])),
        |path, value| {
            if path.ends_with("features.json") {
                value["features"][1]["parameters"][0]["queries"] = json!([{"featureId":"Top"}]);
            }
        },
    );
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model), ["datum_offset_plane", "sketch", "extrude"]);
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"],
        json!({
        "origin":{"x":0.0,"y":0.0,"z":0.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0}, "u_axis":{"x":1.0,"y":0.0,"z":0.0}})
    );
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/sketch.plane-assumed")));
}

#[test]
fn refused_matrices_keep_created_plane_support_and_charge_assumption() {
    for matrix in [
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1e-7, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0]),
        json!([1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0]),
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.009, 0.0, 0.0, 0.0, 1.0]),
    ] {
        let (model, report) = read(&matrix_bundle(&matrix)).unwrap();
        assert_eq!(kinds(&model), ["datum_offset_plane", "sketch", "extrude"]);
        let wire = timeline(&model);
        assert_eq!(
            wire["model"]["sketches"][0]["placement"]["frame"],
            json!({
            "origin":{"x":0.0,"y":0.0,"z":7.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}})
        );
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("onshape/sketch.plane-assumed")));
    }
}

#[test]
fn plane_flags_require_native_construction() {
    for flag in ["flipNormal", "flipAlignment"] {
        let bytes = changed(&offset_bundle(false, "OFFSET"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][0]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":flag,"value":true}));
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model)[0], "native");
        assert!(report.losses().iter().any(|loss| loss.code.as_deref()
            == Some("onshape/plane.not-supported")
            && loss.detail.contains(flag)));
    }
}

#[test]
fn unresolved_plane_operands_charge_history_loss() {
    for quantity in [true, false] {
        let bytes = changed(&offset_bundle(false, "OFFSET"), |path, value| {
            if path.ends_with("features.json") {
                if quantity {
                    value["features"][0]["parameters"][2] =
                        json!({"parameterId":"offset","value":7.0});
                } else {
                    value["features"][0]["parameters"][1]["queries"] =
                        json!([{"featureId":"missing-datum"}]);
                }
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model)[0], "native");
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("onshape/history.not-supported")));
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("onshape/plane.not-supported")));
    }
}

#[test]
fn native_sketch_clears_plane_assumption() {
    let bytes = changed(&offset_bundle(false, "OFFSET"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][1]["entities"] = Value::Null;
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model)[1], "native");
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/sketch.plane-assumed")));
}

#[test]
fn unreadable_and_stale_sketch_responses_are_given_up_and_duplicates_kept() {
    for (data, duplicate) in [
        (b"{malformed".to_vec(), false),
        (
            json::to_vec(&json!({"microversionId":"stale", "sketches":[]})).unwrap(),
            false,
        ),
        (
            json::to_vec(
                &json!({"sketches":[{"featureId":"Base sketch"},{"featureId":"Base sketch"}]}),
            )
            .unwrap(),
            true,
        ),
    ] {
        let mut zip =
            zip::ZipWriter::new_append(Cursor::new(offset_bundle(false, "OFFSET"))).unwrap();
        zip.start_file(
            "partstudios/studio/sketches.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&data).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["datum_offset_plane", "sketch", "extrude"]);
        let decoded = crate::test_support::review::decode(&bytes);
        let asset = decoded.ir().model.assets.iter().find(|asset| {
            asset
                .name
                .as_ref()
                .is_some_and(|name| name.as_str() == "partstudios/studio/sketches.json")
        });
        let given_up = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                    && loss
                        .message
                        .starts_with("part archive-entry \"partstudios/studio/sketches.json\"")
            })
            .count();
        if duplicate {
            let cadmpeg_ir::assets::AssetContent::Embedded { data: retained } =
                &asset.expect("the sketch response stays an asset").content
            else {
                panic!("embedded asset required")
            };
            assert_eq!(retained.as_bytes(), data);
            assert_eq!(given_up, 0);
            assert!(report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/sketch.plane-assumed")
                && loss.detail.contains("identity repeats")));
        } else {
            assert!(asset.is_none());
            assert_eq!(given_up, 1);
            assert!(report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains("onshape/member.unreadable: ")
                    && loss.detail.contains("omitted the member")));
        }
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("onshape/sketch.plane-assumed")));
    }
}

/// The support plane's frame that an unusable stored matrix leaves in place.
fn assert_support_frame_kept(model: &Model, case: &str) {
    assert_eq!(
        kinds(model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{case}"
    );
    assert_eq!(
        timeline(model)["model"]["sketches"][0]["placement"]["frame"],
        json!({
        "origin":{"x":0.0,"y":0.0,"z":7.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}),
        "{case}"
    );
}

/// Whether `report` states `code` with `detail`: in a note of `code`, or in a
/// give-up whose text carries it.
fn has_loss(report: &Report, code: &str, detail: &str) -> bool {
    report
        .losses()
        .iter()
        .any(|loss| loss.states(code) && loss.detail.contains(detail))
}

#[test]
fn malformed_stored_matrices_keep_the_support_frame_with_named_assumption() {
    let valid = [
        0.0, -1.0, 0.0, 0.002, 1.0, 0.0, 0.0, 0.003, 0.0, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0,
    ];
    for case in [
        "nonorthogonal",
        "scaled",
        "projective",
        "null",
        "string",
        "overflow",
        "missing-matrix",
        "wrong-id",
    ] {
        let mut matrix = json!(valid);
        match case {
            "nonorthogonal" => matrix[0] = json!(0.5),
            "scaled" => matrix[4] = json!(2.0),
            "projective" => matrix[12] = json!(0.1),
            "null" => matrix[3] = Value::Null,
            "string" => matrix[3] = json!("0.002"),
            "overflow" => matrix[3] = json!(f64::MAX),
            _ => {}
        }
        let mut row = json!({"featureId":"Base sketch","sketchMatrix":matrix});
        match case {
            "missing-matrix" => {
                row.as_object_mut().unwrap().remove("sketchMatrix");
            }
            "wrong-id" => row["featureId"] = json!("another-sketch"),
            _ => {}
        }
        let mut zip =
            zip::ZipWriter::new_append(Cursor::new(offset_bundle(false, "OFFSET"))).unwrap();
        zip.start_file(
            "partstudios/studio/sketches.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&json::to_vec(&json!({"sketches":[row]})).unwrap())
            .unwrap();
        let (model, report) = read(&zip.finish().unwrap().into_inner()).unwrap();
        assert_support_frame_kept(&model, case);
        assert!(
            has_loss(
                &report,
                "onshape/sketch.plane-assumed",
                "assumed the support"
            ),
            "{case}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn absent_sketch_response_keeps_the_support_frame_with_named_assumption() {
    let (model, report) = read(&offset_bundle(false, "OFFSET")).unwrap();
    assert_support_frame_kept(&model, "absent");
    assert!(has_loss(
        &report,
        "onshape/sketch.plane-assumed",
        "assumed the support"
    ));
}

#[test]
fn sketch_response_microversion_must_match_the_bundle() {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(offset_bundle(false, "OFFSET"))).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let matrix = [
        0.0, -1.0, 0.0, 0.002, 1.0, 0.0, 0.0, 0.003, 0.0, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0,
    ];
    let response = json!({"sourceMicroversion":"another-microversion",
        "sketches":[{"featureId":"Base sketch","sketchMatrix":matrix}]});
    zip.write_all(&json::to_vec(&response).unwrap()).unwrap();
    let (model, report) = read(&zip.finish().unwrap().into_inner()).unwrap();
    assert_support_frame_kept(&model, "wrong-microversion");
    // cadmpeg reports a stale member as unreadable and retains its bytes.
    assert!(has_loss(
        &report,
        "onshape/member.unreadable",
        "microversion"
    ));
}

#[test]
fn sketch_response_row_count_is_bounded() {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(offset_bundle(false, "OFFSET"))).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let response = json!({"sketches":vec![json!({}); crate::reader::ROWS + 1]});
    zip.write_all(&json::to_vec(&response).unwrap()).unwrap();
    let (model, report) = read(&zip.finish().unwrap().into_inner()).unwrap();
    assert_support_frame_kept(&model, "row-limit");
    assert!(has_loss(
        &report,
        "onshape/sketch.plane-assumed",
        "row limit"
    ));
}

#[test]
fn optional_sketch_response_preserves_existing_default_plane_output() {
    let baseline = crate::test_support::review::decode(&fixture("default-plane-regions"));
    let bytes =
        crate::test_support::review::members(&fixture("default-plane-regions"), |members| {
            let response: Value =
                json::from_slice(&members["partstudios/studio/features.json"]).unwrap();
            let native = response["features"][0]["featureId"].clone();
            members.insert(
                "partstudios/studio/sketches.json".into(),
                json::to_vec(&json!({
                "sketches":[{"featureId":native,"sketchMatrix":[
                    1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0,
                    0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0]}]}))
                .unwrap(),
            );
        });
    let updated = crate::test_support::review::decode(&bytes);
    // The response member itself is retained as an asset; everything else is unchanged.
    let mut expected = crate::test_support::review::wire(&baseline);
    let mut actual = crate::test_support::review::wire(&updated);
    expected["model"]["assets"] = json!([]);
    actual["model"]["assets"] = json!([]);
    assert_eq!(expected, actual);
    crate::test_support::review::validate(updated);
}
