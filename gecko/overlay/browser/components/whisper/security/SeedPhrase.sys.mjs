/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Recognises a wallet recovery phrase in text a user is about to give a web page.
 *
 * A phrase is recognised only when all of this holds:
 *   - the text is nothing but words (separated by spaces, line breaks, commas or the
 *     ideographic space Japanese phrases use), optionally numbered ("1. word");
 *   - there are exactly 12, 15, 18, 21 or 24 words;
 *   - every word is in one and the same official BIP39 list (ten languages); accents
 *     may be left out, as BIP39 allows for Spanish and French;
 *   - the phrase checksum is valid: the BIP39 checksum, or, for 12 English words,
 *     Electrum's seed version (Electrum uses the same list but its own checksum).
 *
 * A valid checksum is what makes this strong evidence: random words from a list pass
 * it with probability 1/16 (12 words) down to 1/256 (24 words), and ordinary sentences
 * almost never consist of list words only (gecko/tests/security measures the rates).
 *
 * `phraseStart` is the weaker, earlier test used while someone types: the text so far
 * is only list words, at least `minWords` complete ones. It has no checksum to lean on,
 * so the caller only uses it to pause and ask.
 *
 * Nothing is stored: the text is only read, and its words live in local variables
 * while a call runs. No Gecko dependencies (gecko/tests/security runs it under Node).
 */

import { hmacSha512, sha256, utf8 } from "./WhisperHash.sys.mjs";

export const PHRASE_LENGTHS = Object.freeze([12, 15, 18, 21, 24]);

// A numbered 24-word phrase in the longest list stays well under this; longer text is
// never a phrase and is not split at all.
const MAX_TEXT_LENGTH = 600;
const MAX_WORDS = 24;

// Whitespace (includes U+3000 IDEOGRAPHIC SPACE), commas, the ideographic comma.
const SEPARATORS = /[\s,，、]+/u;
const TRAILING_SEPARATORS = /[\s,，、]+$/u;
// While typing, only whitespace separates words: a comma after a word is what
// ordinary sentences have ("first one thing, then another"), not phrases.
const TYPING_SEPARATORS = /\s+/u;
const TRAILING_WHITESPACE = /\s+$/u;
// "1.", "1)", "12:", "#3": numbering in front of a word, as in a numbered word list.
const NUMBERING = /^#?\d{1,2}[.):]?$/u;
const COMBINING_MARKS = /\p{M}/gu;

let gLists = null;

/** NFKD, lower case, combining marks removed: "Ábaco" -> "abaco". */
export function fold(word) {
  return word.normalize("NFKD").toLowerCase().replace(COMBINING_MARKS, "");
}

/**
 * @param {object} wordlists {language: {words: "w1 w2 ..."}} (SeedWordlists.sys.mjs)
 * @returns {Map<string, Map<string, number>>} folded word -> index, per language
 */
export function buildLists(wordlists) {
  const lists = new Map();
  for (const [language, { words: list }] of Object.entries(wordlists)) {
    const map = new Map();
    list.split(" ").forEach((word, index) => map.set(fold(word), index));
    if (map.size != 2048) {
      throw new Error(`BIP39 list ${language} has ${map.size} distinct words`);
    }
    lists.set(language, map);
  }
  return lists;
}

/** Installs the lists (built once per process by the caller, or by a test). */
export function setLists(lists) {
  gLists = lists;
}

export function haveLists() {
  return gLists !== null;
}

/**
 * Splits text into folded words, or returns null when it cannot be a phrase at all
 * (too long, too many words). Numbering in front of words is dropped.
 *
 * @param {string} text
 * @param {object} [options]
 * @param {boolean} [options.typing] text being typed: only whitespace separates
 *   words, and the last word counts only once whitespace follows it.
 * @returns {string[] | null}
 */
export function words(text, { typing = false } = {}) {
  if (typeof text != "string" || text.length > MAX_TEXT_LENGTH) {
    return null;
  }
  const body = text.replace(typing ? TRAILING_WHITESPACE : TRAILING_SEPARATORS, "");
  const lastComplete = !typing || body.length < text.length;
  const trimmed = body.trim();
  if (!trimmed) {
    return [];
  }
  const out = [];
  for (const part of trimmed.split(typing ? TYPING_SEPARATORS : SEPARATORS)) {
    if (NUMBERING.test(part)) {
      continue;
    }
    out.push(fold(part));
    if (out.length > MAX_WORDS + 1) {
      return null;
    }
  }
  if (!lastComplete) {
    out.pop();
  }
  return out;
}

