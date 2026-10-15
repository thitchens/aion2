#!/usr/bin/env python3
"""Join Wikily monster spawn places onto the bundled pets catalog.

Usage:
  python3 scripts/enrich_spawn_areas.py
"""

from __future__ import annotations

import json
import re
import urllib.request
from collections import OrderedDict
from pathlib import Path

UA = {"User-Agent": "Mozilla/5.0 (compatible; aion2-pet-tracker/1.0)"}
ZONES = [
    "Chaotic Lower Reshanta",
    "Verteron",
    "Altgard",
    "Poeta",
    "Ishalgen",
]
ZONE_SET = set(ZONES)
REGION_SLUGS = {
    "verteron": "Verteron",
    "altgard": "Altgard",
    "poeta": "Poeta",
    "ishalgen": "Ishalgen",
    "chaotic-lower-reshanta": "Chaotic Lower Reshanta",
}

ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = ROOT / "data" / "pets.json"


def fetch_all(slug: str, page_size: int = 120) -> list[dict]:
    rows: list[dict] = []
    offset = 0
    seen: set[str] = set()
    while True:
        url = (
            f"https://wikily.gg/api/wikis/aion-2/datasets/{slug}/list"
            f"?limit={page_size}&offset={offset}"
        )
        req = urllib.request.Request(url, headers=UA)
        with urllib.request.urlopen(req, timeout=90) as resp:
            payload = json.load(resp)
        batch = payload.get("rows") or []
        total = payload.get("totalCount") or 0
        new = 0
        for row in batch:
            key = row["key"]
            if key in seen:
                continue
            seen.add(key)
            rows.append(row)
            new += 1
        print(f"{slug}: offset={offset} +{new} → {len(rows)}/{total}")
        if not batch or new == 0:
            break
        offset += len(batch)
        if len(rows) >= total:
            break
    return rows


def zone_from_text(text: str) -> str | None:
    for zone in ZONES:
        if zone in text:
            return zone
    return None


def split_areas(blob: str) -> list[str]:
    """Split 'A, B and C' style place lists into individual area names."""
    blob = blob.strip()
    blob = re.sub(r",?\s*\d+ more places$", "", blob)
    if not blob:
        return []
    parts = re.split(r",\s*|\s+and\s+", blob)
    out = []
    for part in parts:
        part = part.strip().removeprefix("and ").strip()
        if not part or part in ZONE_SET:
            continue
        if part.startswith("Quest Dungeon"):
            continue
        out.append(part)
    return out


def parse_places(data: dict) -> list[tuple[str, str]]:
    found: list[tuple[str, str]] = []

    for place in data.get("places") or []:
        if not isinstance(place, dict):
            continue
        title = (place.get("title") or "").strip()
        subtitle = (place.get("subtitle") or "").strip()
        after = subtitle.split("·", 1)[1].strip() if "·" in subtitle else ""
        href = place.get("href") or ""

        if title in ZONE_SET:
            for area in split_areas(after):
                found.append((title, area))
            continue

        zone = zone_from_text(after) or zone_from_text(subtitle)
        if not zone:
            match = re.search(r"/regions/([a-z0-9-]+)", href)
            if match:
                zone = REGION_SLUGS.get(match.group(1))
        if zone and title:
            found.append((zone, title))

    if not found:
        lead = data.get("lead") or ""
        for match in re.finditer(
            r" in (.+?) in (Verteron|Altgard|Poeta|Ishalgen|Chaotic Lower Reshanta)\b",
            lead,
        ):
            zone = match.group(2)
            for area in split_areas(match.group(1)):
                found.append((zone, area))

    seen: set[tuple[str, str]] = set()
    unique: list[tuple[str, str]] = []
    for item in found:
        if item in seen:
            continue
        seen.add(item)
        unique.append(item)
    return unique


def enrich(catalog: dict, monsters: list[dict]) -> dict:
    by_name: dict[str, dict] = {}
    for row in monsters:
        data = row["data"]
        name = data.get("name") or row["key"]
        entry = {"areas": parse_places(data)}
        by_name[name] = entry
        by_name[name.lower()] = entry

    matched = 0
    rows = 0
    for pet in catalog["pets"]:
        grouped: OrderedDict[tuple[str, str], dict] = OrderedDict()
        for mob in pet.get("source_mobs") or []:
            info = by_name.get(mob) or by_name.get(mob.lower())
            if not info:
                continue
            for zone, area in info["areas"]:
                key = (zone, area)
                if key not in grouped:
                    grouped[key] = {
                        "zone": zone,
                        "area": area,
                        "source_mobs": [],
                    }
                if mob not in grouped[key]["source_mobs"]:
                    grouped[key]["source_mobs"].append(mob)
        spawn_areas = sorted(grouped.values(), key=lambda x: (x["zone"], x["area"]))
        pet["spawn_areas"] = spawn_areas
        if spawn_areas:
            matched += 1
            rows += len(spawn_areas)

    catalog["version"] = max(int(catalog.get("version") or 0), 3)
    attr = catalog.get("attribution") or ""
    if "spawn areas" not in attr.lower():
        catalog["attribution"] = (
            attr.rstrip(".")
            + ". Spawn areas joined from the Wikily monsters dataset."
        )
    print(f"pets with spawn_areas: {matched}/{len(catalog['pets'])} ({rows} rows)")
    return catalog


def main() -> None:
    monsters = fetch_all("monsters")
    catalog = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))
    enrich(catalog, monsters)
    CATALOG_PATH.write_text(
        json.dumps(catalog, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {CATALOG_PATH}")


if __name__ == "__main__":
    main()
