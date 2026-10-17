#!/usr/bin/env python3
"""Side-by-side comparison of the Dayflow reference harness and a Daytrace replay.

Usage:
  python3 reference/compare.py <harness-out-dir> <daytrace-replay.json> [--width N] [--full]

<harness-out-dir> is the unpacked artifact of .github/workflows/dayflow-reference.yml
(run.json, batches.json, observations.json, cards.json, requests.json).
<daytrace-replay.json> is written by src-tauri/tests/replay.rs when DAYTRACE_REPLAY_JSON is set.

Times are shown as offsets from the first frame (+MM:SS), so runs made at different
clock times and in different time zones line up. Standard library only.
"""

import argparse
import difflib
import json
import os
import sys
from collections import Counter, defaultdict
from itertools import zip_longest

# Claude Haiku 5.5 list price per million tokens (prompts up to 100k tokens).
PRICES = {"claude-haiku-5-5": (0.10, 0.50)}


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def offset(seconds):
    sign = "-" if seconds < 0 else "+"
    seconds = abs(int(seconds))
    return f"{sign}{seconds // 60:02d}:{seconds % 60:02d}"


def clip(text, width):
    text = " ".join(str(text).split())
    return text if len(text) <= width else text[: width - 1] + "…"


def load_dayflow(directory):
    run = load(os.path.join(directory, "run.json"))
    batches = load(os.path.join(directory, "batches.json"))
    observations = load(os.path.join(directory, "observations.json"))
    cards = load(os.path.join(directory, "cards.json"))
    requests = load(os.path.join(directory, "requests.json"))
    calls = []
    for item in requests.get("items", []):
        usage = item.get("usage") or {}
        calls.append(
            {
                "batch_id": item.get("batch_id"),
                "operation": item.get("operation"),
                "input_tokens": usage.get("input_tokens"),
                "output_tokens": usage.get("output_tokens"),
                "error": item.get("error"),
                "images": len(item.get("images") or []),
                "request": item.get("request"),
                "sent": item.get("anthropic_request"),
            }
        )
    return {
        "name": "Dayflow",
        "real": bool(run.get("real_model_output")),
        "mode": run.get("mode"),
        "model": run.get("model"),
        "settings": f"effort={run.get('effort')} temperature_sent={run.get('temperature_sent')}",
        "time_zone": run.get("time_zone"),
        "base_ts": run.get("base_ts"),
        "frames": run.get("frames"),
        "extra": f"dayflow {str(run.get('dayflow_sha') or '?')[:10]}",
        "batches": [
            {
                "id": b.get("id"),
                "start": b["start_ts"] - run["base_ts"],
                "end": b["end_ts"] - run["base_ts"],
                "status": b.get("status"),
                "reason": b.get("reason"),
            }
            for b in batches.get("batches", [])
        ],
        "observations": [
            {"start": o["offset_start_s"], "end": o["offset_end_s"], "text": o["observation"], "batch_id": o.get("batch_id")}
            for o in observations.get("items", [])
        ],
        "cards": [dict(c["card"], start=c["offset_start_s"], end=c["offset_end_s"]) for c in cards.get("final", [])],
        "calls": calls,
        "usage": run.get("usage") or {},
        "notes": run.get("notes") or [],
    }


def load_daytrace(path):
    data = load(path)
    base = data["base_ts"]
    batch_of = {}
    batches = []
    for b in data.get("batches", []):
        batches.append(
            {"id": b["id"], "start": b["start_ts"] - base, "end": b["end_ts"] - base, "status": b.get("status"), "reason": b.get("reason")}
        )
        batch_of[b["id"]] = (b["start_ts"], b["end_ts"])
    observations = []
    for o in data.get("observations", []):
        batch_id = next((k for k, (s, e) in batch_of.items() if s <= o["start_ts"] <= e), None)
        observations.append({"start": o["offset_start_s"], "end": o["offset_end_s"], "text": o["observation"], "batch_id": batch_id})
    return {
        "name": "Daytrace",
        "real": True,
        "mode": "replay",
        "model": data.get("model"),
        "settings": f"pipeline={data.get('pipeline')} reasoning={data.get('reasoning')} batch_api={data.get('batch_api')}",
        "time_zone": data.get("time_zone"),
        "base_ts": base,
        "frames": data.get("frames"),
        "extra": "",
        "batches": batches,
        "observations": observations,
        "cards": [dict(c["card"], start=c["offset_start_s"], end=c["offset_end_s"]) for c in data.get("cards", [])],
        "calls": data.get("requests", []),
        "usage": data.get("usage") or {},
        "notes": [],
    }


