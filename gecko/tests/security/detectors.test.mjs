/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Unit tests for the input guards' detectors: node --test gecko/tests/security/
// Every vector comes from a published source (named next to it) or was cross-checked
// with an independent implementation (Python's hashlib and hmac).

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const dir = path.join(here, "../../overlay/browser/components/whisper/security");
const H = await import(path.join(dir, "WhisperHash.sys.mjs"));
const SP = await import(path.join(dir, "SeedPhrase.sys.mjs"));
const CA = await import(path.join(dir, "CryptoAddress.sys.mjs"));
const { WORDLISTS } = await import(path.join(dir, "SeedWordlists.sys.mjs"));
SP.setLists(SP.buildLists(WORDLISTS));

test("SHA-256 and SHA-512 (FIPS 180-2 examples, then against Node's crypto)", () => {
  assert.equal(
    H.hex(H.sha256(H.utf8("abc"))),
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
  );
  assert.equal(
    H.hex(H.sha512(H.utf8("abc"))),
    "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a" +
      "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
  );
  for (const n of [0, 55, 56, 63, 64, 111, 112, 127, 128, 1000]) {
    const data = new Uint8Array(n).map((_, i) => (i * 7 + 3) & 0xff);
    assert.equal(H.hex(H.sha256(data)), createHash("sha256").update(data).digest("hex"), `sha256 ${n}`);
    assert.equal(H.hex(H.sha512(data)), createHash("sha512").update(data).digest("hex"), `sha512 ${n}`);
  }
});

test("HMAC-SHA-512 (RFC 4231 test case 2)", () => {
  assert.equal(
    H.hex(H.hmacSha512(H.utf8("Jefe"), H.utf8("what do ya want for nothing?"))),
    "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea250554" +
      "9758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
  );
});

test("the word lists are the official files, byte for byte", () => {
  for (const [language, { sha256, words }] of Object.entries(WORDLISTS)) {
    const file = words.split(" ").join("\n") + "\n";
    assert.equal(createHash("sha256").update(file, "utf8").digest("hex"), sha256, language);
  }
});

// BIP39 test vectors (trezor/python-mnemonic vectors.json, referenced by BIP 39).
const VALID = [
  "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
  "legal winner thank year wave sausage worth useful legal winner thank yellow",
  "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
  "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong",
  "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon agent",
  "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art",
  "scheme spot photo card baby mountain device kick cradle pact join borrow",
  "void come effort suffer camp survey warrior heavy shoot primary clutch crush open amazing screen patrol group space point ten exist slush involve unfold",
];

test("BIP39 phrases are recognised, with any separators and numbering", () => {
  for (const phrase of VALID) {
    const r = SP.detectPhrase(phrase);
    assert.ok(r, phrase);
    assert.equal(r.scheme, "bip39");
    assert.equal(r.count, phrase.split(" ").length);
  }
  const p = VALID[1].split(" ");
  assert.ok(SP.detectPhrase(p.join("\n")), "one word per line");
  assert.ok(SP.detectPhrase(p.join(", ")), "comma separated");
  assert.ok(SP.detectPhrase(p.map((w, i) => `${i + 1}. ${w}`).join("\n")), "numbered");
  assert.ok(SP.detectPhrase(`  ${p.join("  ").toUpperCase()}  `), "upper case, extra spaces");
});

test("other languages: Japanese with ideographic spaces, Spanish without accents", () => {
  const japanese =
    "あいこくしん　あいこくしん　あいこくしん　あいこくしん　あいこくしん　あいこくしん　" +
    "あいこくしん　あいこくしん　あいこくしん　あいこくしん　あいこくしん　あおぞら";
  assert.equal(SP.detectPhrase(japanese)?.language, "japanese");
  // Index 0 eleven times, then index 3: the "abandon ... about" entropy in any list.
  const spanish = WORDLISTS.spanish.words.split(" ");
  const phrase = [...Array(11).fill(spanish[0]), spanish[3]].join(" ");
  assert.equal(SP.detectPhrase(phrase)?.language, "spanish");
  const noAccents = phrase.normalize("NFKD").replace(/\p{M}/gu, "");
  assert.equal(SP.detectPhrase(noAccents)?.language, "spanish");
});

test("Electrum seeds (electrum/tests/test_wallet_vertical.py)", () => {
  for (const seed of [
    "cycle rocket west magnet parrot shuffle foot correct salt library feed song",
    "bitter grass shiver impose acquire brush forget axis eager alone wine silver",
    "wild father tree among universe such mobile favorite target dynamic credit identify",
  ]) {
    assert.equal(SP.detectPhrase(seed)?.scheme, "electrum", seed);
  }
});

