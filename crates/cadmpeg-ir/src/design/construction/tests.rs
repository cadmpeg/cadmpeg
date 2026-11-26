use super::{Construction, ConstructionKind};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone)]
struct CountedKind;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct CountedValue {
    value: u32,
    #[serde(skip)]
    clones: Rc<Cell<usize>>,
}

impl Clone for CountedValue {
    fn clone(&self) -> Self {
        self.clones.set(self.clones.get() + 1);
        Self {
            value: self.value,
            clones: Rc::clone(&self.clones),
        }
    }
}

impl ConstructionKind for CountedKind {
    type Value = CountedValue;
}

#[test]
fn construction_serialization_borrows_its_resolved_value() {
    let clones = Rc::new(Cell::new(0));
    let construction = Construction::<CountedKind> {
        resolved: Some(CountedValue {
            value: 7,
            clones: Rc::clone(&clones),
        }),
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved_in: std::collections::BTreeMap::new(),
    };
    let wire = cadmpeg_core::serde_erase::json::to_value(&construction)
        .expect("serialize resolved construction");
    assert_eq!(clones.get(), 0);
    assert_eq!(
        wire,
        serde_json::json!({"inputs":[],"after":[],"resolved":7,"resolved_in":{}})
    );
}
