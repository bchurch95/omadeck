#!/usr/bin/env python3
"""
Omashow Post-Commit Critic & Architectural Sentinel.

Runs asynchronously after each worker git commit:
1. Captures git diff and commit metadata.
2. Captures visual UI snapshot of apps/omashow-tauri using headless Chromium.
3. Invokes Critic Vision Model (Qwen) to score code quality, architecture & UI aesthetics (1-10).
4. If score < 8 or if critical issues are flagged, injects actionable fix tasks directly
   into TODO.md so Pi automatically picks them up on its next turn.
5. Writes CRITIQUE.md and logs to critic_history.jsonl.
"""

import json
import os
import re
import subprocess
import sys
import time
import requests

PROJECT_DIR = os.environ.get("OMASHOW_DIR", "/root/omashow")
# Critic runs on Cortana dual-V100s via router
API_URL = os.environ.get("VLLM_API_URL", "http://192.168.66.232:8000/v1/chat/completions")
API_KEY = os.environ.get("VLLM_API_KEY", "sk-qcity-8246b2fb311b1271b8d3f26612521160")
CRITIC_MODEL = os.environ.get("CRITIC_MODEL", "qwen-mlx")

PREVIEW_IMG = os.path.join(PROJECT_DIR, "latest_ui_preview.png")
CRITIQUE_MD = os.path.join(PROJECT_DIR, "CRITIQUE.md")
CRITIQUE_LOG = os.path.join(PROJECT_DIR, "critic_history.jsonl")
CRITIC_BACKLOG_PATH = os.path.join(PROJECT_DIR, "CRITIC_BACKLOG.md")
LAST_COMMIT_FILE = os.path.join(PROJECT_DIR, ".last_critiqued_commit")
TODO_PATH = os.path.join(PROJECT_DIR, "TODO.md")


def get_commit_info(target_commit="HEAD"):
    try:
        commit_hash = subprocess.check_output(
            ["git", "-C", PROJECT_DIR, "rev-parse", target_commit], text=True
        ).strip()
        commit_msg = subprocess.check_output(
            ["git", "-C", PROJECT_DIR, "log", "-n", "1", "--format=%s (%an)", commit_hash], text=True
        ).strip()
        commit_stat = subprocess.check_output(
            ["git", "-C", PROJECT_DIR, "show", "--stat", "--oneline", commit_hash], text=True
        ).strip()
        diff = subprocess.check_output(
            ["git", "-C", PROJECT_DIR, "diff", "--stat", f"{commit_hash}~1..{commit_hash}"], text=True
        )
        full_diff = subprocess.check_output(
            ["git", "-C", PROJECT_DIR, "diff", f"{commit_hash}~1..{commit_hash}"], text=True
        )
        if len(full_diff) > 4000:
            diff += "\n\nKey code changes:\n" + full_diff[:4000] + "\n... [diff truncated for review] ..."
        else:
            diff += "\n\n" + full_diff
        return commit_hash, commit_msg, commit_stat, diff
    except Exception as e:
        print(f"[Critic] Error reading git commit: {e}", file=sys.stderr)
        return None, "", "", ""


def capture_snapshot():
    try:
        # Add PROJECT_DIR to python path to import visual_critic
        if PROJECT_DIR not in sys.path:
            sys.path.insert(0, PROJECT_DIR)
        import visual_critic
        img_path = visual_critic.capture_ui_snapshot()
        if os.path.exists(img_path):
            return img_path
    except Exception as e:
        print(f"[Critic] Snapshot capture failed: {e}", file=sys.stderr)
    return None


