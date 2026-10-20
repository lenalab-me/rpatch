#!/usr/bin/env python3
"""PreToolUse hook (Bash): stop file edits that cannot be checked, and name what to use instead.

Rules, first match wins (RULES below):
  python-replace     inline python that replaces text and writes a file
  sed-in-place       `sed -i` / `perl -i` on a file tracked by git
  overwrite-tracked  `cat`/`echo`/`printf` `>` (or `&>`, `>&`) or `tee` (no -a) over a file tracked by git

Each of these writes without checking what it replaces, so a wrong guess passes silently.
New files, files git does not track, temp dirs and appends (`>>`) pass. `>|` says "replace
the whole file on purpose" and passes. When the hook cannot tell which file a command writes
(variables, globs, a `cd` it cannot follow), it lets the command through.
Any error in this script lets the command through; RPATCH_GUARD=off turns it off.

The command is read by a small shell lexer (`Lexer`) into simple commands (`Command`): the
directory each runs in, its words and its redirections. The rules only look at those.
"""
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field

# --- python-replace: from the first hook (no-python-replace.py, 2026-10-02) -----------------
# It reads the raw command, before and apart from the lexer. Since 0.13.1 it also sees
# interpreter options before `-` / `-c` (`python3 -I -`, which the first hook let through).

# Interpreter options may come first: `python3 -I - <<EOF`, `python3 -Ic '...'`, `python3 -W error -`.
PY = re.compile(r"(^|[;&|(]|&&|\|\|)\s*(\S*/)?python3?(\.\d+)?"
                r"(\s+(?:-[WX]\s+\S+\s+|-[A-Za-z]+\s+)*(-[A-Za-z]*c\b|-\s|-$)|\s*<<)")
PY_HEREDOC = re.compile(r"<<-?\s*(['\"]?)(\w+)\1")
REPLACE = re.compile(r"\.replace\(|\bre\.sub\(")
# The mode is the second argument: `open('a')` names a file called "a" and reads it.
WRITE = re.compile(r"""open\([^)]*,\s*(mode\s*=\s*)?['"][wa]\+?b?['"]|write_text\(""")


def python_code(command):
    """The code of each inline python invocation outside other commands' heredoc bodies."""
    lines = command.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        opener = PY_HEREDOC.search(line)
        body_end = i + 1
        body = []
        if opener:
            delim = opener.group(2)
            j = i + 1
            while j < len(lines) and lines[j].strip() != delim:
                body.append(lines[j])
                j += 1
            body_end = j + 1
        if PY.search(line):
            # `python3 - <<EOF`: the heredoc body; `python3 -c '...'`: the rest of the command.
            yield "\n".join(body) if opener and not re.search(r"\s-[A-Za-z]*c\b", line) else "\n".join(lines[i:])
        i = body_end


def python_replace(command):
    if "python" in command and any(REPLACE.search(c) and WRITE.search(c) for c in python_code(command)):
        return (
            "Blocked by a hook: this command rewrites a file with python's replace, which does "
            "nothing silently when the text is not found. Use rpatch instead: it fails with line "
            "numbers when an edit does not match, and edits many places and files at once "
            "(`rpatch --help`; `<<<<<<< OLD part` covers part of a line). "
            "If python is really needed for something other than replacing text, run it from a script file."
        )
    return None


# --- the shell, as far as the rules need it -----------------------------------------------------

@dataclass
class Word:
    raw: str
    text: str | None  # the value after quote removal; None if it expands ($, backticks, globs)


@dataclass
class Redirect:
    fd: str  # "" for the default fd
    op: str  # ">", ">>", ">|", "&>", "&>>", ">&", "<", "<<<", ...
    target: Word


@dataclass
class Command:
    cwd: str | None  # None when the hook cannot tell
    words: list = field(default_factory=list)
    redirects: list = field(default_factory=list)


SEPARATORS = ("&&", "||", ";;", "|&", ";", "|", "&", "\n")
REDIRECT_OPS = ("&>>", "&>", ">>", ">|", ">&", "<<<", "<<-", "<<", "<&", "<>", ">", "<")
FD_PREFIX = re.compile(r"(\d+)(?=[<>])")  # `2>file`: digits right before the operator are its fd
WORD_END = set(" \t\n;&|()<>")


