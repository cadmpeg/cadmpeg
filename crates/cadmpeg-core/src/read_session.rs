// SPDX-License-Identifier: Apache-2.0
//! The repairs of a permissive CADIR read.
//!
//! A value's reader refuses every spelling but the canonical one. Inside
//! [`run`], a reader that meets a spelling or a value with one reading
//! (§3.3 of the CADIR specification, Reading CADIR) states that reading
//! instead and calls [`repair`], which records what it changed. Outside a
//! session [`repair`] returns `false` and the reader refuses as before, so
//! `Deserialize` stays strict for every caller that does not read through
//! the permissive reader.
//!
//! The session is thread-local, so it reaches readers that re-read a payload
//! through a fresh `serde_json` deserializer, as the tagged-enum reader does.

use std::cell::RefCell;

/// What a repair changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Repair {
    /// A spelling with one reading was stated in its canonical form.
    Spelling,
    /// A value with one reading was stated as that value.
    Value,
}

/// One repair, with the text that names what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repaired {
    /// What the repair changed.
    pub repair: Repair,
    /// What the reader met, as a phrase.
    pub what: String,
}

thread_local! {
    static SESSION: RefCell<Option<Vec<Repaired>>> = const { RefCell::new(None) };
}

/// Restores the enclosing session when dropped, so a panic inside [`run`]
/// leaves no session open.
struct Restore(Option<Vec<Repaired>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let outer = self.0.take();
        SESSION.with(|session| *session.borrow_mut() = outer);
    }
}

/// Runs `read` in a fresh repair session and returns its result with the
/// repairs it made. An enclosing session is restored afterwards and does not
/// see these repairs.
pub fn run<R>(read: impl FnOnce() -> R) -> (R, Vec<Repaired>) {
    let restore = Restore(SESSION.with(|session| session.borrow_mut().replace(Vec::new())));
    let result = read();
    let repairs = SESSION.with(|session| session.borrow_mut().take().unwrap_or_default());
    drop(restore);
    (result, repairs)
}

/// Whether a repair session is open on this thread.
#[must_use]
pub fn active() -> bool {
    SESSION.with(|session| session.borrow().is_some())
}

/// Records a repair when a session is open and returns whether one is. A
/// reader repairs only where this returns `true`, and refuses otherwise.
pub fn repair(repair: Repair, what: impl FnOnce() -> String) -> bool {
    SESSION.with(|session| match session.borrow_mut().as_mut() {
        Some(repairs) => {
            repairs.push(Repaired {
                repair,
                what: what(),
            });
            true
        }
        None => false,
    })
}

#[cfg(test)]
mod tests {
    use super::{active, repair, run, Repair};

    #[test]
    fn repairs_are_recorded_only_inside_a_session_and_do_not_leak_outward() {
        assert!(!repair(Repair::Spelling, || "outside".into()));
        let ((), outer) = run(|| {
            assert!(active());
            assert!(repair(Repair::Spelling, || "outer".into()));
            let ((), inner) = run(|| {
                assert!(repair(Repair::Value, || "inner".into()));
            });
            assert_eq!(inner.len(), 1);
            assert_eq!(inner[0].what, "inner");
        });
        assert!(!active());
        assert_eq!(outer.len(), 1);
        assert_eq!(outer[0].what, "outer");
    }

    #[test]
    fn a_panic_inside_a_session_closes_it() {
        let caught = std::panic::catch_unwind(|| {
            run(|| panic!("reader panicked"));
        });
        assert!(caught.is_err());
        assert!(!active());
    }
}
