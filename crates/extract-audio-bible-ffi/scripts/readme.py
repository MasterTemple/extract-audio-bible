"""Writes one package's README from README.md, which has every language's examples: npm's in
TypeScript (`ts`), PyPI's in Python (`python`), after checking them.

    python scripts/readme.py ts|python [-o README.md] [--api BINDINGS]
    python scripts/readme.py check [--wasm dist/wasm/pkg] [--python dist/python/extract_audio_bible]
    (or, with only the bindings generated: boltffi generate typescript -o dist/api/ts, then
    python -o dist/api/python, and check --wasm dist/api/ts --python dist/api/python/extract_audio_bible)

README.md is the repository's, read on GitHub, and the source of both (examples/node/readme.ts
and examples/python/readme.py run its examples). For a language, the other language's code blocks
are left out, and so is:
- `<!-- source -->…<!-- /source -->`: what's for the repository alone (building, the checks)
- `<!-- ts: … -->`, `<!-- python: … -->`: hidden in README.md, the text in that language's README
  (where the repository's says both languages' ways, or names only TypeScript's)
Python's README also names things as Python does: in inline code, a camelCase name is
snake_case (`openPackage` is `open_package`), `null` is `None`, and `true` and `false` are
`True` and `False`, but for the few in `PYTHON_NAMES`, which Python spells otherwise.

Writing one checks it first, and fails saying what's wrong:
- README.md: its marks are whole and none is inside another, and its examples come in pairs,
  each TypeScript one followed by its Python one
- the README written: no mark left, every example of its language kept, and every link to a
  heading (`#a-books-text`) one it has
- with `--api` (the bindings built: npm's `extract_audio_bible.d.ts`, Python's `__init__.pyi`), the examples
  against them, without running them (examples/*/readme.* run them against real packages):
  TypeScript's type-checked by `tsc` (npm install first), Python's parsed and every name from
  `extract_audio_bible` imported, attribute, argument, and record field checked, following what each
  variable holds; and every name of the API the prose gives (`newProject`, `ExtractAudioBible`) one
  the bindings have
`check` does all of that for both languages, against what the pack scripts built.
"""

from __future__ import annotations

import argparse
import ast
import builtins
import enum
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LANGUAGES = {"ts": "python", "python": "ts"}

# Inline code that Python spells otherwise, not by snake_case
PYTHON_NAMES = {
    "null": "None",
    "true": "True",
    "false": "False",
    "_import": "import_",
    "ExtractAudioBible.new": "ExtractAudioBible()",
}

# Names the prose gives that the bindings don't export, on purpose
NOT_EXPORTED: set[str] = set()

# A name, a call (`book(id)`), or a chain of them (`info().upstream`)
CHAIN = re.compile(r"[A-Za-z_]\w*(\([\w, ]*\))?(\.[A-Za-z_]\w*(\([\w, ]*\))?)*")
# A code block, and its language
BLOCK = re.compile(r"^```([^\n]*)\n(.*?)^```\n?", re.M | re.S)


def snake(name: str) -> str:
    """`openPackage` as `open_package`; any other name (a class, OSIS's `sID`) as it is"""
    if not re.fullmatch(r"[a-z][a-z0-9]*([A-Z][a-z0-9]+)+", name):
        return name
    return re.sub(r"(?<=[a-z0-9])([A-Z])", lambda m: "_" + m[1].lower(), name)


def python_code(code: str) -> str:
    if code in PYTHON_NAMES:
        return PYTHON_NAMES[code]
    if not CHAIN.fullmatch(code):
        return code
    return re.sub(r"[A-Za-z_]\w*", lambda m: snake(m[0]), code)


def marked(text: str, language: str) -> str:
    """The marked passages for one language: the repository's alone left out, the language's own
    shown"""
    other = LANGUAGES[language]
    # Each stops at its own end, never another's
    source = r"<!-- source -->(?:(?!<!-- /?source -->).)*<!-- /source -->"
    for left_out in [source, rf"<!-- {other}:(?:(?!-->).)*-->"]:
        # Lines of their own go with their line break; inline, the text around them stays
        text = re.sub(rf"^{left_out}\n", "", text, flags=re.S | re.M)
        text = re.sub(left_out, "", text, flags=re.S)
    return re.sub(rf"<!-- {language}:[ \n]?(.*?)[ \n]?-->", r"\1", text, flags=re.S)


