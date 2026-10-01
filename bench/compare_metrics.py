"""Summarize saved Claude stream-json traces; no access to private session logs.

Usage: python3 bench/compare_metrics.py bench/out/compare-...
Counts are heuristics for shell editors; raw traces remain the source of truth.
"""
import argparse
import collections
import json
import re
import shlex
from pathlib import Path

RPATCH = re.compile(r"(?:^|[;&|(\n])\s*(?:[^\s]*/)?rpatch(?:\s|$)")
EDITORS = {
    "python": re.compile(r"\bpython3?\b[\s\S]*(?:\.replace\(|\bre\.sub\(|write_text\(|\.write\()"),
    "sd": re.compile(r"(?:^|[;&|(\n])\s*sd\s"),
    "other_shell": re.compile(r"\bsed\s+[^\n]*-i|\bperl\s+[^\n]*-[^\s]*i|\b(?:writeFileSync|writeFile)\s*\("),
}


def result_text(block):
    content = block.get("content", "")
    if isinstance(content, list):
        return "\n".join(item.get("text", "") for item in content if isinstance(item, dict))
    return content if isinstance(content, str) else json.dumps(content)


def classify(name, arguments):
    if name == "Bash":
        command = arguments.get("command", "")
        try:
            parts = shlex.split(command)
            if len(parts) == 3 and parts[1] in ("-lc", "-c") and parts[0].rsplit("/", 1)[-1] in ("bash", "zsh", "sh"):
                arguments = {"command": parts[2]}
        except ValueError:
            pass
    if name == "mcp__rpatch__rpatch":
        return "rpatch_mcp", bool(arguments.get("dry_run"))
    if name in ("Edit", "MultiEdit", "Write"):
        return "native_edit", False
    if name != "Bash":
        return None, False
    command = arguments.get("command", "")
    if RPATCH.search(command):
        # Help/version discovery is separate from edits, and must not inflate adoption.
        has_input = "<<" in command or bool(re.search(r"(?:^|\s)<\s*[^<]|\|\s*(?:[^\s]*/)?rpatch\b", command))
        if not has_input:
            return "rpatch_cli_discovery", False
        return "rpatch_cli", bool(re.search(r"\brpatch\b[^\n]*\s(?:-n|--dry-run)\b", command))
    for kind, pattern in EDITORS.items():
        if pattern.search(command):
            return kind, False
    return None, False


