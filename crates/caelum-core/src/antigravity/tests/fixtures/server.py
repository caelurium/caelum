#!/usr/bin/env python3
import json
import os
import sys
import threading
import time

if "--print" in sys.argv and "/usage" in sys.argv:
    print(json.dumps({
        "conversation_id": "",
        "status": "SUCCESS",
        "num_turns": 0,
        "command": {
            "name": "usage",
            "data": {
                "description": "Shared weekly and 5-hour limits.",
                "groups": [
                    {
                        "name": "Gemini Models",
                        "description": "Gemini 4 Neon, Gemini 4 Hydrogen",
                        "buckets": [
                            {
                                "id": "gemini-weekly",
                                "name": "Weekly Limit Remaining",
                                "description": "Refreshes in 5 days.",
                                "window": "weekly",
                                "remaining_fraction": 0.4,
                                "reset_time": "2026-11-28T17:16:29Z",
                            }
                        ],
                    }
                ],
            },
        },
    }))
    sys.exit(0)

if "--print" in sys.argv:
    root_dir = os.path.dirname(os.path.abspath(__file__))
    if os.path.exists(os.path.join(root_dir, "title-error")):
        sys.stderr.write("title model unavailable\n")
        sys.exit(1)
    idx = sys.argv.index("--print")
    prompt = sys.argv[idx + 1] if idx + 1 < len(sys.argv) else ""
    with open(os.path.join(root_dir, "title-requests.jsonl"), "a", encoding="utf-8") as f:
        f.write(json.dumps({"prompt": prompt, "cwd": os.getcwd(), "args": sys.argv[1:]}) + "\n")
    print(json.dumps({
        "conversation_id": "title-gen",
        "status": "SUCCESS",
        "response": "\"Regenerated Antigravity Title.\"\n",
    }))
    sys.exit(0)

STATE_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fake-agy-state.json")


def load_state():
    if os.path.exists(STATE_FILE):
        with open(STATE_FILE, "r", encoding="utf-8") as f:
            data = json.load(f)
            return {
                "sessions": data.get("sessions", {}),
                "counter": data.get("counter", 0),
                "pending_permissions": {},
            }
    return {"sessions": {}, "counter": 0, "pending_permissions": {}}


def save_state():
    with open(STATE_FILE, "w", encoding="utf-8") as f:
        json.dump(
            {"sessions": state["sessions"], "counter": state["counter"]},
            f,
        )


state = load_state()
write_lock = threading.Lock()


def send(obj):
    with write_lock:
        sys.stdout.write(json.dumps(obj) + "\n")
        sys.stdout.flush()


def notify(session_id, update):
    send({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {"sessionId": session_id, "update": update},
    })


def session_payload(session_id, model="gemini-4-neon", effort="high", mode="default"):
    return {
        "sessionId": session_id,
        "models": {
            "currentModelId": f"{model}-{effort}",
            "availableModels": [
                {"modelId": "gemini-4-neon-low", "name": "Gemini 4 Neon (Low)"},
                {"modelId": "gemini-4-neon-medium", "name": "Gemini 4 Neon (Medium)"},
                {"modelId": "gemini-4-neon-high", "name": "Gemini 4 Neon (High)"},
                {"modelId": "claude-opus-5-5-medium", "name": "Claude Opus 5.5 (Medium)"},
                {"modelId": "claude-opus-5-5-high", "name": "Claude Opus 5.5 (High)"},
            ],
        },
        "modes": {
            "currentModeId": mode,
            "availableModes": [
                {"id": "default", "name": "Default", "description": "Ask before edits"},
                {"id": "plan", "name": "Plan", "description": "Plan before executing"},
            ],
        },
        "configOptions": [
            {
                "id": "model",
                "name": "Model",
                "category": "model",
                "type": "select",
                "currentValue": model,
                "options": [
                    {"value": "gemini-4-neon", "name": "Gemini 4 Neon"},
                    {"value": "claude-opus-5-5", "name": "Claude Opus 5.5"},
                ],
            },
            {
                "id": "thought_level",
                "name": "Thinking Level",
                "category": "thought_level",
                "type": "select",
                "currentValue": effort,
                "options": [
                    {"value": "low", "name": "Low"},
                    {"value": "medium", "name": "Medium"},
                    {"value": "high", "name": "High"},
                ],
            },
            {
                "id": "mode",
                "name": "Mode",
                "category": "mode",
                "type": "select",
                "currentValue": mode,
                "options": [
                    {"value": "default", "name": "Default"},
                    {"value": "plan", "name": "Plan"},
                ],
            },
        ],
        "_meta": {
            "antigravity": {
                "tier": {
                    "name": "Google AI Pro",
                    "id": "g1-pro-tier",
                    "upgradeUri": "https://antigravity.google/g1-upgrade",
                }
            }
        },
    }