def prose(text: str, language: str) -> str:
    """Text outside code blocks, for one language"""
    if language == "python":
        text = re.sub(r"`([^`\n]+)`", lambda m: f"`{python_code(m[1])}`", text)
    return text


def readme(source: str, language: str) -> str:
    other = LANGUAGES[language]
    source = marked(source, language)
    # Code blocks and the text between them, in turn
    parts = re.split(r"(^```[^\n]*\n.*?^```\n?)", source, flags=re.M | re.S)
    out = []
    for i, part in enumerate(parts):
        if i % 2 == 1:
            if not part.startswith(f"```{other}\n"):
                out.append(part)
        else:
            out.append(prose(part, language))
    # A block left out leaves the blank lines around it, which go (in the prose, not the code)
    parts = re.split(r"(^```[^\n]*\n.*?^```\n?)", "".join(out), flags=re.M | re.S)
    return "".join(p if i % 2 else re.sub(r"\n{3,}", "\n\n", p) for i, p in enumerate(parts))


def outside_code(text: str) -> str:
    """The text with its code blocks blanked, keeping its lines"""
    return BLOCK.sub(lambda m: "\n" * m[0].count("\n"), text)


def line_of(text: str, at: int) -> int:
    return text.count("\n", 0, at) + 1


def blocks(text: str, language: str) -> list[tuple[int, str]]:
    """Each code block in a language, with the line its code starts on"""
    return [(line_of(text, m.start()) + 1, m[2]) for m in BLOCK.finditer(text) if m[1] == language]


# --- README.md -------------------------------------------------------------------------------


def check_source(source: str) -> list[str]:
    problems = []
    text = outside_code(source)
    open_at = None
    for m in re.finditer(r"<!--(.*?)(-->|$)", text, flags=re.S):
        at = f"README.md:{line_of(text, m.start())}"
        body = m[1]
        if not m[2]:
            problems.append(f"{at}: a comment that never ends")
        elif body == " source ":
            if open_at:
                problems.append(f"{at}: <!-- source --> inside the one at {open_at}")
            open_at = at
        elif body == " /source ":
            if not open_at:
                problems.append(f"{at}: <!-- /source --> with no <!-- source --> before it")
            open_at = None
        elif re.match(r" (ts|python):", body):
            if "<!--" in body:
                problems.append(f"{at}: a mark inside a {body.split(':')[0].strip()} passage")
        else:
            problems.append(f"{at}: a comment that isn't a mark: <!--{body[:40]}-->")
    if open_at:
        problems.append(f"{open_at}: <!-- source --> with no <!-- /source -->")
    for m in BLOCK.finditer(source):
        if "<!--" in m[2]:
            problems.append(f"README.md:{line_of(source, m.start())}: a mark inside a code block")
    # Each TypeScript example followed by its Python one
    examples = [(m[1], line_of(source, m.start())) for m in BLOCK.finditer(source) if m[1] in LANGUAGES]
    for i in range(0, len(examples), 2):
        pair = examples[i : i + 2]
        if [language for language, _ in pair] != ["ts", "python"]:
            problems.append(
                f"README.md:{pair[0][1]}: a {pair[0][0]} example not paired as ts, then python"
            )
            break
    return problems


# --- A language's README ----------------------------------------------------------------------


def slug(heading: str) -> str:
    """GitHub's anchor for a heading"""
    return re.sub(r"[^\w\- ]", "", heading.strip().lower()).replace(" ", "-")


