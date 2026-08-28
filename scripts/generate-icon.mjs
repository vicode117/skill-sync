#!/usr/bin/env node
// Generates the SkillSync app icon source PNG (1024x1024) without image
// library dependencies: an indigo rounded tile with a chain-link glyph.
// Output: apps/desktop/app-icon.png
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SIZE = 1024;

const crcTable = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();

function crc32(buf) {
  let c = -1;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

const lerp = (a, b, t) => a + (b - a) * t;

// Signed distance of a rounded rectangle (<= 0 means inside).
function roundedRectSdf(px, py, cx, cy, halfW, halfH, radius) {
  const dx = px - cx;
  const dy = py - cy;
  const qx = Math.abs(dx) - (halfW - radius);
  const qy = Math.abs(dy) - (halfH - radius);
  const ox = Math.max(qx, 0);
  const oy = Math.max(qy, 0);
  return Math.sqrt(ox * ox + oy * oy) + Math.min(Math.max(qx, qy), 0) - radius;
}

function roundedRectCoverage(px, py, cx, cy, halfW, halfH, radius) {
  const d = roundedRectSdf(px, py, cx, cy, halfW, halfH, radius);
  if (d <= -0.5) return 1;
  if (d >= 0.5) return 0;
  return 0.5 - d;
}

// Tile
const TILE_MARGIN = 96;
const TILE_RADIUS = 220;
const TILE_HALF = SIZE / 2 - TILE_MARGIN;

// Chain-link glyph: two rounded-rect strokes side by side, overlapping.
const GLYPH_Y = SIZE / 2;
const GLYPH_HALF_W = 190;
const GLYPH_HALF_H = 130;
const GLYPH_RADIUS = 120;
const STROKE = 34;
const LINK1_CX = SIZE / 2 - 110;
const LINK2_CX = SIZE / 2 + 110;

function glyphCoverage(px, py) {
  const outer1 = roundedRectSdf(px, py, LINK1_CX, GLYPH_Y, GLYPH_HALF_W, GLYPH_HALF_H, GLYPH_RADIUS);
  const inner1 = roundedRectSdf(
    px, py, LINK1_CX, GLYPH_Y,
    GLYPH_HALF_W - STROKE, GLYPH_HALF_H - STROKE, Math.max(GLYPH_RADIUS - STROKE, 8),
  );
  const outer2 = roundedRectSdf(px, py, LINK2_CX, GLYPH_Y, GLYPH_HALF_W, GLYPH_HALF_H, GLYPH_RADIUS);
  const inner2 = roundedRectSdf(
    px, py, LINK2_CX, GLYPH_Y,
    GLYPH_HALF_W - STROKE, GLYPH_HALF_H - STROKE, Math.max(GLYPH_RADIUS - STROKE, 8),
  );
  const ring1 = Math.max(outer1, -inner1);
  const ring2 = Math.max(outer2, -inner2);
  const glyph = Math.min(ring1, ring2);
  if (glyph <= -0.5) return 1;
  if (glyph >= 0.5) return 0;
  return 0.5 - glyph;
}

const px = Buffer.alloc(SIZE * (1 + SIZE * 4));
for (let y = 0; y < SIZE; y++) {
  const rowStart = y * (1 + SIZE * 4);
  px[rowStart] = 0; // PNG filter: none
  for (let x = 0; x < SIZE; x++) {
    const tile = roundedRectCoverage(x, y, SIZE / 2, SIZE / 2, TILE_HALF, TILE_HALF, TILE_RADIUS);
    const t = (x / SIZE + y / SIZE) / 2;
    let r = lerp(0.36, 0.18, t) * 255;
    let g = lerp(0.40, 0.22, t) * 255;
    let b = lerp(0.85, 0.38, t) * 255;
    let a = tile * 255;

    const glyph = glyphCoverage(x, y);
    if (glyph > 0) {
      r = lerp(245, r, 1 - glyph);
      g = lerp(247, g, 1 - glyph);
      b = lerp(255, b, 1 - glyph);
      a = Math.max(a, glyph * 255 * tile);
    }

    const off = rowStart + 1 + x * 4;
    px[off] = Math.round(r);
    px[off + 1] = Math.round(g);
    px[off + 2] = Math.round(b);
    px[off + 3] = Math.round(a);
  }
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // color type RGBA
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(px, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

const out = join(dirname(fileURLToPath(import.meta.url)), "..", "apps", "desktop", "app-icon.png");
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, png);
console.log(`wrote ${out} (${png.length} bytes)`);