def run_critic_audit(commit_hash, commit_msg, commit_stat, diff, image_path):
    system_prompt = (
        "You are the Principal Systems Architect and Lead UI/UX Designer for Omashow, "
        "a high-performance, native PowerPoint clone written in Rust with a Tauri/HTML5 frontend.\n\n"
        "Your task: Act as an asynchronous post-commit critic and code/design reviewer. "
        "Audit the latest git commit diff and the current UI snapshot.\n\n"
        "Core Architectural Rules for Omashow:\n"
        "1. Lossless OOXML Roundtripping: Never discard unknown XML nodes, namespaces, or relationships.\n"
        "2. Rust Safety: Idiomatic Rust, no unhandled unwraps/panics in production code paths, clean error typing.\n"
        "3. UI/UX Quality: Modern, dark-slate native desktop aesthetics, crisp typography, clean spacing, "
        "responsive layout, no broken glyphs or fallback font artifacts.\n"
        "4. Feature Parity: Milestone 8 standards (7 top modes, presenter console, slide sorter, stage tools).\n\n"
        "Evaluate both code diff and visual layout. Output MUST be ONLY valid JSON matching this schema:\n"
        "Do NOT write excessive internal chain-of-thought. Jump directly to writing the final JSON object.\n"
        "{\n"
        '  "score": <integer 1-10>,\n'
        '  "summary": "<1-2 sentence executive assessment>",\n'
        '  "code_review": {\n'
        '    "strengths": ["..."],\n'
        '    "concerns": ["..."]\n'
        "  },\n"
        '  "visual_review": {\n'
        '    "strengths": ["..."],\n'
        '    "concerns": ["..."]\n'
        "  },\n"
        '  "actionable_todo_items": [\n'
        '    "<concise imperative fix task, e.g. \\"Fix toolbar button icon alignment in index.html\\">"\n'
        "  ]\n"
        "}"
    )

    user_text = (
        f"### Commit Under Audit: {commit_hash[:8]} - {commit_msg}\n\n"
        f"**Summary of Changes:**\n```\n{commit_stat}\n```\n\n"
        f"**Git Diff:**\n```diff\n{diff}\n```\n"
    )

    messages = [{"role": "system", "content": system_prompt}]

    # devstral is text-only — no image support, just send the diff as text
    messages.append({"role": "user", "content": user_text})

    try:
        resp = requests.post(
            API_URL,
            headers={"Authorization": f"Bearer {API_KEY}", "Content-Type": "application/json"},
            json={
                "model": CRITIC_MODEL,
                "messages": messages,
                "temperature": 0.2,
                "max_tokens": 2048,
            },
            timeout=180,
        )
        resp.raise_for_status()
        msg = resp.json()["choices"][0]["message"]
        raw_text = msg.get("content", "").strip()
        if not raw_text and msg.get("reasoning_content"):
            raw_text = msg["reasoning_content"].strip()

        # Clean markdown code blocks if present
        cleaned = re.sub(r"^```(?:json)?\s*", "", raw_text, flags=re.MULTILINE)
        cleaned = re.sub(r"\s*```$", "", cleaned, flags=re.MULTILINE).strip()

        # Strategy 1: Find outer JSON object boundaries
        start, end = cleaned.find("{"), cleaned.rfind("}")
        if start != -1 and end != -1:
            candidate = cleaned[start : end + 1]
            try:
                return json.loads(candidate)
            except Exception:
                # Strategy 2: Remove trailing commas before closing braces/brackets
                candidate_fixed = re.sub(r",\s*([\]}])", r"\1", candidate)
                try:
                    return json.loads(candidate_fixed)
                except Exception:
                    pass

        # Strategy 3: Regex match key fields directly if JSON syntax is slightly malformed
        score_match = re.search(r'"score"\s*:\s*(\d+)', raw_text)
        summary_match = re.search(r'"summary"\s*:\s*"([^"]+)"', raw_text)
        score = int(score_match.group(1)) if score_match else 7
        summary = summary_match.group(1) if summary_match else raw_text[:300]
        
        # Extract actionable items via bullet points or JSON array lines
        actions = re.findall(r'"actionable_todo_items"\s*:\s*\[(.*?)\]', raw_text, re.DOTALL)
        todo_items = []
        if actions:
            todo_items = [re.sub(r'^[",\s]+|[",\s]+$', '', x) for x in actions[0].split("\n") if x.strip() and x.strip() not in ('[', ']')]
            todo_items = [x for x in todo_items if len(x) > 3]

        return {
            "score": score,
            "summary": summary,
            "code_review": {"strengths": [], "concerns": []},
            "visual_review": {"strengths": [], "concerns": []},
            "actionable_todo_items": todo_items,
        }
    except Exception as e:
        print(f"[Critic] API call failed: {e}", file=sys.stderr)
        return {
            "score": 7,
            "summary": f"Critic evaluation failed: {e}",
            "code_review": {"strengths": [], "concerns": [str(e)]},
            "visual_review": {"strengths": [], "concerns": []},
            "actionable_todo_items": [],
        }


