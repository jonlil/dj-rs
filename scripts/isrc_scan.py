#!/usr/bin/env python3
"""
Scan Rekordbox library for missing ISRCs via AcoustID + MusicBrainz.
Caches AcoustID and MusicBrainz Recording IDs in file tags to avoid
repeat lookups. Writes found ISRCs to the Rekordbox database and file tags.

Usage:
    python3 scripts/isrc_scan.py
    ACOUSTID_KEY=<key> python3 scripts/isrc_scan.py

    The AcoustID key is read from ~/.config/dj-rs/config.json (acoustid_api_key)
    or the ACOUSTID_KEY environment variable (env takes priority).

Pipeline:
    1. Query DB for all tracks with a file type (includes tracks that already have ISRC)
    2. Skip track if AcoustID is already cached in file tags AND ISRC already in DB (fully enriched)
    3. Check file tags for cached Acoustid Id / MusicBrainz Recording Id — skip fpcalc if MBID present
    4. Run fpcalc to generate chromaprint fingerprint
    5. Look up fingerprint on AcoustID API → get candidate MusicBrainz Recording IDs
    6. For each candidate (top 3): fetch MusicBrainz recording, run match_confidence()
    7. Write AcoustID + MBID (if confident) to file tags; write ISRC to file + DB only if not already set

Confidence matching (match_confidence):
    Four signals are combined with fixed weights:
        AcoustID score   0.35  (fingerprint quality from AcoustID API)
        Title similarity 0.35  (normalized SequenceMatcher ratio)
        Artist similarity 0.20
        Length proximity  0.10  (ratio of shorter/longer duration)

    Titles and artists are normalised before comparison: feat., remix, edit,
    bootleg, version, radio, extended are stripped, as is punctuation. This
    handles imperfect DJ library metadata.

    Verdicts:
        accept  >= 0.72   store MBID + ISRC
        review  >= 0.50   store ISRC only  (MBID omitted — not confident enough)
        reject  <  0.50   store nothing    (only AcoustID fingerprint ID cached)

    Rationale for the MBID threshold: AcoustID is crowd-sourced, so a
    fingerprint can be linked to the wrong MusicBrainz recording by human
    error. Storing a wrong MBID would poison future lookups. ISRCs on review
    matches are usually correct (the recording is right, just the metadata
    confidence is lower), so they are still written.

Tag fields written per format:
    MP3 / WAV / AIFF:   TSRC (ISRC), TXXX:Acoustid Id, TXXX:MusicBrainz Recording Id
    FLAC:               ISRC, ACOUSTID_ID, MUSICBRAINZ_TRACKID
    M4A / AAC:          ----:com.apple.iTunes:ISRC
                        ----:com.apple.iTunes:Acoustid Id
                        ----:com.apple.iTunes:MusicBrainz Track Id

Dependencies:
    mutagen  (pip install mutagen)
    fpcalc   (chromaprint — install via package manager)
    sqlcipher (package manager)
"""

import json
import os
import re
import subprocess
import time
import sys
from difflib import SequenceMatcher
from pathlib import Path
import urllib.request
import urllib.parse

DB_PATH   = Path.home() / ".local/share/dj-rs/master.db"
DB_KEY    = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497"
PATH_FROM = "/home/jonas/Projects/jonlil/dj-rs/dj_jonas/music"
PATH_TO   = str(Path.home() / "Music")
LIMIT     = 1000
MB_DELAY  = 1.1  # MusicBrainz rate limit: 1 req/sec


def load_config():
    p = Path.home() / ".config/dj-rs/config.json"
    try:
        return json.loads(p.read_text())
    except Exception:
        return {}

CONFIG       = load_config()
ACOUSTID_KEY = os.environ.get("ACOUSTID_KEY") or CONFIG.get("acoustid_api_key", "")


