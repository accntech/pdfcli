# Observed core import diagnostics

This is an analyst note transcribed from tool-visible Ghidra log reads, not a retained REA Evidence record or a complete log. The temporary log was removed during successful close before it was copied.

- 2026-10-13 00:03:14 local: default analysis began.
- 00:03:29: invalid PNG data at 04d9f2ce.
- 00:03:42: DecompilerSwitchAnalyzer reported a non-returning function and deferred restart.
- VarnodeContext out-of-address-space errors were reported at 019a9ef8, 01790af0, 00671a74, 01966a3c, 01f18fe4, 00671a34 and 01f18fd8.
- 00:12:28: internal decompiler worker reported java.util.concurrent.ExecutionException caused by ArrayIndexOutOfBoundsException.
- 00:12:58: ApplyDataArchiveAnalyzer applied mac_osx type archive.
- A read-only thread sample of the analysis JVM showed active StackVariableAnalyzer work, rather than a hung connection. Acrobat was not sampled.
- 00:20:26: REA no-return fix reported cleared=0 sites=0 refit=0; REA bridge started after default analysis completed.
- All 21 selected function dossier operations completed; assembly exceeded only the MCP transport budget and was recovered through a retained-evidence view/export.

The retained per-function limitations are authoritative for each dossier. These import diagnostics mean whole-program recovery was not error-free; they do not independently invalidate a specific cited instruction or successfully returned pseudocode body.
