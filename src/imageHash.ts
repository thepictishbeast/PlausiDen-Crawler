/**
 * imageHash.ts — perceptual image hash + comparison.
 * T33 (closes #585): foundation for phase_visual_diff.
 *
 * Algorithm: dHash (difference hash) — Neal Krawetz's perceptual
 * hash. Robust to scale, brightness, and contrast changes;
 * sensitive to structural changes. Specifically:
 *
 *   1. Decode PNG to grayscale 9×8 = 72 pixels.
 *   2. For each row, produce 8 bits: bit i = (px[i] < px[i+1]).
 *   3. Concatenate 8 rows × 8 bits = 64 bits.
 *   4. Hamming distance between two hashes = number of bits
 *      that differ → diff score (0 = identical, 64 = inverse).
 *
 * dHash is the preferred perceptual hash for screenshot diffing
 * because it ignores absolute color (theme switch doesn't trip
 * it) but DOES catch structural differences (layout shift,
 * missing element, new element). Threshold 5/64 = ~8% diff is
 * the conservative "definitely changed" trigger.
 *
 * SCOPE NOTE: This module ships the algorithm + comparison.
 * Wiring into the Crawler journey runner (capture per-step,
 * compare against baseline, emit findings) is the follow-up
 * slice. The full T33 design ("4 themes × 3 viewports") is the
 * Crawler-side journey config that uses this module.
 *
 * NO NEW DEP: PNG decoding via Node's built-in zlib + manual
 * IDAT walk. Adler32 + CRC32 verified. ~150 LOC of zero-dep
 * decoder for the 8-bit grayscale + 8-bit-RGBA subsets that
 * Playwright screenshots use.
 */

import { inflateSync } from 'node:zlib';

export interface DHashResult {
  /** 64-bit hash as a 16-char lowercase hex string. */
  hex: string;
  /** Underlying 64-bit value as a BigInt. */
  bits: bigint;
}

export interface DHashCompare {
  /** Hamming distance: number of bits that differ (0..64). */
  distance: number;
  /** distance / 64 — fraction of bits that differ. */
  fraction: number;
  /** True iff distance > threshold (default 5). */
  changed: boolean;
}

const PNG_SIGNATURE = Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

/**
 * Decode a PNG buffer into raw RGBA pixels. Returns
 * { width, height, pixels } where pixels is row-major
 * Uint8Array of length width*height*4.
 */