def _normalize(s):
    """Strip feat/remix/edit noise and punctuation for loose comparison."""
    s = s.lower()
    s = re.sub(r"\(feat\.?.*?\)", "", s)
    s = re.sub(r"\bfeat\.?\s+\S+", "", s)
    s = re.sub(r"\b(remix|edit|mix|bootleg|recut|version|radio|extended|original)\b", "", s)
    s = re.sub(r"[^\w\s]", "", s)
    s = re.sub(r"\s+", " ", s).strip()
    return s


def _sim(a, b):
    return SequenceMatcher(None, _normalize(a), _normalize(b)).ratio()


def match_confidence(rb_title, rb_artist, rb_length_secs,
                     mb_title, mb_artist, mb_length_ms, acoustid_score):
    """
    Return (confidence 0-1, verdict) combining four signals:
      - AcoustID score (fingerprint quality)
      - Title similarity (normalized)
      - Artist similarity (normalized)
      - Length proximity (±10% tolerance, ignored if either is 0)
    Verdict: 'accept', 'review', or 'reject'
    """
    title_sim  = _sim(rb_title, mb_title)
    artist_sim = _sim(rb_artist or "", mb_artist or "")

    if rb_length_secs and mb_length_ms:
        mb_secs = mb_length_ms / 1000
        length_ratio = min(rb_length_secs, mb_secs) / max(rb_length_secs, mb_secs)
    else:
        length_ratio = 0.8  # unknown — neutral

    # Weighted combination
    confidence = (
        acoustid_score * 0.35 +
        title_sim      * 0.35 +
        artist_sim     * 0.20 +
        length_ratio   * 0.10
    )

    if confidence >= 0.72:
        verdict = "accept"
    elif confidence >= 0.50:
        verdict = "review"
    else:
        verdict = "reject"

    return round(confidence, 2), verdict


def map_path(p):
    if p.startswith(PATH_FROM):
        return PATH_TO + p[len(PATH_FROM):]
    return p


def get_tracks(limit):
    query = (
        f"PRAGMA key='{DB_KEY}';"
        f"SELECT c.ID, c.Title, c.FolderPath, c.FileNameL, a.Name, c.Length, c.ISRC"
        f" FROM djmdContent c LEFT JOIN djmdArtist a ON c.ArtistID = a.ID"
        f" WHERE c.FileType IS NOT NULL LIMIT {limit};"
    )
    r = subprocess.run(["sqlcipher", str(DB_PATH)], input=query, capture_output=True, text=True)
    rows = []
    for line in r.stdout.splitlines():
        if line == "ok":
            continue
        parts = line.split("|")
        if len(parts) == 7:
            rows.append((parts[0], parts[1], parts[2], parts[3], parts[4],
                         int(parts[5]) if parts[5].isdigit() else 0,
                         parts[6] or None))
    return rows


def _read_id3_ids(tags):
    """Extract AcoustID and MBID from an ID3 tag object (MP3, WAV, AIFF)."""
    if not tags:
        return None, None
    aid  = next((f.text[0] for f in tags.getall("TXXX") if f.desc == "Acoustid Id"), None)
    mbid = next((f.text[0] for f in tags.getall("TXXX") if f.desc == "MusicBrainz Recording Id"), None)
    return aid, mbid


def _write_id3_ids(tags, acoustid=None, mbid=None, isrc=None):
    """Write AcoustID, MBID, and/or ISRC into an ID3 tag object."""
    from mutagen.id3 import TXXX, TSRC
    if acoustid:
        tags.delall("TXXX:Acoustid Id")
        tags.add(TXXX(encoding=3, desc="Acoustid Id", text=acoustid))
    if mbid:
        tags.delall("TXXX:MusicBrainz Recording Id")
        tags.add(TXXX(encoding=3, desc="MusicBrainz Recording Id", text=mbid))
    if isrc:
        tags.delall("TSRC")
        tags.add(TSRC(encoding=3, text=isrc))


