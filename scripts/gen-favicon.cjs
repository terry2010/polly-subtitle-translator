// 从 512 源 PNG 生成网站 favicon 全套（纯 Node，无第三方依赖）
// 用法: node scripts/gen-favicon.cjs
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const SRC = path.join(__dirname, '..', 'docs', '卡通Q版鹦鹉icon-512.png');
const OUT = path.join(__dirname, '..', 'public');

// ---------- PNG 解码 ----------
function readPNG(buf) {
  if (buf[0] !== 0x89 || buf[1] !== 0x50) throw new Error('not png');
  let off = 8;
  let width = 0, height = 0;
  let idat = [];
  let bitDepth = 0, colorType = 0;
  let palette = null, trns = null;
  while (off < buf.length) {
    const len = buf.readUInt32BE(off); off += 4;
    const type = buf.toString('ascii', off, off + 4); off += 4;
    const data = buf.slice(off, off + len); off += len;
    off += 4; // crc
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
    } else if (type === 'PLTE') {
      palette = data;
    } else if (type === 'tRNS') {
      trns = data;
    } else if (type === 'IDAT') {
      idat.push(data);
    } else if (type === 'IEND') {
      break;
    }
  }
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const bytesPerPixel = colorType === 6 ? 4 : colorType === 2 ? 3 : colorType === 4 ? 2 : 1;
  const bpp = Math.max(1, bytesPerPixel);
  const stride = width * bpp + 1;
  // 解除行过滤
  const out = Buffer.alloc(width * height * 4);
  let prev = Buffer.alloc(stride);
  let cur = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const rowStart = y * stride;
    const filter = raw[rowStart];
    const srcRow = raw.slice(rowStart + 1, rowStart + stride);
    cur[0] = filter;
    srcRow.copy(cur, 1);
    unfilter(cur, prev, bpp, width);
    // 转成 RGBA
    for (let x = 0; x < width; x++) {
      const si = 1 + x * bpp;
      const di = (y * width + x) * 4;
      if (colorType === 6) {
        out[di] = cur[si]; out[di+1] = cur[si+1]; out[di+2] = cur[si+2]; out[di+3] = cur[si+3];
      } else if (colorType === 2) {
        out[di] = cur[si]; out[di+1] = cur[si+1]; out[di+2] = cur[si+2]; out[di+3] = 255;
      } else if (colorType === 4) {
        out[di] = cur[si]; out[di+1] = cur[si]; out[di+2] = cur[si]; out[di+3] = cur[si+1];
      } else if (colorType === 3) {
        const idx = cur[si];
        out[di] = palette[idx*3]; out[di+1] = palette[idx*3+1]; out[di+2] = palette[idx*3+2];
        out[di+3] = trns ? trns[idx] : 255;
      }
    }
    const tmp = prev; prev = cur; cur = tmp;
  }
  return { width, height, data: out };
}

function unfilter(cur, prev, bpp, width) {
  const filter = cur[0];
  const stride = width * bpp + 1;
  if (filter === 0) return;
  for (let i = 1; i < stride; i++) {
    const a = i > bpp ? cur[i - bpp] : 0;
    const b = prev[i];
    const c = i > bpp ? prev[i - bpp] : 0;
    if (filter === 1) cur[i] = (cur[i] + a) & 0xff;
    else if (filter === 2) cur[i] = (cur[i] + b) & 0xff;
    else if (filter === 3) cur[i] = (cur[i] + ((a + b) >> 1)) & 0xff;
    else if (filter === 4) {
      const p = a + b - c;
      const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      let pr = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      cur[i] = (cur[i] + pr) & 0xff;
    }
  }
}

// ---------- PNG 编码 ----------
function writePNG(rgba, w, h) {
  const sig = Buffer.from([137,80,78,71,13,10,26,10]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; ihdr[9] = 6; // 8bit RGBA
  // 行过滤全用 0
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 4 + 1)] = 0;
    rgba.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4);
  }
  const idat = zlib.deflateSync(raw, { level: 9 });
  return Buffer.concat([sig, chunk('IHDR', ihdr), chunk('IDAT', idat), chunk('IEND', Buffer.alloc(0))]);
}