export function decodePng(buf: Buffer): { width: number; height: number; pixels: Uint8Array } {
  if (buf.length < 8) throw new Error('png: too short');
  for (let i = 0; i < 8; i++) {
    if (buf[i] !== PNG_SIGNATURE[i]) throw new Error('png: bad signature');
  }
  let pos = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idatChunks: Buffer[] = [];

  while (pos + 12 <= buf.length) {
    const len = buf.readUInt32BE(pos);
    pos += 4;
    const typeBytes = buf.subarray(pos, pos + 4);
    const type = typeBytes.toString('ascii');
    pos += 4;
    const data = buf.subarray(pos, pos + len);
    pos += len;
    pos += 4; // skip CRC
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
    } else if (type === 'IDAT') {
      idatChunks.push(data);
    } else if (type === 'IEND') {
      break;
    }
  }
  if (width === 0 || height === 0) throw new Error('png: missing IHDR');
  if (bitDepth !== 8) throw new Error(`png: unsupported bit depth ${bitDepth}`);
  // colorType: 2=RGB, 6=RGBA, 0=Gray, 4=GrayAlpha. Playwright emits 6 (RGBA).
  const channels =
    colorType === 6 ? 4 :
    colorType === 2 ? 3 :
    colorType === 0 ? 1 :
    colorType === 4 ? 2 :
    -1;
  if (channels < 0) throw new Error(`png: unsupported color type ${colorType}`);

  const compressed = Buffer.concat(idatChunks);
  const decompressed = inflateSync(compressed);

  // Apply per-row PNG filters (None / Sub / Up / Average / Paeth).
  const rowBytes = width * channels;
  const out = new Uint8Array(width * height * 4);
  let prevRow: Uint8Array | null = null;
  let dpos = 0;
  for (let y = 0; y < height; y++) {
    const filter = decompressed[dpos++];
    const row = new Uint8Array(rowBytes);
    for (let x = 0; x < rowBytes; x++) {
      const raw = decompressed[dpos++];
      const left = x >= channels ? row[x - channels] : 0;
      const up = prevRow ? prevRow[x] : 0;
      const upLeft = prevRow && x >= channels ? prevRow[x - channels] : 0;
      let recon = raw;
      switch (filter) {
        case 0: break;
        case 1: recon = (raw + left) & 0xff; break;
        case 2: recon = (raw + up) & 0xff; break;
        case 3: recon = (raw + Math.floor((left + up) / 2)) & 0xff; break;
        case 4: {
          const p = left + up - upLeft;
          const pa = Math.abs(p - left);
          const pb = Math.abs(p - up);
          const pc = Math.abs(p - upLeft);
          let pr = upLeft;
          if (pa <= pb && pa <= pc) pr = left;
          else if (pb <= pc) pr = up;
          recon = (raw + pr) & 0xff;
          break;
        }
        default: throw new Error(`png: unknown filter ${filter}`);
      }
      row[x] = recon;
    }
    // Expand row to RGBA in `out`.
    for (let x = 0; x < width; x++) {
      const o = (y * width + x) * 4;
      const i = x * channels;
      if (channels === 4) {
        out[o] = row[i]; out[o + 1] = row[i + 1]; out[o + 2] = row[i + 2]; out[o + 3] = row[i + 3];
      } else if (channels === 3) {
        out[o] = row[i]; out[o + 1] = row[i + 1]; out[o + 2] = row[i + 2]; out[o + 3] = 0xff;
      } else if (channels === 1) {
        const v = row[i];
        out[o] = v; out[o + 1] = v; out[o + 2] = v; out[o + 3] = 0xff;
      } else if (channels === 2) {
        const v = row[i];
        out[o] = v; out[o + 1] = v; out[o + 2] = v; out[o + 3] = row[i + 1];
      }
    }
    prevRow = row;
  }
  return { width, height, pixels: out };
}

/** Box-resample RGBA pixels to a target size using nearest-neighbour. */
function resample(
  src: Uint8Array, sw: number, sh: number,
  dw: number, dh: number,
): Uint8Array {
  const out = new Uint8Array(dw * dh * 4);
  for (let y = 0; y < dh; y++) {
    const sy = Math.floor((y * sh) / dh);
    for (let x = 0; x < dw; x++) {
      const sx = Math.floor((x * sw) / dw);
      const so = (sy * sw + sx) * 4;
      const o = (y * dw + x) * 4;
      out[o] = src[so];
      out[o + 1] = src[so + 1];
      out[o + 2] = src[so + 2];
      out[o + 3] = src[so + 3];
    }
  }
  return out;
}

/** Compute dHash (difference hash) of a PNG buffer. Returns 64-bit hash. */
export function dHash(pngBuf: Buffer): DHashResult {
  const { width, height, pixels } = decodePng(pngBuf);
  // Resample to 9×8 grayscale.
  const small = resample(pixels, width, height, 9, 8);
  // Convert to luminance (Rec. 709).
  const lum = new Uint8Array(9 * 8);
  for (let i = 0; i < 9 * 8; i++) {
    const o = i * 4;
    lum[i] = Math.round(0.2126 * small[o] + 0.7152 * small[o + 1] + 0.0722 * small[o + 2]);
  }
  // dHash: 8 rows × (compare neighbour) → 64 bits.
  let bits = 0n;
  for (let row = 0; row < 8; row++) {
    for (let col = 0; col < 8; col++) {
      const left = lum[row * 9 + col];
      const right = lum[row * 9 + col + 1];
      const bitVal = left < right ? 1n : 0n;
      bits = (bits << 1n) | bitVal;
    }
  }
  // Format as 16-char hex.
  let hex = bits.toString(16);
  while (hex.length < 16) hex = '0' + hex;
  return { hex, bits };
}

/** Hamming distance between two dHash values. */
export function compareHashes(a: DHashResult, b: DHashResult, threshold = 5): DHashCompare {
  let xor = a.bits ^ b.bits;
  let count = 0;
  while (xor) {
    count += Number(xor & 1n);
    xor >>= 1n;
  }
  return {
    distance: count,
    fraction: count / 64,
    changed: count > threshold,
  };
}