def read_cached_ids(filepath):
    """Read AcoustID and MusicBrainz Recording ID already stored in file tags."""
    try:
        ext = Path(filepath).suffix.lower()
        if ext == ".mp3":
            from mutagen.mp3 import MP3
            return _read_id3_ids(MP3(filepath).tags)
        elif ext == ".flac":
            from mutagen.flac import FLAC
            audio = FLAC(filepath)
            return audio.get("ACOUSTID_ID", [None])[0], audio.get("MUSICBRAINZ_TRACKID", [None])[0]
        elif ext in (".m4a", ".aac", ".m4p"):
            from mutagen.mp4 import MP4
            audio = MP4(filepath)
            aid  = audio.tags.get("----:com.apple.iTunes:Acoustid Id")
            mbid = audio.tags.get("----:com.apple.iTunes:MusicBrainz Track Id")
            # iTunes freeform values are bytes
            aid  = aid[0].decode()  if aid  else None
            mbid = mbid[0].decode() if mbid else None
            return aid, mbid
        elif ext in (".wav", ".aif", ".aiff"):
            from mutagen.wave import WAVE
            from mutagen.aiff import AIFF
            audio = WAVE(filepath) if ext == ".wav" else AIFF(filepath)
            return _read_id3_ids(audio.tags)
    except Exception:
        pass
    return None, None


def write_ids_to_file(filepath, acoustid=None, mbid=None, isrc=None):
    """Write AcoustID, MusicBrainz Recording ID, and/or ISRC to file tags."""
    try:
        ext = Path(filepath).suffix.lower()
        if ext == ".mp3":
            from mutagen.mp3 import MP3
            audio = MP3(filepath)
            if audio.tags is None:
                audio.add_tags()
            _write_id3_ids(audio.tags, acoustid, mbid, isrc)
            audio.save()
            return True
        elif ext == ".flac":
            from mutagen.flac import FLAC
            audio = FLAC(filepath)
            if acoustid: audio["ACOUSTID_ID"] = acoustid
            if mbid:     audio["MUSICBRAINZ_TRACKID"] = mbid
            if isrc:     audio["ISRC"] = isrc
            audio.save()
            return True
        elif ext in (".m4a", ".aac", ".m4p"):
            from mutagen.mp4 import MP4, MP4FreeForm
            audio = MP4(filepath)
            if audio.tags is None:
                audio.add_tags()
            if acoustid:
                audio.tags["----:com.apple.iTunes:Acoustid Id"] = [MP4FreeForm(acoustid.encode())]
            if mbid:
                audio.tags["----:com.apple.iTunes:MusicBrainz Track Id"] = [MP4FreeForm(mbid.encode())]
            if isrc:
                audio.tags["----:com.apple.iTunes:ISRC"] = [MP4FreeForm(isrc.encode())]
            audio.save()
            return True
        elif ext in (".wav", ".aif", ".aiff"):
            from mutagen.wave import WAVE
            from mutagen.aiff import AIFF
            audio = WAVE(filepath) if ext == ".wav" else AIFF(filepath)
            if audio.tags is None:
                audio.add_tags()
            _write_id3_ids(audio.tags, acoustid, mbid, isrc)
            audio.save()
            return True
    except Exception:
        pass
    return False


def fingerprint(path):
    try:
        r = subprocess.run(["fpcalc", "-json", path], capture_output=True, text=True, timeout=30)
        if r.returncode != 0:
            return None, None
        d = json.loads(r.stdout)
        return int(d["duration"]), d["fingerprint"]
    except Exception:
        return None, None


def acoustid_lookup_by_id(acoustid_id):
    """Look up recordings linked to a known AcoustID without re-fingerprinting."""
    data = urllib.parse.urlencode({
        "client": ACOUSTID_KEY,
        "trackid": acoustid_id,
        "meta": "recordings isrcs",
    }).encode()
    req = urllib.request.Request("https://api.acoustid.org/v2/lookup", data=data)
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.load(r)