def check_readme(text: str, source: str, language: str) -> list[str]:
    name = f"{language} README"
    problems = []
    prose_only = outside_code(text)
    for m in re.finditer(r"<!--|-->", prose_only):
        problems.append(f"{name}:{line_of(prose_only, m.start())}: a mark left in it")
    other = LANGUAGES[language]
    if blocks(text, other):
        problems.append(f"{name}: {len(blocks(text, other))} {other} examples left in it")
    if [code for _, code in blocks(text, language)] != [code for _, code in blocks(source, language)]:
        problems.append(f"{name}: its {language} examples aren't README.md's")
    anchors = {slug(m[1]) for m in re.finditer(r"^#+ (.*)$", prose_only, flags=re.M)}
    for m in re.finditer(r"\]\(#([^)]*)\)", prose_only):
        if m[1] not in anchors:
            problems.append(f"{name}:{line_of(prose_only, m.start())}: a link to #{m[1]}, which no heading is")
    return problems


def api_names(api: str, language: str) -> set[str]:
    """Every name the bindings declare; for Python, also each variant's own name
    (`ExtractAudioBibleErrorIo` is the `Io` of `ExtractAudioBibleError`)"""
    names = set(re.findall(r"[A-Za-z_]\w*", api))
    if language == "python":
        for m in re.finditer(r"^class (\w+)\((\w+)\)", api, flags=re.M):
            if m[1].startswith(m[2]) and m[1] != m[2]:
                names.add(m[1][len(m[2]) :])
    return names


def check_prose_names(text: str, api: str, language: str) -> list[str]:
    """The API's names the prose gives (camelCase or snake_case functions, classes), each one the
    bindings have"""
    known = api_names(api, language) | set(dir(builtins))
    looks_like_api = re.compile(
        r"[a-z][a-z0-9]*(?:[A-Z][a-z0-9]+)+|[A-Z][a-z]+(?:[A-Z][a-z0-9]*)*"
        + (r"|[a-z][a-z0-9]*(?:_[a-z0-9]+)+" if language == "python" else "")
    )
    problems = []
    prose_only = outside_code(text)
    for m in re.finditer(r"`([^`\n]+)`", prose_only):
        if not CHAIN.fullmatch(m[1]):
            continue
        for name in re.findall(r"[A-Za-z_]\w*", m[1]):
            if looks_like_api.fullmatch(name) and name not in known and name not in NOT_EXPORTED:
                at = line_of(prose_only, m.start())
                problems.append(f"{language} README:{at}: `{m[1]}` names {name}, which the bindings don't have")
    return problems


# --- TypeScript's examples --------------------------------------------------------------------


def check_typescript(source: str, dts: Path) -> list[str]:
    """README.md's TypeScript examples as one program (as examples/node/readme.ts runs them),
    type-checked by `tsc` against the bindings' types: the package's `extract_audio_bible.d.ts`, or the
    `extract_audio_bible.ts` that `boltffi generate typescript` writes (under the package's folder, so
    `@boltffi/runtime` is found)"""
    tsc = ROOT / "node_modules" / ".bin" / "tsc"
    if not tsc.exists():
        return [f"no {tsc.relative_to(ROOT)}: run `npm install` to check the TypeScript examples"]
    imports: dict[str, dict[str, bool]] = {}
    lines: list[str] = []
    origin: list[int] = []  # README.md's line for each line of the program

    def take_import(m: re.Match) -> str:
        names = imports.setdefault(m[2], {})
        for name in filter(None, (n.strip() for n in m[1].split(","))):
            only_type = name.startswith("type ")
            name = name.removeprefix("type ")
            names[name] = names.get(name, True) and only_type
        return ""

    for start, code in blocks(source, "ts"):
        for offset, line in enumerate(code.splitlines()):
            line = re.sub(r'^import \{([^}]*)\} from "([^"]+)";$', take_import, line)
            lines.append(line)
            origin.append(start + offset)
    header = [
        "import { " + ", ".join(("type " if only else "") + n for n, only in names.items()) + f' }} from "{module}";'
        for module, names in imports.items()
    ]
    program = header + lines
    origin = [0] * len(header) + origin
    # Inside the package's folder, so its node_modules (@types/node) are found
    with tempfile.TemporaryDirectory(dir=ROOT / "dist", prefix="readme-check-") as folder:
        Path(folder, "readme.ts").write_text("\n".join(program) + "\n", encoding="utf-8")
        config = {
            "compilerOptions": {
                "strict": True,
                "noEmit": True,
                "target": "es2023",
                "module": "esnext",
                "moduleResolution": "bundler",
                "lib": ["es2023", "dom"],
                "types": ["node"],
                "skipLibCheck": True,
                "paths": {"extract-audio-bible": [str(dts.resolve())]},
            },
            "files": ["readme.ts"],
        }
        Path(folder, "tsconfig.json").write_text(json.dumps(config))
        run = subprocess.run([str(tsc), "-p", folder, "--pretty", "false"], capture_output=True, text=True)
    problems = []
    for line in (run.stdout + run.stderr).splitlines():
        m = re.match(r".*readme\.ts\((\d+),(\d+)\): (.*)", line)
        if m:
            at = origin[int(m[1]) - 1]
            problems.append(f"README.md:{at}: {m[3]}" if at else f"the examples' imports: {m[3]}")
        elif re.match(r".*\.ts\(\d+,\d+\): ", line) and not dts.name.endswith(".d.ts"):
            # The bindings as source (`boltffi generate typescript`, no package built): their
            # own code isn't held to the examples' strictness
            continue
        elif line.strip() and run.returncode:
            problems.append(f"tsc: {line}")
    return problems


