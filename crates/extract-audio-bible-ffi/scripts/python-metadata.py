"""Adds PyPI metadata to the Python package that `boltffi generate python` makes.

Run after generating and before building (`boltffi pack python --regenerate false`); setuptools
merges setup.cfg with the generated setup.py.
"""

import shutil
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
package = root / "dist" / "python"
if not (package / "setup.py").is_file():
    sys.exit(f"{package} has no setup.py; run `boltffi generate python` first")
if not (package / "THIRD-PARTY-NOTICES").is_file():
    sys.exit(f"{package} has no THIRD-PARTY-NOTICES; run scripts/third-party-notices.py first")

# README.md in Python alone, its examples checked against the package's __init__.pyi
sys.dont_write_bytecode = True
sys.path.insert(0, str(root / "scripts"))
from readme import checked, fail  # noqa: E402

text, problems = checked((root / "README.md").read_text(encoding="utf-8"), "python", package / "extract_audio_bible" / "__init__.pyi")
fail(problems)
(package / "README.md").write_text(text, encoding="utf-8")
shutil.copy(root.parent.parent / "LICENSE", package / "LICENSE")
(package / "setup.cfg").write_text(
    """[metadata]
description = Build an audio Bible from the Scripture read in sermons: find each verse's readings in transcripts, choose them, and export tagged MP3s
long_description = file: README.md
long_description_content_type = text/markdown
license_expression = CC0-1.0
license_files =
    LICENSE
    THIRD-PARTY-NOTICES
url = https://github.com/MasterTemple/extract-audio-bible
project_urls =
    Source = https://github.com/MasterTemple/extract-audio-bible
    Issues = https://github.com/MasterTemple/extract-audio-bible/issues
keywords = bible, audio bible, sermon, transcript, whisperx, mp3, scripture
classifiers =
    Development Status :: 3 - Alpha
    Intended Audience :: Developers
    Programming Language :: Python :: 3
    Programming Language :: Rust
    Topic :: Multimedia :: Sound/Audio
    Topic :: Religion
    Topic :: Text Processing
"""
)
print(f"added metadata to {package}")
