#!/usr/bin/env bash
# Builds the npm package (extract-audio-bible) into dist/wasm/pkg, with its README (README.md in TypeScript
# alone, its examples type-checked against the package: scripts/readme.py, which needs `npm
# install`), license, and the notices of the crates compiled into it (THIRD-PARTY-NOTICES).
set -euo pipefail
cd "$(dirname "$0")/.."
python=$(command -v python3 || command -v python)
boltffi pack wasm --release
"$python" scripts/readme.py ts --api dist/wasm/pkg/extract_audio_bible.d.ts -o dist/wasm/pkg/README.md
cp ../../LICENSE dist/wasm/pkg/LICENSE
# web.js, bundler.js, and node.js export `initialized`, which BoltFFI's types leave out
types=dist/wasm/pkg/extract_audio_bible.d.ts
if ! grep -q 'export declare const initialized' "$types"; then
  printf '%s\n' '' \
    '/** Resolves once the WebAssembly module is loaded: await it before the first call in a browser (in Node and Bun it already has) */' \
    'export declare const initialized: Promise<void>;' >> "$types"
fi
"$python" scripts/third-party-notices.py --target wasm32-unknown-unknown -o dist/wasm/pkg/THIRD-PARTY-NOTICES
# The generated package.json lists its files, and npm adds only README and LICENSE on its own
node -e '
const fs = require("fs");
const path = "dist/wasm/pkg/package.json";
const pkg = JSON.parse(fs.readFileSync(path, "utf8"));
if (pkg.files && !pkg.files.includes("THIRD-PARTY-NOTICES")) pkg.files.push("THIRD-PARTY-NOTICES");
fs.writeFileSync(path, JSON.stringify(pkg, null, 2) + "\n");
'