/** The languages whose list holds every one of `tokens`, with the word indices. */
function matchingLists(tokens) {
  const result = [];
  if (!gLists || !tokens.length) {
    return result;
  }
  for (const [language, map] of gLists) {
    const indices = [];
    for (const token of tokens) {
      const index = map.get(token);
      if (index === undefined) {
        break;
      }
      indices.push(index);
    }
    if (indices.length == tokens.length) {
      result.push({ language, indices });
    }
  }
  return result;
}

/** BIP39: the first N/3 bits of SHA-256(entropy) end the 11*N-bit string. */
export function bip39ChecksumValid(indices) {
  const n = indices.length;
  if (!PHRASE_LENGTHS.includes(n)) {
    return false;
  }
  const totalBits = n * 11;
  const checksumBits = totalBits / 33;
  const entropyBits = totalBits - checksumBits;
  const bits = new Uint8Array(totalBits);
  indices.forEach((index, i) => {
    for (let b = 0; b < 11; b++) {
      bits[i * 11 + b] = (index >> (10 - b)) & 1;
    }
  });
  const entropy = new Uint8Array(entropyBits / 8);
  for (let i = 0; i < entropyBits; i++) {
    entropy[i >> 3] |= bits[i] << (7 - (i & 7));
  }
  const digest = sha256(entropy);
  for (let i = 0; i < checksumBits; i++) {
    if (((digest[i >> 3] >> (7 - (i & 7))) & 1) != bits[entropyBits + i]) {
      return false;
    }
  }
  return true;
}

// Electrum 2.x seed versions (electrum/version.py): the hex of
// HMAC-SHA512(key "Seed version", seed) starts with one of these.
const ELECTRUM_PREFIXES = ["01", "100", "101", "102"];

/** Electrum's seed version check on the normalised words, joined by single spaces. */
export function electrumSeedValid(tokens) {
  const mac = hmacSha512(utf8("Seed version"), utf8(tokens.join(" ")));
  const head =
    mac[0].toString(16).padStart(2, "0") + mac[1].toString(16).padStart(2, "0");
  return ELECTRUM_PREFIXES.some(prefix => head.startsWith(prefix));
}

/**
 * @param {string} text
 * @returns {null | {scheme: "bip39" | "electrum", language: string, count: number}}
 */
export function detectPhrase(text) {
  const tokens = words(text);
  if (!tokens || !PHRASE_LENGTHS.includes(tokens.length)) {
    return null;
  }
  for (const { language, indices } of matchingLists(tokens)) {
    if (bip39ChecksumValid(indices)) {
      return { scheme: "bip39", language, count: tokens.length };
    }
    if (language == "english" && tokens.length == 12 && electrumSeedValid(tokens)) {
      return { scheme: "electrum", language, count: 12 };
    }
  }
  return null;
}

/**
 * The text typed so far is the start of a phrase: only words of one list, at least
 * `minWords` complete ones and at most 24.
 *
 * @param {string} text
 * @param {number} minWords 0 turns this test off
 * @returns {null | {language: string, count: number}}
 */
export function phraseStart(text, minWords) {
  if (!minWords) {
    return null;
  }
  const tokens = words(text, { typing: true });
  if (!tokens || tokens.length < minWords || tokens.length > MAX_WORDS) {
    return null;
  }
  const [first] = matchingLists(tokens);
  return first ? { language: first.language, count: tokens.length } : null;
}

/**
 * Cheap test that needs no word list: could this text be a phrase, or the start of
 * one? Only the length and the number of words are looked at.
 */
export function mightBePhrase(text, minWords) {
  const tokens = words(text);
  if (!tokens) {
    return false;
  }
  return (
    PHRASE_LENGTHS.includes(tokens.length) ||
    (minWords > 0 && tokens.length >= minWords && tokens.length <= MAX_WORDS + 1)
  );
}
