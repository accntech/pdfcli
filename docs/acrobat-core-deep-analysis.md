# Acrobat core: native Office conversion and document assembly

REA/Ghidra 12.1.4 successfully recovered **21 selected core function dossiers**, including complete returned pseudocode, for the installed Acrobat 26.002.22008 ARM64 image. Twenty dossiers fit the MCP response budget; the large assembly dossier was retained in full and recovered through an evidence view and export. The bundle contains 22 records (21 dossiers plus the view), 38,319,706 bytes. The session was closed successfully.

This complements [the PDFL analysis](acrobat-pdf-merge-deep-analysis.md). Pseudocode is a reconstruction, not original source. This core investigation did not launch or attach to Acrobat; separate synthetic runtime tests are documented in the PDFL report.

## Artifact and address convention

- Original: `/Applications/Adobe Acrobat DC/Adobe Acrobat.app/Contents/Frameworks/Acrobat.framework/Versions/A/Acrobat`.
- Original universal SHA-256: `ddbe975f487125b09dc8038fe91a6d1f106b070ba62069f8cc6ec2928fe27bd1`.
- Analyzed ARM64 slice SHA-256: `863f10faf69df6bb26432906dbaa6a0df59110e53571c7c78493f7b4190d3c22`.
- All addresses below are unslid ARM64 virtual addresses. For the cited code/data, they equal offsets within the thin image; add 83,263,488 for original universal-file offsets. See `evidence/ghidra/artifact-derivation.json`.

## Office conversion is an explicit native subsystem

The registration function at `0x1b589a4` checks `NSWorkspace.sharedWorkspace`, `URLForApplicationWithBundleIdentifier:`, and `path` for `com.microsoft.Word`, `com.microsoft.Powerpoint`, and `com.microsoft.Excel`. A null path skips the respective registration. The records identify `com.adobe.acrobat.office.word`, `.ppt`, and `.excel`. Word and PowerPoint use import callback `0x1b58c68`; Excel registers `0x1b5956c`, which delegates to `0x1b58c68` for Office versions >=15. The older Excel branch reads a Sheets/Paths manifest and combines its generated PDFs.

The generic import callback calls conversion bridge `0x1b5ecbc`. That bridge uses factory `0x1b53538`, which selects three distinct C++ vtables, previously linked to their shipped RTTI names. It submits work through `0x1b53ba4`, scheduling `0x1b5aca4`; the latter enters worker `0x1b5adb8`. At `0x1b5ae4c` the worker invokes vtable slot `+0x28`, connecting the factory to the converter functions directly.

| Factory kind | RTTI class | Vtable | Slot +0x28 |
| --- | --- | --- | --- |
| 0 | `CMacWordToPDFConverter` | `0x4a47538` | `0x1b5524c` |
| 1 | `CMacExcelToPDFConverter` | `0x4a475f8` | `0x1b56d44` |
| 2 | `CMacPPntToPDFConverter` | `0x4a475a0` | `0x1b56648` |

The scheduled callback is registered alongside the shipped source-location string `AVMacOfficeConversion.cpp` and procedure label `sProcessRequestProc`. The worker checks application state and conversion return status, rescheduling pending work through the same callback. Exact scheduling-time units have not been recovered.

```mermaid
flowchart LR
    A[Installed Office app check] --> B[Register Office import callback]
    B --> C[Conversion bridge and class factory]
    C --> D[Scheduled native worker]
    D --> E[Word / Excel / PowerPoint converter]
    E --> F[Temporary PDF path]
    F --> G[Open PDF and apply page selection]
    G --> H[Insert PDF pages]
```

### Word

The converter gets `SBApplication` using `applicationWithProcessIdentifier:`. Its helper `0x1b55cec` finds or opens the requested document; lookup `0x1b5e038` enumerates `documents` and compares the source path with `posixFullName` for version >=15, or `fullName` for older versions. Thus the observed save receiver is the document matched to the source path.

At `0x1b5543c`, `CFURLCopyFileSystemPath` receives style 0 (POSIX) for version >=15 and style 1 (HFS) for older versions. The output is wrapped with `descriptorWithString:` and sent to the matched document with event class `sWRD`, ID `1420`, parameter `5015` for the filename and `5238` for the format. At `0x1b55448` the format code is constructed as `0x02310010`, changed to `0x02310013` for versions <13 and `0x02310011` for versions >=16. These are literal binary values; translating them to installed Office terminology requires its dictionary, and successfully compiling an AppleScript expression is a separate question.

For versions <15, additional flag, template and protection checks can select a macro branch involving `linkCreation.dotm`, `/tmp/AcrOutputFilePath.txt`, and `AcrobatLinkCreation.link.startPrintFromAcrobat`, plus a page-copy/write branch using PDFKit. The analysis does not establish that the macro branch is active on this machine.

### PowerPoint

