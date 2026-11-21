// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_curves_and_surfaces::point_file_with_global;
use crate::IgesCodec;

const SOURCE_KEYS: [&str; 13] = [
    "sender_product",
    "native_file_name",
    "native_system_id",
    "preprocessor_version",
    "model_scale",
    "units_flag",
    "native_units",
    "author",
    "organization",
    "iges_version_flag",
    "drafting_standard",
    "parameter_delimiter",
    "record_delimiter",
];

fn hollerith(text: &str) -> String {
    format!("{}H{text}", text.len())
}

struct Global<'a> {
    sender_product: &'a str,
    file_name: &'a str,
    native_system: &'a str,
    preprocessor_version: &'a str,
    model_scale: &'a str,
    units_flag: &'a str,
    units_name: &'a str,
    author: &'a str,
    organization: &'a str,
    version_flag: &'a str,
    drafting_standard: &'a str,
}

impl Global<'_> {
    fn bytes(&self) -> Vec<u8> {
        let string = |text: &str| {
            if text.is_empty() {
                String::new()
            } else {
                hollerith(text)
            }
        };
        let mut fields = vec![
            "1H,".to_owned(),
            "1H;".to_owned(),
            string(self.sender_product),
            string(self.file_name),
            string(self.native_system),
            string(self.preprocessor_version),
            "32".to_owned(),
            "38".to_owned(),
            "6".to_owned(),
            "308".to_owned(),
            "15".to_owned(),
            String::new(),
            self.model_scale.to_owned(),
            self.units_flag.to_owned(),
            string(self.units_name),
            "1".to_owned(),
            "1.0".to_owned(),
            "15H20260714.000000".to_owned(),
            "0.001".to_owned(),
            "1000.0".to_owned(),
            string(self.author),
            string(self.organization),
            self.version_flag.to_owned(),
            self.drafting_standard.to_owned(),
        ];
        if self.version_flag != "6" {
            fields.push("15H20260714.000000".to_owned());
            fields.push(String::new());
        }
        let mut global = fields.join(",");
        global.push(';');
        point_file_with_global(global.as_bytes())
    }
}

const SOLIDWORKS: Global<'static> = Global {
    sender_product: "Bracket",
    file_name: "C:\\parts\\Bracket.IGS",
    native_system: "SolidWorks 2023",
    preprocessor_version: "SolidWorks 2023 SP1.0",
    model_scale: "1.0",
    units_flag: "2",
    units_name: "MM",
    author: "jdoe",
    organization: "Acme",
    version_flag: "11",
    drafting_standard: "0",
};

fn global_attributes(global: &Global<'_>) -> BTreeMap<String, String> {
    let result = IgesCodec
        .decode(&mut Cursor::new(global.bytes()), &DecodeOptions::default())
        .expect("decode");
    result
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes
        .iter()
        .filter(|(key, _)| SOURCE_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.as_str().to_owned(), value.clone()))
        .collect()
}

fn expected(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn solidworks_global_fields_become_source_attributes() {
    assert_eq!(
        global_attributes(&SOLIDWORKS),
        expected(&[
            ("parameter_delimiter", ","),
            ("record_delimiter", ";"),
            ("sender_product", "Bracket"),
            ("native_file_name", "C:\\parts\\Bracket.IGS"),
            ("native_system_id", "SolidWorks 2023"),
            ("preprocessor_version", "SolidWorks 2023 SP1.0"),
            ("model_scale", "1"),
            ("units_flag", "2"),
            ("native_units", "MM"),
            ("author", "jdoe"),
            ("organization", "Acme"),
            ("iges_version_flag", "11"),
            ("drafting_standard", "0"),
        ])
    );
}

#[test]
fn other_producer_global_fields_become_source_attributes() {
    let global = Global {
        sender_product: "shaft",
        file_name: "shaft.igs",
        native_system: "Rhinoceros",
        preprocessor_version: "Rhino 8.0",
        model_scale: "2.5",
        units_flag: "1",
        units_name: "INCH",
        author: "Designer",
        organization: "Shop",
        version_flag: "10",
        drafting_standard: "3",
    };
    assert_eq!(
        global_attributes(&global),
        expected(&[
            ("parameter_delimiter", ","),
            ("record_delimiter", ";"),
            ("sender_product", "shaft"),
            ("native_file_name", "shaft.igs"),
            ("native_system_id", "Rhinoceros"),
            ("preprocessor_version", "Rhino 8.0"),
            ("model_scale", "2.5"),
            ("units_flag", "1"),
            ("native_units", "INCH"),
            ("author", "Designer"),
            ("organization", "Shop"),
            ("iges_version_flag", "10"),
            ("drafting_standard", "3"),
        ])
    );
}

#[test]
fn empty_global_fields_keep_only_the_specification_defaults() {
    let global = Global {
        sender_product: "",
        file_name: "",
        native_system: "",
        preprocessor_version: "",
        model_scale: "",
        units_flag: "",
        units_name: "",
        author: "",
        organization: "",
        version_flag: "11",
        drafting_standard: "",
    };
    assert_eq!(
        global_attributes(&global),
        expected(&[
            ("parameter_delimiter", ","),
            ("record_delimiter", ";"),
            ("model_scale", "1"),
            ("units_flag", "1"),
            ("native_units", "IN"),
            ("iges_version_flag", "11"),
            ("drafting_standard", "0"),
        ])
    );
}

#[test]
fn a_malformed_drafting_standard_resolves_to_no_standard() {
    let global = Global {
        drafting_standard: "9",
        ..SOLIDWORKS
    };
    assert_eq!(global_attributes(&global)["drafting_standard"], "0");
}

#[test]
fn hollerith_strings_keep_delimiters_and_quotes() {
    let global = Global {
        native_system: "Sys, \"v2\"; 'beta'",
        author: "O'Brien",
        ..SOLIDWORKS
    };
    let attributes = global_attributes(&global);
    assert_eq!(attributes["native_system_id"], "Sys, \"v2\"; 'beta'");
    assert_eq!(attributes["author"], "O'Brien");
}

#[test]
fn global_strings_leave_out_control_characters() {
    let global = Global {
        native_system: "Solid\u{1}Works\u{7f}",
        version_flag: "6",
        ..SOLIDWORKS
    };
    assert_eq!(global_attributes(&global)["native_system_id"], "SolidWorks");
}

#[test]
fn long_global_strings_are_kept_whole() {
    let long = "x".repeat(5000);
    let global = Global {
        native_system: &long,
        author: &long,
        ..SOLIDWORKS
    };
    let attributes = global_attributes(&global);
    assert_eq!(attributes["native_system_id"], long);
    assert_eq!(attributes["author"], long);
}