def acoustid_lookup(duration, fp):
    data = urllib.parse.urlencode({
        "client": ACOUSTID_KEY,
        "duration": duration,
        "fingerprint": fp,
        "meta": "recordings isrcs",
    }).encode()
    req = urllib.request.Request("https://api.acoustid.org/v2/lookup", data=data)
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.load(r)


def mb_lookup(mbid):
    url = f"https://musicbrainz.org/ws/2/recording/{mbid}?inc=isrcs+artists&fmt=json"
    req = urllib.request.Request(url, headers={"User-Agent": "dj-rs/0.1.0 (jonas)"})
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.load(r)


def write_isrc_to_db(track_id, isrc):
    query = (
        f"PRAGMA key='{DB_KEY}';"
        f"UPDATE djmdContent SET ISRC = '{isrc}' WHERE ID = {track_id};"
    )
    r = subprocess.run(["sqlcipher", str(DB_PATH)], input=query, capture_output=True, text=True)
    return r.returncode == 0


def main():
    tracks = get_tracks(LIMIT)
    print(f"Scanning {len(tracks)} tracks...\n")

    matched_with_isrc = []
    matched_no_isrc   = []
    no_match          = []
    not_found         = []

    needs_review = []

    for i, (tid, title, folder, filename, rb_artist, rb_length, db_isrc) in enumerate(tracks):
        filepath = map_path(folder)
        sys.stdout.write(f"[{i+1}/{len(tracks)}] {title[:50]:<50} ")
        sys.stdout.flush()

        if not Path(filepath).exists():
            print("FILE NOT FOUND")
            not_found.append({"id": tid, "title": title, "path": filepath})
            continue

        # --- Check cache first ---
        cached_aid, cached_mbid = read_cached_ids(filepath)

        # Skip if already fully enriched (AcoustID cached in file + ISRC in DB)
        if cached_aid and db_isrc:
            print(f"SKIP (already enriched)")
            continue

        # AcoustID cached but no MBID — skip fingerprint, re-check for new MB contributions
        if cached_aid and not cached_mbid and not db_isrc:
            sys.stdout.write(f"AID-RECHECK ")
            sys.stdout.flush()
            try:
                result = acoustid_lookup_by_id(cached_aid)
            except Exception as e:
                print(f"ACOUSTID ERROR: {e}")
                continue
            results = result.get("results", [])
            if not results or not results[0].get("recordings"):
                print("NO RECORDINGS LINKED")
                continue
            best      = results[0]
            aid_score = best.get("score", 1.0)
            recordings = best["recordings"]
            found_isrc = None
            found_mbid = None
            best_conf  = 0.0
            best_verdict = "reject"
            for rec in recordings[:3]:
                mbid = rec["id"]
                try:
                    mb = mb_lookup(mbid)
                    time.sleep(MB_DELAY)
                except Exception:
                    time.sleep(MB_DELAY)
                    continue
                mb_title  = mb.get("title", "")
                mb_artist = " ".join(c["artist"]["name"] for c in mb.get("artist-credit", []))
                mb_length = mb.get("length") or 0
                conf, verdict = match_confidence(title, rb_artist, rb_length, mb_title, mb_artist, mb_length, aid_score)
                if verdict == "reject":
                    continue
                isrcs = mb.get("isrcs", [])
                if conf > best_conf:
                    best_conf    = conf
                    best_verdict = verdict
                    found_mbid   = mbid if verdict == "accept" else None
                    if isrcs:
                        found_isrc = isrcs[0]
            if not found_mbid and not found_isrc:
                print(f"STILL NO MATCH")
                needs_review.append({"id": tid, "title": title, "rb_artist": rb_artist, "acoustid": cached_aid})
                continue
            effective_isrc = found_isrc or db_isrc
            if effective_isrc:
                print(f"NEW ISRC: {effective_isrc} (conf={best_conf})")
                matched_with_isrc.append({"id": tid, "title": title, "isrc": effective_isrc,
                                          "write_isrc_to_db": not db_isrc,
                                          "mbid": found_mbid, "acoustid": cached_aid,
                                          "path": filepath, "conf": best_conf, "verdict": best_verdict})
            else:
                print(f"NEW MB MATCH, NO ISRC (conf={best_conf})")
                matched_no_isrc.append({"id": tid, "title": title, "mbid": found_mbid, "acoustid": cached_aid, "path": filepath, "conf": best_conf, "verdict": best_verdict})
            continue

        if cached_mbid:
            sys.stdout.write(f"CACHED (mbid={cached_mbid[:8]}…) ")
            sys.stdout.flush()
            try:
                mb = mb_lookup(cached_mbid)
                time.sleep(MB_DELAY)
            except Exception:
                time.sleep(MB_DELAY)
                print("MB ERROR")
                continue
            isrcs     = mb.get("isrcs", [])
            mb_title  = mb.get("title", "")
            mb_artist = " ".join(c["artist"]["name"] for c in mb.get("artist-credit", []))
            mb_length = mb.get("length") or 0
            conf, verdict = match_confidence(title, rb_artist, rb_length, mb_title, mb_artist, mb_length, 1.0)
            if verdict == "reject":
                print(f"REJECTED (conf={conf}, mb='{mb_artist} — {mb_title}')")
                needs_review.append({"id": tid, "title": title, "rb_artist": rb_artist, "mb_title": mb_title, "mb_artist": mb_artist, "conf": conf, "mbid": cached_mbid})
                continue
            # Only store MBID when accept (>=0.72); review verdict may still yield ISRC
            store_mbid = cached_mbid if verdict == "accept" else None
            found_isrc = isrcs[0] if isrcs else None
            effective_isrc = found_isrc or db_isrc
            if effective_isrc:
                print(f"→ ISRC: {effective_isrc} (conf={conf})")
                matched_with_isrc.append({"id": tid, "title": title, "isrc": effective_isrc,
                                          "write_isrc_to_db": not db_isrc,
                                          "mbid": store_mbid, "acoustid": cached_aid,
                                          "path": filepath, "conf": conf, "verdict": verdict})
            else:
                print(f"→ NO ISRC (conf={conf})")
                matched_no_isrc.append({"id": tid, "title": title, "mbid": store_mbid, "acoustid": cached_aid, "path": filepath, "conf": conf, "verdict": verdict})
            continue

        # --- Full fingerprint + AcoustID lookup ---
        duration, fp = fingerprint(filepath)
        if not duration:
            print("FPCALC FAILED")
            continue

        try:
            result = acoustid_lookup(duration, fp)
        except Exception as e:
            print(f"ACOUSTID ERROR: {e}")
            continue

        results = result.get("results", [])
        if not results:
            print("NO MATCH")
            no_match.append({"id": tid, "title": title})
            continue

        best       = results[0]
        found_aid  = best["id"]
        aid_score  = best.get("score", 1.0)
        recordings = best.get("recordings", [])
        if not recordings:
            print("NO RECORDINGS LINKED")
            write_ids_to_file(filepath, acoustid=found_aid)
            no_match.append({"id": tid, "title": title, "acoustid": found_aid})
            continue

        found_isrc = None
        found_mbid = None
        best_conf  = 0.0
        best_verdict = "reject"
        for rec in recordings[:3]:
            mbid = rec["id"]
            try:
                mb = mb_lookup(mbid)
                time.sleep(MB_DELAY)
            except Exception:
                time.sleep(MB_DELAY)
                continue
            mb_title  = mb.get("title", "")
            mb_artist = " ".join(c["artist"]["name"] for c in mb.get("artist-credit", []))
            mb_length = mb.get("length") or 0
            conf, verdict = match_confidence(title, rb_artist, rb_length, mb_title, mb_artist, mb_length, aid_score)
            if verdict == "reject":
                continue
            isrcs = mb.get("isrcs", [])
            if conf > best_conf:
                best_conf    = conf
                best_verdict = verdict
                # Only store MBID when accept (>=0.72)
                found_mbid = mbid if verdict == "accept" else None
                if isrcs:
                    found_isrc = isrcs[0]

        if not found_mbid and not found_isrc:
            print(f"REJECTED (no confident match in top recordings)")
            needs_review.append({"id": tid, "title": title, "rb_artist": rb_artist, "acoustid": found_aid})
            write_ids_to_file(filepath, acoustid=found_aid)
            continue

        effective_isrc = found_isrc or db_isrc
        if effective_isrc:
            print(f"ISRC: {effective_isrc} (conf={best_conf})")
            matched_with_isrc.append({"id": tid, "title": title, "isrc": effective_isrc,
                                      "write_isrc_to_db": not db_isrc,
                                      "mbid": found_mbid, "acoustid": found_aid,
                                      "path": filepath, "conf": best_conf, "verdict": best_verdict})
        else:
            print(f"MB MATCH, NO ISRC (conf={best_conf})")
            matched_no_isrc.append({"id": tid, "title": title, "mbid": found_mbid, "acoustid": found_aid, "path": filepath, "conf": best_conf, "verdict": best_verdict})

    # --- Write back ---
    write_count = len(matched_with_isrc) + len(matched_no_isrc)
    if write_count:
        print(f"\nWriting tags to {write_count} files...")
        db_ok = file_ok = 0
        for t in matched_with_isrc:
            if t.get("write_isrc_to_db"):
                write_isrc_to_db(t["id"], t["isrc"])
                db_ok += 1
            if write_ids_to_file(t["path"], acoustid=t.get("acoustid"), mbid=t["mbid"], isrc=t["isrc"]):
                file_ok += 1
        for t in matched_no_isrc:
            if write_ids_to_file(t["path"], acoustid=t.get("acoustid"), mbid=t["mbid"]):
                file_ok += 1
        print(f"  Database ISRCs: {db_ok} updated")
        print(f"  File tags: {file_ok}/{write_count} written")

    # --- Summary ---
    print("\n" + "="*60)
    print(f"RESULTS FOR {len(tracks)} TRACKS")
    print("="*60)
    print(f"  ISRC found & written: {len(matched_with_isrc)}")
    print(f"  MB match, no ISRC:    {len(matched_no_isrc)}")
    print(f"  No fingerprint match: {len(no_match)}")
    print(f"  File not found:       {len(not_found)}")

    if matched_with_isrc:
        print(f"\n--- ISRC WRITTEN ({len(matched_with_isrc)}) ---")
        for t in matched_with_isrc:
            print(f"  {t['isrc']}  {t['title']}")

    if matched_no_isrc:
        print(f"\n--- MB MATCH, NO ISRC — candidates for contribution ({len(matched_no_isrc)}) ---")
        for t in matched_no_isrc:
            print(f"  https://musicbrainz.org/recording/{t['mbid']}  {t['title']}")

    if no_match:
        print(f"\n--- NO MATCH (bootlegs/edits/unknowns) ({len(no_match)}) ---")
        for t in no_match:
            print(f"  {t['title']}")

    if needs_review:
        print(f"\n--- NEEDS REVIEW (low confidence / rejected) ({len(needs_review)}) ---")
        for t in needs_review:
            mbid = t.get("mbid", "")
            rb_artist = t.get("rb_artist", "?")
            mb_artist = t.get("mb_artist", "?")
            mb_title  = t.get("mb_title", "?")
            conf      = t.get("conf", "?")
            line = f"  [{conf}] {rb_artist} — {t['title']}"
            if mbid:
                line += f"\n         MB: {mb_artist} — {mb_title}  https://musicbrainz.org/recording/{mbid}"
            print(line)

    if not_found:
        print(f"\n--- FILES NOT FOUND ({len(not_found)}) ---")
        for t in not_found:
            print(f"  {t['title']}  →  {t['path']}")


if __name__ == "__main__":
    main()
