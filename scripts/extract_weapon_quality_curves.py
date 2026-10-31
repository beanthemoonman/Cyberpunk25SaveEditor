#!/usr/bin/env python3
"""Export four numeric curves from locally extracted 2.5 CR2W CurveSet files.

Input: already decompressed base/gameplay/curves/statcurves/items/quality_curves.curveset
and the game's random_distributions.curveset. Archive decompression is external.
Only curve names and numeric points are exported, never original resources.
"""
import argparse
import json
import pathlib
import struct


def read_curves(path):
    data = path.read_bytes()
    assert data[:4] == b'CR2W'
    tables = [struct.unpack_from('<III', data, 40 + 12 * i) for i in range(10)]
    names = []
    for i in range(tables[1][1]):
        pos = tables[0][0] + struct.unpack_from('<I', data, tables[1][0] + 8 * i)[0]
        names.append(data[pos:data.index(0, pos)].decode())

    def fields(pos):
        assert data[pos] == 0
        pos += 1
        result = {}
        while True:
            name = struct.unpack_from('<H', data, pos)[0]
            pos += 2
            if not name:
                return result, pos
            kind, size = struct.unpack_from('<HI', data, pos)
            pos += 6
            result[names[name]] = (names[kind], pos, size - 4)
            pos += size - 4

    for i in range(tables[4][1]):
        cls, _, _, _, pos, _, _ = struct.unpack_from('<HHIIIII', data, tables[4][0] + 24 * i)
        if names[cls] != 'CurveSet':
            continue
        _, pos, _ = fields(pos)[0]['curves']
        count = struct.unpack_from('<I', data, pos)[0]
        pos += 4
        result = {}
        for _ in range(count):
            row, pos = fields(pos)
            name = names[struct.unpack_from('<H', data, row['name'][1])[0]]
            offset = row['curve'][1]
            size = struct.unpack_from('<I', data, offset)[0]
            points = [list(struct.unpack_from('<ff', data, offset + 4 + 8 * j)) for j in range(size)]
            interpolation = data[offset + 4 + 8 * size]
            assert interpolation in (0, 1)
            if name in ('iconic_level_to_random_range_mult_new', 'iconic_upgrades_amount_to_quality',
                        'iconic_upgrades_amount_to_plus', 'quality_distribution_new'):
                assert interpolation == 0, f'{name} is no longer a constant curve'
                result[name] = points
        return result
    raise ValueError(f'No CurveSet in {path}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('resources', nargs='+', type=pathlib.Path)
    p.add_argument('--out', type=pathlib.Path, default=pathlib.Path('assets/weapon_quality_curves_25.json'))
    args = p.parse_args()
    result = {}
    for path in args.resources:
        result.update(read_curves(path))
    assert len(result) == 4, 'Both quality_curves and random_distributions are required'
    args.out.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