At `0x1b56848` the converter sends `core/save`, with `kfil` holding the output filename and `fltp` an enum descriptor containing `0x00cc000e`. Its `UsePosixPathForPPT` preference defaults to 1 and controls POSIX versus HFS output-path conversion. It queries slides, has a whole-presentation save path and a per-slide copying path, and closes with a no-save option in the inspected path.

### Excel

At `0x1b5720c` the converter sends `smXL/xSwA`, with observed keys `5016`, `1813`, and `5321`; the last descriptor is boolean true. For modern versions >=16, the output-format descriptor is `0x02bc0039`; an older branch uses `0x02bc002e`. Version 15 has an additional `core/save` step. Other branches enumerate sheets and inspect names, visibility, used ranges and print area, and use `copyPicture` or `copyChartAsPicture`. These branches make workbook conversion sensitive to selection and print settings. Full spreadsheet layout equivalence is not established by static analysis.

## Converted PDFs enter the page-insertion API

The assembly procedure `0x152d0d0` reads `DocAssembly:ConversionHandler`, `Settings`, `IsPDF`, `StartPages`, `EndPages` and `PageRanges`. It writes `PageRangeText`, `AlwaysAddBookmarks`, and `UseAssemblyView` into the per-item settings. Its input loader `0x1527108` dispatches through `0xf445cc` -> `0xf44204` -> `0xf446a0`. The last function checks the handler record size and calls **record offset `+0x148`**, passing settings, source path/filesystem, output-PDDoc pointer and progress context. This is exactly the callback slot populated by the Office registration code. The assembly-to-Office-import connection is therefore supported by matching field layout and data flow, rather than by function-name similarity.

The generic Office import callback obtains the conversion result through `0x1b5ecbc`, then opens it through `0x34ddfb8`. It reads `UseAssemblyView`, `AlwaysAddBookmarks`, and `PageRangeText`. `AlwaysAddBookmarks` adds mask `0x1` to an insertion flag word whose base value is `0x2`.

When copying is required, it creates a destination via `0x34f3920`. At `0x1b5917c` it calls the core page-insertion wrapper `0x352406c`, iterating validated source ranges and passing the append sentinel -2, first source page, and inclusive range length. The alternative at `0x1b591c4` uses append -2, first page 0, and count -3. The named PDFL implementation independently explains the page sentinels. A zero-page output is closed and rejected in this callback. In another branch the callback returns the opened source PDDoc directly rather than copying it.

The identity of core `0x352406c` as `PDDocInsertPages` is **derived**: its first 68 bytes uniquely match the named ARM64 PDFL function at `0x1a021c`, and its permission-check control flow agrees. The core adds an initial guard. It invalidates document state, performs permission checks and transfers to `0x35241e4`, corresponding to the no-permission-wrapper internal implementation. These are inferred names for stripped core routines, not recovered original symbols.

## Combine dispatch and assembly boundary

The command-registration code at `0x2bc496c` references `MultiFileViewPerformCombine` at `0x2bc5b8c` and associates command identifier `0xcdb`. A vtable method linked to RTTI `CombineAppCommandHandler`, `0x1582fa4`, compares the requested command with `0xcdb` at `0x15830f4` and calls `0x1578808` at `0x1583140`. That function tailcalls `0x2870944`, which reaches virtual methods through `0xf31968` (slot +0x38) and `0xb28074` (slot +0x110).

Separately, the large assembly procedure `0x152d0d0` references `AVDocAssembly.cpp` and `AVCollabCombine_PerformCombine`. It directly calls the same core page-insertion wrapper at `0x152f15c` and `0x152f338`, passing append -2 and the selected source range, with flags dependent on bookmark mode (`0x2`, `0x3`, or `0x1002`); helper `0x15358f8`, called at `0x152eed0`, also inserts pages. Assembly is stored in a callback table at `0x4db1b58`, within the record at `0x4db1b08` registered by `0x3fc00`.

The full virtual/function-table connection between that UI command prefix and the assembly procedure remains unresolved. The report does not join the two with an invented direct edge. The Office-import-to-page-insertion path above is independently established.

## Implementation implications

The supported architectural reconstruction is: convert non-PDF inputs with a format-aware producer, validate/open the resulting PDF, select pages, copy document objects and page relationships, and serialize the assembled document. Adobe's page merger also reconciles document-level structures; the separate PDFL analysis describes that work. Merely concatenating PDF file bytes cannot reproduce the observed process.

An independent Rust implementation can use the installed Microsoft apps for conversion and its own PDF library for assembly. That is an implementation choice informed by this investigation, not a claim to reuse Adobe code or reproduce every Acrobat feature. Runtime testing of each chosen Office command and resulting file is still necessary.

## Evidence and limitations

The portable [core evidence bundle](evidence/ghidra/core-evidence-bundle.json) retains the complete dossiers and their original limitations. The assembly transport error in `core-assembly.json` means only that the response exceeded 10 MiB; it does **not** mean the analysis failed or was truncated. `core-assembly-pseudocode.json` reports all 107,897 UTF-16 code units recovered and `exhausted: true`.

