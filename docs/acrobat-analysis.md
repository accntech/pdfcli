# Acrobat merge and Office conversion: reverse engineering report

Investigated 2026-10-12/13 using the requested REA skill. **Ghidra now works,
and 35 selected native functions were analyzed successfully**: 21 in Acrobat's
core framework and 14 in its bundled AdobePDFL library. Two controlled
page-insertion experiments also ran in the installed Acrobat. The earlier
provider-unavailable paragraph has been superseded by actual native analysis.

The established architecture is **Office application → temporary PDF →
validated page selection → PDF object-tree insertion and reference repair**.
The code explicitly selects installed Word, Excel and PowerPoint; it does not
use LibreOffice in the local native paths examined.

## Identity, scope and evidence

Target: `/Applications/Adobe Acrobat DC/Adobe Acrobat.app`, bundle
`com.adobe.Acrobat.Pro`, build `26.002.22008`, ARM64. Its installed files were
not changed. Universal binaries were copied into thin ARM64 analysis artifacts
with Apple's `lipo`; [original and derived SHA-256 values](evidence/ghidra/artifact-derivation.json)
retain their provenance.

This investigation covers PDF/Office combining, as requested. It is not an
exhaustive reverse engineering of Acrobat's unrelated features.

| Evidence collection | Coverage |
| --- | --- |
| [Core native analysis](acrobat-core-deep-analysis.md) | 21 function dossiers: registration, Combine command dispatch, document assembly, converter callbacks/factory/worker, Office-specific converters, PDF insertion wrapper. |
| [PDF merge native analysis](acrobat-pdf-merge-deep-analysis.md) | 14 function dossiers: page insertion, merge-state allocation, inherited attributes, reference fixups, outlines, destinations and tagged structure. |
| [Runtime observations](evidence/runtime/observations.json) | Synthetic PDF fixtures, actual Acrobat `insert pages` calls with bookmark copying enabled/disabled, saved outputs and hashes. |
| [Office scripting dictionaries](evidence/microsoft-office-scripting.json) | Installed Word/Excel/PowerPoint event names, parameter codes and PDF enums, with file hashes. |
| [Provider repair](rea-provider-setup.md) | Verified Ghidra release, native ARM64 decompiler build, Java and durable REA configuration, backup and successful readiness check. |

Full [core](evidence/ghidra/core-evidence-bundle.json) and
[PDFL](evidence/ghidra/pdfl-evidence-bundle.json) REA bundles retain recovered
pseudocode, instructions, references, provider profiles, Evidence IDs and
limitations. Both native sessions were exported and closed. Pseudocode is
recovered analysis, not original C++ source. The [first-pass report](acrobat-initial-static-analysis.md)
is retained as historical context.

## Native path from document assembly through Office

The assembly routine at `0x152d0d0` reads each input's conversion handler,
settings, PDF status and page ranges. It writes `PageRangeText`,
`AlwaysAddBookmarks` and `UseAssemblyView` into per-input settings. Its input
loader ultimately calls the handler record's function pointer at offset
`+0x148`.

The registration routine at `0x1b589a4` checks installed Microsoft bundle paths
and populates exactly that record slot with Word/PowerPoint importer
`0x1b58c68` or Excel importer `0x1b5956c`. Modern Excel delegates to the generic
importer. The importer calls `0x1b5ecbc`, whose factory selects a concrete
converter class. A scheduled worker calls vtable slot `+0x28`, resolving to the
three native exporters below. This connection is supported by record layout,
call instructions, vtable contents and recovered data flow.

| Application | Converter entry | Modern export event established by native code and installed dictionary |
| --- | --- | --- |
| Word | `0x1b5524c` | `sWRD/1420`: **save as**, filename key `5015`, format key `5238`, enum `0x02310011` = **format PDF**. Modern output paths use POSIX syntax; the source document is found by its path. |
| PowerPoint | `0x1b56648` | `core/save`: **save**, `kfil` and `fltp`, enum `0x00cc000e` = **save as PDF**. A preference selects POSIX/HFS path style. |
| Excel | `0x1b56d44` | `smXL/xSwA`: **save workbook as**, filename key `5016`, format key `1813`, enum `0x02bc0039` = **PDF file format**. Other branches inspect sheet visibility, selection and print area. |

The converted file is opened as a PDF. When page copying is required, the
importer validates ranges and calls the core insertion wrapper at `0x352406c`,
using append sentinel `-2`, source start and inclusive count; an all-pages path
uses count `-3`. Other settings let it return the opened PDF directly.

Key Evidence IDs:

- Registration: `ev_427bb7a17be8db1e78591a590a427c254280d75cb9cdee443ceb26b36e86d251`.
- Assembly: `ev_bc5b1b9542aebc27479ae7a46cf4daa40f17d0c9ac17262f3d1cac355832a5e0`.
- Importer: `ev_45a7e8493ee231b17e0ddeedcca816ea6de95538e2277a46765f9ad9457b969f`.
- Word: `ev_3142117c17c51ec4245766a7b84f57ce59255082b892ccbd28e4e3243fdbb71d`.
- Excel: `ev_f4467abf2795aaca0655bb42752e2c14d8f7cac20956205e2ed8d0e76624533c`.
- PowerPoint: `ev_88f2fbe0caa9559656271300aee51aad827dc4bc4935e3f99ccff5d49b8e581d`.

The Word macro/template branch is conditioned on older Office versions and
additional flags; it is not evidence that modern Word conversion requires
running Adobe's `linkCreation.dotm`. Selection-oriented paths also use
pasteboard/PDFKit operations. This is a family of version/settings-dependent
converters, not one universal save operation.

## How the PDF merge works

The named PDFL `PDDocInsertPages` first checks permissions, then enters the
internal insertion implementation. Core `0x352406c` is identified as its
counterpart by a unique code-prefix match and matching control flow; that name
is derived for the stripped core function.

The insertion body validates the range, allocates merge mappings, and copies
page object trees with `_CosObjCopyTree`. It transfers inherited geometry and
resources, inserts pages in reverse traversal at a fixed location to preserve
order, then resolves deferred references through original-to-copy maps. It
also merges optional-content information, handles bookmark/article/name flags,
reconciles tagged structures, invokes font merging and repairs page labels.
It does not flatten ordinary PDF pages into screenshots in this routine.

Evidence: `ev_d7061548e739c8c4c1521ba7180da47c0da95172284bc38b82995c5d62ab2914`.
The [detailed PDFL report](acrobat-pdf-merge-deep-analysis.md) links the additional
helper dossiers and distinguishes established steps from unresolved policies.
Adobe's [public insertion API](https://opensource.adobe.com/dc-acrobat-sdk-docs/acrobatsdk/apireference/PD_Layer/PDDoc.html#PDDocInsertPages())
corroborates the parameter/flag vocabulary; it is not substituted for the native
evidence.

## What actual Acrobat output confirmed

Two generated PDFs had distinct text, inherited page geometry and rotation,
bookmarks, and colliding named destinations. Acrobat inserted the two source
pages after the base page and saved both bookmark-on and bookmark-off results.

Observed output preserved text order, media/crop geometry and rotation.
Bookmark-on copied the source bookmark under a filename parent; bookmark-off
did not. This AppleScript insertion mode retained the base named destination
and left the inserted named link referring to it. The native full-document
branch separately contains name-collision repair. The experiment therefore
supports **mode-dependent behavior**, not blanket link-preservation claims.

This test used the shipped `CARO/inpg` API, not the Combine button. It proves
those output properties, not that a particular GUI Combine run selected the
same flags. Only synthetic files were used; the scripts and outputs remain
reviewable. No network activity or live debugger call trace was captured.

## Remaining boundaries

| Question | Result |
| --- | --- |
| Is a deep native REA provider available and actually used? | **Answered:** Ghidra 12.1.4 served all 35 selected dossiers. IDA/Hopper were not needed. |
| Are installed Microsoft apps used in the inspected local path? | **Answered:** explicit installed-app checks, converter classes and AppleEvents. |
| Does assembly connect to Office conversion and PDF insertion? | **Answered statically:** handler-record dispatch, factory/vtable call and importer insertion path. |
| Does the entire Combine-button virtual dispatch resolve to assembly? | **Partial:** command `0xcdb` and handler prefix recovered; intervening virtual/function-table connection remains unresolved. |
| What happens inside PDF insertion? | **Answered at algorithm level:** object copying, inherited attributes, mappings, fixups and feature-specific branches. Exact font deduplication, forms and all conflict/error policies remain untraced. |
| Which Office/service branch runs for each real document? | **Unresolved for Acrobat runtime:** version/settings checks recovered; local Office output fidelity and service selection were not traced live. |
| Is the Rust CLI feature-equivalent to Acrobat? | **No such claim:** it uses independent PDF code and documents its narrower feature set. |

Ghidra completed default analysis but reported some analyzer warnings; six core
dossiers also have bounded native value-flow collections. The complete
pseudocode/instruction facets and exact caveats are retained. No missing
indirect edge or truncated value-flow facet is treated as proven.

## Reconstruction status

The Rust PDF merger uses `lopdf`, independently of Adobe. The macOS conversion
backend has been changed to the installed Microsoft apps using the observed
modern PDF export commands. It stages private copies, serializes Office access
and closes only its staged documents. Word and Excel expose macro automation
settings, which are disabled during open and then restored. PowerPoint 16.113.4
returns `missing value` for this advertised property; the CLI preserves its
existing macro policy in that case. It does not call Adobe libraries or private services.

