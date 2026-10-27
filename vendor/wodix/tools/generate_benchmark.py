#!/usr/bin/env python3
"""Reproduce the historical MDX benchmark inputs using only Python 3.9+ stdlib.

Usage: python3 tools/generate_benchmark.py OUTPUT_DIR [SIZE]
Without SIZE, writes bench-10000.mdx and bench-100000.mdx.

The data algorithm is extracted unchanged from
benchmarks/mdict-comparison.json: reproduction.corpus_generator_script
(and mdict-improvements.json: reproduction.corpus_generator).
It uses random.Random(918732), 512-byte records, 512 keys per key block,
128 records per record block, and zlib level 6. No timestamps or downloads.
The historical SHA-256 values are:
  10000:  2522693e52adef8449aaa7877747bbdd13e2a4ca89f2e9f3d326b0920ef511ca
  100000: ea5b68d9d3d33c3aae55351e2aca95d7032e660c01fa3e618559adda8f97e867

This is NOT the Rust default benchmark corpus: its random stream and metadata
are different. Always record the actual input hash; do not mix their results.
"""

import argparse
import hashlib
import random
import struct
import zlib
from pathlib import Path


class Corpus:
    @staticmethod
    def block(payload):
        checksum = zlib.adler32(payload)
        return struct.pack('>II', 0x02000000, checksum) + zlib.compress(payload)

    @classmethod
    def build(cls, count, block_records=128, key_records=512):
        randomizer = random.Random(918732)
        rows = []
        for number in range(count):
            word = f'word{number:08}'
            entropy = randomizer.randbytes(256).hex()
            record = (word + '|' + entropy)[:511].encode() + b'\0'
            rows.append((word, record))
        xml = '<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="0" Title="woyodict independent benchmark" Description="Generated ASCII unique keys" Left2Right="Yes" KeyCaseSensitive="Yes" StripKey="No" />'
        header_text = xml.encode('utf-16le') + b'\0\0'
        header = struct.pack('>I', len(header_text)) + header_text + struct.pack('<I', zlib.adler32(header_text))
        key_blocks = []
        key_info = bytearray()
        for first in range(0, count, key_records):
            group = rows[first:first + key_records]
            raw = b''.join(struct.pack('>Q', (first + offset) * 512) + word.encode() + b'\0' for offset, (word, _) in enumerate(group))
            block = cls.block(raw)
            key_blocks.append(block)
            key_info.extend(struct.pack('>Q', len(group)))
            for word in (group[0][0], group[-1][0]):
                key_info.extend(struct.pack('>H', len(word)) + word.encode() + b'\0')
            key_info.extend(struct.pack('>QQ', len(block), len(raw)))
        key_info_block = cls.block(key_info)
        key_header = struct.pack('>QQQQQ', len(key_blocks), count, len(key_info), len(key_info_block), sum(map(len, key_blocks)))
        key_header += struct.pack('>I', zlib.adler32(key_header))
        record_blocks = []
        record_info = bytearray()
        for first in range(0, count, block_records):
            raw = b''.join(record for _, record in rows[first:first + block_records])
            block = cls.block(raw)
            record_blocks.append(block)
            record_info.extend(struct.pack('>QQ', len(block), len(raw)))
        record_header = struct.pack('>QQQQ', len(record_blocks), count, len(record_info), sum(map(len, record_blocks)))
        return header + key_header + key_info_block + b''.join(key_blocks) + record_header + record_info + b''.join(record_blocks)


    @classmethod
    def main(cls):
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("output_dir", type=Path)
        parser.add_argument("size", nargs="?", type=int,
                            help="entry count in 1..100000; default writes 10000 and 100000")
        args = parser.parse_args()
        if args.size is not None and not 1 <= args.size <= 100000:
            parser.error("size must be in 1..100000")
        counts = (10000, 100000) if args.size is None else (args.size,)
        args.output_dir.mkdir(parents=True, exist_ok=True)
        for count in counts:
            data = cls.build(count)
            path = args.output_dir / f"bench-{count}.mdx"
            path.write_bytes(data)
            print(f"{path}: {count} entries, {len(data)} bytes, SHA-256 {hashlib.sha256(data).hexdigest()}")


if __name__ == '__main__':
    Corpus.main()