Six dossiers have truncated **native value-flow** collections: assembly, its input loader, Combine handler, Excel converter, Excel import, and Office registration. Their omitted value-flow operations remain unknown. The pseudocode and observed assembly/calls cited here are separate retained facets; no claim relies on an omitted value-flow edge. Ghidra's recovered types and Objective-C vararg prototypes are imperfect, so AppleEvent parameter details were checked against bounded ARM64 instructions and selector stubs.

Initial auto-analysis reported invalid PNG data at `0x4d9f2ce`, several VarnodeContext out-of-address-space warnings and an internal decompiler-worker `ArrayIndexOutOfBoundsException`, then completed default analysis and served every selected function. This caveat precludes claiming error-free or exhaustive whole-core recovery. The temporary import log was deleted by normal session close; the diagnostic note records the observed messages and their boundary. No native-provider-unavailable limitation remains for these selected functions.

The full UI virtual-dispatch path remains unresolved. Office version numbers and policy settings select branches; static presence does not establish the branch taken on a particular document. Layout fidelity, automation permission behavior, and modern Office command compatibility need runtime tests.

The following IDs identify `analyze_function` records in the bundle:

| Procedure / focus | Evidence ID |
| --- | --- |
| `0x1b5524c` | `ev_3142117c17c51ec4245766a7b84f57ce59255082b892ccbd28e4e3243fdbb71d` |
| `0xf44204` | `ev_3ba3f18fbff880c546ec93ec38b143f8d3b0bef9cd3383c7f95dcb2b32181481` |
| `0x1b589a4` | `ev_427bb7a17be8db1e78591a590a427c254280d75cb9cdee443ceb26b36e86d251` |
| `0x1b58c68` | `ev_45a7e8493ee231b17e0ddeedcca816ea6de95538e2277a46765f9ad9457b969f` |
| `0xf445cc` | `ev_555040237c38acc102d061e29d94e31133625046c75946a8974f548633d93e9c` |
| `0x1578808` | `ev_5f20965ffe261298998298aaa32233d0ca94553b0c597bf2130a1fc097b19e10` |
| `0x1b5956c` | `ev_6882aa24638c9d0ef5ec83a610448563fb874dbb927f74a9ab1626a9a162ded5` |
| `0x352406c` | `ev_7147d512063ace18d798c08f5ff70f6b97a3e257ff941860532bf78fb0f2e31e` |
| `0xf446a0` | `ev_883479def0c4350a713b05a27216af6aadc8872492992cdc25fb2b9b685d8672` |
| `0x1b56648` | `ev_88f2fbe0caa9559656271300aee51aad827dc4bc4935e3f99ccff5d49b8e581d` |
| `0x1b5adb8` | `ev_acbd11674f6af188c3e7fa5f829d68f24d63dd7396bc9cb677454b0879fc388f` |
| `0x1582fa4` | `ev_b724a8ea411280257cfeb2a78dca591e9d23906b42d6764a3e51607322c917ac` |
| `0x1b55cec` | `ev_bbf6388687d432f2f64fa5b511052fffa6b99f50a3b2bb94e06aa09a9c4c67f0` |
| `0x152d0d0` | `ev_bc5b1b9542aebc27479ae7a46cf4daa40f17d0c9ac17262f3d1cac355832a5e0` |
| `0x1b5e038` | `ev_c13b0f7bfa2b768315baf0cd13cdb93f6ebfcd5433f1065a2800acfd62585d2b` |
| `0x1b5aca4` | `ev_c1604f291a87bbe59a8b648292ddf550660a12bb913528fe72f5be54a400c979` |
| `0x1b53ba4` | `ev_c52b3e75f6a2ee182284fa693d74e9aff7ac737fb1f4f815b946302612fa6448` |
| `0x1527108` | `ev_d74875d5f194019c05e1d04e5565298419e52b7c0cff4730806d687716ff509f` |
| `0x1b5ecbc` | `ev_e944793b94c367b6d064ce3d34952d05e48b15f4f5a16243be6ae2d53384128d` |
| `0x1b56d44` | `ev_f4467abf2795aaca0655bb42752e2c14d8f7cac20956205e2ed8d0e76624533c` |
| `0x1b53538` | `ev_f69c27692f81ac191ac1610c9db3837073b8aab79f48b3c56b36793e82b9d005` |

The [static seed ledger](evidence/combine-and-office-seeds.json), [dispatch capture](evidence/office-native-dispatch-arm64.txt), [import capture](evidence/office-import-arm64.txt), [selector-annotated converters](evidence/office-converters-annotated-arm64.txt), and [Word document lookup](evidence/word-document-lookup-arm64.txt) are supplementary read-only shell observations, clearly separated from REA Evidence IDs.