# --- Python's examples ------------------------------------------------------------------------


class PythonApi:
    """The classes in a generated `__init__.pyi`: each one's fields, methods (what they take and
    give back), enum members, and bases"""

    def __init__(self, pyi: str):
        self.classes: dict[str, dict] = {}
        self.names: set[str] = set()
        for node in ast.parse(pyi).body:
            if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
                self.names.add(node.name)
            elif isinstance(node, (ast.Assign, ast.AnnAssign)):
                targets = node.targets if isinstance(node, ast.Assign) else [node.target]
                self.names |= {t.id for t in targets if isinstance(t, ast.Name)}
            if not isinstance(node, ast.ClassDef):
                continue
            members, fields = {}, []
            for item in node.body:
                if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                    members[item.target.id] = ("field", item.annotation)
                    if not item.target.id.startswith("_"):
                        fields.append(item.target.id)
                elif isinstance(item, ast.Assign):
                    for target in item.targets:
                        if isinstance(target, ast.Name):
                            members[target.id] = ("member", None)
                elif isinstance(item, ast.FunctionDef):
                    members[item.name] = ("method", item)
            self.classes[node.name] = {
                "bases": [b.id for b in node.bases if isinstance(b, ast.Name)],
                "members": members,
                "fields": fields,
                "dataclass": any("dataclass" in ast.unparse(d) for d in node.decorator_list),
            }

    def lineage(self, name: str) -> list[str]:
        """The class and its bases, then the classes made from it (a variant a check narrows to)"""
        found, queue = [], [name]
        while queue:
            current = queue.pop(0)
            if current in self.classes and current not in found:
                found.append(current)
                queue += self.classes[current]["bases"]
        found += [c for c, info in self.classes.items() if name in info["bases"] and c not in found]
        return found

    def member(self, name: str, attribute: str):
        for current in self.lineage(name):
            if attribute in self.classes[current]["members"]:
                return self.classes[current]["members"][attribute]
        for base in self.foreign_bases(name):
            if hasattr(base, attribute):
                return ("foreign", None)
        return None

    def foreign_bases(self, name: str) -> list[type]:
        """Bases the bindings don't declare (`IntEnum`, `RuntimeError`)"""
        known = {"IntEnum": enum.IntEnum}
        return [
            known.get(base, getattr(builtins, base, object))
            for current in self.lineage(name)
            for base in self.classes[current]["bases"]
            if base not in self.classes
        ]

    def type_of(self, annotation):
        """What an annotation says a value is: a class's name, `("list", name)`, or None"""
        if annotation is None:
            return None
        if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
            annotation = ast.parse(annotation.value, mode="eval").body
        if isinstance(annotation, ast.Name):
            return annotation.id if annotation.id in self.classes else None
        if isinstance(annotation, ast.BinOp) and isinstance(annotation.op, ast.BitOr):
            sides = [self.type_of(s) for s in (annotation.left, annotation.right)]
            sides = [s for s in sides if s]
            return sides[0] if len(sides) == 1 else None
        if isinstance(annotation, ast.Subscript) and ast.unparse(annotation.value) in ("list", "Sequence"):
            inner = self.type_of(annotation.slice)
            return ("list", inner) if isinstance(inner, str) else None
        return None


