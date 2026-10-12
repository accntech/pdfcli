# REA Ghidra provider repair

Repaired 2026-10-12/13 for this investigation. REA 6.4.0 now has a working
Ghidra 12.1.4 provider on this ARM64 Mac. IDA and Hopper were not required
or configured by this repair. Success is established by actual function dossiers, including
`ev_18ce835f955226b009f5cbb0e007c936ab6bb43711cae7f8a835f6aed3edf9e3`,
not just a readiness check.

## Installed components

- Ghidra: `/Users/ogie/.local/share/rea/ghidra_12.1.4_PUBLIC`
- Java: existing Homebrew OpenJDK 21 at
  `/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home`
- REA CLI: `/Users/ogie/.nvm/versions/node/v24.12.0/bin/rea`
- Native ARM64 decompiler:
  `Ghidra/Features/Decompiler/os/mac_arm_64/decompile` within that installation.

The [official Ghidra release](https://github.com/NationalSecurityAgency/ghidra/releases/tag/Ghidra_12.1.4_build)
archive `ghidra_12.1.4_PUBLIC_20260921.zip` was verified against the official
GitHub asset digest:
`ddac49f903da9d5bac833e5cc79395098b9c33cfd3279be5f31bd00387d2d4db`.
The release did not ship the needed macOS native decompiler. It was built
from that archive's source with Apple's installed clang:

```sh
cd "$GHIDRA_INSTALL_DIR/Ghidra/Features/Decompiler/src/decompile/cpp"
make -j8 ghidra_opt ARCH_TYPE='-arch arm64' OSDIR=mac_arm_64 \
  ADDITIONAL_FLAGS='-mmacosx-version-min=11.0'
```

The resulting executable was installed in the provider's expected `os/mac_arm_64`
directory. Build log: `/Users/ogie/.local/share/rea/ghidra-native-build.log`.

## Durable registration

`/Users/ogie/.codex/config.toml` now has `GHIDRA_INSTALL_DIR` and `JAVA_HOME` in
`[mcp_servers.rea.env]`. The previous config was saved at
`/Users/ogie/.codex/config.toml.before-ghidra-20261012`; the earlier REA backup
was preserved. A scoped dry run preceded the applied `rea setup --client codex
--skill=false --yes --json`. No skill replacement was performed.

The filtered successful check is retained in
[evidence/ghidra/provider-readiness.json](evidence/ghidra/provider-readiness.json).
To repeat:

```sh
GHIDRA_INSTALL_DIR=/Users/ogie/.local/share/rea/ghidra_12.1.4_PUBLIC \
JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home \
/Users/ogie/.nvm/versions/node/v24.12.0/bin/rea doctor --client codex --provider ghidra --json
```

An already-running MCP server retains its old environment until reconnected.
This investigation used fresh REA MCP processes launched through the package's
public MCP SDK with the configured environment. Their results are real REA
Evidence records with Ghidra provider identity. Restart/reconnect REA in Codex
to use the durable registration from subsequent chats.

## Target handling

REA rejected the universal framework as `architecture_unsupported` because it
could not guarantee the selected slice. Apple's `lipo -thin arm64` extracted
separate analysis copies. The installed app was not modified. Both original
and extracted hashes, paths, and transformations are retained in
[artifact-derivation.json](evidence/ghidra/artifact-derivation.json).

Ghidra performed its default auto-analysis before returning functions. No
partial import was presented as completed analysis. Recovered pseudocode has
inferred types and compiler artifacts; raw instructions and references remain
in each dossier for cross-checking.