function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length, 0);
  const t = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])), 0);
  return Buffer.concat([len, t, data, crc]);
}

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// ---------- 缩放（box 平均）----------
function resize(src, sw, sh, dw, dh) {
  const dst = Buffer.alloc(dw * dh * 4);
  const xRatio = sw / dw, yRatio = sh / dh;
  for (let dy = 0; dy < dh; dy++) {
    const y0 = Math.floor(dy * yRatio), y1 = Math.max(y0 + 1, Math.floor((dy + 1) * yRatio));
    for (let dx = 0; dx < dw; dx++) {
      const x0 = Math.floor(dx * xRatio), x1 = Math.max(x0 + 1, Math.floor((dx + 1) * xRatio));
      let r = 0, g = 0, b = 0, a = 0, cnt = 0;
      for (let yy = y0; yy < y1 && yy < sh; yy++) {
        for (let xx = x0; xx < x1 && xx < sw; xx++) {
          const si = (yy * sw + xx) * 4;
          const al = src[si + 3];
          r += src[si] * al; g += src[si+1] * al; b += src[si+2] * al; a += al;
          cnt++;
        }
      }
      const di = (dy * dw + dx) * 4;
      if (a > 0) {
        dst[di] = Math.round(r / a); dst[di+1] = Math.round(g / a); dst[di+2] = Math.round(b / a); dst[di+3] = Math.round(a / cnt);
      } else {
        dst[di] = dst[di+1] = dst[di+2] = 0; dst[di+3] = 0;
      }
    }
  }
  return dst;
}

// ---------- ICO 编码 ----------
function writeICO(images) {
  // images: [{width, height, png: Buffer}]
  const count = images.length;
  const headerSize = 6 + count * 16;
  const parts = [Buffer.alloc(0)];
  let offset = headerSize;
  const dir = Buffer.alloc(headerSize);
  dir.writeUInt16LE(0, 0); dir.writeUInt16LE(1, 2); dir.writeUInt16LE(count, 4);
  images.forEach((img, i) => {
    const e = 6 + i * 16;
    dir[e] = img.width >= 256 ? 0 : img.width;
    dir[e+1] = img.height >= 256 ? 0 : img.height;
    dir[e+2] = 0; dir[e+3] = 0; // palette, reserved
    dir.writeUInt16LE(1, e+4); // planes
    dir.writeUInt16LE(32, e+6); // bpp
    dir.writeUInt32LE(img.png.length, e+8);
    dir.writeUInt32LE(offset, e+12);
    offset += img.png.length;
  });
  return Buffer.concat([dir, ...images.map(i => i.png)]);
}

// ---------- 主流程 ----------
const src = readPNG(fs.readFileSync(SRC));
console.log('source:', src.width + 'x' + src.height);

fs.mkdirSync(OUT, { recursive: true });

// PNG 各尺寸
const sizes = [16, 32, 48, 180, 192, 512];
for (const s of sizes) {
  const r = resize(src.data, src.width, src.height, s, s);
  const png = writePNG(r, s, s);
  const name = s === 512 ? 'favicon-512.png' : `favicon-${s}.png`;
  fs.writeFileSync(path.join(OUT, name), png);
  console.log('wrote', name, png.length, 'bytes');
}

// favicon.ico (16/32/48)
const icoSizes = [16, 32, 48];
const icoImgs = icoSizes.map(s => {
  const r = resize(src.data, src.width, src.height, s, s);
  return { width: s, height: s, png: writePNG(r, s, s) };
});
fs.writeFileSync(path.join(OUT, 'favicon.ico'), writeICO(icoImgs));
console.log('wrote favicon.ico');

// apple-touch-icon.png = 180
fs.copyFileSync(path.join(OUT, 'favicon-180.png'), path.join(OUT, 'apple-touch-icon.png'));
console.log('wrote apple-touch-icon.png');

console.log('done ->', OUT);