class PythonCheck(ast.NodeVisitor):
    """Follows what each variable of the examples holds (an instance, a class, a list of
    instances) and checks every use of the bindings' classes against `PythonApi`"""

    def __init__(self, api: PythonApi, origin):
        self.api = api
        self.origin = origin
        self.types: dict[str, object] = {}
        self.functions: dict[str, object] = {}
        self.problems: list[str] = []

    def problem(self, node: ast.AST, message: str) -> None:
        self.problems.append(f"README.md:{self.origin(node.lineno)}: {message}")

    def infer(self, node):
        if isinstance(node, ast.Name):
            return self.types.get(node.id)
        if isinstance(node, ast.Call):
            func = node.func
            if isinstance(func, ast.Name) and func.id in ("list", "sorted") and node.args:
                return self.infer(node.args[0])
            # `next(e for e in entries if …)`: one of them
            if isinstance(func, ast.Name) and func.id == "next" and node.args:
                found = node.args[0]
                if isinstance(found, ast.GeneratorExp) and isinstance(found.elt, ast.Name):
                    generator = found.generators[0]
                    if isinstance(generator.target, ast.Name) and generator.target.id == found.elt.id:
                        return self.element(generator.iter)
                return None
            if isinstance(func, ast.Name):
                held = self.types.get(func.id)
                if isinstance(held, tuple) and held[0] == "class":
                    return held[1]
                return self.functions.get(func.id)
            if isinstance(func, ast.Attribute):
                owner = self.owner(self.infer(func.value))
                member = owner and self.api.member(owner, func.attr)
                if member and member[0] == "method":
                    return self.api.type_of(member[1].returns)
            return None
        if isinstance(node, ast.Attribute):
            held = self.infer(node.value)
            owner = self.owner(held)
            member = owner and self.api.member(owner, node.attr)
            if member and member[0] == "field":
                return self.api.type_of(member[1])
            if member and member[0] == "member" and isinstance(held, tuple):
                return owner
            return None
        if isinstance(node, ast.Subscript):
            held = self.infer(node.value)
            if isinstance(held, tuple) and held[0] == "list" and not isinstance(node.slice, ast.Slice):
                return held[1]
            return held if isinstance(held, tuple) and held[0] == "list" else None
        return None

    @staticmethod
    def owner(held):
        """The class whose members a value has: an instance's, or a class's own"""
        if isinstance(held, str):
            return held
        if isinstance(held, tuple) and held[0] == "class":
            return held[1]
        return None

    def bind(self, target, held) -> None:
        if isinstance(target, ast.Name):
            if held is None:
                self.types.pop(target.id, None)
            else:
                self.types[target.id] = held
        elif isinstance(target, (ast.Tuple, ast.List)):
            for element in target.elts:
                self.bind(element, None)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        if node.module != "extract_audio_bible":
            return
        for alias in node.names:
            if alias.name not in self.api.names:
                self.problem(node, f"extract_audio_bible has no {alias.name}")
            elif alias.name in self.api.classes:
                self.types[alias.asname or alias.name] = ("class", alias.name)

    def visit_Assign(self, node: ast.Assign) -> None:
        self.visit(node.value)
        for target in node.targets:
            if isinstance(target, (ast.Tuple, ast.List)) and isinstance(node.value, (ast.Tuple, ast.List)):
                for element, value in zip(target.elts, node.value.elts):
                    self.bind(element, self.infer(value))
            else:
                self.visit(target) if not isinstance(target, ast.Name) else None
                self.bind(target, self.infer(node.value))

    def visit_AnnAssign(self, node: ast.AnnAssign) -> None:
        if node.value:
            self.visit(node.value)
        self.bind(node.target, self.api.type_of(node.annotation) or (node.value and self.infer(node.value)))

    def visit_AugAssign(self, node: ast.AugAssign) -> None:
        self.visit(node.value)

    def element(self, iterable):
        held = self.infer(iterable)
        return held[1] if isinstance(held, tuple) and held[0] == "list" else None

    def visit_For(self, node: ast.For) -> None:
        self.visit(node.iter)
        self.bind(node.target, self.element(node.iter))
        for statement in node.body + node.orelse:
            self.visit(statement)

    def comprehension(self, node) -> None:
        for generator in node.generators:
            self.visit(generator.iter)
            self.bind(generator.target, self.element(generator.iter))
            for condition in generator.ifs:
                self.visit(condition)
        for part in ("elt", "key", "value"):
            if hasattr(node, part):
                self.visit(getattr(node, part))

    visit_ListComp = visit_SetComp = visit_GeneratorExp = visit_DictComp = comprehension

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self.functions[node.name] = self.api.type_of(node.returns)
        for argument in node.args.args:
            self.bind(ast.Name(id=argument.arg), self.api.type_of(argument.annotation))
        for statement in node.body:
            self.visit(statement)

    def visit_ExceptHandler(self, node: ast.ExceptHandler) -> None:
        if node.type:
            self.visit(node.type)
        if node.name:
            held = self.infer(node.type) if node.type else None
            self.bind(ast.Name(id=node.name), self.owner(held))
        for statement in node.body:
            self.visit(statement)

    def visit_match_case(self, node: ast.match_case) -> None:
        self.visit(node.pattern)
        if node.guard:
            self.visit(node.guard)
        for statement in node.body:
            self.visit(statement)

    def visit_MatchClass(self, node: ast.MatchClass) -> None:
        name = ast.unparse(node.cls)
        if name in self.api.classes:
            for attribute, pattern in zip(node.kwd_attrs, node.kwd_patterns):
                member = self.api.member(name, attribute)
                if member is None:
                    self.problem(node, f"{name} has no {attribute}")
                elif isinstance(pattern, ast.MatchAs) and pattern.name and member[0] == "field":
                    self.bind(ast.Name(id=pattern.name), self.api.type_of(member[1]))
        self.generic_visit(node)

    def visit_Attribute(self, node: ast.Attribute) -> None:
        self.visit(node.value)
        owner = self.owner(self.infer(node.value))
        if owner and self.api.member(owner, node.attr) is None:
            self.problem(node, f"{owner} has no {node.attr}")

    def visit_Call(self, node: ast.Call) -> None:
        self.generic_visit(node)
        func = node.func
        held = self.infer(func.value) if isinstance(func, ast.Attribute) else None
        if isinstance(func, ast.Name) and isinstance(self.types.get(func.id), tuple):
            self.check_record(node, self.types[func.id][1], node.keywords, node.args)
        elif ast.unparse(func) in ("dataclasses.replace", "replace") and node.args:
            record = self.infer(node.args[0])
            if isinstance(record, str):
                self.check_record(node, record, node.keywords, [], every=False)
        elif isinstance(func, ast.Attribute) and self.owner(held):
            member = self.api.member(self.owner(held), func.attr)
            if member and member[0] == "method":
                self.check_arguments(node, f"{self.owner(held)}.{func.attr}", member[1])

    def check_record(self, node, name, keywords, args, every=True) -> None:
        """A record made: every field given, by name, and only its fields"""
        info = self.api.classes.get(name)
        if not info or not info["dataclass"] or not info["fields"]:
            if info and "__init__" in info["members"] and info["members"]["__init__"][0] == "method":
                self.check_arguments(node, name, info["members"]["__init__"][1])
            return
        given = [k.arg for k in keywords if k.arg]
        for field in given:
            if field not in info["fields"]:
                self.problem(node, f"{name} has no field {field}")
        missing = [f for f in info["fields"] if f not in given][len(args) :]
        if every and missing and not any(k.arg is None for k in keywords):
            self.problem(node, f"{name} needs {', '.join(missing)}")

    def check_arguments(self, node: ast.Call, name: str, method: ast.FunctionDef) -> None:
        """A method called with as many arguments as it takes, any by name its own"""
        if any(isinstance(a, ast.Starred) for a in node.args) or any(k.arg is None for k in node.keywords):
            return
        parameters = [a.arg for a in method.args.args]
        if method.args.args and parameters[0] in ("self", "cls"):
            parameters = parameters[1:]
        for keyword in node.keywords:
            if keyword.arg not in parameters:
                self.problem(node, f"{name} takes no {keyword.arg}")
        given = len(node.args) + len(node.keywords)
        required = len(parameters) - len(method.args.defaults)
        if not method.args.vararg and not required <= given <= len(parameters):
            self.problem(node, f"{name} takes {len(parameters)} arguments, given {given}")


