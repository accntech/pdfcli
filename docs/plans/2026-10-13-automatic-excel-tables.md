# Automatic Excel Table Export Implementation Plan

**Goal:** Make automatic table sections and width-only fitting the macOS Excel default, preserving source files.
**Approach:** Inspect layout through installed Excel on a private workbook copy. Detect independent column bands from occupancy and merged-heading structure in Rust, protecting spanning content and graphics, then use the existing native per-section PDF exporter.
**Constraints:** No source saves, LibreOffice substitution, worktrees, subagents or commits. Keep vertical pagination, all visible content, per-sheet paper geometry and blank-page handling. Provide a saved-layout opt-out and keep manual overrides available.

### Task 1: Layout inspection and detection
**Files:** src/office/excel-layout.applescript, src/office/excel_layout.rs, src/office/macos.rs; synthetic tests in tests/fixtures and tests/merge.rs.
**Interfaces:** Native inspection produces numeric layout metadata (no cell text); Rust produces worksheet-indexed rectangular print sections.
- [x] Probe native bulk layout APIs without changing source workbooks.
- [x] Add and run failing default-export tests for independent tables, merged headings, protected spans and vertical continuation.
- [x] Implement bounded inspection and conservative band detection; retain all occupied cells and visible graphics.

### Task 2: Default and compatibility
**Files:** src/main.rs, src/office.rs, src/office/excel.applescript, tests/merge.rs.
**Interfaces:** Normal merge enables automatic width fitting; --excel-preserve-layout keeps saved pagination. Manual print areas override detected sections.
- [x] Wire automatic detection into conversion and keep total conversion timeout bounded.
- [x] Update legacy-layout regressions to request preservation explicitly.
- [x] Validate focused tests, source preservation and Office staging cleanup.

### Task 3: Verification and documentation
**Files:** README.md, docs/acrobat-analysis.md, docs/evidence, output/pdf; private validation in tmp/pdfs/auto-tables.
- [x] Run native Office tests, Clippy, formatting and release build.
- [x] Rerun the supplied workbook and notes without Excel flags; inspect rendered pages and compare with the approved manual-section result.
- [x] Verify both original hashes, all visible sheets, complete table columns, multi-page continuation and zero blank pages.
- [x] Document detection boundaries and opt-out; no commit unless separately confirmed by the user.

**Verified result:** 39 distinct tests passed, including installed Word/Excel/PowerPoint regressions. Formatting, Clippy and release build passed. The no-flags private-document rerun produced 21 pages (13 Excel + 8 Word), identical to the earlier manual-section output. Both original hashes match and Office staging is empty. No commit or worktree was created.
