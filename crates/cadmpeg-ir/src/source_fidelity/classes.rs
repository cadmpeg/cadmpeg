// SPDX-License-Identifier: Apache-2.0
//! Wire form of a set of arena classes: an array of class names sorted in
//! declared class order, each named once.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserializer, Serializer};

use cadmpeg_core::decode::ArenaClass;

/// Returns the wire name of an arena class.
pub(super) const fn name(class: ArenaClass) -> &'static str {
    match class {
        ArenaClass::Model => "model",
        ArenaClass::Design => "design",
        ArenaClass::Process => "process",
    }
}

const NAMES: [&str; ArenaClass::ALL.len()] = {
    let mut names = [""; ArenaClass::ALL.len()];
    let mut index = 0;
    while index < names.len() {
        names[index] = name(ArenaClass::ALL[index]);
        index += 1;
    }
    names
};

fn parse(text: &str) -> Option<ArenaClass> {
    ArenaClass::ALL
        .into_iter()
        .find(|class| name(*class) == text)
}

/// Writes a class set as its sorted array of names.
pub(super) fn serialize<S: Serializer>(
    classes: &BTreeSet<ArenaClass>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut sequence = serializer.serialize_seq(Some(classes.len()))?;
    for class in classes {
        sequence.serialize_element(name(*class))?;
    }
    sequence.end()
}

/// Reads a class set, refusing an unknown name, a repeated name, and names
/// out of declared class order.
pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeSet<ArenaClass>, D::Error> {
    deserializer.deserialize_seq(ClassSetVisitor)
}

struct ClassSetVisitor;

impl<'de> Visitor<'de> for ClassSetVisitor {
    type Value = BTreeSet<ArenaClass>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of arena class names in class order")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut classes = BTreeSet::new();
        let mut last = None;
        while let Some(text) = sequence.next_element::<String>()? {
            let class = parse(&text).ok_or_else(|| de::Error::unknown_variant(&text, &NAMES))?;
            if last.is_some_and(|last| class <= last) {
                return Err(de::Error::custom(format_args!(
                    "arena class {text:?} repeats or is out of class order"
                )));
            }
            last = Some(class);
            classes.insert(class);
        }
        Ok(classes)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::{Deserialize, Serialize};

    use cadmpeg_core::decode::ArenaClass;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder {
        #[serde(with = "super")]
        classes: BTreeSet<ArenaClass>,
    }

    #[test]
    fn a_class_set_writes_its_names_in_class_order() {
        let holder = Holder {
            classes: [ArenaClass::Process, ArenaClass::Design, ArenaClass::Model].into(),
        };
        let text = serde_json::to_string(&holder).expect("serializes");
        assert_eq!(text, r#"{"classes":["model","design","process"]}"#);
        assert_eq!(
            serde_json::from_str::<Holder>(&text).expect("reads back"),
            holder
        );
        assert_eq!(
            serde_json::to_string(&Holder {
                classes: BTreeSet::new()
            })
            .expect("serializes"),
            r#"{"classes":[]}"#
        );
    }

    #[test]
    fn a_class_set_has_one_spelling() {
        for text in [
            r#"{"classes":["design","model"]}"#,
            r#"{"classes":["model","model"]}"#,
        ] {
            let error = serde_json::from_str::<Holder>(text)
                .expect_err("a set is written once, in class order")
                .to_string();
            assert!(error.contains("out of class order"), "{text}: {error}");
        }
        let error = serde_json::from_str::<Holder>(r#"{"classes":["geometry"]}"#)
            .expect_err("only arena class names are classes")
            .to_string();
        assert!(error.contains("geometry"), "{error}");
    }
}