def check_python(source: str, pyi: Path) -> list[str]:
    """README.md's Python examples, each parsed, then all as one program (as
    examples/python/readme.py runs them) checked against the bindings' `__init__.pyi`"""
    problems = []
    examples = blocks(source, "python")
    for start, code in examples:
        try:
            ast.parse(code)
        except SyntaxError as error:
            problems.append(f"README.md:{start + (error.lineno or 1) - 1}: {error.msg}")
    if problems:
        return problems
    program, origin = [], []
    for start, code in examples:
        for offset, line in enumerate(code.splitlines()):
            program.append(line)
            origin.append(start + offset)
    check = PythonCheck(PythonApi(pyi.read_text(encoding="utf-8")), lambda line: origin[line - 1])
    check.visit(ast.parse("\n".join(program)))
    return check.problems


# --- Running ----------------------------------------------------------------------------------


def checked(source: str, language: str, api: Path | None) -> tuple[str, list[str]]:
    """A language's README, and what's wrong with it and its examples"""
    text = readme(source, language)
    problems = check_source(source) + check_readme(text, source, language)
    if api:
        problems += check_prose_names(text, api.read_text(encoding="utf-8"), language)
        problems += check_typescript(source, api) if language == "ts" else check_python(source, api)
    return text, problems


def fail(problems: list[str]) -> None:
    if problems:
        sys.exit("README.md's documentation has problems:\n" + "\n".join(f"  {p}" for p in problems))


