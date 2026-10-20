#!/usr/bin/env bash
# Builds release wheels with PyPI metadata into dist/python/wheelhouse, each with the notices of
# the crates compiled into it (THIRD-PARTY-NOTICES, for the host's target).
# Pass interpreters to build for several Pythons: scripts/pack-python.sh --python python3.12 ...
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf dist/python
boltffi generate python
# manylinux's own Python first: its python3 may be older than scripts/readme.py needs (3.10)
python=/opt/python/cp312-cp312/bin/python
[ -x "$python" ] || python=$(command -v python3 || command -v python)
"$python" scripts/third-party-notices.py -o dist/python/THIRD-PARTY-NOTICES
"$python" scripts/python-metadata.py
boltffi pack python --release --regenerate false "$@"