def run_prompt(req_id, session_id, blocks):
    text = " ".join(b.get("text", "") for b in blocks if b.get("type") == "text").strip()
    sess = state["sessions"][session_id]
    sess["history"].append({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": text}})

    if "ask-me" in text:
        q_input = {
            "questions": [
                {
                    "question": "Which color do you prefer?",
                    "options": ["Red", "Blue"],
                    "is_multi_select": False,
                }
            ]
        }
        q_start = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-ask",
            "name": "ask_question",
            "kind": "other",
            "title": "Run ask_question",
            "status": "in_progress",
            "rawInput": q_input,
        }
        q_pending = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-ask",
            "name": "ask_question",
            "kind": "other",
            "title": "Run ask_question",
            "status": "pending",
            "rawInput": q_input,
        }
        q_msg = {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "\nWhich color do you prefer?\n- [1] Red\n- [2] Blue\n"},
        }
        for u in (q_start, q_pending, q_msg):
            notify(session_id, u)
            sess["history"].append(u)
        sess["pending_question"] = "call-ask"
        save_state()
        send({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}})
        return

    if sess.pop("pending_question", None) == "call-ask":
        q_done = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-ask",
            "name": "ask_question",
            "status": "completed",
            "rawOutput": {"output": text},
        }
        notify(session_id, q_done)
        sess["history"].append(q_done)
        reply = f"answered:{text}"
    elif "background-task" in text:
        bg_start = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-bg",
            "name": "run_command",
            "kind": "execute",
            "title": "make check",
            "status": "in_progress",
            "rawInput": {"CommandLine": "make check", "WaitMsBeforeAsync": 500},
        }
        yield_msg = {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Running make check. I'll update you once it finishes."},
        }
        yield_usage = {"sessionUpdate": "usage_update", "used": 1000, "size": 1048576}
        notify(session_id, bg_start)
        notify(session_id, yield_msg)
        notify(session_id, yield_usage)
        sess["history"].extend([bg_start, yield_msg, yield_usage])
        save_state()
        send({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}})

        time.sleep(0.3)
        bg_done = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-bg",
            "status": "completed",
            "rawOutput": {"output": "Command exited with code 0.\n\nCommand output:\nok"},
        }
        notify(session_id, bg_done)
        sess["history"].append(bg_done)

        followup_usage = {"sessionUpdate": "usage_update", "used": 1200, "size": 1048576}
        followup_start = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-followup",
            "name": "run_command",
            "kind": "execute",
            "title": "echo verified",
            "status": "in_progress",
            "rawInput": {"CommandLine": "echo verified"},
        }
        followup_done = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-followup",
            "status": "completed",
            "rawOutput": {"output": "Command exited with code 0.\n\nCommand output:\nverified"},
        }
        final_msg = {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "\n\nAll checks passed."},
        }
        final_usage = {"sessionUpdate": "usage_update", "used": 1400, "size": 1048576}
        for u in (followup_usage, followup_start, followup_done, final_msg, final_usage):
            notify(session_id, u)
            sess["history"].append(u)
        save_state()
        return

    elif "need-permission" in text:
        perm_id = f"perm-{req_id}"
        ev = threading.Event()
        box = {}
        state["pending_permissions"][perm_id] = (ev, box)
        send({
            "jsonrpc": "2.0",
            "id": perm_id,
            "method": "session/request_permission",
            "params": {
                "sessionId": session_id,
                "toolCall": {
                    "toolCallId": "call-perm",
                    "name": "run_command",
                    "title": "Run shell command",
                    "rawInput": {"command": "echo hi"},
                },
                "options": [
                    {"optionId": "allow_always", "name": "Always allow", "kind": "allow_always"},
                    {"optionId": "allow_once", "name": "Allow once", "kind": "allow_once"},
                    {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"},
                ],
            },
        })
        ev.wait(timeout=10)
        outcome = box.get("result", {}).get("outcome", {})
        reply = f"permission:{outcome.get('optionId', outcome.get('outcome', 'unknown'))}"
    else:
        thought = {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Checking files."}}
        cwd = sess["cwd"]
        read_start = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-read",
            "name": "read_file",
            "kind": "read",
            "title": "Read README.md",
            "status": "in_progress",
            "rawInput": {"Path": f"{cwd}/README.md", "StartLine": 1, "EndLine": 5},
        }
        read_done = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-read",
            "status": "completed",
            "rawOutput": {
                "output": "Created At: 2026-11-22T01:00:00Z\nCompleted At: 2026-11-22T01:00:01Z\nShowing lines 1-2 of 2\n1: # Caelum\n2: Hello"
            },
        }
        edit_start = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-edit",
            "name": "edit_file",
            "kind": "edit",
            "title": "File src/lib.rs",
            "status": "in_progress",
            "rawInput": {
                "Path": f"{cwd}/src/lib.rs",
                "Target": "fn old() {}\n",
                "Replacement": "fn new() {\n    println!(\"ok\");\n}\n",
            },
        }
        edit_done = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-edit",
            "status": "completed",
            "content": [{
                "type": "diff",
                "path": f"{cwd}/src/lib.rs",
                "oldText": "fn old() {}\n",
                "newText": "fn new() {\n    println!(\"ok\");\n}\n",
            }],
        }
        write_done = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-write",
            "name": "write_file",
            "kind": "edit",
            "title": "File notes.md",
            "status": "completed",
            "rawInput": {"Path": "notes.md", "Content": "line 1\nline 2\n", "Overwrite": True},
        }
        run_done = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-run",
            "name": "run_command",
            "kind": "execute",
            "title": "cargo test",
            "status": "completed",
            "rawInput": {"CommandLine": "cargo test", "toolSummary": "Run tests"},
            "rawOutput": {
                "output": "Created At: 2026-11-22T01:00:00Z\nCompleted At: 2026-11-22T01:00:01Z\nCommand exited with code 0.\n\nCommand output:\ntest result: ok."
            },
        }
        subagent_done = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-subagent",
            "name": "invoke_subagent",
            "kind": "other",
            "title": "Run invoke_subagent",
            "status": "completed",
            "rawInput": {
                "TypeName": "research",
                "Role": "Codebase Researcher",
                "Prompt": "Inspect lib.rs",
            },
            "rawOutput": {"output": "Found 1 symbol"},
        }
        updates = [thought, read_start, read_done, edit_start, edit_done, write_done, run_done, subagent_done]
        for u in updates:
            notify(session_id, u)
        sess["history"].extend(updates)
        reply = f"echo:{sess['model']}:{sess['effort']}:{sess['mode']}:{text}"

    msg = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}}
    usage = {"sessionUpdate": "usage_update", "used": 1234, "size": 1048576}
    notify(session_id, msg)
    notify(session_id, usage)
    sess["history"].extend([msg, usage])
    save_state()
    send({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}})


