#!/usr/bin/env python3
"""
Migrate Giggs folder → PRIVATE / VENUES / CORPORATE

Dry-run by default — prints every planned action without touching the DB.
Pass --execute to apply changes.
Pass --output gigs_migration.json to write the resulting gigs.json.

Usage:
    python3 migration/migrate_giggs.py
    python3 migration/migrate_giggs.py --execute --output dj_jonas/gigs.json
"""

import sys
import json
import uuid
import argparse
from pathlib import Path

DB_PATH  = "dj_jonas/master.db"
DB_KEY   = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497"

# ── Known IDs from DB ─────────────────────────────────────────────────────────

GIGGS_ID     = 3279556618
CORPORATE_ID = 958506794
VENUES_ID    = 904031143
IMPAN_ID     = 1535888789

# Giggs existing sub-folders → become PRIVATE/contact/event
# (contact_name is the cleaned-up name; event_name is the sub-folder to create)
EXISTING_FOLDERS = [
    dict(giggs_id=2851493803, contact_name="Elin & Axel",       event_name="Wedding 2025-09-13", date="2025-09-13", tags=["wedding"]),
    dict(giggs_id=2638400919, contact_name="Lotta",              event_name="Party",              date=None,        tags=[]),
    dict(giggs_id=1752972733, contact_name="Anton Ramnäs",       event_name="Party 2024-07-28",   date="2024-07-28",tags=[]),
    dict(giggs_id=3068503306, contact_name="Caroline & David",   event_name="Wedding",            date=None,        tags=["wedding"]),
    dict(giggs_id=658471454,  contact_name="Emil",               event_name="30th Birthday",      date=None,        tags=["birthday"]),
    dict(giggs_id=34272904,   contact_name="Ella & Brian",       event_name="Wedding",            date=None,        tags=["wedding"]),
    dict(giggs_id=2051692046, contact_name="Johan",              event_name="40th Birthday",      date=None,        tags=["birthday"]),
    dict(giggs_id=3169032952, contact_name="Camila & Viktor",    event_name="Wedding",            date=None,        tags=["wedding"]),
    dict(giggs_id=1845376662, contact_name="Frida & Thomas",     event_name="Wedding",            date=None,        tags=["wedding"]),
    dict(giggs_id=2246176041, contact_name="Cattis",             event_name="40th Birthday",      date=None,        tags=["birthday"]),
    dict(giggs_id=447555469,  contact_name="Niklas & Hanna",     event_name="Wedding",            date=None,        tags=["wedding"]),
    dict(giggs_id=3206974438, contact_name="Victoria & Fredrik", event_name="Wedding",            date=None,        tags=["wedding"]),
    dict(giggs_id=305292799,  contact_name="Budimovic",          event_name="Wedding",            date=None,        tags=["wedding"]),
]

# Giggs flat playlists → PRIVATE/new-contact/new-event/playlist
NEW_PRIVATE = [
    dict(playlist_id=3793501028, contact_name="Nils",             event_name="2025-06-28",       date="2025-06-28", tags=[]),
    dict(playlist_id=1425589933, contact_name="Lazlo & Charlotte", event_name="Wedding",          date=None,         tags=["wedding"]),
    dict(playlist_id=1681538698, contact_name="Noel",             event_name="Wedding 2020-08-05",date="2020-08-05", tags=["wedding"]),
    dict(playlist_id=799318451,  contact_name="Bröllop 20190608", event_name="Wedding 2019-06-08",date="2019-06-08", tags=["wedding"]),
    dict(playlist_id=4211903789, contact_name="Micke",            event_name="40th Birthday",    date=None,         tags=["birthday"]),
    dict(playlist_id=1487648467, contact_name="Hedenlunda",       event_name="Wedding",          date=None,         tags=["wedding"]),
    dict(playlist_id=530771457,  contact_name="Cillaohenke",      event_name="Wedding",          date=None,         tags=["wedding"]),
    dict(playlist_id=2253833192, contact_name="Göransson",        event_name="Wedding",          date=None,         tags=["wedding"]),
]