def request_view(request):
    """System text, first text part (the prompt), other text parts, image count and parameters."""
    if isinstance(request, str):
        try:
            request = json.loads(request)
        except ValueError:
            return None
    if not isinstance(request, dict):
        return None

    def text_of(content):
        if isinstance(content, str):
            return content
        return "\n\n".join(p.get("text", "") for p in content or [] if p.get("type") == "text")

    system = request.get("system")
    system = text_of(system) if system is not None else None
    parts = []
    for message in request.get("messages", []):
        if message.get("role") == "system":
            system = text_of(message.get("content"))
        elif message.get("role") == "user":
            content = message.get("content")
            parts = [{"type": "text", "text": content}] if isinstance(content, str) else content or []
    texts = [p.get("text", "") for p in parts if p.get("type") == "text"]
    keys = ("max_tokens", "max_completion_tokens", "temperature", "output_config", "thinking", "reasoning")
    return {
        "system": system,
        "prompt": texts[0] if texts else "",
        "labels": texts[1:],
        "images": sum(1 for p in parts if p.get("type") in ("image", "image_url")),
        "params": {k: request[k] for k in keys if k in request},
    }


def first_request(side, operation):
    for call in side["calls"]:
        if call.get("operation") == operation and call.get("request") is not None:
            view = request_view(call["request"])
            sent = request_view(call.get("sent")) if call.get("sent") else None
            if view and sent:
                view["params"] = sent["params"]  # what actually went over the wire (live mode)
            return view
    return None


def print_prompt_diff(a, b, names, width):
    if a == b:
        print("    identical")
        return
    for line in difflib.unified_diff(a.splitlines(), b.splitlines(), names[0], names[1], lineterm="", n=0):
        print("    " + clip(line, width))


def print_prompts(sides, width):
    print("PROMPTS (first request of each operation)")
    names = [s["name"] for s in sides]
    for operation in ("transcribe_screenshots", "generate_cards"):
        views = [first_request(s, operation) for s in sides]
        if not all(views):
            missing = [n for n, v in zip(names, views) if not v]
            print(f"  {operation}: no stored request on {', '.join(missing)}")
            continue
        a, b = views
        print(f"  {operation}:")
        print(f"    system message {'identical' if a['system'] == b['system'] else 'differs'}: {clip(a['system'], 60)!r}")
        print(f"    images {names[0]}={a['images']} {names[1]}={b['images']}; "
              f"labels {'identical' if a['labels'] == b['labels'] else 'differ'}")
        print(f"    parameters {names[0]}={json.dumps(a['params'])} {names[1]}={json.dumps(b['params'])}")
        prompt_a, prompt_b = a["prompt"], b["prompt"]
        if operation == "generate_cards":
            # previous cards and observations differ by nature; compare the instructions after them
            marker = "</observations>"
            prompt_a = prompt_a[prompt_a.find(marker):] if marker in prompt_a else prompt_a
            prompt_b = prompt_b[prompt_b.find(marker):] if marker in prompt_b else prompt_b
            print("    prompt after </observations>:")
        else:
            print("    prompt:")
        print_prompt_diff(prompt_a, prompt_b, names, width)
    print()


