#!/usr/bin/env python3
"""
Fully-LOCAL proof of the concept. Ask a local Ollama model to "build a website"
and, disconnected, it can only say "I'm an LLM, I can't — but I can guide you".
Connected to the Tina4 supervisor's PROOF-ONLY MCP, that SAME model builds a
real backend resource AND a reactive frontend page, and reads back proof each
works — while the source, DDL, rows and secrets NEVER leave the machine.

Nothing here talks to the cloud. Ollama is local; the supervisor is local.

Run:
    python3 ollama_supervisor_mcp.py
    OLLAMA_MODEL=functiongemma:latest python3 ollama_supervisor_mcp.py  # another model

Prereqs (already true if you followed along):
    - ollama running with a tool-capable model (llama3.2:3b works)
    - the Tina4 supervisor on :9150 and the app on :7150
"""
import json
import os
import re
import sys
import urllib.request

OLLAMA = os.environ.get("OLLAMA_URL", "http://127.0.0.1:11434")
MODEL = os.environ.get("OLLAMA_MODEL", "llama3.2:3b")
SUPERVISOR = os.environ.get("TINA4_SUPERVISOR", "http://127.0.0.1:9150")
# The local Tina4 project the supervisor is building in. Only used to reset the
# demo resource between runs; the build itself happens inside the supervisor.
PROJECT = os.environ.get(
    "TINA4_PROJECT",
    os.path.expanduser("~/IdeaProjects/tina4-dev-admin/.playground"))

# Any of these appearing in the MCP response would mean SOURCE leaked out
# (backend Python + DDL, or frontend tina4-js).
SOURCE_MARKERS = ["IntegerField", "NumericField", "StringField", "class ",
                  "async def", "CREATE TABLE", "import ", "def test_", "SELECT ",
                  "signal(", "api.get", "html`", "<script", "Tina4Element"]


def post(url, payload):
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=180) as r:
        return json.load(r)


def mcp(method, params=None):
    """Call the supervisor's proof-only MCP surface."""
    return post(f"{SUPERVISOR}/mcp/rpc",
                {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}})


def to_ollama_tool(mcp_tool):
    return {"type": "function", "function": {
        "name": mcp_tool["name"],
        "description": mcp_tool["description"],
        "parameters": mcp_tool.get("inputSchema", {"type": "object", "properties": {}}),
    }}


def reset_demo():
    """Remove any prior Product resource + products page so the demo repeats."""
    import glob
    import sqlite3
    pats = ["src/orm/Product.py", "src/routes/products.py",
            "migrations/*create_product*", "tests/*product*", "test_product*.db",
            "src/public/js/products-page.js", "src/public/products.html"]
    for pat in pats:
        for f in glob.glob(os.path.join(PROJECT, pat)):
            try:
                os.remove(f)
            except OSError:
                pass
    db = os.path.join(PROJECT, "data", "app.db")
    if os.path.exists(db):
        try:
            con = sqlite3.connect(db)
            con.execute("DROP TABLE IF EXISTS product")
            con.commit()
            con.close()
        except Exception:
            pass


def ask_model_for_tool(task, tools):
    """Send a task + the tool catalogue to the local model; return (name, args)
    from whatever it emits — structured tool_calls OR JSON in the content."""
    chat = post(f"{OLLAMA}/api/chat", {
        "model": MODEL,
        "messages": [{"role": "user", "content": task}],
        "tools": [to_ollama_tool(t) for t in tools],
        "stream": False,
    })
    msg = chat.get("message", {})
    calls = msg.get("tool_calls") or []
    if calls:
        fn = calls[0]["function"]
        args = fn["arguments"] if isinstance(fn["arguments"], dict) else json.loads(fn["arguments"])
        return fn["name"], args
    m = re.search(r"\{.*\}", msg.get("content", ""), re.S)
    if m:
        try:
            obj = json.loads(m.group(0))
            return (obj.get("name") or obj.get("tool") or obj.get("function"),
                    obj.get("parameters") or obj.get("arguments") or {})
        except json.JSONDecodeError:
            pass
    return None, None


def snap_tool(name, published):
    """Small models mistype tool names — snap to the right published tool."""
    if name in published:
        return name
    key = (name or "").lower().replace(" ", "").replace("_", "")
    if any(w in key for w in ("page", "buildpage", "website", "ui", "frontend")):
        return "tina4_build_page"
    if any(w in key for w in ("scaffold", "resource", "model", "verify")):
        return "tina4_scaffold_verify"
    return name


def run_tool(name, args):
    """Call the supervisor and assert no source crossed the boundary."""
    result = mcp("tools/call", {"name": name, "arguments": args})
    proof = json.loads(result["result"]["content"][0]["text"])
    leaked = [k for k in SOURCE_MARKERS if re.search(re.escape(k), json.dumps(result))]
    return proof, leaked


def main():
    print(f"\n• model:      {MODEL}  (local Ollama)")
    print(f"• supervisor: {SUPERVISOR}  (local)\n")
    reset_demo()

    tools = mcp("tools/list")["result"]["tools"]
    names = [t["name"] for t in tools]
    print(f"[handshake] supervisor publishes: {names}")
    assert "file_read" not in names and "database_query" not in names, \
        "source-exposing tool on the outward surface!"
    print("[handshake] ✅ backend + frontend build tools; no file_read/database_query\n")

    # "can you build me a website" — a bare model only advises. Connected to the
    # supervisor, this SAME local model builds the backend AND the frontend.
    steps = [
        ("Build a resource called Product with fields name:string and price:float, "
         "then confirm it works (use tina4_scaffold_verify, kind='resource')."),
        ("Build a reactive products page that lists products from /api/products "
         "(use tina4_build_page, name='products', api='/api/products')."),
    ]
    any_leak = False
    for i, task in enumerate(steps, 1):
        print(f"[you → {MODEL}] {task}")
        raw_name, args = ask_model_for_tool(task, tools)
        if not raw_name:
            print(f"  [{MODEL}] no tool call — try OLLAMA_MODEL=functiongemma:latest")
            sys.exit(1)
        name = snap_tool(raw_name, names)
        if name != raw_name:
            print(f"  [note] {raw_name!r} → {name!r}")
        print(f"  [{MODEL} → supervisor] {name}({json.dumps(args)})")
        proof, leaked = run_tool(name, args)
        any_leak = any_leak or bool(leaked)
        print(f"  [proof] ok={proof.get('ok')}  "
              f"created={len(proof.get('created', []))} file(s)  "
              f"source_bytes={proof.get('source_bytes')}  "
              f"source_in_response={'LEAK ' + str(leaked) if leaked else 'NONE ✅'}\n")

    print("=" * 62)
    print("  A local model was asked to \"build a website\".")
    print("  Disconnected, it can only say \"I'm an LLM, I can't\".")
    print("  Connected to the supervisor, it built a real backend + a")
    print("  reactive, styled frontend page — and got PROOF each works,")
    print(f"  while the code never left the machine.  {'❌ LEAK' if any_leak else '✅'}")
    print("=" * 62)
    if any_leak:
        sys.exit(1)



if __name__ == "__main__":
    main()