class Lexer:
    """Splits a command line into words, operators and redirections, the way bash does for the
    cases that decide where a command writes: quotes (also across lines), backslashes and line
    continuations, comments, `$(...)` and backticks (read as nested command lists), and heredocs
    with any delimiter, wherever they appear (their bodies are skipped)."""

    def __init__(self, src):
        self.s, self.i, self.pending = src, 0, []  # pending: heredoc (delimiter, strip tabs)

    def tokens(self, stop=None):
        """Tokens up to `stop` (")" or "`") or the end: ("word", Word), ("op", str),
        ("redir", Redirect) and ("sub", [tokens]) for a nested `( ... )`."""
        out, s = [], self.s
        while self.i < len(s):
            c = s[self.i]
            if stop and c == stop:
                self.i += 1
                return out
            if c in " \t":
                self.i += 1
            elif c == "\\" and s.startswith("\\\n", self.i):
                self.i += 2
            elif c == "#":  # a word that starts with `#` starts a comment
                while self.i < len(s) and s[self.i] != "\n":
                    self.i += 1
            elif c == "\n":
                self.i += 1
                out.append(("op", "\n"))
                self.read_heredoc_bodies()
            elif c == "(":
                self.i += 1
                out.append(("sub", self.tokens(")")))
            elif c == ")":
                self.i += 1  # unbalanced (a `case` pattern): ignore
            elif (m := FD_PREFIX.match(s, self.i)) and (op := self.redirect_op(at=m.end(), fd=m.group(1))):
                out.append(("redir", op))
            elif (op := self.redirect_op()) is not None:
                out.append(("redir", op))
            elif (sep := next((p for p in SEPARATORS if s.startswith(p, self.i)), None)) is not None:
                self.i += len(sep)
                out.append(("op", sep))
            else:
                word, subs = self.word()
                out.extend(("sub", t) for t in subs)
                out.append(("word", word))
        return out

    def redirect_op(self, at=None, fd=""):
        s = self.s
        at = self.i if at is None else at
        op = next((p for p in REDIRECT_OPS if s.startswith(p, at)), None)
        if op is None:
            return None
        self.i = at + len(op)
        while self.i < len(s) and s[self.i] in " \t":
            self.i += 1
        if op in ("<<", "<<-"):
            delim, _ = self.word()
            self.pending.append((delim.text if delim.text is not None else delim.raw, op == "<<-"))
            return Redirect(fd, op, delim)
        target, _ = self.word()
        return Redirect(fd, op, target)

    def read_heredoc_bodies(self):
        s = self.s
        for delim, strip_tabs in self.pending:
            while self.i < len(s):
                end = s.find("\n", self.i)
                end = len(s) if end < 0 else end
                line = s[self.i:end]
                self.i = end + 1
                if (line.lstrip("\t") if strip_tabs else line) == delim:
                    break
        self.pending = []

    def word(self):
        """One word from here: (Word, [nested token lists from $(...) and backticks])."""
        s, start = self.s, self.i
        text, literal, subs = [], True, []
        while self.i < len(s):
            c = s[self.i]
            if c in WORD_END:
                break
            if c == "\\":
                if s.startswith("\\\n", self.i):
                    self.i += 2
                    continue
                text.append(s[self.i + 1:self.i + 2])
                self.i += 2
            elif c == "'":
                end = s.find("'", self.i + 1)
                end = len(s) if end < 0 else end
                text.append(s[self.i + 1:end])
                self.i = end + 1
            elif c == '"':
                self.i += 1
                while self.i < len(s) and s[self.i] != '"':
                    d = s[self.i]
                    if d == "\\" and self.i + 1 < len(s):
                        nxt = s[self.i + 1]
                        if nxt == "\n":
                            self.i += 2
                            continue
                        text.append(nxt if nxt in '$`"\\' else "\\" + nxt)
                        self.i += 2
                    elif d in "$`":
                        literal = False
                        subs.extend(self.expansion())
                    else:
                        text.append(d)
                        self.i += 1
                self.i += 1
            elif c in "$`":
                literal = False
                subs.extend(self.expansion())
            elif c in "*?[":
                literal = False
                text.append(c)
                self.i += 1
            elif c == "~" and self.i == start:
                literal = literal and (self.i + 1 >= len(s) or s[self.i + 1] in "/" or s[self.i + 1] in WORD_END)
                text.append(os.path.expanduser("~") if literal else c)
                self.i += 1
            else:
                text.append(c)
                self.i += 1
        return Word(s[start:self.i], "".join(text) if literal else None), subs

    def expansion(self):
        """Skips `$name`, `${...}`, `$((...))`, `$(...)` or a backtick quote; returns the token
        lists of the command substitutions it holds."""
        s = self.s
        if s[self.i] == "`":
            self.i += 1
            return [self.tokens("`")]
        if s.startswith("$((", self.i):
            depth, self.i = 0, self.i + 1
            while self.i < len(s):
                depth += {"(": 1, ")": -1}.get(s[self.i], 0)
                self.i += 1
                if depth == 0:
                    break
            return []
        if s.startswith("$(", self.i):
            self.i += 2
            return [self.tokens(")")]
        if s.startswith("${", self.i):
            end = s.find("}", self.i)
            self.i = len(s) if end < 0 else end + 1
            return []
        self.i += 1
        while self.i < len(s) and (s[self.i].isalnum() or s[self.i] == "_"):
            self.i += 1
        if self.i < len(s) and s[self.i] in "?#@*!$-0123456789" and s[self.i - 1] == "$":
            self.i += 1
        return []


