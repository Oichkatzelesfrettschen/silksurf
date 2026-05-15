# COLOR.md -- SilkSurf Color Science Policy

WHY: Correct color reproduction requires explicit policy at every stage of
the pipeline. Without a written policy, engineers make inconsistent choices:
some code paths composite in sRGB space (incorrect), some store premultiplied
alpha without documenting it, and wide-gamut inputs get silently clipped.
This document is the single authoritative source of truth for color decisions
in silksurf. Every color-touching patch must be consistent with it.

WHAT: Covers color space, alpha handling, numeric representation, precision,
wide-gamut posture, and conversion formulas for all code under crates/ and
silksurf-js/.

HOW: Read this document before touching any color path. When a new color
feature is added, update this document first (specification before code).

---

## Color Space Policy

sRGB is the canonical interchange format throughout the pipeline.

WHY: CSS Color Level 4 section 4 establishes sRGB as the default color space
for all CSS color values. Content authors target sRGB. Nearly all consumer
displays present sRGB natively. Using a different interchange space would
silently shift all CSS-authored colors.

WHAT:
- All Color structs (silksurf_css::Color) carry sRGB-encoded u8 channel values.
- All ARGB u32 framebuffer pixels carry sRGB-encoded u8 channels.
- Network-fetched image data (PNG, JPEG without embedded profile) is assumed
  sRGB at decode time.
- The rasterizer receives colors in sRGB and stores pixels in sRGB.

Exception: intermediate compositing buffers operate in linear light (see
"Precision Policy" below).

---

## sRGB <-> Linear Conversion

WHY: The sRGB transfer function is perceptual, not additive. Blending two
sRGB-encoded colors produces visually incorrect results (too dark at the
midpoint). All alpha compositing and linear blending MUST be performed in
linear-light coordinates.

The formulas used are the IEC 61966-2-1:1999 piecewise functions:

### sRGB to Linear (IEC 61966-2-1, section 4.2)

    Given u8 channel c:
      c_f   = c / 255.0                           (normalize to [0.0, 1.0])
      c_lin = c_f / 12.92                          if c_f <= 0.04045
            = ((c_f + 0.055) / 1.055) ^ 2.4       otherwise

    Output: f32 in [0.0, 1.0], clamped.

### Linear to sRGB (IEC 61966-2-1, section 4.2, inverse)

    Given f32 c_lin in [0.0, 1.0]:
      c_encoded = c_lin * 12.92                   if c_lin <= 0.0031308
                = 1.055 * c_lin ^ (1/2.4) - 0.055 otherwise
      c_u8      = round(c_encoded * 255.0)

    Output: u8 in [0, 255], clamped.

### Implementation

See crates/silksurf-render/src/lib.rs:
  pub(crate) fn srgb_to_linear(c: u8) -> f32
  pub(crate) fn linear_to_srgb(c: f32) -> u8

### Round-trip error

The round-trip sRGB(u8) -> linear(f32) -> sRGB(u8) introduces at most 1 LSB
of error for all 256 input values. This is verified by the full-sweep test in
crates/silksurf-render/tests/color.rs.

---

## Alpha Premultiplication Policy

WHY: CSS compositing uses Porter-Duff "over" semantics (CSS Compositing and
Blending Level 1, section 9). Porter-Duff equations are derived assuming
premultiplied (associated) alpha. Using straight alpha in Porter-Duff
arithmetic produces incorrect colors at partially-transparent edges and
requires an extra division per pixel.

### Definitions

Straight alpha (unassociated):  the RGB channels are independent of alpha.
  Stored as: (r, g, b, a) where r, g, b are full-range sRGB values.

Premultiplied alpha (associated): each RGB channel is pre-scaled by alpha/255.
  Stored as: (r*a/255, g*a/255, b*a/255, a).

### When premultiplication is applied

- At the boundary between CSS-parsed color values and compositing operations.
  CSS color parsing produces straight-alpha Color structs.
  Before any Porter-Duff "over" operation the RGB channels are premultiplied.

- Image data that arrives pre-multiplied from the decoder stays premultiplied
  through the compositing stack and is only unpremultiplied at export time.

### When unpremultiplication is applied

- When a premultiplied pixel must be re-encoded to sRGB for storage or export
  (e.g. saving to PNG with straight-alpha convention).
- When a premultiplied pixel value is read back for color-picking or
  accessibility queries (the DOM color value must be in straight-alpha form).

### Fully-transparent pixels (alpha = 0)

When alpha is zero there is no colour to recover. unpremultiply(r,g,b,0)
returns (0,0,0,0) by definition. Code must not divide by a=0.

### Precision note

The premultiply/unpremultiply functions in lib.rs operate in sRGB encoded
space (premultiplied-sRGB). This is the correct convention for framebuffer
storage and matches the behaviour of Cairo, Skia, and pixman. For full
spectral correctness during compositing, convert to linear FIRST, premultiply
in linear space, composite, then convert back. The current implementation
does not yet do this; see "Precision Policy" below for the planned upgrade.

### Implementation

