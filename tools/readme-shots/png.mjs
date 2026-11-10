// Lossless PNG optimizer without dependencies: decodes the browser's screenshot, drops an alpha
// channel that is fully opaque, picks the best filter per row and deflates at the highest level.
// Supports what Chromium writes: 8 bit, RGB or RGBA, not interlaced.
import { deflateSync, inflateSync, constants } from "node:zlib";

const SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])), 0);
  return Buffer.concat([head, data, crc]);
}

function paeth(a, b, c) {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
}

/** Decodes to raw pixel rows (without filter bytes). */
function decode(png) {
  if (!png.subarray(0, 8).equals(SIGNATURE)) throw new Error("Not a PNG file.");
  let offset = 8;
  let header = null;
  const idat = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      header = {
        width: data.readUInt32BE(0),
        height: data.readUInt32BE(4),
        bitDepth: data[8],
        colorType: data[9],
        interlace: data[12],
      };
    } else if (type === "IDAT") {
      idat.push(data);
    }
    offset += 12 + length;
  }
  if (!header || header.bitDepth !== 8 || ![2, 6].includes(header.colorType) || header.interlace) {
    throw new Error("Unsupported PNG layout (expected 8 bit RGB or RGBA, not interlaced).");
  }
  const channels = header.colorType === 6 ? 4 : 3;
  const stride = header.width * channels;
  const filtered = inflateSync(Buffer.concat(idat));
  const pixels = Buffer.alloc(stride * header.height);
  for (let y = 0; y < header.height; y++) {
    const filter = filtered[y * (stride + 1)];
    const src = filtered.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const row = pixels.subarray(y * stride, (y + 1) * stride);
    const prev = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? row[x - channels] : 0;
      const b = prev ? prev[x] : 0;
      const c = prev && x >= channels ? prev[x - channels] : 0;
      const predictor = [0, a, b, (a + b) >>> 1, paeth(a, b, c)][filter];
      if (predictor === undefined) throw new Error(`Unknown PNG filter ${filter}.`);
      row[x] = (src[x] + predictor) & 0xff;
    }
  }
  return { width: header.width, height: header.height, channels, pixels };
}

/** Removes the alpha channel if every pixel is fully opaque. */
function dropOpaqueAlpha(image) {
  if (image.channels !== 4) return image;
  const { pixels } = image;
  for (let i = 3; i < pixels.length; i += 4) if (pixels[i] !== 255) return image;
  const rgb = Buffer.alloc((pixels.length / 4) * 3);
  for (let i = 0, j = 0; i < pixels.length; i += 4, j += 3) {
    rgb[j] = pixels[i];
    rgb[j + 1] = pixels[i + 1];
    rgb[j + 2] = pixels[i + 2];
  }
  return { ...image, channels: 3, pixels: rgb };
}

/** Filters every row with the filter that minimizes the sum of absolute differences. */
function filterRows({ width, height, channels, pixels }) {
  const stride = width * channels;
  const out = Buffer.alloc((stride + 1) * height);
  const candidate = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const row = pixels.subarray(y * stride, (y + 1) * stride);
    const prev = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : null;
    let best = -1;
    let bestScore = Infinity;
    for (let filter = 0; filter < 5; filter++) {
      let score = 0;
      for (let x = 0; x < stride; x++) {
        const a = x >= channels ? row[x - channels] : 0;
        const b = prev ? prev[x] : 0;
        const c = prev && x >= channels ? prev[x - channels] : 0;
        const predictor =
          filter === 0
            ? 0
            : filter === 1
              ? a
              : filter === 2
                ? b
                : filter === 3
                  ? (a + b) >>> 1
                  : paeth(a, b, c);
        const value = (row[x] - predictor) & 0xff;
        candidate[x] = value;
        score += value < 128 ? value : 256 - value;
      }
      if (score < bestScore) {
        bestScore = score;
        best = filter;
        out[y * (stride + 1)] = filter;
        candidate.copy(out, y * (stride + 1) + 1);
      }
    }
    if (best < 0) throw new Error("No PNG filter chosen.");
  }
  return out;
}

/** Re-encodes a PNG losslessly and as small as this simple encoder can make it. */
export function optimizePng(png) {
  const image = dropOpaqueAlpha(decode(png));
  const filtered = filterRows(image);
  const level = constants.Z_BEST_COMPRESSION;
  const compressed = [constants.Z_DEFAULT_STRATEGY, constants.Z_FILTERED]
    .map((strategy) => deflateSync(filtered, { level, memLevel: 9, strategy }))
    .reduce((smallest, next) => (next.length < smallest.length ? next : smallest));
  const header = Buffer.alloc(13);
  header.writeUInt32BE(image.width, 0);
  header.writeUInt32BE(image.height, 4);
  header[8] = 8;
  header[9] = image.channels === 4 ? 6 : 2;
  const result = Buffer.concat([
    SIGNATURE,
    chunk("IHDR", header),
    chunk("IDAT", compressed),
    chunk("IEND", Buffer.alloc(0)),
  ]);
  // Guard against encoder bugs: the result must decode to exactly the same pixels.
  const check = decode(result);
  if (check.channels !== image.channels || !check.pixels.equals(image.pixels)) {
    throw new Error("PNG optimization changed the image.");
  }
  return result.length < png.length ? result : png;
}

/** Width and height from the IHDR chunk. */
export function pngSize(png) {
  return { width: png.readUInt32BE(16), height: png.readUInt32BE(20) };
}
