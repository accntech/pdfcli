# Native PDF merge implementation

This is a static Ghidra investigation of the **ARM64 AdobePDFL library shipped
inside Acrobat 26.002.22008**, with a separate controlled Acrobat runtime test.
It is not Adobe source code and is not a full trace of the Combine button.
See [provider repair](rea-provider-setup.md) and
[artifact derivation](evidence/ghidra/artifact-derivation.json) for provenance.

## Entry and permission boundary

At `0x1a021c`, the named `_PDDocInsertPages` invalidates cached document state,
checks destination permissions with `_PDDocCheckPermReq`, checks the separate
source document's crypt handler and permission request, raises an exception on
denial, then tail-calls `_PDDocInsertPagesNoPerms` at `0x1a0348`.
The internal name does **not** mean the ordinary entry bypasses permissions;
the wrapper has just checked them.

Evidence: [insert-pages dossier](evidence/ghidra/pdfl-insert-pages.json),
`ev_18ce835f955226b009f5cbb0e007c936ab6bb43711cae7f8a835f6aed3edf9e3`.

## Object copying and page assembly

The 5,096-byte analyzed body at `0x1a0348` establishes this sequence:

1. Normalize/validate the source page interval. Handle all-pages flag `0x1000`,
   count sentinel `-3`, append sentinel `-2`, and reject an invalid insertion
   index. Check temporary disk and memory availability.
2. Prepare font merging, create `PDMergeDataRec`, and issue before-insert
   notifications. The merge record includes object mappings, deferred reference
   hooks, structure maps, destination-name mappings, and whether the destination
   was initially empty.
3. Visit source pages **from the end of the selected range toward the start**.
   Copy each page with `_CosObjCopyTree` and `pdmCopyMonitor`; read its inherited
   attributes; insert the copied object after the fixed destination index with
   `_PageTreeInsertPageObj`; restore those attributes with `SetPageAttrs`.
   Reverse traversal plus insertion at the same position preserves source order.
   Same-document insertion has additional index adjustments.
4. Copy inherited resources when absent on the source page object and register
   the original-to-copied page mapping. Check the cancellation callback during
   the loop; the routine tracks how many pages were actually inserted.
5. Merge optional-content information; conditionally merge bookmarks, article
   threads, and named destinations; resolve and reconnect deferred references.
6. Raise the destination's required PDF version to accommodate source content,
   update page numbers, merge or strip tagged structure according to flags,
   merge fonts again for the inserted range, repair page labels, dispose merge
   state, and emit after-insert/page-change/thumbnail notifications.

Evidence: [full insertion dossier](evidence/ghidra/pdfl-insert-no-perms.json),
`ev_d7061548e739c8c4c1521ba7180da47c0da95172284bc38b82995c5d62ab2914`.
This is structured object copying. There is no page-render-to-image call in
this insertion body. That does not rule out rasterization in a preceding
Office conversion branch.

`GetPageAttrs`/`SetPageAttrs` explicitly handle an integer attribute and two
rectangles, restoring differences after page-tree insertion. Their meaning as
rotation/media/crop geometry is corroborated by the runtime test below.
Ghidra's recovered call signatures sometimes omit arguments, so its C-like
text should not be compiled or treated as exact source.

## Reference and name repair

`PDMergeDataCreate` at `0x1a1bd0` allocates a 0xa0-byte merge record and multiple
hash dictionaries, including a CosObj-to-CosObj map. `ResolveHookProc` at
`0x1a364c` fills unresolved targets by looking up original objects in that map.
`LinkHookProc` at `0x1a36d0` installs the resolved target into a dictionary or
array. These are direct evidence of a deferred fixup phase, not merely object
number renumbering.

