# SOLIDWORKS SLDPRT: CADIR projection and writing

The decoder projects readable bodies and topology, feature history, sketches,
parameters, configurations, presentation, and PMI into their neutral arenas.
Typed native records retain SOLIDWORKS-specific content. Feature
`source_properties` are empty; the decoder consumes feature fields into their
typed homes. Custom properties project to neutral attributes. Parameter properties are
empty; `EquationId` and `Driven` stay in the native dimension record.
`Driven` supplies the parameter's `driving` state.

The active configuration's `swConfigurationNeedsUpdate="YES"` marks every
feature `needs_recompute`. The decoder keeps the cached geometry and parameter
values. The writer sets the active configuration flag when any feature is stale;
the format flag has configuration granularity.

The writer requests full decode. It writes from the current neutral model and
its own typed native records, and reports each arena's disposition. Patch and
replay require empty `unrepresented`; an edit that cannot be patched selects
synthesis. The export report names omitted content.
