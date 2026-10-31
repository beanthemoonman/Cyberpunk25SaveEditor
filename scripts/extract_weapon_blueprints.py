#!/usr/bin/env python3
"""Refresh the experimental weapon catalog's spawn parts and dual-wield limits.

Reads a user's local 2.5 TweakDB files, base game followed by Phantom Liberty.
Exports numeric record references and tags only, never game resource files.
"""
import argparse
import json
import pathlib
import struct
from extract_item_metadata import Reader, fnv, tdb, flat_id


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tweakdb', nargs='+', type=pathlib.Path, required=True)
    parser.add_argument('--catalog', type=pathlib.Path, default=pathlib.Path('assets/iconic_weapons.json'))
    args = parser.parse_args()
    catalog = json.loads(args.catalog.read_text())
    types = {fnv(kind): kind for kind in ('TweakDBID', 'CName', 'String', 'Float', 'Int32',
             'Bool', 'Uint8', 'array:TweakDBID', 'array:CName', 'array:Float')}
    flats = {}
    for path in args.tweakdb:
        data = path.read_bytes()
        assert struct.unpack_from('<II', data) == (0x0BB1DB47, 8)
        start = struct.unpack_from('<I', data, 16)[0]
        for i in range(struct.unpack_from('<I', data, start)[0]):
            kind_hash, _, _, pos = struct.unpack_from('<QIII', data, start + 4 + 20 * i)
            if kind_hash not in types:
                continue
            reader = Reader(data, pos)
            values = [reader.read(types[kind_hash]) for _ in range(reader.unpack('I'))]
            for _ in range(reader.unpack('I')):
                key, index = reader.unpack('QI')
                flats[key] = values[index]
    def get(record, prop, default=None):
        return flats.get(flat_id(record, prop), default)
    for weapon in catalog:
        record = tdb(weapon['record'])
        tags = get(record, 'tags', [])
        assert 'IconicWeapon' in tags, weapon['record']
        weapon['dual_wield'] = 'DualWield' in tags
        weapon['limit'] = 2 if weapon['dual_wield'] else 1
        weapon['quest_sensitive'] = weapon['group'] == 'skippy' or 'Quest' in tags
        parts = []
        for part in get(record, 'slotPartListPreset', []):
            item = get(part, 'itemPartPreset', 0)
            slot = get(part, 'slot', 0)
            if item:
                assert slot
                parts.append({'record': item, 'slot': slot})
        assert parts, weapon['record']
        weapon['parts'] = parts
        modifiers = get(record, 'statModifiers', []) + [m
            for group in get(record, 'statModifierGroups', [])
            for m in get(group, 'statModifiers', [])]
        assert any(get(m, 'column') == 'iconic_upgrades_amount_to_quality'
                   and get(m, 'refStat') == tdb('BaseStats.WasItemUpgraded')
                   for m in modifiers), f"Native upgrade quality curve missing: {weapon['record']}"

    args.catalog.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + '\n')
    print(f'Updated {len(catalog)} weapons from local game data')


if __name__ == '__main__':
    main()