# Giggs flat playlists → VENUES/Impan (flat, no event wrapper)
IMPAN_MOVES = [
    dict(playlist_id=3110021804, date="2017-09-01"),
    dict(playlist_id=4072800253, date="2017-09-16"),
]

# New VENUES contact folders + what moves into them
NEW_VENUES = [
    dict(
        contact_name="Sundbyholm",
        playlists=[
            dict(playlist_id=2039697145, date="2018-11-16"),
            dict(playlist_id=586757342,  date="2018-09-08"),
        ],
        folders=[],
    ),
    dict(
        contact_name="Motorverkstan",
        playlists=[],
        folders=[
            dict(folder_id=3112670152, date="2020-09-26", tags=["wedding"]),  # 2020-09-26 folder
            dict(folder_id=3985674138, date=None,         tags=["wedding"]),  # Motorverkstaden folder
        ],
    ),
]

# CORPORATE: British junior is a flat playlist, needs a contact wrapper
BRITISH_JUNIOR_ID = 642420324

# Skip: Intro (3073154188) — only 2 tracks, not a real gig


# ── DB helpers ────────────────────────────────────────────────────────────────

def connect():
    try:
        from pysqlcipher3 import dbapi2 as sqlite
    except ImportError:
        import sqlcipher3 as sqlite
    conn = sqlite.connect(DB_PATH)
    conn.execute(f"PRAGMA key = '{DB_KEY}'")
    conn.execute("PRAGMA cipher_compatibility = 4")
    return conn


def next_db_id(conn):
    row = conn.execute(
        "SELECT COALESCE(MAX(CAST(ID AS INTEGER)), 0) + 1 FROM djmdPlaylist"
    ).fetchone()
    return row[0]


def next_seq(conn, parent_id):
    parent_str = str(parent_id) if parent_id is not None else "root"
    row = conn.execute(
        "SELECT COALESCE(MAX(Seq), 0) + 1 FROM djmdPlaylist WHERE ParentID = ?",
        (parent_str,),
    ).fetchone()
    return row[0]


def get_children(conn, parent_id):
    parent_str = str(parent_id) if parent_id is not None else "root"
    rows = conn.execute(
        "SELECT CAST(ID AS INTEGER), Name, Attribute, Seq FROM djmdPlaylist WHERE ParentID = ? ORDER BY Seq",
        (parent_str,),
    ).fetchall()
    return [{"id": r[0], "name": r[1], "attr": r[2], "seq": r[3]} for r in rows]


def find_top_level(conn, name):
    row = conn.execute(
        "SELECT CAST(ID AS INTEGER) FROM djmdPlaylist WHERE Name = ? AND ParentID = 'root'",
        (name,),
    ).fetchone()
    return row[0] if row else None


def db_create_folder(conn, name, parent_id, dry_run):
    parent_str = str(parent_id) if parent_id is not None else "root"
    new_id = next_db_id(conn)
    seq    = next_seq(conn, parent_id)
    new_uuid = str(uuid.uuid4()).upper()
    print(f"    [CREATE FOLDER] '{name}' under {parent_id}  →  new id={new_id}")
    if not dry_run:
        conn.execute(
            "INSERT INTO djmdPlaylist "
            "(ID, Seq, Name, Attribute, ParentID, UUID, rb_local_deleted, rb_local_data_status, created_at, updated_at) "
            "VALUES (?,?,?,1,?,?,0,1,datetime('now'),datetime('now'))",
            (str(new_id), seq, name, parent_str, new_uuid),
        )
        conn.commit()
    return new_id


def db_rename(conn, item_id, new_name, dry_run):
    print(f"    [RENAME] {item_id} → '{new_name}'")
    if not dry_run:
        conn.execute(
            "UPDATE djmdPlaylist SET Name = ?, updated_at = datetime('now'), rb_local_data_status = 1 WHERE ID = ?",
            (new_name, str(item_id)),
        )
        conn.commit()


