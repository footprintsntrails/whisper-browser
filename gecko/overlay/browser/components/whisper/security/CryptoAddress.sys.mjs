/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Recognises a cryptocurrency address in copied or pasted text, and compares two.
 *
 * Used by the clipboard address guard (WhisperInputGuardChild.sys.mjs): clipboard
 * hijacking malware watches the clipboard and swaps a copied address for one of the
 * same chain that belongs to the attacker. The guard remembers the address the user
 * copied in Whisper and, when an address of the same chain is pasted, compares.
 *
 * Only text that is a single address (optionally inside a payment link such as
 * "bitcoin:ADDRESS?amount=1") is recognised. Checksums are verified where the format
 * has one that needs no extra hash function: Base58Check (Bitcoin, Litecoin, Dogecoin,
 * Tron, XRP), Bech32 and Bech32m (SegWit, Cardano, Cosmos). Ethereum-style addresses
 * are recognised with or without the EIP-55 mixed-case checksum (the case itself is
 * not verified, and comparison ignores it); Solana, Monero, TON and Bitcoin Cash are
 * recognised by their shape.
 *
 * No Gecko dependencies (gecko/tests/security runs it under Node).
 */

import { sha256 } from "./WhisperHash.sys.mjs";

const B58_BITCOIN = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const B58_RIPPLE = "rpshnaf39wBUDNEGHJKLM4PQRST7VWXYZ2bcdeCg65jkm8oFqi1tuvAxyz";
const BECH32_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/** Base58 to bytes, or null on a character outside the alphabet. */
export function base58Decode(text, alphabet = B58_BITCOIN) {
  const bytes = []; // little-endian while decoding
  for (const ch of text) {
    const value = alphabet.indexOf(ch);
    if (value < 0) {
      return null;
    }
    let carry = value;
    for (let i = 0; i < bytes.length; i++) {
      carry += bytes[i] * 58;
      bytes[i] = carry & 0xff;
      carry >>= 8;
    }
    while (carry) {
      bytes.push(carry & 0xff);
      carry >>= 8;
    }
  }
  // Each leading zero digit ("1" in Bitcoin's alphabet) is a leading zero byte.
  for (const ch of text) {
    if (ch != alphabet[0]) {
      break;
    }
    bytes.push(0);
  }
  return Uint8Array.from(bytes.reverse());
}

/** Base58Check payload (version byte included), or null if the checksum is wrong. */
export function base58Check(text, alphabet = B58_BITCOIN) {
  const raw = base58Decode(text, alphabet);
  if (!raw || raw.length < 5) {
    return null;
  }
  const payload = raw.subarray(0, raw.length - 4);
  const check = sha256(sha256(payload));
  for (let i = 0; i < 4; i++) {
    if (check[i] != raw[raw.length - 4 + i]) {
      return null;
    }
  }
  return payload;
}

function bech32Polymod(values) {
  const GEN = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
  let chk = 1;
  for (const v of values) {
    const top = chk >>> 25;
    chk = (((chk & 0x1ffffff) << 5) ^ v) >>> 0;
    for (let i = 0; i < 5; i++) {
      if ((top >>> i) & 1) {
        chk = (chk ^ GEN[i]) >>> 0;
      }
    }
  }
  return chk;
}

/**
 * Bech32 or Bech32m (BIP 173, BIP 350): {hrp, data, variant} or null. Mixed case is
 * invalid. No length limit: Cardano addresses are longer than 90 characters.
 */
export function bech32Decode(text) {
  if (text != text.toLowerCase() && text != text.toUpperCase()) {
    return null;
  }
  const s = text.toLowerCase();
  const pos = s.lastIndexOf("1");
  if (pos < 1 || pos + 7 > s.length) {
    return null;
  }
  const hrp = s.slice(0, pos);
  const data = [];
  for (const ch of s.slice(pos + 1)) {
    const v = BECH32_CHARSET.indexOf(ch);
    if (v < 0) {
      return null;
    }
    data.push(v);
  }
  const expanded = [];
  for (const c of hrp) {
    expanded.push(c.charCodeAt(0) >> 5);
  }
  expanded.push(0);
  for (const c of hrp) {
    expanded.push(c.charCodeAt(0) & 31);
  }
  const mod = bech32Polymod([...expanded, ...data]);
  const variant = mod === 1 ? "bech32" : mod === 0x2bc830a3 ? "bech32m" : null;
  return variant ? { hrp, data: data.slice(0, -6), variant } : null;
}

/** A SegWit address: witness version 0 uses Bech32, 1 to 16 Bech32m (BIP 350). */
function segwit(text, hrps) {
  const d = bech32Decode(text);
  if (!d || !hrps.includes(d.hrp) || !d.data.length) {
    return false;
  }
  const version = d.data[0];
  return version === 0 ? d.variant == "bech32" : version <= 16 && d.variant == "bech32m";
}

const B58 = "[1-9A-HJ-NP-Za-km-z]";
const BECH = "[02-9ac-hj-np-z]";

/**
 * Chain families and how to recognise an address of each. `normalize` gives the form
 * in which two addresses are compared (hex and Bech32 ignore case).
 */