The initial native-Office validation passed **all 18 tests**, including real Word/Excel/PowerPoint conversion, a
five-input mixed merge with duplicate Word basenames, both visible Excel sheets
with the hidden sheet excluded, and byte-for-byte Poppler page rendering checks.
The suite includes legacy DOC/XLS/PPT and Excel pagination coverage and
completed in 55.55 seconds after each app's initial folder-access grant.
Clippy with `-D warnings` and formatting also pass.

With `--excel-preserve-layout`, the CLI preserves Excel's saved print-preview
layout, including print areas, scale and manual page breaks. It exports visible worksheets
separately from the private workbook and then combines their PDFs, preserving
each sheet's paper dimensions. Other worksheets are temporarily hidden, without
deleting sheets or breaking cross-sheet formulas; the copy is not saved.
The [print-preview regression](evidence/excel-print-preview-validation.json)
checks four pages within one worksheet and a fifth page from another sheet.
The manual-section implementation passed **31 tests**, including page editing
and both Excel print-layout modes. Investigation of the remaining native workbook-export bugs
found that the combined export omitted a visible Folio worksheet and reused
an A4 media box for Letter content, moving content about 50 points down the
physical page. Separate exports include the missing worksheet and retain the
correct media boxes. The measured top positions then match the Print dialog
within 0.001 points. Pages proven to contain no PDF drawing operations are
removed; image-only and vector pages survive. These repairs are independent
CLI behavior, not claims about Acrobat's policy. The
[sheet-export validation](evidence/excel-sheet-export-validation.json) records
synthetic coverage; private document evidence remains outside shared reports.

The default automatic width preparation is independent CLI behavior,
not a claim about Acrobat's exact policy. On private copies it limits each visible worksheet's
print area to content, resets manual page breaks and fits columns to one page
wide, with room for vertical continuation. Empty worksheets are skipped;
hidden sheets remain hidden. The
[pagination regression](evidence/excel-pagination-validation.json) produced
17 pages before the fix and two afterward, including both visible sheets.
Excel's AppleScript setter rejects both `false` and `0` for automatic fit
height, so the CLI uses a large height limit. This was verified against the
installed Excel app; no workbook changes are saved.

For side-by-side tables, the CLI also accepts repeated
`--excel-print-area SHEET=RANGE` overrides.
Each selected rectangle is exported independently with one-page width and
vertical continuation. This avoids shrinking two independent tables onto the
same page. The [table-section regression](evidence/excel-table-sections-validation.json)
uses two 90-row tables and checks that every label/value pair stays on the same
page while both tables continue across multiple pages. Those manual rectangles are explicit layout configuration.

The current default detects independent column bands using native Excel
occupancy, merged cells, structured tables and graphic bounds, then fits each
section to one page wide with vertical continuation. Blank gutters and
staggered headings provide separation evidence; spanning content and shared
headings prevent cuts. Ambiguous layouts remain whole. The inspection reads a
private copy, emits only numeric coordinates, closes without saving, and shares
the conversion timeout with the export pass. More than 200,000 used-range cells
triggers a complete-width fallback for that sheet, rather than partial detection.
`--excel-preserve-layout` opts out; `--excel-fit-width` remains a compatible alias
for the default. The [automatic-table validation](evidence/excel-auto-tables-validation.json)
records native regression checks. This policy is an independent CLI feature,
not a claim inferred from Acrobat binaries.

Runtime verification exposed two implementation details: Excel's `full name`
is a POSIX path, and passing a file reference to `open workbook` avoids the
per-file permission prompt caused by passing a text path. Word and PowerPoint
also use file references for opening. These are observed properties of the
CLI's native Office calls, separate from the Acrobat static evidence.

The [CLI validation record](evidence/microsoft-office-validation.json) records
passed checks and the runtime limitations. Document counts and macro settings
were unchanged after the suite; the staging folder was empty. The interrupted
Word fixture was closed and its recovery files removed after the folder grant.

A separate [ownership check](evidence/runtime/verify-native-office.py) left a
synthetic Word source open while the CLI merged it. The source stayed open and
its bytes were unchanged. The [runtime record](evidence/runtime/cli-native-office.json)
and [five-page result](evidence/runtime/cli-native-office.pdf) are retained.
The subsequent [legacy validation](evidence/legacy-office-validation.json)
uses genuine DOC/XLS/PPT files saved by the installed Microsoft apps. Individual
conversions, hidden-sheet exclusion for XLS, unchanged source bytes, and a
seven-input legacy/modern merge all pass. The
[seven-page output](evidence/runtime/cli-legacy-office.pdf) also passed an
independent `pypdf` text-order check. Office document counts and macro settings
were unchanged, and the staging folder was empty afterward. No production fix
was needed; a test assertion was corrected to account for `lopdf`'s text
extractor missing an inherited inline font dictionary in the synthetic PDF.
These small fixtures establish basic legacy compatibility, not coverage of
all legacy Office features. RTF remains untested.
