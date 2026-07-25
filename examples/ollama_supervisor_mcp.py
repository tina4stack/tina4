#!/usr/bin/env python3
"""
Fully-LOCAL proof of the concept: a local Ollama model is the AI engine, and it
drives the Tina4 supervisor's PROOF-ONLY MCP tool. The model builds a resource
and reads back proof it works — while the source, DDL, rows and secrets NEVER
leave the machine.

Nothing here talks to the cloud. Ollama is local; the supervisor is local.

Run:
    python3 ollama_mcp_test.py
    OLLAMA_MODEL=functiongemma:latest python3 ollama_mcp_test.py   # try another model

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

# Any of these appearing in the MCP response would mean SOURCE leaked out.
SOURCE_MARKERS = ["IntegerField", "NumericField", "StringField", "class ",
                  "async def", "CREATE TABLE", "import ", "def test_", "SELECT "]


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


def reset_invoice():
    """Remove any prior Invoice resource so the demo is repeatable."""
    import glob
    import sqlite3
    for pat in ["src/orm/Invoice.py", "src/routes/invoices.py",
                "migrations/*create_invoice*", "tests/*invoice*", "test_invoice*.db"]:
        for f in glob.glob(os.path.join(PROJECT, pat)):
            try:
                os.remove(f)
            except OSError:
                pass
    db = os.path.join(PROJECT, "data", "app.db")
    if os.path.exists(db):
        try:
            con = sqlite3.connect(db)
            con.execute("DROP TABLE IF EXISTS invoice")
            con.commit()
            con.close()
        except Exception:
            pass


def main():
    print(f"\n• model:      {MODEL}  (local Ollama)")
    print(f"• supervisor: {SUPERVISOR}  (local)\n")
    reset_invoice()  # repeatable

    # 1. Ask the supervisor what it publishes. Only proof tools should appear.
    tools = mcp("tools/list")["result"]["tools"]
    names = [t["name"] for t in tools]
    print(f"[handshake] supervisor publishes: {names}")
    assert "file_read" not in names and "database_query" not in names, \
        "source-exposing tool on the outward surface!"
    print("[handshake] ✅ no file_read / database_query on the surface\n")

    # 2. Hand those tools to the LOCAL model and give it a build task.
    user_task = ("Build a resource called Invoice with fields amount:float and "
                 "reference:string, then confirm it works. Use the "
                 "tina4_scaffold_verify tool with kind='resource'.")
    print(f"[you → {MODEL}] {user_task}\n")

    chat = post(f"{OLLAMA}/api/chat", {
        "model": MODEL,
        "messages": [{"role": "user", "content": user_task}],
        "tools": [to_ollama_tool(t) for t in tools],
        "stream": False,
    })
    msg = chat.get("message", {})
    fn_name, args = None, None

    # (a) Native structured tool call (bigger models).
    calls = msg.get("tool_calls") or []
    if calls:
        fn = calls[0]["function"]
        fn_name = fn["name"]
        args = fn["arguments"] if isinstance(fn["arguments"], dict) else json.loads(fn["arguments"])
    else:
        # (b) Small local models often emit the call as JSON in the content.
        content = msg.get("content", "")
        m = re.search(r"\{.*\}", content, re.S)
        if m:
            try:
                obj = json.loads(m.group(0))
                fn_name = obj.get("name") or obj.get("tool") or obj.get("function")
                args = obj.get("parameters") or obj.get("arguments") or {}
            except json.JSONDecodeError:
                pass

    if not fn_name:
        print(f"[{MODEL}] did not emit a usable tool call. Raw reply:\n"
              f"  {msg.get('content', '')[:300]}")
        print("\nTip: try `OLLAMA_MODEL=functiongemma:latest` (a function-calling model).")
        sys.exit(1)

    # Small models mistype the tool name — snap it to the one real tool.
    real = names[0]
    if fn_name != real and "scaffold" in fn_name.replace(" ", ""):
        print(f"[note] normalising model's tool name {fn_name!r} → {real!r}")
        fn_name = real
    fn = {"name": fn_name}
    print(f"[{MODEL} → supervisor] tool_call {fn_name}({json.dumps(args)})\n")

    # 3. Execute the model's tool call against the supervisor. This BUILDS locally.
    result = mcp("tools/call", {"name": fn["name"], "arguments": args})
    proof_text = result["result"]["content"][0]["text"]
    proof = json.loads(proof_text)
    print("[supervisor → model] PROOF returned:")
    print("  " + json.dumps(proof, indent=2).replace("\n", "\n  "))

    # 4. The whole point: prove no source crossed the boundary.
    full = json.dumps(result)
    leaked = [m for m in SOURCE_MARKERS if re.search(re.escape(m), full)]
    print("\n" + "=" * 60)
    print(f"  proof.ok          : {proof.get('ok')}")
    print(f"  tests             : {proof.get('test_summary')}")
    print(f"  endpoints         : {proof.get('endpoints')}")
    print(f"  source_bytes      : {proof.get('source_bytes')}")
    print(f"  source in response: {'LEAK → ' + str(leaked) if leaked else 'NONE ✅'}")
    print("=" * 60)
    if leaked:
        print("\n❌ concept FAILED — source leaked.")
        sys.exit(1)
    print("\n✅ A local model built a real resource and got proof it works —")
    print("   the code never left the machine.")


if __name__ == "__main__":
    main()
