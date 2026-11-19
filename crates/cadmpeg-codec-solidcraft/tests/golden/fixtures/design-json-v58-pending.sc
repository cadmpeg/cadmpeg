{
  "format": "solidcraft/58",
  "name": "Block",
  "units": "mm",
  "params": [
    {
      "name": "w",
      "expr": "20 mm",
      "unit": "mm",
      "model": false
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "Box1",
      "suppressed": false,
      "type": "box",
      "corner": [
        0.0,
        0.0,
        0.0
      ],
      "length": "w",
      "width": "10 mm",
      "height": "5 mm",
      "operation": "new_body"
    }
  ],
  "appearances": {
    "bodies": {}
  },
  "format_pending": [
    "bounding-solid-side-margins",
    "joint-lateral-offsets",
    "move-rotate-about-edge",
    "render-coat-flakes",
    "sketch-3d-constraints",
    "sketch-3d-dimensions",
    "sketch-3d-relations",
    "sketch-reference-placements",
    "step-import-solid-items",
    "surface-sweep-exact-spline-path",
    "whole-profile-holes-share-a-curve"
  ]
}
