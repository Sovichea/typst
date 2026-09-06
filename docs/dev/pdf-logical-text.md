# PDF logical text across split source spans

## Incident

Typsastra Enhanced Unicode Engine 0.4.0 produced duplicate Khmer text in five valid-source cases where regular text followed strong content. For example, ក្រសួង extracted as ក្រសួសួង, and គ្រប់គ្រង extracted as គ្រប់ប់គ្រង. Rendering was visually correct, and Poppler, PDFium, and PyMuPDF agreed on the duplicated logical text.

Six additional reports from the same corpus had a separate cause: the synthetic generator placed a paragraph break inside an explicit paragraph block. Typst warns that this construct is ignored. Those cases belong to the generator and are not addressed by this PDF fix.

## Root cause

Complex-script shaping can split one source run into multiple TextItems at different visual baselines. The PDF exporter batches overlapping items so that their visual glyphs share one authoritative logical string.

At a style boundary, however, the first regular item can contain two source spans:

- a separator prefix such as a space from the surrounding markup
- Khmer text from the following source node

For an item like ក្រសួ with a leading space, the Khmer source starts at source offset zero but at local UTF-8 byte offset one. Its affine source-to-item delta is therefore -1. The old text_source_base helper required one span for the whole item and represented the delta with an unsigned checked subtraction. It rejected the mixed-span item before batching could consider the overlapping baseline fragment.

The rejected first item was emitted independently. Batching then started at the following fragment, so the PDF contained consecutive semantic units for both ក្រសួ and សួង.... Multiple CIDs can legally map to equal Unicode, but emitting both for the same source interval duplicated extraction.

## Fix

The PDF batcher keeps the complete first item as the authoritative string when it has a strictly contiguous foreign prefix. It finds the span shared with the next visual fragment, calculates the source delta as a signed value, and shifts subsequent source coordinates past the prefix.

The fallback is deliberately conservative. It applies only when:

- the next item has matching font, size, paint, stroke, language, and region
- the first item has a non-empty foreign prefix
- all glyphs after that prefix use the shared source span
- all shared glyphs have one consistent signed source delta
- all byte ranges are valid UTF-8 boundaries
- the existing source-overlap and baseline-span checks succeed
- overlapping text bytes agree exactly

Conflicting, non-contiguous, or ambiguous layouts continue through the ordinary text path.

## Invariants

- deduplicate semantic ownership by a validated source interval, never by Unicode equality alone
- preserve every visual glyph and baseline position
- preserve unrelated prefix text exactly once
- preserve legitimate repeated text at distinct source offsets
- stop batching at tags, non-text frame items, style changes, language changes, region changes, and visual line boundaries
- fail closed when source mappings or overlapping bytes conflict

## Regression coverage

The unit tests model the reported leading-space plus Khmer layout directly and verify that a source delta of -1 becomes a one-byte logical prefix shift. A second test verifies that foreign text appearing after the prefix is rejected. Existing tests continue to cover overlapping Khmer fragments, legitimate repeated text, conflicting overlaps, and line-boundary protection.

The end-to-end reproducer retains both Khmer-label and Latin-label forms because the bold label content is not part of the trigger. It also includes legitimately repeated text as a false-positive control:

    #strong[ផ្នែក ក] ក្រសួងប្រៃសណីយ៍។
    #strong[Label] ការគ្រប់គ្រងគម្រោង។
    #strong[Control] ពាក្យដដែល ពាក្យដដែល។

Validation should compare extracted UTF-8 bytes and inspect the regular-font ToUnicode CMap. Each source cluster must have one semantic CID even when several visual glyph components contribute to it.

## End-to-end validation

The regression was reproduced with the static Regular and Bold TTF faces from
the official Noto Sans Khmer v2.004 release. System fonts were disabled during
compilation so both builds used the same font files.

Font SHA-256 hashes:

- `NotoSansKhmer-Regular.ttf`: `CC8AF91F5558AC8E53FCE83213328DADA31B22EDB562E44938F04238A2A65B6A`
- `NotoSansKhmer-Bold.ttf`: `4CF9803A479D68CB637FA8094FDF7BBD6B63EA546856E406E435101C12A6B24D`

The parent build at `821bf1bf6` reproduced both reported failures:

- `ក្រសួង` extracted as `ក្រសួសួង`
- `គ្រប់គ្រង` extracted as `គ្រប់ប់គ្រង`

The fixed build at `08e0d58b7` matched the authored UTF-8 text exactly in
Poppler, PDFium, and pypdf. PDFium search also changed as expected:

| Search text | Parent | Fixed | Expected |
|---|---:|---:|---:|
| `ក្រសួង` | 0 | 1 | 1 |
| `គ្រប់គ្រង` | 0 | 2 | 2 |
| `ពាក្យដដែល` | 2 | 2 | 2 |

The unchanged control count verifies that batching removes overlapping
semantic ownership without deduplicating equal text at distinct source
positions.

`qpdf --check` reported no syntax or stream-encoding errors for the parent,
fixed, or standards-enabled files. The fixed document also compiled with
combined PDF/A-2b and PDF/UA-1 settings, and veraPDF passed both profiles.

Visual inspection at 144 DPI found no layout, clipping, overlap, or legibility
regression. The raster images were not byte-identical because the repaired
words use different logical text grouping. Differences were confined to
antialiased glyph edges: 2,217 of 447,198 pixels (`0.4958%`), with a mean
absolute grayscale error of `0.097248` on a 0-255 scale.