When full-document flag `0x1000` is set, the main body handles both legacy
`Dests` dictionaries and name trees. `MergeDestsEnumProc` at `0x1a4738` and
`MergeNamesEnumProc` at `0x1a48b4` test for collisions, call
`GenerateUniqueString`, and record old-to-new names. The main body later walks
bookmarks and link annotations to fix renamed destinations/actions. The exact
unique-name suffix algorithm was not decompiled in this pass.

## Document-level structures

| Structure | Behavior established by native code | Boundary |
| --- | --- | --- |
| Bookmarks | Flag bit `0x0001`; recursively copies title, destinations/actions and other fields; rebuilds children. An empty destination can take the root; otherwise the insertion body can add a filename subtree. | Branch availability is static; actual GUI flags are not established by PDFL alone. |
| Article threads | Flag bit `0x0002`, copied through `MergeThreadsEnumProc`. | Callback internals not decompiled. |
| Named destinations | Full-document flag `0x1000`, collision mapping and later link/bookmark fixups. | Other insertion modes differ, as the runtime result shows. |
| Layers | Calls `_PDDocInsertOCInfoFromPDDoc` with an object-map callback. | Full optional-content conflict policy not decompiled. |
| Tagged PDF | Clones selected structure elements and ancestors; copies/remaps the parent tree; handles role/class maps and OBJR fixups. Separate bits select structure stripping. | PDF/UA conformance, every role-map conflict and failure path were not verified. |
| Fonts | Calls `_PDDocMergeExistingFonts` before/after insertion. | Exact font equivalence/subset deduplication rules remain unresolved. |
| Page labels | Calls `_PDDocFixPageLabelsForInsert`. | Exact label renumbering policy not decompiled. |
| Info dictionary | When destination was empty, copies selected fields and sets creation/modification dates. | Does not establish blanket copying of every catalog or metadata key. |
| Forms/signatures | Page-copy and notification infrastructure exists. | Forms plug-in handling and signature validity semantics were not traced; do not claim parity. |

Adobe's [public API reference](https://opensource.adobe.com/dc-acrobat-sdk-docs/acrobatsdk/apireference/PD_Layer/PDDoc.html#PDInsertFlags)
independently names flags `0x0001` as `PDInsertBookmarks`, `0x0002` as
`PDInsertThreads`, and `0x1000` as `PDInsertAll`, and specifies the public
insertion parameters. These names aid interpretation; native observations
above come from this installed build.

## Controlled runtime corroboration

Acrobat itself executed two `CARO/inpg` AppleScript insertions on generated
fixtures. This used the documented page-insertion entry, **not the Combine
button**. Scripts, PDFs, logs, input/output hashes, and parsed observations are
retained under [evidence/runtime](evidence/runtime/observations.json). These are
controlled runtime records, not REA Evidence IDs or a debugger trace.

Both trials inserted a two-page PDF after a one-page base. Parsed output proved:

- Text order: `BASE PAGE`, `SOURCE FIRST`, `SOURCE SECOND`.
- Source inherited media box `[0,0,600,300]`, crop box `[10,20,590,280]`, and
  90-degree rotation survived on both inserted pages.
- With `insert bookmarks true`, the source bookmark was added under a
  `rea-source.pdf` parent. With false, only the base bookmark remained.
- Both inputs deliberately used `shared-destination` for different pages.
  This AppleScript insertion retained only the base named destination, while
  the inserted link still used that name. It therefore resolves to the base
  page, not the original source target. **This does not contradict the
  full-document collision branch:** this API experiment did not establish that
  `PDInsertAll` was set. Do not generalize it to the Combine tool.

Only synthetic fixtures were opened/changed and all fixture documents were
closed. No user document content was used. No network trace was taken, so this
experiment makes no claim about Acrobat's background network activity.

## Implications for the Rust implementation

The independent Rust design should preserve source page order, inherited
attributes, resource references and intra-document links, while validating
input page trees. A source-local mapping or destination resolution must avoid
cross-input name collisions. Whole-document features require explicit handling;
copying pages alone does not reproduce Acrobat's bookmarks, layers, structure,
forms, or font deduplication. The current CLI's documented exclusions remain
necessary until those features are implemented and verified.

