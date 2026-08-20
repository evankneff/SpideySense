// Generates the app icon PNGs from scratch (no image deps): dark rounded square + lens circle.
const fs = require('fs'), zlib = require('zlib');

function crc32(buf) {
  let t = crc32.t;
  if (!t) { t = crc32.t = []; for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } }
  let c = 0xffffffff;
  for (const b of buf) c = t[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
function png(w, h, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; ihdr[9] = 6; // 8-bit RGBA
  const raw = Buffer.alloc(h * (w * 4 + 1));
  for (let y = 0; y < h; y++) { raw[y * (w * 4 + 1)] = 0; rgba.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4); }
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw, {level:9})), chunk('IEND', Buffer.alloc(0))]);
}

// 4x supersampled coverage so edges are smooth at 16px tray size.
function render(S, lens) {
  const buf = Buffer.alloc(S * S * 4);
  const R = S * 0.22, C = S / 2, lensR = S * 0.30, ringR = S * 0.40, SS = 4;
  const inRound = (x, y) => { // rounded-square signed distance test
    const dx = Math.max(Math.abs(x - C) - (S * 0.5 - R), 0), dy = Math.max(Math.abs(y - C) - (S * 0.5 - R), 0);
    return Math.hypot(dx, dy) <= R;
  };
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    let bg = 0, ring = 0, len = 0;
    for (let sy = 0; sy < SS; sy++) for (let sx = 0; sx < SS; sx++) {
      const px = x + (sx + 0.5) / SS, py = y + (sy + 0.5) / SS, d = Math.hypot(px - C, py - C);
      if (inRound(px, py)) bg++;
      if (d <= ringR && d >= ringR - S * 0.045) ring++;
      if (d <= lensR) len++;
    }
    const n = SS * SS, i = (y * S + x) * 4;
    let r = 27, g = 31, b = 36, a = bg / n;                       // #1b1f24 body
    const mix = (cr, cg, cb, w) => { r = r * (1 - w) + cr * w; g = g * (1 - w) + cg * w; b = b * (1 - w) + cb * w; a = Math.max(a, w); };
    if (ring) mix(...(lens ? [90, 96, 104] : [255, 107, 53]), ring / n);
    if (len)  mix(...(lens ? [70, 76, 84]  : [255, 158, 53]), len / n);
    buf[i] = Math.round(r); buf[i+1] = Math.round(g); buf[i+2] = Math.round(b); buf[i+3] = Math.round(a * 255);
  }
  return png(S, S, buf);
}
fs.writeFileSync(process.argv[2], render(parseInt(process.argv[4] || "1024", 10), process.argv[3] === "paused"));