See crates/silksurf-render/src/lib.rs:
  pub(crate) fn premultiply(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8)
  pub(crate) fn unpremultiply(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8)

Integer approximation used (same as Cairo/pixman):
  premult_c = (c * a + 127 + ((c * a + 127) >> 8)) >> 8

This produces correctly-rounded results and avoids floating-point conversion
in the hot per-pixel path.

---

## ARGB u32 Packing Format

WHY: The framebuffer is a Vec<u8> addressed as u32 pixels via unsafe reinterpret.
A single shared convention for channel order prevents R/B transpositions that
are invisible in grayscale but obvious in colored output.

### Convention

    packed: u32 = A<<24 | R<<16 | G<<8 | B

    Unpack:
      a = (packed >> 24) & 0xFF
      r = (packed >> 16) & 0xFF
      g = (packed >> 8)  & 0xFF
      b = packed         & 0xFF

All code that constructs or destructures u32 pixels MUST use this layout.
The layout is little-endian within the u32 but the u32 itself is stored in
native byte order by the framebuffer.

### Relationship to Vec<u8> layout

The Vec<u8> framebuffer stores pixels row-major, 4 bytes per pixel.
For pixel at (x, y) in a viewport of width W:
  byte_offset = (y * W + x) * 4
  buffer[byte_offset + 0] = R (on little-endian hardware: low byte of u32)
  buffer[byte_offset + 1] = G
  buffer[byte_offset + 2] = B
  buffer[byte_offset + 3] = A

XCB ARGB visual expects this layout. See docs/XCB_GUIDE.md for display
submission details.

---

## Precision Policy

WHY: u8 arithmetic on sRGB values introduces visible banding in gradients
and blended regions. f32 linear-light intermediate storage eliminates this
at modest memory cost (4x per-pixel during compositing, not in storage).

### Current state (Phase 3 / P8)

- Fill-rect operations write sRGB u8 values directly (no blending yet).
- sRGB->linear and linear->sRGB conversions are implemented in f32.
- Premultiply/unpremultiply operate in integer sRGB space (fast path).

### Target state (Phase 5, compositing)

- Per-layer compositing buffer: f32 RGBA, linear light.
- Each layer accumulates Porter-Duff "over" in f32 linear.
- Final composite is converted to sRGB u8 for the framebuffer.
- Alpha is premultiplied in linear space before compositing.

This matches the precision model of CSS Compositing and Blending Level 1,
section 9.1.4 (compositing in linear light).

---

## Wide-Gamut Posture

WHY: Display P3 and Rec.2020 are increasingly common on consumer hardware
and are addressed by CSS Color Level 4 (color() function, display-p3 space).
Supporting them without a policy leads to ad-hoc clipping that silently
destroys out-of-gamut values.

### Current decision: sRGB only (Display P3 deferred)

silksurf currently supports sRGB only. All colors are clamped to [0, 255]
per channel before storage or display.

WHY deferred: Implementing a wide-gamut pipeline requires:
  1. A gamut-mapping algorithm (CSS Color Level 4, section 13).
  2. Display detection (ICC profile, EDID query).
  3. Per-layer color space tracking.
  4. Conversion matrices (sRGB->P3: 3x3 float matrix per pixel group).

These are non-trivial and not on the critical path for Phase 3 correctness.

### ADR reference

This decision is recorded as ADR-COLOR-001 in docs/design/ARCHITECTURE-DECISIONS.md.
When wide-gamut support is added, update ADR-COLOR-001 and this section.

### Future upgrade path

When Display P3 is implemented:
  1. Add a ColorSpace enum (Srgb, DisplayP3) to silksurf_css::Color.
  2. Add a 3x3 f32 gamut-mapping matrix for P3->sRGB fallback on sRGB displays.
  3. Add display color space detection to the XCB window setup.
  4. All intermediate compositing already in f32 linear (see Precision Policy),
     so the only new cost is the color space tag and the 3x3 multiply.

---

## Numeric Representation Summary

| Domain                  | Type     | Space         | Alpha        |
|-------------------------|----------|---------------|--------------|
| CSS parsed colors       | u8 x4    | sRGB          | straight     |
| Framebuffer pixels      | u32      | sRGB          | premultiplied|
| Compositing intermediate| f32 x4   | linear light  | premultiplied|
| Test reference values   | f32/u8   | sRGB/linear   | straight     |

---

## References

- IEC 61966-2-1:1999 -- sRGB: Multimedia systems and equipment -- Colour
  measurement and management -- Part 2-1: Colour management -- Default RGB
  colour space -- sRGB.

- CSS Color Level 4, W3C Working Draft:
  https://www.w3.org/TR/css-color-4/

- CSS Compositing and Blending Level 1, W3C Candidate Recommendation:
  https://www.w3.org/TR/compositing-1/

- Porter, T. and Duff, T. "Compositing Digital Images." SIGGRAPH 1984.
  Proceedings of the 11th Annual Conference on Computer Graphics and
  Interactive Techniques. ACM, 1984.

- ICC Profile Format Specification, Version 4.4.0:
  https://www.color.org/specification/ICC1v43_2010-12.pdf