RESERVED = {"if", "then", "elif", "else", "fi", "while", "until", "do", "done", "!", "{", "}", "time"}
ASSIGNMENT = re.compile(r"^[A-Za-z_]\w*=")
# Wrappers that run the next word as a command, with the options that take an argument.
WRAPPERS = {"env": {"-u", "-C", "-S"}, "sudo": {"-u", "-g", "-C", "-h", "-p", "-U", "-r", "-t", "-D"},
            "command": set(), "exec": {"-a"}, "nohup": set(), "nice": {"-n"}, "builtin": set(),
            "timeout": {"-s", "-k"}, "stdbuf": {"-i", "-o", "-e"}}


def command_words(words):
    """The command name and its arguments, past assignments, reserved words and wrappers."""
    k = 0
    while k < len(words):
        w = words[k]
        if ASSIGNMENT.match(w.raw) or w.text in RESERVED:
            k += 1
            continue
        name = os.path.basename(w.text) if w.text else None
        if name in WRAPPERS:
            k += 1
            while k < len(words) and words[k].text and (words[k].text.startswith("-") or
                                                        (name == "env" and ASSIGNMENT.match(words[k].raw))):
                k += 2 if words[k].text in WRAPPERS[name] else 1
            if name == "timeout" and k < len(words):
                k += 1  # the duration
            continue
        return name, words[k + 1:]
    return None, []


def resolve(cwd, word):
    """Absolute path of a literal word, or None when it cannot be known."""
    text = word.text
    if cwd is None or not text or text == "-" or text.startswith("/dev/") or text.isdigit():
        return None
    return os.path.normpath(os.path.join(cwd, text))


def change_dir(cwd, args):
    """The directory after `cd args`, or None when it cannot be known (or the cd would fail)."""
    rest = [a for a in args if a.text not in ("-L", "-P", "-e", "-@", "--")]
    if not rest:
        target = os.path.expanduser("~")
    elif rest[0].text in (None, "-"):
        return None
    else:
        target = resolve(cwd, rest[0])
    return target if target and os.path.isdir(target) else None


def commands(tokens, cwd):
    """Simple commands with the directory each runs in. A `cd` lasts until the end of its
    subshell, and has no effect in a pipeline or in the background, as in bash."""
    out, current, in_pipeline = [], Command(cwd), False

    def finish(sep):
        nonlocal cwd, current, in_pipeline
        moved, new_cwd = False, None
        if current.words or current.redirects:
            out.append(current)
            name, args = command_words(current.words)
            if name in ("cd", "pushd"):
                moved, new_cwd = True, change_dir(current.cwd, args)
            elif name == "popd":
                moved = True
        piped = sep in ("|", "|&")
        # Only a command alone in the foreground changes the shell's directory.
        if moved and not piped and not in_pipeline and sep != "&":
            cwd = new_cwd
        in_pipeline = piped
        current = Command(cwd)

    for kind, value in tokens:
        if kind == "word":
            if value.text == "case":
                cwd = None  # `case` patterns end in a bare `)`: stop following directories
            current.words.append(value)
        elif kind == "redir":
            current.redirects.append(value)
        elif kind == "sub":
            out.extend(commands(value, current.cwd))
        else:
            finish(value)
    finish(None)
    return out


# --- the files that matter ----------------------------------------------------------------------

def temp_roots():
    roots = {os.environ.get("TMPDIR", ""), "/tmp", "/private/tmp", "/var/folders", "/private/var/folders"}
    roots = {r for r in roots if r}
    return {p.rstrip("/") for r in roots for p in (r, os.path.realpath(r))}


def is_temp(path):
    return any(path == r or path.startswith(r + "/") for r in temp_roots())


class Files:
    """Answers "would writing this path replace a file git tracks?" from the disk as it is."""

    def __init__(self):
        self.cache = {}

    def in_index(self, real):
        if real not in self.cache:
            try:
                r = subprocess.run(["git", "-C", os.path.dirname(real), "ls-files", "--error-unmatch", "--",
                                    os.path.basename(real)], capture_output=True, timeout=3)
                self.cache[real] = r.returncode == 0
            except (OSError, subprocess.SubprocessError):
                self.cache[real] = False
        return self.cache[real]

    def is_tracked(self, path):
        if not os.path.isfile(path):
            return False
        real = os.path.realpath(path)
        return not is_temp(real) and self.in_index(real)


# --- rules --------------------------------------------------------------------------------------