def summarize_trace(path):
    events = []
    results = {}
    final = {}
    init = {}
    parse_errors = 0
    seen = set()
    for line in path.read_text().splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            parse_errors += 1
            continue
        # Codex exec has item events rather than Claude message/tool blocks.
        if item.get("type") == "item.completed":
            entry = item.get("item") or {}
            tool_id = entry.get("id")
            category = entry.get("type")
            if category == "mcp_tool_call":
                name = f"mcp__{entry.get('server')}__{entry.get('tool')}"
                kind, dry = classify(name, entry.get("arguments") or {})
                events.append((tool_id, kind, dry))
                # Codex marks an isError result as "failed"; rpatch sends no structured content.
                results[tool_id] = {
                    "is_error": entry.get("status") == "failed" or bool(entry.get("error")),
                    "content": (entry.get("result") or {}).get("content", []),
                }
            elif category == "command_execution":
                kind, dry = classify("Bash", {"command": entry.get("command", "")})
                events.append((tool_id, kind, dry))
                results[tool_id] = {"is_error": entry.get("exit_code") != 0, "content": entry.get("aggregated_output", "")}
            elif category == "file_change":
                events.append((tool_id, "native_edit", False))
                results[tool_id] = {"is_error": entry.get("status") != "completed", "content": ""}
        if item.get("type") == "turn.completed":
            final = {"type": "result", "subtype": "success", "is_error": False, "usage": item.get("usage")}
        elif item.get("type") in ("turn.failed", "error"):
            final = {"type": "result", "subtype": "error", "is_error": True, "terminal_reason": item.get("message") or item.get("error")}
        if item.get("type") == "result":
            final = item
        if item.get("type") == "system" and item.get("subtype") == "init":
            init = item
        message = item.get("message") or {}
        content = message.get("content") or []
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "tool_use" and block.get("id") not in seen:
                seen.add(block.get("id"))
                kind, dry_run = classify(block.get("name"), block.get("input") or {})
                events.append((block.get("id"), kind, dry_run))
            elif block.get("type") == "tool_result":
                results[block.get("tool_use_id")] = block
    counts = collections.Counter()
    for tool_id, kind, dry in events:
        if not kind:
            continue
        counts[kind + ("_dry_run" if dry else "")] += 1
        result = results.get(tool_id)
        if result is None:
            counts[kind + "_missing_result"] += 1
            continue
        failed = bool(result.get("is_error"))
        # Some client versions put an MCP tool error inside the text envelope.
        if kind == "rpatch_mcp":
            try:
                failed |= bool(json.loads(result_text(result)).get("isError"))
            except (json.JSONDecodeError, AttributeError):
                pass
        if failed:
            counts[kind + ("_dry_run" if dry else "") + "_failed"] += 1
    edit_kinds = ("rpatch_cli", "rpatch_mcp", "native_edit", "python", "sd", "other_shell")
    edits = sum(counts[kind] for kind in edit_kinds)
    rpatch_edits = counts["rpatch_cli"] + counts["rpatch_mcp"]
    return {
        "tool_calls": len(events),
        "counts": dict(counts),
        "editing_calls": edits,
        "rpatch_editing_calls": rpatch_edits,
        "rpatch_share": round(rpatch_edits / edits, 4) if edits else None,
        "autonomous_rpatch_use": bool(rpatch_edits),
        "complete": final.get("type") == "result" and not final.get("is_error", False),
        "cost_usd": final.get("total_cost_usd"),
        "num_turns": final.get("num_turns"),
        "result_subtype": final.get("subtype"),
        "terminal_reason": final.get("terminal_reason"),
        "api_error_status": final.get("api_error_status"),
        "usage": final.get("usage"),
        "mcp_servers": init.get("mcp_servers", []),
        "mcp_server_errors": init.get("mcp_server_errors", []),
        "parse_errors": parse_errors,
    }


def summarize(root):
    rows = []
    for path in sorted(root.glob("trial-*/*/*.jsonl")):
        row = {"trial": path.parent.parent.name, "arm": path.parent.name, "prompt": path.stem}
        row.update(summarize_trace(path))
        run = path.with_suffix(".run.json")
        if run.exists():
            row.update(json.loads(run.read_text()))
        row["eligible"] = bool(
            row["complete"] and row.get("runner_exit_code") == 0
            and row.get("verify_exit_code") == 0 and not row["parse_errors"]
        )
        if row["arm"] == "mcp" and row.get("engine") == "codex":
            # required=true makes Codex fail startup if this server cannot connect.
            row["eligible"] &= row.get("required_mcp") is True
        elif row["arm"] == "mcp":
            row["eligible"] &= any(
                server.get("name") == "rpatch" and server.get("status") == "connected"
                for server in row["mcp_servers"]
            ) and not row["mcp_server_errors"]
        row["cli_contamination"] = row["arm"] == "mcp" and bool(
            row["counts"].get("rpatch_cli") or row["counts"].get("rpatch_cli_dry_run")
        )
        row["eligible"] &= not row["cli_contamination"]
        rows.append(row)
    pairs = collections.defaultdict(dict)
    for row in rows:
        pairs[(row["trial"], row["prompt"])][row["arm"]] = row
    paired = [pair for pair in pairs.values() if all(arm in pair and pair[arm]["eligible"] for arm in ("cli", "mcp"))]
    arms = {}
    for arm in ("cli", "mcp"):
        samples = [pair[arm] for pair in paired]
        total = sum(row["editing_calls"] for row in samples)
        used = sum(row["rpatch_editing_calls"] for row in samples)
        arms[arm] = {
            "paired_eligible_runs": len(samples),
            "runs_with_rpatch_use": sum(row["autonomous_rpatch_use"] for row in samples),
            "editing_calls": total,
            "rpatch_editing_calls": used,
            "rpatch_share": round(used / total, 4) if total else None,
        }
    return {"rows": rows, "paired_comparison": arms}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(args.output), ensure_ascii=False, indent=2))