const FORMATS = [
  {
    family: "bitcoin",
    test: a =>
      (new RegExp(`^[13]${B58}{25,34}$`).test(a) &&
        [0x00, 0x05].includes(base58Check(a)?.[0])) ||
      (new RegExp(`^bc1${BECH}{8,87}$`, "i").test(a) && segwit(a, ["bc"])),
    normalize: a => (/^bc1/i.test(a) ? a.toLowerCase() : a),
  },
  {
    family: "bitcoin-testnet",
    test: a =>
      (new RegExp(`^[mn2]${B58}{25,34}$`).test(a) &&
        [0x6f, 0xc4].includes(base58Check(a)?.[0])) ||
      (new RegExp(`^(tb|bcrt)1${BECH}{8,87}$`, "i").test(a) && segwit(a, ["tb", "bcrt"])),
    normalize: a => (/^(tb|bcrt)1/i.test(a) ? a.toLowerCase() : a),
  },
  {
    family: "litecoin",
    test: a =>
      (new RegExp(`^[LM3]${B58}{25,34}$`).test(a) &&
        [0x30, 0x32].includes(base58Check(a)?.[0])) ||
      (new RegExp(`^ltc1${BECH}{8,87}$`, "i").test(a) && segwit(a, ["ltc"])),
    normalize: a => (/^ltc1/i.test(a) ? a.toLowerCase() : a),
  },
  {
    family: "dogecoin",
    test: a =>
      new RegExp(`^[DA9]${B58}{25,34}$`).test(a) &&
      [0x1e, 0x16].includes(base58Check(a)?.[0]),
    normalize: a => a,
  },
  {
    family: "tron",
    test: a => new RegExp(`^T${B58}{33}$`).test(a) && base58Check(a)?.[0] === 0x41,
    normalize: a => a,
  },
  {
    family: "xrp",
    test: a =>
      new RegExp(`^r${B58}{24,34}$`).test(a) && base58Check(a, B58_RIPPLE)?.[0] === 0x00,
    normalize: a => a,
  },
  {
    // Every EVM chain (Ethereum, BNB Chain, Polygon, Arbitrum, Base, ...).
    family: "ethereum",
    test: a => /^0x[0-9a-fA-F]{40}$/.test(a),
    normalize: a => a.toLowerCase(),
  },
  {
    family: "bitcoin-cash",
    test: a => new RegExp(`^(bitcoincash:)?[qp]${BECH}{41}$`, "i").test(a),
    normalize: a => a.toLowerCase().replace(/^bitcoincash:/, ""),
  },
  {
    family: "cardano",
    test: a => new RegExp(`^addr1${BECH}{50,200}$`, "i").test(a) && !!bech32Decode(a),
    normalize: a => a.toLowerCase(),
  },
  {
    family: "cosmos",
    test: a =>
      new RegExp(`^(cosmos|osmo|cro|juno|kava|inj|celestia|dydx)1${BECH}{38,58}$`, "i").test(a) &&
      !!bech32Decode(a),
    normalize: a => a.toLowerCase(),
  },
  {
    family: "monero",
    test: a => new RegExp(`^[48]${B58}{94}(${B58}{11})?$`).test(a),
    normalize: a => a,
  },
  {
    family: "ton",
    test: a => /^[EUk0]Q[A-Za-z0-9_-]{46}$/.test(a),
    normalize: a => a,
  },
  {
    // Last: any Base58 string that decodes to 32 bytes looks like a Solana key.
    family: "solana",
    test: a => new RegExp(`^${B58}{32,44}$`).test(a) && base58Decode(a)?.length === 32,
    normalize: a => a,
  },
];

// Payment links: "bitcoin:ADDRESS?amount=...", "ethereum:0x...@1/transfer?...".
const PAYMENT_LINK =
  /^(?:bitcoin|litecoin|dogecoin|ethereum|tron|solana|monero|ripple|xrp|cardano|ton):(?:\/\/)?([^?@/\s]+)/i;

/**
 * @param {string} text the whole copied or pasted text
 * @returns {null | {family: string, address: string, normalized: string}}
 */
export function classifyAddress(text) {
  if (typeof text != "string" || text.length > 300) {
    return null;
  }
  let candidate = text.trim();
  const link = PAYMENT_LINK.exec(candidate);
  if (link) {
    candidate = link[1];
  }
  if (!candidate || /\s/.test(candidate)) {
    return null;
  }
  for (const format of FORMATS) {
    if (format.test(candidate)) {
      return { family: format.family, address: candidate, normalized: format.normalize(candidate) };
    }
  }
  return null;
}

/**
 * Which characters differ, for highlighting: the longest common subsequence of the
 * two strings counts as "the same", everything else is marked.
 *
 * @returns {{a: boolean[], b: boolean[], changed: number}} true where it differs
 */
export function diffAddresses(a, b) {
  const n = a.length;
  const m = b.length;
  // Addresses are at most a few hundred characters.
  const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] == b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const da = new Array(n).fill(true);
  const db = new Array(m).fill(true);
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] == b[j]) {
      da[i++] = false;
      db[j++] = false;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  return {
    a: da,
    b: db,
    changed: Math.max(da.filter(Boolean).length, db.filter(Boolean).length),
  };
}