def flags_and_operands(args, takes_arg, program_flags):
    """For a sed/perl command line: (in place?, program given?, operands). `takes_arg` are the
    single-letter options whose argument follows (attached or as the next word); a program
    flag (`-e`) also marks the program as given."""
    in_place, program, operands, k = False, False, [], 0
    while k < len(args):
        t = args[k].text
        if t is None or t == "-" or not t.startswith("-"):
            operands.append(args[k])
            k += 1
            continue
        if t == "--":
            operands.extend(args[k + 1:])
            break
        if t.startswith("--"):
            name = t[2:].split("=", 1)[0]
            in_place |= name == "in-place"
            program |= name in ("expression", "file")
            k += 2 if name in ("expression", "file") and "=" not in t else 1
            continue
        j, skip_next = 1, False
        while j < len(t):
            c = t[j]
            if c == "i":
                in_place = True
                # BSD sed: `-i ''` (the suffix as the next word); GNU and perl: `-i.bak` attached.
                if j == len(t) - 1 and k + 1 < len(args) and args[k + 1].text == "":
                    skip_next = True
                break
            if c in takes_arg:
                program |= c in program_flags
                skip_next = j == len(t) - 1
                break
            j += 1
        k += 2 if skip_next else 1
    return in_place, program, operands


def in_place_files(name, args):
    """The files a `sed -i` or `perl -i` command edits in place, or [] when it is not in place."""
    if name in ("sed", "gsed"):
        in_place, program, operands = flags_and_operands(args, takes_arg="efl", program_flags="ef")
    elif name == "perl":
        in_place, program, operands = flags_and_operands(args, takes_arg="eEMmIdDxC", program_flags="eE")
    else:
        return []
    if not in_place:
        return []
    # Without -e/-f, the first operand is the program (sed) or the script file (perl).
    return operands if program else operands[1:]


def sed_in_place(cmds, files):
    for cmd in cmds:
        name, args = command_words(cmd.words)
        for target in in_place_files(name, args):
            path = resolve(cmd.cwd, target)
            if path and files.is_tracked(path):
                return (
                    f"Blocked by a hook: `{name} -i` edits {target.text} in place and does nothing silently "
                    "when the pattern does not match. Use rpatch instead: it fails with line numbers when "
                    "an edit does not match, and edits many places and files at once "
                    "(`rpatch --help`; `<<<<<<< OLD part` covers part of a line)."
                )
    return None


def overwritten(name, args, redirects):
    """The words naming files this command replaces outright, as (word, how it was written)."""
    out = []
    if name in ("cat", "echo", "printf"):
        for r in redirects:
            replaces = (r.op == ">" and r.fd in ("", "1")) or r.op == "&>" or \
                (r.op == ">&" and r.fd in ("", "1") and not (r.target.text or "").isdigit() and r.target.text != "-")
            if replaces:
                out.append((r.target, f"{name} {r.fd}{r.op} {r.target.text}"))
    elif name == "tee":
        flags = [a.text for a in args if a.text and a.text.startswith("-")]
        if not any(f == "--append" or (not f.startswith("--") and "a" in f[1:]) for f in flags):
            out.extend((a, f"tee {a.text}") for a in args if not (a.text or "").startswith("-"))
    return out


def overwrite_tracked(cmds, files):
    for cmd in cmds:
        name, args = command_words(cmd.words)
        for target, how in overwritten(name, args, cmd.redirects):
            path = resolve(cmd.cwd, target)
            if path and files.is_tracked(path):
                return (
                    f"Blocked by a hook: `{how}` would replace all of {target.text}, a file git tracks, without "
                    "checking what is in it now. To change part of it, use rpatch "
                    "(`rpatch --help`). To rewrite the whole file on purpose, read it first and "
                    f"write it with the Write tool, or say so with `>|` (`cat >| {target.text} <<'EOF'`)."
                )
    return None


RULES = [("sed-in-place", sed_in_place), ("overwrite-tracked", overwrite_tracked)]


def tag(rule):
    return f" [edit-guard: {rule}]"


def decide(command, cwd, files):
    """(rule, reason) for the first rule that blocks the command, or None."""
    reason = python_replace(command)
    if reason:
        return "python-replace", reason + tag("python-replace")
    cmds = commands(Lexer(command).tokens(), cwd)
    for rule, check in RULES:
        reason = check(cmds, files)
        if reason:
            return rule, reason + tag(rule)
    return None


def main():
    try:
        if os.environ.get("RPATCH_GUARD") == "off":
            return
        data = json.load(sys.stdin)
        command = (data.get("tool_input") or {}).get("command") or ""
        verdict = decide(command, data.get("cwd") or os.getcwd(), Files())
        if verdict:
            print(json.dumps({"hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": verdict[1],
            }}))
    except Exception:
        return


if __name__ == "__main__":
    main()