def minute_categories(cards, last_minute):
    """Category per minute after the first frame (None where no card covers it)."""
    result = [None] * last_minute
    for card in cards:
        if card.get("category") == "System":
            continue
        for minute in range(max(0, card["start"] // 60), min(last_minute, -(-card["end"] // 60))):
            result[minute] = card.get("category")
    return result


def print_header(sides):
    print("RUNS")
    for side in sides:
        label = "" if side["real"] else "   << DRY RUN: canned answers, not model output >>"
        print(
            f"  {side['name']:<9} mode={side['mode']} model={side['model']} {side['settings']} "
            f"tz={side['time_zone']} frames={side['frames']} {side['extra']}{label}"
        )
        for note in side["notes"]:
            print(f"            note: {note}")
    print()


def print_batches(sides):
    print("BATCHES")
    for side in sides:
        parts = [
            f"#{b['id']} {offset(b['start'])}..{offset(b['end'])} {b['status']}" + (f" ({clip(b['reason'], 60)})" if b.get("reason") else "")
            for b in side["batches"]
        ]
        print(f"  {side['name']:<9} {len(side['batches'])}: " + "; ".join(parts))
    print()


def print_observations(sides, width, full):
    print("OBSERVATIONS")
    for side in sides:
        per_batch = Counter(o["batch_id"] for o in side["observations"])
        lengths = [len(o["text"]) for o in side["observations"]] or [0]
        spans = [o["end"] - o["start"] for o in side["observations"]] or [0]
        print(
            f"  {side['name']:<9} total={len(side['observations'])} per batch="
            + ", ".join(f"#{k}: {v}" for k, v in sorted(per_batch.items(), key=lambda kv: (kv[0] is None, kv[0])))
            + f"  mean length={sum(lengths) // len(lengths)} chars  mean span={sum(spans) // len(spans)} s"
        )
    for side in sides:
        print(f"  -- {side['name']}")
        for o in side["observations"]:
            text = o["text"] if full else clip(o["text"], width)
            print(f"    {offset(o['start'])}-{offset(o['end'])}  {text}")
    print()


def print_cards(sides, width):
    print("CARDS (offset from first frame, clock string as stored, category, title)")
    left, right = sides
    col = max(40, width // 2)

    def cell(card):
        if card is None:
            return ""
        return clip(
            f"{offset(card['start'])}-{offset(card['end'])} {card.get('startTime')}-{card.get('endTime')} "
            f"[{card.get('category')}] {card.get('title')}",
            col,
        )

    print(f"  {left['name']:<{col}} | {right['name']}")
    print(f"  {'-' * col}-+-{'-' * col}")
    for a, b in zip_longest(left["cards"], right["cards"]):
        print(f"  {cell(a):<{col}} | {cell(b)}")
    last = max([c["end"] for s in sides for c in s["cards"]] + [0])
    last_minute = -(-last // 60)
    if last_minute:
        a = minute_categories(left["cards"], last_minute)
        b = minute_categories(right["cards"], last_minute)
        both = [(x, y) for x, y in zip(a, b) if x is not None and y is not None]
        agree = sum(1 for x, y in both if x == y)
        print(
            f"  minute coverage: {left['name']} {sum(x is not None for x in a)}/{last_minute}, "
            f"{right['name']} {sum(y is not None for y in b)}/{last_minute}; "
            f"category agreement where both cover: {agree}/{len(both)}"
        )
        boundaries_a = sorted({c["start"] for c in left["cards"]} | {c["end"] for c in left["cards"]})
        boundaries_b = sorted({c["start"] for c in right["cards"]} | {c["end"] for c in right["cards"]})
        print(f"  card boundaries: {left['name']} {[offset(x) for x in boundaries_a]}")
        print(f"                   {right['name']} {[offset(x) for x in boundaries_b]}")
    for side in sides:
        print(f"  -- {side['name']} summaries")
        for card in side["cards"]:
            sites = card.get("appSites") or {}
            print(f"    {offset(card['start'])}-{offset(card['end'])} {card.get('title')}")
            print(f"        {clip(card.get('summary'), width)}")
            print(
                f"        subcategory={card.get('subcategory')!r} sites={sites.get('primary')}/{sites.get('secondary')} "
                f"distractions={len(card.get('distractions') or [])} detailed lines={len((card.get('detailedSummary') or '').splitlines())}"
            )
    print()


def print_usage(sides):
    print("TOKEN USAGE")
    for side in sides:
        usage = side["usage"]
        total_in = usage.get("input_tokens") or 0
        total_out = usage.get("output_tokens") or 0
        by_op = defaultdict(lambda: [0, 0, 0, 0])
        for call in side["calls"]:
            entry = by_op[call.get("operation")]
            entry[0] += 1
            entry[1] += call.get("input_tokens") or 0
            entry[2] += call.get("output_tokens") or 0
            entry[3] += 1 if call.get("error") else 0
        price = PRICES.get(side["model"])
        cost = f"  ≈ ${(total_in * price[0] + total_out * price[1]) / 1e6:.4f}" if price and side["real"] else ""
        print(f"  {side['name']:<9} requests={len(side['calls'])} input={total_in} output={total_out}{cost}")
        for op, (n, i, o, errors) in sorted(by_op.items(), key=lambda kv: str(kv[0])):
            print(f"            {op}: {n} calls, in={i} out={o}" + (f", errors={errors}" if errors else ""))
    print()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("harness_dir")
    parser.add_argument("daytrace_json")
    parser.add_argument("--width", type=int, default=150, help="line width for clipped text")
    parser.add_argument("--full", action="store_true", help="print observation texts in full")
    args = parser.parse_args()
    sides = [load_dayflow(args.harness_dir), load_daytrace(args.daytrace_json)]
    print_header(sides)
    print_batches(sides)
    print_observations(sides, args.width, args.full)
    print_cards(sides, args.width)
    print_prompts(sides, args.width)
    print_usage(sides)
    if not sides[0]["real"]:
        print("NOTE: the Dayflow side is a dry run; its observations and cards are canned placeholders.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