def db_move(conn, item_id, new_parent_id, dry_run):
    parent_str = str(new_parent_id) if new_parent_id is not None else "root"
    seq = next_seq(conn, new_parent_id)
    print(f"    [MOVE] {item_id} → parent={new_parent_id}")
    if not dry_run:
        conn.execute(
            "UPDATE djmdPlaylist SET ParentID = ?, Seq = ?, updated_at = datetime('now'), rb_local_data_status = 1 WHERE ID = ?",
            (parent_str, seq, str(item_id)),
        )
        conn.commit()


def db_delete(conn, item_id, dry_run):
    print(f"    [DELETE] {item_id}")
    if not dry_run:
        conn.execute(
            "UPDATE djmdPlaylist SET rb_local_deleted = 1, updated_at = datetime('now') WHERE ID = ?",
            (str(item_id),),
        )
        conn.commit()


# ── Migration steps ───────────────────────────────────────────────────────────

def run(dry_run: bool) -> dict:
    """Run migration and return gigs data for JSON output."""
    conn = connect()

    contacts = []  # {id, name, customer_type, rekordbox_folder_id}
    gigs     = []  # {id, contact_id, name, date, tags, rekordbox_folder_id}

    # ── 1. Ensure PRIVATE folder exists ──────────────────────────────────────
    print("\n=== PRIVATE folder ===")
    private_id = find_top_level(conn, "PRIVATE")
    if private_id is None:
        private_id = db_create_folder(conn, "PRIVATE", None, dry_run)
    else:
        print(f"    [EXISTS] PRIVATE id={private_id}")

    # ── 2. Move existing Giggs sub-folders → PRIVATE ─────────────────────────
    print("\n=== Existing Giggs folders → PRIVATE ===")
    for spec in EXISTING_FOLDERS:
        print(f"\n  Contact: {spec['contact_name']}")
        contact_folder_id = spec["giggs_id"]

        # Rename contact folder (strip date / event type)
        db_rename(conn, contact_folder_id, spec["contact_name"], dry_run)

        # Move contact folder under PRIVATE
        db_move(conn, contact_folder_id, private_id, dry_run)

        # Snapshot children BEFORE creating the event sub-folder to avoid
        # the new folder appearing in its own child list
        children = get_children(conn, contact_folder_id)

        # Create event sub-folder inside the contact folder
        event_folder_id = db_create_folder(conn, spec["event_name"], contact_folder_id, dry_run)

        # Move pre-existing children into the event sub-folder
        for child in children:
            db_move(conn, child["id"], event_folder_id, dry_run)

        # Register contact + gig
        contact_uid = str(uuid.uuid4())
        gig_uid     = str(uuid.uuid4())
        contacts.append({
            "id":                   contact_uid,
            "name":                 spec["contact_name"],
            "customer_type":        "private",
            "rekordbox_folder_id":  contact_folder_id,
        })
        gigs.append({
            "id":                   gig_uid,
            "contact_id":           contact_uid,
            "name":                 spec["event_name"],
            "date":                 spec["date"],
            "tags":                 spec["tags"],
            "notes":                "",
            "spotify_playlist_url": None,
            "rekordbox_folder_id":  event_folder_id,
        })

    # ── 3. Flat Giggs playlists → new PRIVATE contact + event folders ─────────
    print("\n=== New PRIVATE contacts (from flat playlists) ===")
    for spec in NEW_PRIVATE:
        print(f"\n  Contact: {spec['contact_name']}")

        contact_folder_id = db_create_folder(conn, spec["contact_name"], private_id, dry_run)
        event_folder_id   = db_create_folder(conn, spec["event_name"], contact_folder_id, dry_run)
        db_move(conn, spec["playlist_id"], event_folder_id, dry_run)

        contact_uid = str(uuid.uuid4())
        gig_uid     = str(uuid.uuid4())
        contacts.append({
            "id":                   contact_uid,
            "name":                 spec["contact_name"],
            "customer_type":        "private",
            "rekordbox_folder_id":  contact_folder_id,
        })
        gigs.append({
            "id":                   gig_uid,
            "contact_id":           contact_uid,
            "name":                 spec["event_name"],
            "date":                 spec["date"],
            "tags":                 spec["tags"],
            "notes":                "",
            "spotify_playlist_url": None,
            "rekordbox_folder_id":  event_folder_id,
        })

    # ── 4. Imperiet playlists → flat under Impan ─────────────────────────────
    print("\n=== Imperiet → Impan ===")
    impan_uid = str(uuid.uuid4())
    contacts.append({
        "id":                   impan_uid,
        "name":                 "Impan",
        "customer_type":        "venue",
        "rekordbox_folder_id":  IMPAN_ID,
    })
    for spec in IMPAN_MOVES:
        print(f"\n  Move playlist {spec['playlist_id']} → Impan")
        db_move(conn, spec["playlist_id"], IMPAN_ID, dry_run)
        gig_uid = str(uuid.uuid4())
        gigs.append({
            "id":                   gig_uid,
            "contact_id":           impan_uid,
            "name":                 "Night",
            "date":                 spec["date"],
            "tags":                 [],
            "notes":                "",
            "spotify_playlist_url": None,
            "rekordbox_folder_id":  spec["playlist_id"],
        })

    # Also register existing Impan playlists as gigs
    existing_impan = get_children(conn, IMPAN_ID)
    for child in existing_impan:
        if child["attr"] == 0:  # playlist only, skip pool folders
            gig_uid = str(uuid.uuid4())
            gigs.append({
                "id":                   gig_uid,
                "contact_id":           impan_uid,
                "name":                 child["name"],
                "date":                 None,
                "tags":                 [],
                "notes":                "",
                "spotify_playlist_url": None,
                "rekordbox_folder_id":  child["id"],
            })

    # ── 5. New VENUES contact folders ────────────────────────────────────────
    print("\n=== New VENUES folders ===")
    for spec in NEW_VENUES:
        print(f"\n  Venue: {spec['contact_name']}")
        venue_folder_id = db_create_folder(conn, spec["contact_name"], VENUES_ID, dry_run)
        contact_uid = str(uuid.uuid4())
        contacts.append({
            "id":                   contact_uid,
            "name":                 spec["contact_name"],
            "customer_type":        "venue",
            "rekordbox_folder_id":  venue_folder_id,
        })

        for pl in spec["playlists"]:
            db_move(conn, pl["playlist_id"], venue_folder_id, dry_run)
            gig_uid = str(uuid.uuid4())
            gigs.append({
                "id":                   gig_uid,
                "contact_id":           contact_uid,
                "name":                 "Night",
                "date":                 pl["date"],
                "tags":                 pl.get("tags", []),
                "notes":                "",
                "spotify_playlist_url": None,
                "rekordbox_folder_id":  pl["playlist_id"],
            })

        for folder in spec["folders"]:
            db_move(conn, folder["folder_id"], venue_folder_id, dry_run)
            gig_uid = str(uuid.uuid4())
            gigs.append({
                "id":                   gig_uid,
                "contact_id":           contact_uid,
                "name":                 "Night",
                "date":                 folder["date"],
                "tags":                 folder.get("tags", []),
                "notes":                "",
                "spotify_playlist_url": None,
                "rekordbox_folder_id":  folder["folder_id"],
            })

    # ── 6. Register existing VENUES contacts (Biliwi, Tuna Park) ─────────────
    print("\n=== Existing VENUES contacts ===")
    for name, folder_id in [("Biliwi", 1025720368), ("Tuna Park", 2097228128)]:
        print(f"  {name}")
        contact_uid = str(uuid.uuid4())
        contacts.append({
            "id":                   contact_uid,
            "name":                 name,
            "customer_type":        "venue",
            "rekordbox_folder_id":  folder_id,
        })
        for child in get_children(conn, folder_id):
            gig_uid = str(uuid.uuid4())
            gigs.append({
                "id":                   gig_uid,
                "contact_id":           contact_uid,
                "name":                 child["name"],
                "date":                 None,
                "tags":                 [],
                "notes":                "",
                "spotify_playlist_url": None,
                "rekordbox_folder_id":  child["id"],
            })

    # ── 7. Register existing CORPORATE contacts ───────────────────────────────
    print("\n=== CORPORATE contacts ===")
    corporate_children = get_children(conn, CORPORATE_ID)
    for child in corporate_children:
        if child["id"] == BRITISH_JUNIOR_ID:
            # Flat playlist — wrap in a contact folder
            print(f"\n  British junior (flat playlist → wrap)")
            wrapper_id = db_create_folder(conn, "British junior", CORPORATE_ID, dry_run)
            db_move(conn, BRITISH_JUNIOR_ID, wrapper_id, dry_run)
            contact_uid = str(uuid.uuid4())
            gig_uid     = str(uuid.uuid4())
            contacts.append({
                "id":                   contact_uid,
                "name":                 "British junior",
                "customer_type":        "corporate",
                "rekordbox_folder_id":  wrapper_id,
            })
            gigs.append({
                "id":                   gig_uid,
                "contact_id":           contact_uid,
                "name":                 "British junior",
                "date":                 None,
                "tags":                 [],
                "notes":                "",
                "spotify_playlist_url": None,
                "rekordbox_folder_id":  BRITISH_JUNIOR_ID,
            })
        elif child["attr"] == 1:
            # Sub-folder = contact
            print(f"  {child['name']}")
            contact_uid = str(uuid.uuid4())
            contacts.append({
                "id":                   contact_uid,
                "name":                 child["name"],
                "customer_type":        "corporate",
                "rekordbox_folder_id":  child["id"],
            })
            for pl in get_children(conn, child["id"]):
                gig_uid = str(uuid.uuid4())
                gigs.append({
                    "id":                   gig_uid,
                    "contact_id":           contact_uid,
                    "name":                 pl["name"],
                    "date":                 None,
                    "tags":                 [],
                    "notes":                "",
                    "spotify_playlist_url": None,
                    "rekordbox_folder_id":  pl["id"],
                })

    # ── 8. Delete Giggs root folder ───────────────────────────────────────────
    print("\n=== Remove Giggs root ===")
    remaining = get_children(conn, GIGGS_ID)
    skipped = [c for c in remaining if c["id"] == 3073154188]  # Intro
    if dry_run:
        print(f"  Remaining children after migration: {[c['name'] for c in remaining]}")
        if skipped:
            print(f"  Will delete skipped items: {[c['name'] for c in skipped]}")
    else:
        for child in skipped:
            db_delete(conn, child["id"], dry_run)
    db_delete(conn, GIGGS_ID, dry_run)

    return {"contacts": contacts, "gigs": gigs}


# ── Entry point ───────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="Apply changes to DB (default: dry-run)")
    parser.add_argument("--output",  metavar="FILE",      help="Write gigs.json to FILE")
    args = parser.parse_args()

    dry_run = not args.execute
    if dry_run:
        print("=== DRY RUN — no changes will be made ===")
    else:
        print("=== EXECUTING — changes will be written to DB ===")

    result = run(dry_run)

    print(f"\n=== Summary ===")
    print(f"  Contacts: {len(result['contacts'])}")
    print(f"  Gigs:     {len(result['gigs'])}")

    if args.output:
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        with open(args.output, "w") as f:
            json.dump(result, f, indent=2, ensure_ascii=False)
        print(f"  Written: {args.output}")
    elif dry_run:
        print("\n  (pass --output <file> to preview the gigs.json)")
    else:
        print("\n  (pass --output <file> to write gigs.json)")


if __name__ == "__main__":
    main()
