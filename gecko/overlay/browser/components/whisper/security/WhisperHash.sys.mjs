/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * SHA-256, SHA-512 and HMAC-SHA-512, synchronous, in plain JavaScript.
 *
 * Why not WebCrypto: crypto.subtle is asynchronous, and the input guards decide inside
 * an event listener, before the page sees the event, so they need an answer at once.
 * The inputs are tiny (a recovery phrase is at most a few hundred bytes), so speed
 * does not matter here. Used for the BIP39 and Electrum checksums (SeedPhrase.sys.mjs)
 * and for Base58Check addresses (CryptoAddress.sys.mjs). Nothing here keeps a secret:
 * these are checksums of text the user is entering right now.
 *
 * The module has no Gecko dependencies, so gecko/tests/security runs it under Node
 * against published test vectors.
 */

const K256 = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
  0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
  0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
  0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
  0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
  0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

/** Pads a message the way SHA-256 and SHA-512 do (big-endian bit length). */
function pad(bytes, blockSize, lengthBytes) {
  const total =
    Math.ceil((bytes.length + 1 + lengthBytes) / blockSize) * blockSize;
  const out = new Uint8Array(total);
  out.set(bytes);
  out[bytes.length] = 0x80;
  // Inputs here are far below 2^32 bytes; the higher length words stay zero.
  const bits = bytes.length * 8;
  const view = new DataView(out.buffer);
  view.setUint32(total - 4, bits >>> 0);
  view.setUint32(total - 8, Math.floor(bits / 0x100000000));
  return out;
}

/**
 * @param {Uint8Array} bytes
 * @returns {Uint8Array} 32 bytes
 */
export function sha256(bytes) {
  const data = pad(bytes, 64, 8);
  const view = new DataView(data.buffer);
  const h = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
    0x1f83d9ab, 0x5be0cd19,
  ]);
  const w = new Uint32Array(64);
  const rotr = (x, n) => (x >>> n) | (x << (32 - n));
  for (let off = 0; off < data.length; off += 64) {
    for (let i = 0; i < 16; i++) {
      w[i] = view.getUint32(off + i * 4);
    }
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 64; i++) {
      const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (hh + S1 + ch + K256[i] + w[i]) >>> 0;
      const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (S0 + maj) >>> 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    h[0] += a;
    h[1] += b;
    h[2] += c;
    h[3] += d;
    h[4] += e;
    h[5] += f;
    h[6] += g;
    h[7] += hh;
  }
  const out = new Uint8Array(32);
  const ov = new DataView(out.buffer);
  for (let i = 0; i < 8; i++) {
    ov.setUint32(i * 4, h[i]);
  }
  return out;
}

// SHA-512 works on 64-bit words; each is kept as a high and a low 32-bit half.
const K512 = [
  "428a2f98d728ae22", "7137449123ef65cd", "b5c0fbcfec4d3b2f", "e9b5dba58189dbbc",
  "3956c25bf348b538", "59f111f1b605d019", "923f82a4af194f9b", "ab1c5ed5da6d8118",
  "d807aa98a3030242", "12835b0145706fbe", "243185be4ee4b28c", "550c7dc3d5ffb4e2",
  "72be5d74f27b896f", "80deb1fe3b1696b1", "9bdc06a725c71235", "c19bf174cf692694",
  "e49b69c19ef14ad2", "efbe4786384f25e3", "0fc19dc68b8cd5b5", "240ca1cc77ac9c65",
  "2de92c6f592b0275", "4a7484aa6ea6e483", "5cb0a9dcbd41fbd4", "76f988da831153b5",
  "983e5152ee66dfab", "a831c66d2db43210", "b00327c898fb213f", "bf597fc7beef0ee4",
  "c6e00bf33da88fc2", "d5a79147930aa725", "06ca6351e003826f", "142929670a0e6e70",
  "27b70a8546d22ffc", "2e1b21385c26c926", "4d2c6dfc5ac42aed", "53380d139d95b3df",
  "650a73548baf63de", "766a0abb3c77b2a8", "81c2c92e47edaee6", "92722c851482353b",
  "a2bfe8a14cf10364", "a81a664bbc423001", "c24b8b70d0f89791", "c76c51a30654be30",
  "d192e819d6ef5218", "d69906245565a910", "f40e35855771202a", "106aa07032bbd1b8",
  "19a4c116b8d2d0c8", "1e376c085141ab53", "2748774cdf8eeb99", "34b0bcb5e19b48a8",
  "391c0cb3c5c95a63", "4ed8aa4ae3418acb", "5b9cca4f7763e373", "682e6ff3d6b2b8a3",
  "748f82ee5defb2fc", "78a5636f43172f60", "84c87814a1f0ab72", "8cc702081a6439ec",
  "90befffa23631e28", "a4506cebde82bde9", "bef9a3f7b2c67915", "c67178f2e372532b",
  "ca273eceea26619c", "d186b8c721c0c207", "eada7dd6cde0eb1e", "f57d4f7fee6ed178",
  "06f067aa72176fba", "0a637dc5a2c898a6", "113f9804bef90dae", "1b710b35131c471b",
  "28db77f523047d84", "32caab7b40c72493", "3c9ebe0a15c9bebc", "431d67c49c100d4c",
  "4cc5d4becb3e42b6", "597f299cfc657e2a", "5fcb6fab3ad6faec", "6c44198c4a475817",
];
const K512_HI = new Uint32Array(K512.map(k => parseInt(k.slice(0, 8), 16)));
const K512_LO = new Uint32Array(K512.map(k => parseInt(k.slice(8), 16)));

