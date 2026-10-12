> Historical first pass, superseded by [the deep investigation](acrobat-analysis.md). Provider availability and implementation statements below describe that earlier pass.

# Acrobat PDF and Office merge investigation

Analyzed on 2026-10-12, using the requested REA skill. The practical architecture
is **convert Office inputs to PDF, then assemble PDF pages in the requested
order**. The local binary contains separate Word, Excel, and PowerPoint
conversion implementations which communicate with Office applications through
macOS scripting/AppleEvents. This report distinguishes those static observations
from the independent design used by `pdfcli`.

## Scope and identity

Only `/Applications/Adobe Acrobat DC/Adobe Acrobat.app` was inspected. Acrobat
and Office were not launched or attached to, no documents were submitted to
Adobe services, and the application bundle was not modified. LLDB was used only
to create an offline target and disassemble file-backed instructions.

| Artifact | Identity |
| --- | --- |
| App | `com.adobe.Acrobat.Pro`, version `26.002.22008` |
| `Contents/MacOS/AdobeAcrobat` | SHA-256 `fe75b41b753c79be27931429987b64b704e7de76eee9157ed4b641ae516b4986` |
| `Contents/Frameworks/Acrobat.framework/Versions/A/Acrobat` | SHA-256 `ddbe975f487125b09dc8038fe91a6d1f106b070ba62069f8cc6ec2928fe27bd1` |
| Core architecture examined | ARM64 slice, original universal-file offset `83263488`; native addresses below are unslid ARM64 addresses |

The core also contains an x86-64 slice. Findings below are scoped to the ARM64
code and the identified build.

## Evidence ledger

The complete three-record REA export is [acrobat-rea.json](evidence/acrobat-rea.json).

| Evidence | Observation and boundary |
| --- | --- |
| `ev_92110980e24b7f6eb47d1f373f6f5774ea8a29ef10174e699a0e44ce3cf15318` | REA parsed the app Info.plist, establishing bundle identity/version. |
| `ev_362d4f9e2788a98059532aadd85c875be5ea3f014b0fdf72bf8009fa24147224` | REA Mach-O inspection of the main launcher. No merge implementation was established from this small executable. |
| `ev_e35b54713cf60b512ee31680375e308bee0fc09198e84f8255e9c0e1c323c5b9` | REA Mach-O inspection of the core framework: ScriptingBridge and PDFKit dependencies, and AppleEvent imports including `AECreateAppleEvent`, `AESend`, and `AESendMessage`. Dependencies/imports alone do not establish call paths. |
| [Static observations](evidence/acrobat-static-observations.json) | Supplementary Python XML parsing, original file hashes, exact string offsets, and C++ RTTI/vtable associations. These are **not REA Evidence IDs**. |
| [Native assembly](evidence/office-converters-arm64.txt), [annotated assembly](evidence/office-converters-annotated-arm64.txt) | Offline LLDB instructions for the bounded Office converter region and cleanup routines. Selector annotations decode the observed Objective-C stubs and chained selector references. These are **not source code or runtime traces**. |

REA reported all deep native providers unavailable: Ghidra was not configured,
the Hopper launcher was absent, and IDA was not configured. No engine was
installed. Existing Apple LLDB/LLVM tools supplied the bounded supplementary
disassembly. REA sessions were closed after evidence export.

## What the shipped application establishes

### Combine interface and page-level composition

The parsed `Resources/AcroApp/ENU/Combine.aapp` declares `CombineApp`, registers
`NewDocCombineFiles`, and binds controls for adding files, moving files up/down,
and `MultiFileViewPerformCombine`. This establishes the packaged command and UI
contract; XML does not prove the implementation behind those handlers.

The parsed `Resources/Acrobat.sdef` exposes the `insert pages` command (`CAROinpg`):
a destination document, insertion location, source document, first source page,
number of pages, and optional bookmark copying. This is concrete shipped API
metadata for page-level document composition.