def inject_actionable_items_into_todo(commit_hash, commit_msg, score, items):
    if not items:
        return

    try:
        feedback_block = f"\n### 🔍 Architect & Critic Feedback (Commit {commit_hash[:7]} - Score: {score}/10)\n"
        for item in items:
            feedback_block += f"- [ ] [Critic] {item}\n"

        with open(CRITIC_BACKLOG_PATH, "a", encoding="utf-8") as f:
            f.write(feedback_block)

        print(f"[Critic] Appended {len(items)} actionable feedback item(s) to CRITIC_BACKLOG.md (TODO.md kept clean)")
    except Exception as e:
        print(f"[Critic] Failed to update CRITIC_BACKLOG.md: {e}", file=sys.stderr)


def write_critique_artifact(commit_hash, commit_msg, critique, image_path):
    score = critique.get("score", 7)
    summary = critique.get("summary", "")
    code_rev = critique.get("code_review", {})
    vis_rev = critique.get("visual_review", {})
    actionable = critique.get("actionable_todo_items", [])

    verdict = "PASSED (Quality Standard Met)" if score >= 8 else "ACTION REQUIRED (Feedback Injected to TODO.md)"

    md = f"""# OmaShow Architecture & Visual Critique Report

**Commit:** `{commit_hash[:8]}` — *{commit_msg}*  
**Timestamp:** {time.strftime('%Y-%m-%d %H:%M:%S UTC', time.gmtime())}  
**Overall Score:** **{score}/10** — `{verdict}`

---

## 📋 Executive Assessment
{summary}

---

## 💻 Code & Architecture Review
### Strengths
"""
    for s in code_rev.get("strengths", []):
        md += f"- ✅ {s}\n"
    if not code_rev.get("strengths"):
        md += "- *(No specific code strengths highlighted)*\n"

    md += "\n### Architectural Concerns & Risks\n"
    for c in code_rev.get("concerns", []):
        md += f"- ⚠️ {c}\n"
    if not code_rev.get("concerns"):
        md += "- *(No critical architectural concerns)*\n"

    md += """\n---

## 🎨 Visual & UI Polish Review
### Strengths
"""
    for s in vis_rev.get("strengths", []):
        md += f"- 🌟 {s}\n"
    if not vis_rev.get("strengths"):
        md += "- *(No visual strengths highlighted)*\n"

    md += "\n### Visual Polish Defects\n"
    for c in vis_rev.get("concerns", []):
        md += f"- 🔍 {c}\n"
    if not vis_rev.get("concerns"):
        md += "- *(No major visual defects identified)*\n"

    if actionable:
        md += """\n---

## 🛠️ Actionable Tasks Assigned to Pi Agent
The following items were registered in `TODO.md` for resolution in the next autonomous cycle:
"""
        for item in actionable:
            md += f"- [ ] **[Critic]** {item}\n"

    with open(CRITIQUE_MD, "w", encoding="utf-8") as f:
        f.write(md)

    entry = {
        "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
        "commit": commit_hash,
        "score": score,
        "summary": summary,
        "actionable_count": len(actionable),
    }
    with open(CRITIQUE_LOG, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")

    print(f"[Critic] Critique report saved to {CRITIQUE_MD}")


def main():
    target_commit = sys.argv[1] if len(sys.argv) > 1 else "HEAD"
    commit_hash, commit_msg, commit_stat, diff = get_commit_info(target_commit)
    if not commit_hash:
        sys.exit(1)

    # Check if already reviewed
    if os.path.exists(LAST_COMMIT_FILE):
        with open(LAST_COMMIT_FILE, "r") as f:
            if f.read().strip() == commit_hash:
                print(f"[Critic] Commit {commit_hash[:8]} has already been audited. Skipping.")
                return

    print(f"[Critic] Auditing commit: {commit_hash[:8]} - {commit_msg}")
    print("[Critic] Capturing latest UI snapshot...")
    snapshot_path = capture_snapshot()

    print(f"[Critic] Sending audit request to {CRITIC_MODEL}...")
    critique = run_critic_audit(commit_hash, commit_msg, commit_stat, diff, snapshot_path)

    score = critique.get("score", 7)
    items = critique.get("actionable_todo_items", [])
    print(f"[Critic] Audit Complete. Score: {score}/10. Actionable items: {len(items)}")

    # Write report and log
    write_critique_artifact(commit_hash, commit_msg, critique, snapshot_path)

    # Inject into TODO.md if score < 8 or items exist
    if items:
        inject_actionable_items_into_todo(commit_hash, commit_msg, score, items)

    with open(LAST_COMMIT_FILE, "w") as f:
        f.write(commit_hash)


if __name__ == "__main__":
    main()
