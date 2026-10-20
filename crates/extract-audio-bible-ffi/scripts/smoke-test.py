"""Checks an installed extract-audio-bible wheel: it imports, its native library loads, and a few
calls that need no files answer. Run it in a fresh environment, from outside the source tree:

    python -m venv /tmp/smoke && /tmp/smoke/bin/pip install --no-index --find-links dist/python/wheelhouse extract-audio-bible
    (cd /tmp && /tmp/smoke/bin/python path/to/scripts/smoke-test.py)
"""

import extract_audio_bible
from extract_audio_bible import ExtractAudioBible, InputFile

eab = ExtractAudioBible()
version = eab.version()
assert version, "extract-audio-bible has a version"

bible = eab.import_bible(
    [InputFile(path="kjv.txt", bytes=b"John 11:35 Jesus wept.\nJohn 11:36 Then said the Jews, Behold how he loved him!")],
    "King James Version",
    "KJV",
)
assert bible.verse(43, 11, 35) == "Jesus wept.", bible.verse(43, 11, 35)

transcript = eab.read_transcript(
    '{"segments": [{"start": 0, "end": 6, "text": "Jesus wept. Then said the Jews, behold how he loved him."}]}'
)
found = eab.searcher(bible, "John", 0.85).find(transcript, "s")
assert [r.verse for r in found.readings] == [35, 36], found
assert eab.book_id("1 Thess") == 52

print(f"extract-audio-bible {version} from {extract_audio_bible.__file__}: ok")
