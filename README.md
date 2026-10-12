# pdfcli

A Rust CLI that combines PDF, Word, Excel, and PowerPoint files into one PDF in
the exact order supplied, compresses PDFs, and removes or reorders pages.
PDF pages remain vector/text content; they are not rendered into images.
On macOS, each Office input uses its matching Microsoft Word, Excel or
PowerPoint app when installed, with LibreOffice as the fallback when that app
is absent. OpenDocument files and other platforms use LibreOffice.
Originals stay unchanged.

## Installation

With Rust and Cargo installed, run this from the project folder to install the
optimized release build:

```sh
cargo install --path . --locked --profile release
pdfcli --version
```

Cargo installs the executable into its bin directory, normally `~/.cargo/bin`.
With that directory on your `PATH`, you can run `pdfcli` from any folder. If your
shell cannot find it, add this line to `~/.zshrc` on macOS and open a new terminal:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

To update the installed command after changing the project:

```sh
cargo install --path . --locked --profile release --force
```

To build without installing:

```sh
cargo build --release --locked
./target/release/pdfcli --help
```

## Quick start

Merge existing PDFs:

```sh
pdfcli merge -o combined.pdf first.pdf second.pdf
```

Merge all PDFs directly inside a folder, sorted by filename:

```sh
pdfcli merge --folder "/path/to/PDF files" -o combined.pdf

# Merge the current folder; replace a previous combined.pdf safely.
pdfcli merge --folder . -o combined.pdf --force
```

`--folder DIR` includes `.pdf` files regardless of extension case and skips
subfolders and other file types. Sorting is case-sensitive and lexicographic:
`10.pdf` comes before `2.pdf`, and uppercase letters sort before lowercase.
Use zero-padded names such as `01-cover.pdf`, `02-report.pdf` and
`10-appendix.pdf` for a numbered sequence. The destination PDF and any aliases
to it are excluded, so rerunning does not append the previous merged output.
The folder must contain at least one remaining PDF. Use either `--folder` or
an explicit input list; they cannot be combined.

Mix PDFs with modern or legacy Office files:

```sh
pdfcli merge -o combined.pdf cover.pdf report.docx budget.xlsx slides.pptx appendix.pdf
pdfcli merge -o legacy.pdf report.doc budget.xls slides.ppt
```

Convert a single Office file to PDF:

```sh
pdfcli merge -o workbook.pdf workbook.xlsx
```

