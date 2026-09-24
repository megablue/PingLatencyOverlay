// Generates the Tauri app icons without any external dependencies.
// Run with: npm run icons
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "icons");
mkdirSync(outDir, { recursive: true });

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crcBuf]);
}

function encodePng(size, pixel) {
  const stride = size * 4 + 1;
  const raw = Buffer.alloc(stride * size);
  for (let y = 0; y < size; y++) {
    raw[y * stride] = 0; // filter: none
    for (let x = 0; x < size; x++) {
      const [r, g, b, a] = pixel(x, y, size);
      const o = y * stride + 1 + x * 4;
      raw[o] = r;
      raw[o + 1] = g;
      raw[o + 2] = b;
      raw[o + 3] = a;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// --- Icon artwork: dark rounded tile with a latency trace. ---

function mix(a, b, t) {
  return a + (b - a) * t;
}

function distanceToSegment(px, py, x1, y1, x2, y2) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const lenSq = dx * dx + dy * dy;
  let t = lenSq === 0 ? 0 : ((px - x1) * dx + (py - y1) * dy) / lenSq;
  t = Math.max(0, Math.min(1, t));
  const cx = x1 + t * dx;
  const cy = y1 + t * dy;
  return Math.hypot(px - cx, py - cy);
}

// Normalised trace points in [0,1] space.
const trace = [
  [0.16, 0.62],
  [0.3, 0.4],
  [0.44, 0.72],
  [0.58, 0.34],
  [0.72, 0.52],
  [0.86, 0.46],
];

function pixel(x, y, size) {
  const nx = x / size;
  const ny = y / size;

  // Rounded-square mask.
  const r = 0.22;
  const cx = Math.min(nx, 1 - nx);
  const cy = Math.min(ny, 1 - ny);
  let inside = true;
  if (cx < r && cy < r) {
    inside = Math.hypot(r - cx, r - cy) <= r;
  }
  if (!inside) return [0, 0, 0, 0];

  // Background gradient.
  const t = (nx + ny) / 2;
  let R = mix(15, 30, t);
  let G = mix(23, 41, t);
  let B = mix(42, 59, t);

  const stroke = size * 0.045;

  // Green latency trace.
  let best = Infinity;
  for (let i = 0; i < trace.length - 1; i++) {
    const [x1, y1] = trace[i];
    const [x2, y2] = trace[i + 1];
    const d = distanceToSegment(nx, ny, x1, y1, x2, y2);
    if (d < best) best = d;
  }
  if (best * size < stroke) {
    R = 74;
    G = 222;
    B = 128;
  }

  // Red timeout marker.
  const timeoutX = 0.5;
  if (Math.abs(nx - timeoutX) * size < stroke * 0.6 && ny > 0.24 && ny < 0.76) {
    R = 239;
    G = 68;
    B = 68;
  }

  return [Math.round(R), Math.round(G), Math.round(B), 255];
}

function icoFromPng(png, size) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(1, 4);
  const entry = Buffer.alloc(16);
  entry.writeUInt8(size >= 256 ? 0 : size, 0);
  entry.writeUInt8(size >= 256 ? 0 : size, 1);
  entry.writeUInt8(0, 2);
  entry.writeUInt8(0, 3);
  entry.writeUInt16LE(1, 4);
  entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(png.length, 8);
  entry.writeUInt32LE(22, 12);
  return Buffer.concat([header, entry, png]);
}

const png32 = encodePng(32, pixel);
const png128 = encodePng(128, pixel);
const png256 = encodePng(256, pixel);

writeFileSync(join(outDir, "32x32.png"), png32);
writeFileSync(join(outDir, "128x128.png"), png128);
writeFileSync(join(outDir, "128x128@2x.png"), png256);
writeFileSync(join(outDir, "icon.png"), png256);
writeFileSync(join(outDir, "icon.ico"), icoFromPng(png256, 256));

console.log("Wrote icons to", outDir);