## Native evidence index

| Dossier | Entry | REA Evidence ID |
| --- | --- | --- |
| [pdfl-get-page-attrs](evidence/ghidra/pdfl-get-page-attrs.json) | `0x1a2f08` | `ev_f3ef1141325db560cfe5313c31d3ab72808aa6ca2c1c22adfa0a83c025477914` |
| [pdfl-insert-no-perms](evidence/ghidra/pdfl-insert-no-perms.json) | `0x1a0348` | `ev_d7061548e739c8c4c1521ba7180da47c0da95172284bc38b82995c5d62ab2914` |
| [pdfl-insert-pages](evidence/ghidra/pdfl-insert-pages.json) | `0x1a021c` | `ev_18ce835f955226b009f5cbb0e007c936ab6bb43711cae7f8a835f6aed3edf9e3` |
| [pdfl-link-hooks](evidence/ghidra/pdfl-link-hooks.json) | `0x1a36d0` | `ev_a8ac05c8f2b21588d524d134432536352597165eeef397c09cc50e997290ca30` |
| [pdfl-merge-annots](evidence/ghidra/pdfl-merge-annots.json) | `0x1a3a0c` | `ev_d6aa0d74ce7309f5ed0081890e13ff6b71bd63bccbd800b0957438496621bc27` |
| [pdfl-merge-data](evidence/ghidra/pdfl-merge-data.json) | `0x1a1bd0` | `ev_cbc77fc12113cc0661e9fb275cd5ed100c6deb5991a4cd46dbb1df78bdad76c9` |
| [pdfl-merge-dests](evidence/ghidra/pdfl-merge-dests.json) | `0x1a4738` | `ev_e00b3d3d2101bf0bd19fbc395bcd32f3731db65bccd7a28002261465556ed759` |
| [pdfl-merge-names](evidence/ghidra/pdfl-merge-names.json) | `0x1a48b4` | `ev_35f26e57b44eb2cf353df8ccbc659f658b37197e53906c684b74517fe89b306c` |
| [pdfl-merge-outlines](evidence/ghidra/pdfl-merge-outlines.json) | `0x1a3c9c` | `ev_0408a9e2ca0b8fb3fd5dbbe1e56547b7b9684224b9aea1a59c000f03664e3d0b` |
| [pdfl-merge-structure](evidence/ghidra/pdfl-merge-structure.json) | `0x1a5364` | `ev_9143ed06e109b1d159da063a2ae1d6d842c187151482ed335f5fbe54159ed4f8` |
| [pdfl-parent-tree](evidence/ghidra/pdfl-parent-tree.json) | `0x1a5590` | `ev_06398dae169b420f6a362f762cab8cb404576375886d1bbea631af8ffddfdcbe` |
| [pdfl-resolve-hooks](evidence/ghidra/pdfl-resolve-hooks.json) | `0x1a364c` | `ev_27f4293c10b36b16a240998fc7e1059c167c88c67d906996db85d2715d2a647e` |
| [pdfl-set-page-attrs](evidence/ghidra/pdfl-set-page-attrs.json) | `0x1a2ff4` | `ev_0f90375b2c3eb652e1823733e9192bdbaa082c841ac5ac3ea29bbd03fc2550b8` |
| [pdfl-unhook-objects](evidence/ghidra/pdfl-unhook-objects.json) | `0x1a1fb8` | `ev_c9c55e0c7fbfb04c659cea0ac255698e660357ee3ce3066a2d6697c00de05cc2` |

Each dossier retains full assembly, pseudocode, body ranges, callers, callees, references, provider identity and limitations. The [portable REA bundle](evidence/ghidra/pdfl-evidence-bundle.json) retains the session ledger. Ghidra warning paths and unresolved indirect flows are not treated as verified source semantics.