// 64-bit helpers on (hi, lo) pairs of unsigned 32-bit numbers; each returns [hi, lo].
function add64(ah, al, bh, bl) {
  const lo = al + bl;
  return [(ah + bh + (lo > 0xffffffff ? 1 : 0)) >>> 0, lo >>> 0];
}
function rotr64(h, l, n) {
  if (n < 32) {
    return [((h >>> n) | (l << (32 - n))) >>> 0, ((l >>> n) | (h << (32 - n))) >>> 0];
  }
  const m = n - 32;
  return [((l >>> m) | (h << (32 - m))) >>> 0, ((h >>> m) | (l << (32 - m))) >>> 0];
}
function shr64(h, l, n) {
  return [h >>> n, ((l >>> n) | (h << (32 - n))) >>> 0];
}
function xor3(a, b, c) {
  return [(a[0] ^ b[0] ^ c[0]) >>> 0, (a[1] ^ b[1] ^ c[1]) >>> 0];
}

/**
 * @param {Uint8Array} bytes
 * @returns {Uint8Array} 64 bytes
 */
export function sha512(bytes) {
  const data = pad(bytes, 128, 16);
  const view = new DataView(data.buffer);
  const hh = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
    0x1f83d9ab, 0x5be0cd19,
  ]);
  const hl = new Uint32Array([
    0xf3bcc908, 0x84caa73b, 0xfe94f82b, 0x5f1d36f1, 0xade682d1, 0x2b3e6c1f,
    0xfb41bd6b, 0x137e2179,
  ]);
  const wh = new Uint32Array(80);
  const wl = new Uint32Array(80);

  for (let off = 0; off < data.length; off += 128) {
    for (let i = 0; i < 16; i++) {
      wh[i] = view.getUint32(off + i * 8);
      wl[i] = view.getUint32(off + i * 8 + 4);
    }
    for (let i = 16; i < 80; i++) {
      const s0 = xor3(
        rotr64(wh[i - 15], wl[i - 15], 1),
        rotr64(wh[i - 15], wl[i - 15], 8),
        shr64(wh[i - 15], wl[i - 15], 7)
      );
      const s1 = xor3(
        rotr64(wh[i - 2], wl[i - 2], 19),
        rotr64(wh[i - 2], wl[i - 2], 61),
        shr64(wh[i - 2], wl[i - 2], 6)
      );
      let [h1, l1] = add64(wh[i - 16], wl[i - 16], s0[0], s0[1]);
      [h1, l1] = add64(h1, l1, wh[i - 7], wl[i - 7]);
      [h1, l1] = add64(h1, l1, s1[0], s1[1]);
      wh[i] = h1;
      wl[i] = l1;
    }
    // Working variables a..h as [hi, lo] pairs.
    const v = [];
    for (let i = 0; i < 8; i++) {
      v.push([hh[i], hl[i]]);
    }
    for (let i = 0; i < 80; i++) {
      const [a, b, c, d, e, f, g, x] = v;
      const S1 = xor3(rotr64(e[0], e[1], 14), rotr64(e[0], e[1], 18), rotr64(e[0], e[1], 41));
      const ch = [((e[0] & f[0]) ^ (~e[0] & g[0])) >>> 0, ((e[1] & f[1]) ^ (~e[1] & g[1])) >>> 0];
      let t1 = add64(x[0], x[1], S1[0], S1[1]);
      t1 = add64(t1[0], t1[1], ch[0], ch[1]);
      t1 = add64(t1[0], t1[1], K512_HI[i], K512_LO[i]);
      t1 = add64(t1[0], t1[1], wh[i], wl[i]);
      const S0 = xor3(rotr64(a[0], a[1], 28), rotr64(a[0], a[1], 34), rotr64(a[0], a[1], 39));
      const maj = [
        ((a[0] & b[0]) ^ (a[0] & c[0]) ^ (b[0] & c[0])) >>> 0,
        ((a[1] & b[1]) ^ (a[1] & c[1]) ^ (b[1] & c[1])) >>> 0,
      ];
      const t2 = add64(S0[0], S0[1], maj[0], maj[1]);
      v[7] = g;
      v[6] = f;
      v[5] = e;
      v[4] = add64(d[0], d[1], t1[0], t1[1]);
      v[3] = c;
      v[2] = b;
      v[1] = a;
      v[0] = add64(t1[0], t1[1], t2[0], t2[1]);
    }
    for (let i = 0; i < 8; i++) {
      [hh[i], hl[i]] = add64(hh[i], hl[i], v[i][0], v[i][1]);
    }
  }
  const out = new Uint8Array(64);
  const ov = new DataView(out.buffer);
  for (let i = 0; i < 8; i++) {
    ov.setUint32(i * 8, hh[i]);
    ov.setUint32(i * 8 + 4, hl[i]);
  }
  return out;
}

/**
 * HMAC-SHA-512 (RFC 2104), as Electrum uses it for its seed version check.
 *
 * @param {Uint8Array} key
 * @param {Uint8Array} message
 * @returns {Uint8Array} 64 bytes
 */
export function hmacSha512(key, message) {
  const block = 128;
  const k = key.length > block ? sha512(key) : key;
  const padded = new Uint8Array(block);
  padded.set(k);
  const inner = new Uint8Array(block + message.length);
  const outer = new Uint8Array(block + 64);
  for (let i = 0; i < block; i++) {
    inner[i] = padded[i] ^ 0x36;
    outer[i] = padded[i] ^ 0x5c;
  }
  inner.set(message, block);
  outer.set(sha512(inner), block);
  return sha512(outer);
}

/** UTF-8 bytes of a string. */
export function utf8(text) {
  return new TextEncoder().encode(text);
}

/** Lower-case hex of bytes. */
export function hex(bytes) {
  let s = "";
  for (const b of bytes) {
    s += b.toString(16).padStart(2, "0");
  }
  return s;
}