test("near misses are not phrases", () => {
  assert.equal(SP.detectPhrase(Array(12).fill("abandon").join(" ")), null, "bad checksum");
  assert.equal(SP.detectPhrase(VALID[0].split(" ").slice(0, 11).join(" ")), null, "11 words");
  assert.equal(SP.detectPhrase(VALID[0].replace("about", "aboutt")), null, "not a list word");
  assert.equal(SP.detectPhrase(VALID[0] + " abandon"), null, "13 words");
  assert.equal(
    SP.detectPhrase("The quick brown fox jumps over the lazy dog again and again today"),
    null
  );
  assert.equal(SP.detectPhrase("a".repeat(5000)), null);
});

test("phrase start while typing: complete list words only, whitespace only", () => {
  const words = VALID[2].split(" ");
  assert.equal(SP.phraseStart(words.slice(0, 5).join(" ") + " ", 6), null);
  assert.equal(SP.phraseStart(words.slice(0, 6).join(" "), 6), null, "last word not finished");
  assert.equal(SP.phraseStart(words.slice(0, 6).join(" ") + " ", 6)?.count, 6);
  assert.equal(SP.phraseStart("first one thing, then another, there ", 6), null, "commas");
  assert.equal(SP.phraseStart(words.slice(0, 6).join(" ") + " ", 0), null, "turned off");
});

const ADDRESSES = [
  ["1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", "bitcoin"], // genesis block
  ["3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", "bitcoin"], // P2SH, Bitcoin wiki
  ["bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", "bitcoin"], // BIP 173
  ["BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4", "bitcoin"], // BIP 173, upper case
  ["bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0", "bitcoin"], // BIP 350
  ["tb1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3q0sl5k7", "bitcoin-testnet"],
  ["LVg2kJoFNg45Nbpy53h7Fe1wKyeXVRhMH9", "litecoin"],
  ["DH5yaieqoZN36fDVciNyRueRGvGLR3mr7L", "dogecoin"],
  ["TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t", "tron"], // USDT contract on Tron
  ["rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh", "xrp"], // XRP Ledger genesis account
  ["0x52908400098527886E0F7030069857D2E4169EE7", "ethereum"], // EIP-55 example
  ["0x52908400098527886e0f7030069857d2e4169ee7", "ethereum"], // the same without checksum
  ["So11111111111111111111111111111111111111112", "solana"], // wrapped SOL
  ["bitcoincash:qpm2qsznhks23z7629mms6s4cwef74vcwvy22gdx6a", "bitcoin-cash"],
  [
    "44AFFq5kSiGBoZ4NMDwYtN18obc8AemS33DBLWs3H7otXft3XjrpDtQGv7SqSsaBYBb98uNbr2VBBEt7f2wfn3RVGQBEP3A",
    "monero",
  ],
];

test("addresses are recognised by chain", () => {
  for (const [address, family] of ADDRESSES) {
    assert.equal(CA.classifyAddress(address)?.family, family, address);
    assert.equal(CA.classifyAddress(`  ${address}\n`)?.family, family, "with whitespace");
  }
  assert.equal(
    CA.classifyAddress("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?amount=0.1")?.address,
    "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"
  );
});

test("broken checksums and ordinary text are not addresses", () => {
  assert.equal(CA.classifyAddress("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb"), null);
  assert.equal(CA.classifyAddress("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t5"), null);
  assert.equal(CA.classifyAddress("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6u"), null);
  assert.equal(CA.classifyAddress("hello world"), null);
  assert.equal(CA.classifyAddress("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa and more"), null);
  assert.equal(CA.classifyAddress("https://example.com/"), null);
});

test("EVM addresses compare without case", () => {
  const a = CA.classifyAddress("0x52908400098527886E0F7030069857D2E4169EE7");
  const b = CA.classifyAddress("0x52908400098527886e0f7030069857d2e4169ee7");
  assert.equal(a.normalized, b.normalized);
});

test("the differing characters are marked", () => {
  const d = CA.diffAddresses("0xabcdef1234", "0xabcXef1234");
  const marked = flags => flags.map((x, i) => (x ? i : -1)).filter(i => i >= 0);
  assert.deepEqual(marked(d.a), [5]);
  assert.deepEqual(marked(d.b), [5]);
  assert.equal(d.changed, 1);
  assert.equal(CA.diffAddresses("abc", "abc").changed, 0);
});
