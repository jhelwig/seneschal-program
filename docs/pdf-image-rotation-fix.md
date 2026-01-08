# Plan: Fix PDF Image Rotation/Mirroring in Extraction

## Problem Summary

PDF images are extracted with incorrect orientation. The current implementation uses poppler-rs's `page.image()` which returns **raw, untransformed image data**. PDF transformation matrices (CTM) that specify rotation, mirroring, and scaling are NOT applied.

**Evidence**:
- Page 130 pistol: weapon is tiny with wrong z-order relative to its shadow
- Page 133 grenade launcher: weapon points left but shadow points right (opposite orientations)

## Root Cause

When images are stored in a PDF, they may be stored in any orientation. The PDF content stream uses the Current Transformation Matrix (CTM) to position, scale, rotate, and mirror images when rendering. Poppler's glib API (`image_mapping()` + `page.image()`) provides:
- Bounding boxes (already transformed to page coordinates)
- Raw image data (NOT transformed)

The glib bindings do NOT expose the transformation matrix for each image placement.

## Constraints

- **pdfium-render was already tried and failed** - it detected only ~60 1x1 pixel images instead of 300+ actual images
- **lopdf/pdf-extract cannot parse this PDF** - parsing fails completely
- **mupdf-rs is AGPL licensed** - license incompatibility
- **Region rendering is not acceptable** - extracted images must be clean without surrounding page content
- **Must preserve layer compositing** - drop shadows and overlapping images need proper z-order handling

## Solution: qpdf-rs Crate for CTM Extraction

Use the **qpdf** Rust crate (v0.3.2, Apache-2.0 license, binds to libqpdf) to extract transformation matrices from content streams, then apply them to poppler-extracted images.

**Why hybrid approach (qpdf + poppler)?**
- **qpdf** excels at PDF structure parsing and content stream access, but cannot rasterize images
- **poppler** excels at image rasterization with color space conversion and mask compositing, but doesn't expose CTMs

**Key qpdf APIs**:
- `QPdf::read()` - load PDF file
- `get_pages()` - get page objects
- `get_object_by_id()` - get objects by reference (for Form XObjects)
- `QPdfStream::get_data(StreamDecodeLevel)` - get decoded content stream

### Verified: qpdf CAN extract CTMs from this PDF

Testing confirmed qpdf successfully parses the PDF and extracts transformation matrices:
- Page 130 CTM: `-181.6975393 -149.7798976 -96.6655364 117.2646609 295.1147399 600.738513`
- Page 133 CTM: `-214.2315989 -189.5361407 -89.5764981 101.2478045 297.4960538 640.6231061`

The negative `a` and complex `b,c` values confirm rotation + mirroring transformations.

### PDF Structure (from investigation)

```
Page -> Form XObject (e.g., I129) -> Content Stream with:
    q
    /GS0 gs
    [a b c d e f] cm    <-- transformation matrix
    /Im0 Do             <-- draw image
    Q
```

### Transformation Matrix Components
```
[a b 0]
[c d 0]   where [a b c d e f] are the 6 values
[e f 1]
```
- `a`, `d`: scaling (negative values = mirroring)
- `b`, `c`: rotation/skewing
- `e`, `f`: translation

## Implementation Steps

### 1. Add qpdf dependency
**File**: `Cargo.toml` (workspace) and `seneschal-service/Cargo.toml`

```toml
qpdf = "0.3"
```

Note: Requires libqpdf to be installed on the system (libqpdf-dev on Debian/Ubuntu).

### 2. Create qpdf CTM extraction function
**File**: `seneschal-service/src/ingestion.rs` (or new `pdf_transform.rs`)

```rust
use qpdf::{QPdf, StreamDecodeLevel};

struct ImageTransform {
    page_num: usize,
    xobject_name: String,
    matrix: [f64; 6],
    image_obj_id: Option<(u32, u16)>,  // (object_id, generation) for matching
}

fn extract_image_transforms_with_qpdf(path: &Path) -> Result<Vec<ImageTransform>>
```

Logic:
1. Load PDF with `QPdf::read(path)`
2. Iterate pages with `get_pages()`
3. For each page, get Resources dictionary, then XObject dictionary
4. For each Form XObject, get decoded content stream with `stream.get_data(StreamDecodeLevel::All)`
5. Parse content stream text for patterns: `[6 numbers] cm` followed by `/ImN Do`
6. Track graphics state stack (`q`/`Q`) to handle cumulative CTMs
7. Handle nested Form XObjects recursively

### 3. Create image transformation function
**File**: `seneschal-service/src/ingestion.rs`

```rust
fn apply_transform(image: &RgbaImage, matrix: &[f64; 6]) -> RgbaImage
```

Decompose CTM into operations:
1. Extract rotation angle: `theta = atan2(b, a)` (or `atan2(-c, d)`)
2. Detect horizontal flip: check if determinant `(a*d - b*c)` is negative
3. Detect vertical flip: check matrix sign patterns
4. Apply transformations using `image::imageops`:
   - `rotate90`, `rotate180`, `rotate270` for 90° increments
   - `flip_horizontal`, `flip_vertical` for mirroring
   - For arbitrary angles, use affine transformation

### 4. Match poppler images to qpdf CTM data
**File**: `seneschal-service/src/ingestion.rs`

Use **position-based matching**:
1. Calculate expected bounding box from CTM + image dimensions
2. Compare to poppler's `image_mapping()` bounding boxes
3. Match by closest overlap (within floating-point tolerance)

### 5. Integrate into extraction pipeline
**File**: `seneschal-service/src/ingestion.rs`

Modify `extract_pdf_images()`:
1. Call `extract_image_transforms_with_qpdf()` first
2. Extract raw images with poppler (existing code)
3. For each image, find matching CTM and apply transformation
4. Continue with existing grouping/compositing logic

### Files to Modify

1. `Cargo.toml` - add qpdf dependency
2. `seneschal-service/Cargo.toml` - add qpdf dependency
3. `seneschal-service/src/ingestion.rs` - main implementation

### System Dependencies

- `libqpdf-dev` (Debian/Ubuntu) or equivalent must be installed for compilation

## Verification

1. Re-extract images from the test PDF after implementation
2. Compare page 130 pistol - should be correctly sized and oriented relative to shadow
3. Compare page 133 grenade launcher - weapon and shadow should have matching orientations
4. Verify other weapon images (pages 131-135) have correct orientation
5. Run existing tests to ensure no regressions

## Alternative Approaches Considered

1. **pdfium-render only** - Failed image detection (60 1x1 images)
2. **lopdf/pdf-extract** - Cannot parse this PDF
3. **mupdf-rs** - AGPL license, incompatible
4. **Region rendering** - Would include unwanted content
5. **Poppler C++ API** - Requires custom Rust bindings, maintenance burden
6. **Heuristic from dimensions** - Can detect 90° rotation but not mirroring direction

## Open Questions

1. Does nested Form XObject structure require recursive CTM tracking?
   - Initial investigation shows simple structure, but may need recursion for edge cases
   - Will implement iteratively: simple first, add recursion if needed

2. Are there images with arbitrary rotation angles (not 90° multiples)?
   - The CTMs found show complex rotation values
   - May need affine transformation for non-90° angles
