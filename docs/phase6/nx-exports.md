# NX facade export measurement

Measured before Phase 6 narrowing from `HEAD`:

- Root `pub mod` declarations: 11.
- Recursive `pub` token count across tracked `src/*.rs`: 4,282. This includes
  `pub(crate)` declarations and documentation text; it is a coarse baseline.

Measured after narrowing:

- Root public modules: 6 product parser modules (`container`, `geometry`,
  `intersection`, `nurbs`, `parasolid`, `topology`), plus the
  feature-gated hidden `fuzz` module.
- Recursive coarse `pub` token count: 4,283, including the new fuzz entry
  points. Internal declarations remain unchanged.