Adobe's current [Combine files documentation](https://helpx.adobe.com/acrobat/desktop/edit-documents/combine-files/combine-files.html)
describes selecting mixed file formats, arranging individual pages, and saving
one sequential PDF. Its [Acrobat SDK PDF creation guide](https://opensource.adobe.com/dc-acrobat-sdk-docs/library/jsdevguide/JS_Dev_PDFCreation.html)
documents combining PDFs through repeated `Doc.insertPages` calls followed by
saving. These corroborate the architecture but do not prove that this build's
Combine button uses that exact public JavaScript API.

### Office conversion has application-specific native paths

C++ RTTI names and their vtable slots identify three converter implementations:

| Derived class association | Sixth virtual function pointer | Distinct native behavior observed |
| --- | --- | --- |
| `CMacWordToPDFConverter` | `0x1b5524c` | Obtains an application by PID, handles text ranges/selections, creates output path descriptors, sends Word events, and includes a page-copy/write path. |
| `CMacPPntToPDFConverter` | `0x1b56648` | Obtains an application by PID, queries slide count, constructs a file save event, and includes a per-slide copy/write path. |
| `CMacExcelToPDFConverter` | `0x1b56d44` | Obtains an application by PID, examines sheets, names, visibility, used ranges, charts and print area, and sends Excel events. |

The RTTI-to-function association is derived from the Itanium C++ ABI layout;
original C++ method names were stripped and remain unknown. This is substantially
stronger than inferring conversion from class-name strings alone: the function
pointers lead to the captured call sequences below.

- **Word:** `0x1b552e0` calls the selector
  `applicationWithProcessIdentifier:`. At `0x1b55638`, the routine invokes
  `sendEvent:id:parameters:` with event class `sWRD` (`0x73575244`) and ID
  `1420` (`0x31343230`), carrying a path descriptor and an enum descriptor.
  An earlier conditional branch checks `templates`/`protectionType`, references
  `linkCreation.dotm`, writes an output-path text file, and sends a separate
  event with the macro name `AcrobatLinkCreation.link.startPrintFromAcrobat`.
  It is not established which branch runs for a given installed Office version.
- **PowerPoint:** `0x1b56708` queries `slides`; `0x1b56848` invokes
  `sendEvent:id:parameters:` with event class `core` (`0x636f7265`), ID
  `save` (`0x73617665`), and parameter keys `kfil` and `fltp`, populated by a
  filesystem path and format descriptor. `0x1b56870` calls
  `closeSaving:savingIn:` with the `no  ` enum. The exact meaning of the
  format enum `0x00cc000e` was not independently resolved against PowerPoint's
  scripting dictionary.
- **Excel:** `0x1b56e88` queries `sheets`; the following loop examines names and
  visibility. `0x1b5720c` invokes `sendEvent:id:parameters:` with class `smXL`
  (`0x736d584c`) and ID `xSwA` (`0x78537741`), plus path/format-related arguments.
  Alternate branches call `usedRange`, `copyPictureAppearance:format:`,
  `copyChartAsPictureAppearance:format:outputSize:`, and inspect `printArea`.
  The code explicitly has multiple paths; a claim that every Office input
  follows a single identical conversion mechanism would be unsupported.

The shared code at `0x1b56048` reads `generalPasteboard`/`dataForType:`, attempts
`initWithData:`, and has a fallback using `initWithImage:` and
`insertPage:atIndex:`. In the Word conversion routine, `0x1b55980` through
`0x1b55a64` includes page retrieval, copying, bounds changes, and page insertion;
`0x1b55ad0` calls `writeToFile:`. These establish local native paths which create
and manipulate a paged output, alongside the Office save-event path. They do
not establish which path is active for an ordinary Combine operation.

### Service conversion is a separate unresolved option

The core contains the preference key
`bServiceConversionIsEnabledForOfficeOnMac` and related service-conversion UI
selectors. Presence is observed; enabled state, remote endpoints, payloads,
fallback ordering, and whether a user's documents would use that path were not
observed. The reconstruction does not depend on any private Adobe service.

## Reconstruction decisions

The implemented Rust CLI preserves input order, normalizes supported Office
files into temporary PDFs with LibreOffice, validates them, then uses `lopdf`
to graft each input's page subtree under a new output page tree. Object IDs are
remapped to avoid collisions. Both libraries/backends are independent choices;
no Adobe implementation code or private service is reused.

Existing PDF pages remain structured PDF content rather than being rendered
to screenshots. The CLI rejects AcroForm/signature, encrypted, and layered
documents, and warns when document-level metadata is omitted. Conversion
fidelity depends on LibreOffice, installed fonts, document layout and print
settings. Matching Acrobat's handling of forms, bookmarks, tags, attachments
or signatures was not established by this investigation.

## Checklist and remaining unknowns

| Investigation question | Status |
| --- | --- |
| Which installed app/build was examined? | Answered by REA metadata and hashes. |
| What is the mixed-file merge contract? | Answered by shipped UI/API metadata and Adobe documentation. |
| Does native Office-specific conversion code exist, and what does it call? | Answered for bounded ARM64 Word/PowerPoint/Excel routines using RTTI plus direct disassembly. |
| Is normalization followed by ordered PDF assembly a supported reconstruction? | Supported inference from conversion code, page API, and documented behavior. |
| Exact Combine handler → converter → final native PDF merge call graph? | Partially answered; no complete end-to-end native trace. |
| Exact PDF object remapping, font deduplication, bookmark/form/tag merge rules? | Unresolved. |
| Active Office/service branch and observed output fidelity for sample inputs? | Unresolved; no Acrobat runtime experiment was performed. |

## Reproducing the bounded native evidence

The assembly was captured with the installed `xcrun lldb`, using only
`target create --arch arm64`, `disassemble --start-address ... --end-address ...`,
and `quit`. The captured ranges are `[0x1b5524c, 0x1b583c0)` and
`[0x1b5a4d8, 0x1b5ab50)`. The metadata inspection used
`xcrun llvm-objdump --macho --arch=arm64 --objc-meta-data` and `lipo -detailed_info`.

[annotate-office-stubs.py](evidence/annotate-office-stubs.py) regenerates selector
annotations from the original installed binary and verifies its SHA-256 first.
It is intentionally specific to this build. REA's normalized Mach-O record
retains its own tool command provenance and limitations. All captured evidence
is static; no timing, network activity, live process state, or successful
conversion is implied.