def main() -> None:
    if sys.version_info < (3, 10):
        sys.exit("scripts/readme.py needs Python 3.10 or later (the examples use `match`)")
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    for language in sorted(LANGUAGES):
        write = sub.add_parser(language, help=f"write {language}'s README")
        write.add_argument("-o", "--output", type=Path)
        write.add_argument("--api", type=Path, help="the bindings built (extract_audio_bible.d.ts, __init__.pyi)")
    check = sub.add_parser("check", help="check both languages' READMEs against the bindings built")
    check.add_argument("--wasm", type=Path, default=ROOT / "dist" / "wasm" / "pkg")
    check.add_argument("--python", type=Path, default=ROOT / "dist" / "python" / "extract_audio_bible")
    args = parser.parse_args()
    source = (ROOT / "README.md").read_text(encoding="utf-8")
    if args.command == "check":
        problems = []
        # The package built (dist/wasm/pkg), or only its bindings generated (`boltffi generate
        # typescript -o …`, no extract_audio_bible.d.ts but extract_audio_bible.ts)
        types = args.wasm / "extract_audio_bible.d.ts"
        types = types if types.exists() else args.wasm / "extract_audio_bible.ts"
        for language, api in [("ts", types), ("python", args.python / "__init__.pyi")]:
            if not api.exists():
                problems.append(
                    f"no {api}: build the package (npm run pack:wasm, pack:python) or generate its"
                    " bindings (boltffi generate typescript|python -o …)"
                )
                continue
            problems += checked(source, language, api)[1]
        fail(sorted(set(problems), key=problems.index))
        print("README.md's examples and names are valid in TypeScript and Python")
        return
    text, problems = checked(source, args.command, args.api)
    fail(problems)
    if args.output:
        args.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()