for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    if "method" not in msg and "id" in msg:
        perm_id = str(msg["id"])
        if perm_id in state["pending_permissions"]:
            ev, box = state["pending_permissions"].pop(perm_id)
            box["result"] = msg.get("result", {})
            ev.set()
        continue

    method = msg.get("method")
    req_id = msg.get("id")
    params = msg.get("params") or {}

    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"list": {}, "load": {}, "close": {}}},
                "agentInfo": {"name": "Fake Antigravity ACP", "version": "1.0.0"},
            },
        })
    elif method == "session/new":
        state["counter"] += 1
        sid = f"acp-sess-{state['counter']}"
        state["sessions"][sid] = {
            "cwd": params.get("cwd", ""),
            "title": "New session",
            "updatedAt": "2026-11-22T01:00:00Z",
            "model": "gemini-4-neon",
            "effort": "high",
            "mode": "default",
            "history": [],
        }
        save_state()
        send({"jsonrpc": "2.0", "id": req_id, "result": session_payload(sid)})
        notify(sid, {
            "sessionUpdate": "available_commands_update",
            "availableCommands": [
                {"name": "plan", "description": "Plan before executing"},
                {"name": "compact", "description": "Compact conversation"},
            ],
        })
    elif method == "session/list":
        cwd = params.get("cwd", "")
        sessions = [
            {
                "sessionId": sid,
                "cwd": info["cwd"],
                "title": info["title"],
                "updatedAt": info["updatedAt"],
            }
            for sid, info in state["sessions"].items()
            if not cwd or info["cwd"] == cwd
        ]
        send({"jsonrpc": "2.0", "id": req_id, "result": {"sessions": sessions}})
    elif method == "session/load":
        sid = params["sessionId"]
        info = state["sessions"].get(sid)
        if info is None:
            send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32002, "message": f"Session not found: {sid}"}})
            continue
        if info.pop("pending_question", None) == "call-ask":
            info["history"].append({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-ask",
                "name": "ask_question",
                "status": "failed",
                "rawOutput": {"error": "Tool execution was canceled"},
            })
            save_state()
        for update in info["history"]:
            notify(sid, update)
        send({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": session_payload(sid, info["model"], info["effort"], info["mode"]),
        })
    elif method == "session/close":
        state["sessions"].pop(params.get("sessionId"), None)
        save_state()
        send({"jsonrpc": "2.0", "id": req_id, "result": {}})
    elif method == "session/set_config_option":
        sid = params["sessionId"]
        if params.get("configId") == "model":
            state["sessions"][sid]["model"] = params["value"]
        elif params.get("configId") == "thought_level":
            state["sessions"][sid]["effort"] = params["value"]
        save_state()
        send({"jsonrpc": "2.0", "id": req_id, "result": {}})
    elif method == "session/set_mode":
        sid = params["sessionId"]
        state["sessions"][sid]["mode"] = params["modeId"]
        save_state()
        send({"jsonrpc": "2.0", "id": req_id, "result": {}})
    elif method == "session/prompt":
        sid = params["sessionId"]
        blocks = params.get("prompt", [])
        threading.Thread(target=run_prompt, args=(req_id, sid, blocks), daemon=True).start()
    elif method == "session/cancel":
        pass
    elif req_id is not None:
        send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": f"Unknown method {method}"}})