When Microsoft Excel is installed on macOS, pdfcli automatically detects separate
tables, keeps their columns on one page wide, and allows long tables to continue
onto more pages. No print-area flags
are needed. All visible sheets are included; empty sheets and proven blank
export pages are skipped. LibreOffice uses its own PDF export and the workbook's
saved print settings. See [Excel export](#excel-export) for details and overrides.

Explicit inputs are processed left to right. Repeating an input repeats its pages. Quote
paths containing spaces. Shell wildcards follow your shell's expansion order.
The output directory must already exist. Existing outputs require `--force`;
input files cannot be overwritten even with that option.

```sh
pdfcli merge -o "Combined report.pdf" "Cover page.pdf" "Annual report.docx"
pdfcli merge --force -o combined.pdf revised.pdf appendix.pdf
pdfcli merge --timeout 180 -o combined.pdf report.docx slides.pptx
pdfcli merge --help
```

## Office conversion requirements

PDF-only merges need no Acrobat, Office, LibreOffice, or external PDF tool.
The `compress`, `remove` and `reorder` commands also work entirely in Rust.
On macOS, pdfcli checks `/Applications` and `~/Applications` for the Microsoft
app matching each input: Word for DOC/DOCX/RTF, Excel for XLS/XLSX, and PowerPoint
for PPT/PPTX. If that app is missing, it automatically uses LibreOffice. Partial
Office installations work: a merge can use Word for one input and LibreOffice
for another. ODT/ODS/ODP files always use LibreOffice. The selected renderer is
reported during conversion.

Installed Microsoft apps must be activated. Use an interactive macOS login session.
macOS may ask you to allow your terminal/Codex to automate Office, and Office may ask once for
access to the dedicated `~/Library/Caches/pdfcli/office` staging folder. The CLI
uses that folder for private input copies and PDFs, then removes them.

Native Office conversion was verified on this machine with a five-input
PDF/DOCX/XLSX/PPTX merge, repeated Word basenames, and an Excel workbook with
two visible sheets and one hidden sheet. Native binary DOC/XLS/PPT fixtures
also pass individual conversion and a seven-input legacy/modern merge; XLS
sheet visibility is covered too. Excel pagination tests cover oversized print
areas, formatting-only trailing cells, manual page breaks, mixed paper sizes,
and hidden/empty sheets. See the
[legacy validation record](docs/evidence/legacy-office-validation.json) and
[Excel export validation](docs/evidence/excel-pagination-validation.json).

Install [LibreOffice](https://www.libreoffice.org/download/download-libreoffice/)
for fallback on macOS, OpenDocument inputs, or Office conversion on Linux/Windows.
All platforms accept `--soffice PATH` or `PDFCLI_SOFFICE`; otherwise pdfcli searches
`PATH` and standard install paths. This setting locates LibreOffice and does not
override an installed Microsoft app. It is only checked when LibreOffice is needed.
Errors from an installed Microsoft app, including activation or Automation
permission failures, are reported rather than retried with a different renderer.

```sh
pdfcli merge --soffice /Applications/LibreOffice.app/Contents/MacOS/soffice \
  -o combined.pdf report.docx budget.xlsx slides.pptx notes.odt
```

The timeout defaults to 120 seconds per Office input, with a short additional
cleanup allowance. Rust tests run on macOS arm64 with Rust 1.97.1;
Windows/Linux runtime behavior has not been exercised here.

## Formats and behavior

### Compress PDFs

```sh
pdfcli compress input.pdf -o smaller.pdf

# Replace an earlier output after compression succeeds.
pdfcli compress input.pdf -o smaller.pdf --force

# Merge first, then compress the resulting PDF.
pdfcli merge --folder ./reports -o combined.pdf
pdfcli compress combined.pdf -o combined-small.pdf
```

Compression is **lossless**: it compresses uncompressed streams, packs PDF
objects into compressed object streams, and removes unreachable objects. It
does not downsample images, re-encode JPEGs or rasterize pages. Page order,
dimensions, text and image quality remain unchanged. The whole document catalog
is retained, including bookmarks, metadata, attachments, links and unsigned
forms. Originals are never modified, and output aliases to the input are refused.

The command reports the before/after byte counts and percentage saved. Already
compressed PDFs, especially image-heavy scans, may have little or no room for
lossless savings. If the optimized file would be the same size or larger, the
command writes an exact unchanged copy and reports that result instead.

Inputs must be PDFs. Encrypted documents, signature fields, signed PDFs and
certification permissions are rejected; use an unencrypted, unsigned copy.
Optimized outputs use PDF 1.5 or newer object/xref streams. Compression does not
preserve linearization or certify PDF/A or PDF/UA conformance. Existing outputs
require `--force`, and failures leave those outputs intact.

### Remove or reorder pages

```sh
# Remove pages 2, 5, 6 and 7; retain the other pages in their original order.
pdfcli remove input.pdf --pages 2,5-7 -o trimmed.pdf

# Move page 5 to the front; every other page follows in its original order.
pdfcli reorder input.pdf --pages 5 -o reordered.pdf

# Move pages 10 through 12 to the front, without listing the remaining pages.
pdfcli reorder input.pdf --pages 10-12 -o reordered.pdf

# Move page 5 to the end, without knowing the total page count.
pdfcli reorder input.pdf --pages rest,5 -o reordered.pdf

# Keep pages 1-3 first, insert page 20 next, then append the remaining pages.
pdfcli reorder input.pdf --pages 1-3,20 -o reordered.pdf

# Reverse all five pages.
pdfcli reorder input.pdf --pages 5-1 -o reversed.pdf
```

Page numbers start at **1** and refer to positions in the input PDF, not printed
page labels. Comma-separated numbers and inclusive ranges are accepted; quote
the list if it contains spaces. Descending ranges reverse their order.

`reorder` accepts a partial page list. Listed pages appear in the order supplied;
all unlisted pages are automatically appended in their original order. Every
page is retained exactly once. For example, `--pages 3,1` on a five-page PDF
produces `3,1,2,4,5`. A complete page list still works as before.

Use `rest` once to place the unlisted pages somewhere other than the end:
`--pages 4,rest,1` produces `4,2,3,5,1` for that same five-page PDF. `rest` uses
the original order and excludes every explicitly listed page, including pages
listed after it. It is supported only by `reorder`.

`remove` must leave at least one page. Both commands reject duplicate page
numbers, overlapping ranges, invalid numbers and numbers outside the PDF's page
count. These commands accept PDFs; merge Office inputs to PDF first when needed.

Each command writes a separate output and leaves the original unchanged,
including when `--force` replaces a previous output. Input aliases cannot be
overwritten. Invalid requests leave existing output intact. Retained pages keep
their content, resources, geometry, rotation and annotations. Local links keep
pointing to the same retained pages after reordering; links to removed pages are
disabled. These commands share the PDF feature limits described below, including
omission of document-level bookmarks and accessibility structure.

### Supported inputs for merge

| Input | Handling |
| --- | --- |
| `.pdf` | Merge PDF object graphs and page subtrees in Rust using `lopdf` |
| `.doc`, `.docx`, `.rtf` | Microsoft Word on macOS when installed; otherwise LibreOffice Writer |
| `.xls`, `.xlsx` | Microsoft Excel on macOS when installed; otherwise LibreOffice Calc |
| `.ppt`, `.pptx` | Microsoft PowerPoint on macOS when installed; otherwise LibreOffice Impress |
| `.odt`, `.ods`, `.odp` | LibreOffice on all platforms |

Extensions are case insensitive. Installed fonts, paper sizes, sheet visibility,
document layout and the Office renderer determine pagination.
The native backend uses each application's modern PDF save command. It does
not run Adobe's Word macros or replicate its selection/clipboard fallbacks.

### Excel export

On macOS with Microsoft Excel installed, pdfcli automatically detects independent
tables and fits each print section to one page **wide**, with automatic vertical
continuation. No Excel arguments are needed:

```sh
pdfcli merge -o combined.pdf workbook.xlsx notes.docx
```

Detection uses native Excel layout metadata: occupied cells, merged headings,
Excel table objects and visible graphics. Persistent blank column gutters or
staggered independent headings can separate side-by-side tables. A description
and its distant amount column stay together. Merged cells, shared headings,
images and charts prevent cuts through spanning content. Worksheets print in
workbook order and detected sections print from left to right, each retaining
its complete vertical extent. Ambiguous layouts stay together as one section;
wide tables may therefore need smaller text. Detection is a layout heuristic,
not a semantic understanding of arbitrary spreadsheets.

The CLI trims trailing formatting-only cells, skips empty worksheets, and
resets manual page breaks on its private copy. It preserves each worksheet's
paper size, orientation and margins. Inspection is bounded to 200,000 used-range
cells per sheet; larger layouts use complete-content width fitting with a
notice, without guessing from a partial scan. The height limit is 32,767 pages,
so long tables are not squeezed onto one page. For a single-cell print area,
one adjacent blank column avoids Excel's confirmation dialog.

Each visible worksheet/section is exported separately. This avoids Excel's
whole-workbook PDF bugs that can omit a sheet or put Letter content on an A4
page with an incorrect vertical offset. Other worksheets are temporarily hidden
on the private copy, keeping cross-sheet formulas intact. Originally hidden
worksheets stay hidden. Workbook structure protection must permit these
visibility changes. The copy is closed without saving; originals are unchanged.

Pages proven to contain no PDF drawing content are removed from Excel exports.
Image-only pages, charts, vector graphics and annotations are retained, as are
unknown or undecodable content, painted white content, and pages containing
only headers/footers. Blank pages in PDF and Word inputs are unchanged.

To use Excel's saved print areas, scale and manual page breaks instead:

```sh
pdfcli merge --excel-preserve-layout -o saved-layout.pdf workbook.xlsx
```

This opt-out still exports every visible worksheet separately and removes proven
blank pages. `--excel-fit-width` remains accepted for compatibility and selects
the default automatic behavior.

Optional manual sections override detection for a named sheet:

```sh
pdfcli merge \
  --excel-print-area 'Schedules=A1:F90' \
  --excel-print-area 'Schedules=G1:L45' \
  -o tables.pdf workbook.xlsx notes.docx
```

Repeat `--excel-print-area SHEET=RANGE` for multiple sections. Rectangles print
in option order within each sheet, one page wide with vertical continuation;
other sheets still use automatic detection. The supplied rectangles replace the
selected sheet's print area, so include all desired content. Each named sheet
must exist and be visible in every Excel input. A1 ranges must contain at least
two cells. Excel options require Microsoft Excel export on macOS; manual areas
and width fitting cannot be combined with `--excel-preserve-layout`. Explicit
Excel options are rejected for spreadsheets using LibreOffice so they cannot
be silently ignored. Omit them to let LibreOffice use its own saved-layout export;
automatic table detection is specific to the Microsoft Excel backend.

### Conversion safety and cleanup

Each Microsoft Office conversion uses a private copy. Where the app exposes its
macro automation setting, macros are forced off while opening the copy, then the
prior setting is restored. PowerPoint 16.113.4 returns no value for this setting;
in that case its existing macro policy is left unchanged. Only the staged document is closed;
Office apps are never quit or killed. A per-user lock serializes pdfcli Office
jobs because macro settings are app-wide. Avoid interacting with the same
Office app during conversion. Timeout kills the script process and attempts
bounded cleanup. If Office is blocked by a dialog and cleanup fails, the error
identifies retained recovery files and an exact recovery command.

LibreOffice conversions use a private profile with Very High macro security.
On Unix their timeout terminates the converter process group; on Windows it
terminates the launched process. No input is submitted to a cloud conversion
service by pdfcli. The external Office app may still process linked resources.

### PDF support and limitations

The merger preserves page order, content streams, resource references,
inherited page geometry/rotation, and page annotations. Local named links are
resolved against their source PDF before object IDs are remapped, so identical
destination names in different inputs do not collide. Both modern destination
name trees and legacy destination dictionaries are supported.

The `merge`, `remove` and `reorder` commands have these **PDF page operation** limits:

- Encrypted PDFs, AcroForms, widget/signature fields, certification permissions,
  and optional-content/layer configurations are rejected. Export an appropriate
  unencrypted, flattened copy before processing. Digital signatures cannot remain
  valid through a PDF rewrite.
- Original bookmarks, accessibility structure, page labels, document-level name
  trees/attachments, and output intents are omitted, with a warning when those
  catalog entries are present. Resolved page links survive the name-tree removal.
  Source metadata, viewer preferences, and document open actions are not copied.
  The result makes no PDF/A, PDF/UA, or accessibility-conformance claim.
- Removing a page removes its page object and prunes unused objects. Shared
  resources used by retained pages remain; page removal is not a redaction tool.
- Broken/cyclic page trees and unresolved named destinations fail the operation.
- PDF objects are held in memory. Very large inputs may need substantial RAM.

The output directory must already exist. Existing output is refused unless
`--force` is specified; input files and aliases are protected even with `--force`.
The CLI writes a temporary PDF beside the destination, flushes it, and publishes
it only after all inputs succeed. Failures leave an existing destination intact.
Conversion files are removed after success and handled failures when Office
cleanup succeeds. Failed cleanup retains recovery files. Force-killing the CLI
can leave temporary files or an open staged document.

## Acrobat investigation

[The investigation report](docs/acrobat-analysis.md) records the installed
Acrobat build, native/static evidence, source locations, and remaining unknowns.
The implementation is independent: it prefers Microsoft Office on macOS,
uses LibreOffice for fallback and other platforms, and uses the open-source
[`lopdf`](https://docs.rs/lopdf/0.45.0/lopdf/) crate, and does not call or redistribute
Adobe libraries. The shared design is **convert Office input to PDF, then append
PDF pages**; matching Acrobat's layout or all of its document features is not
claimed.

LibreOffice's supported [command-line parameters](https://help.libreoffice.org/latest/en-US/text/shared/guide/start_parameters.html)
and [PDF export filters](https://help.libreoffice.org/latest/en-US/text/shared/guide/pdf_params.html)
provide the conversion interface.

## Verify

```sh
cargo test --locked
cargo test --locked -- --include-ignored --test-threads=1
cargo test --locked --test merge page_ -- --include-ignored
cargo test --locked --test merge real_office -- --ignored --test-threads=1
cargo test --locked --test merge real_office_legacy -- --ignored --test-threads=1
cargo test --locked real_libreoffice_fallback -- --ignored --test-threads=1
cargo test --locked --test merge rendered_pages -- --ignored
cargo test --locked --test compress -- --include-ignored
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

The first suite needs only Rust. It covers partial Office installations, fallback
for every supported Office extension, OpenDocument filter selection, missing
converters, timeout failures, and output preservation. The opt-in Office tests
require Microsoft Office on macOS (LibreOffice elsewhere). The macOS
`real_libreoffice_fallback` test simulates missing Microsoft apps and converts
modern and legacy Word/Excel/PowerPoint fixtures with real LibreOffice.
The rendering test needs Poppler
(`pdftoppm`). Office tests check extracted page text and order for modern and
legacy mixed merges, basename collisions, unchanged sources, and visible Excel
sheets in both XLSX and XLS, plus Excel pagination and blank-page regression
coverage. Legacy fixtures were saved by the installed Microsoft
apps; their generation script and hashes are in `tests/fixtures/`. The
Poppler test compares source and merged page pixels byte for byte. Synthetic
Office fixtures contain only generated test content. Page-edit tests cover
single pages, ascending/descending ranges, validation errors, output protection,
internal/external links and unchanged sources. A second Poppler test compares
removed/reordered pages to their sources, including inherited and overridden
page geometry, crop boxes, fonts and rotation.
Single-cell Excel print areas also have a bounded regression test to catch
unexpected confirmation dialogs during unattended conversion.
The [page-edit validation record](docs/evidence/page-edit-validation.json)
records two consecutive unattended runs with all 25 tests passing.
The [print-preview validation record](docs/evidence/excel-print-preview-validation.json)
records the earlier saved-layout default, now available through
`--excel-preserve-layout`, including four pages within one worksheet.
The [sheet-export validation record](docs/evidence/excel-sheet-export-validation.json)
covers mixed A4/Letter/Folio paper sizes, multi-page sheets, blank-page removal,
image-only pages and the 29-test suite at that stage.
The [table-section validation record](docs/evidence/excel-table-sections-validation.json)
adds a long, side-by-side table regression: each row's columns stay on the same
page and both tables continue vertically (31 tests at that stage).
The [automatic-table validation record](docs/evidence/excel-auto-tables-validation.json)
covers the current default, including independent table detection, protected
shared headings, full-width merged content, source preservation and vertical flow.

Compression tests check substantial size reduction on uncompressed text/image
fixtures, unchanged decoded content and catalog objects, retained unsigned form
values, input/output protection, rejection of encryption and signatures, and an
exact-copy fallback when optimization cannot reduce size. The optional Poppler
test compares every source and compressed page pixel for pixel, including
images and inherited page geometry.
