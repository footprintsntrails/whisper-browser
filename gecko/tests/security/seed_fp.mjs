#!/usr/bin/env node
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * False-positive rate of the recovery phrase guard on ordinary text.
 *
 * Usage: node gecko/tests/security/seed_fp.mjs TEXT_FILE... [--common WORDLIST]
 *
 * Runs the shipped detector (SeedPhrase.sys.mjs) unchanged over plain text files
 * (books, documentation, interface strings) and counts, for every phrase length N:
 *   windows   every run of exactly N consecutive words, as if someone typed or pasted
 *             exactly that into a field;
 *   all-list  windows whose words are all in one BIP39 list (punctuation stripped:
 *             the worst case, kinder to the guard than what it really sees);
 *   blocked   windows the guard would stop (exact count, one list, valid checksum);
 * and, for the early pause while typing, the share of sentences (each taken as the
 * value of one field, typed as written) whose first K words are all list words.
 *
 * --common FILE: common English words, one per line; reports how many are BIP39 words
 * and the chance that N random common words are all list words.
 *
 * The corpora are not in the repository; the reported numbers come from public-domain
 * books (Project Gutenberg) and Firefox's own documentation and interface strings.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const security = path.join(here, "../../overlay/browser/components/whisper/security");
const SP = await import(path.join(security, "SeedPhrase.sys.mjs"));
const { WORDLISTS } = await import(path.join(security, "SeedWordlists.sys.mjs"));
const lists = SP.buildLists(WORDLISTS);
SP.setLists(lists);

const args = process.argv.slice(2);
let common = null;
const files = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] == "--common") {
    common = args[++i];
  } else {
    files.push(args[i]);
  }
}

const KS = [3, 4, 5, 6, 8];
const totals = Object.fromEntries(
  SP.PHRASE_LENGTHS.map(n => [n, { windows: 0, allList: 0, blocked: 0, examples: [] }])
);
const early = Object.fromEntries(KS.map(k => [k, { fields: 0, hits: 0, examples: [] }]));
let wordsSeen = 0;

const PUNCT = /[\p{P}\p{S}]+/gu;
const inSomeList = w => [...lists.values()].some(map => map.has(w));

for (const file of files) {
  const text = fs.readFileSync(file, "utf8");
  const clean = text
    .split(/\s+/u)
    .map(w => SP.fold(w.replace(PUNCT, "")))
    .filter(Boolean);
  wordsSeen += clean.length;
  const member = clean.map(inSomeList);
  for (const n of SP.PHRASE_LENGTHS) {
    const t = totals[n];
    let run = 0;
    for (let i = 0; i < clean.length; i++) {
      run = member[i] ? run + 1 : 0;
      if (i + 1 >= n) {
        t.windows++;
      }
      if (run >= n) {
        const window = clean.slice(i + 1 - n, i + 1).join(" ");
        if (SP.phraseStart(window + " ", n)) {
          t.allList++;
          if (SP.detectPhrase(window)) {
            t.blocked++;
            if (t.examples.length < 5) {
              t.examples.push(window);
            }
          }
        }
      }
    }
  }
  for (const s of text.split(/(?<=[.!?;:])\s+|\n\s*\n/u)) {
    const tokens = s.replace(/\s+/gu, " ").trim().split(" ").filter(Boolean);
    for (const k of KS) {
      if (tokens.length < k) {
        continue;
      }
      const e = early[k];
      e.fields++;
      const typed = tokens.slice(0, k).join(" ") + " ";
      if (SP.phraseStart(typed, k)) {
        e.hits++;
        if (e.examples.length < 6) {
          e.examples.push(typed.trim());
        }
      }
    }
  }
}

const pct = (a, b) => (b ? `${((100 * a) / b).toPrecision(3)} %` : "-");
console.log(`Corpus: ${files.length} files, ${wordsSeen} words\n`);
console.log("N   windows      all one list   blocked   blocked rate");
for (const n of SP.PHRASE_LENGTHS) {
  const t = totals[n];
  // Rule of three: 0 events in W trials puts the 95 % upper bound at 3/W.
  const rate = t.blocked
    ? pct(t.blocked, t.windows)
    : `0 (< ${(3 / t.windows).toExponential(1)} at 95 %)`;
  console.log(
    `${String(n).padEnd(3)} ${String(t.windows).padEnd(12)} ${String(t.allList).padEnd(14)} ${String(t.blocked).padEnd(9)} ${rate}`
  );
  for (const ex of t.examples) {
    console.log(`      e.g. "${ex}"`);
  }
}
console.log("\nEarly pause while typing (first K words of a sentence-sized field):");
for (const k of KS) {
  const e = early[k];
  console.log(`K=${k}: ${e.hits} of ${e.fields} fields (${pct(e.hits, e.fields)})`);
  for (const ex of e.examples) {
    console.log(`      e.g. "${ex}"`);
  }
}

if (common) {
  const vocab = fs.readFileSync(common, "utf8").split(/\s+/u).filter(Boolean).map(SP.fold);
  const english = lists.get("english");
  const share = vocab.filter(w => english.has(w)).length / vocab.length;
  console.log(
    `\nCommon English words (${vocab.length}): ${(share * 100).toFixed(1)} % are BIP39 English words.`
  );
  for (const n of SP.PHRASE_LENGTHS) {
    console.log(`  ${n} random common words all from the list: ${(share ** n).toExponential(2)}`);
  }
}

// The checksum itself: random words from the English list pass with 2^-(N/3).
const english = WORDLISTS.english.words.split(" ");
// xorshift32: the low bits of a plain LCG repeat every 2048 draws, which skews this.
let seed = 0x9e3779b9;
const rand = () => {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return seed >>> 0;
};
console.log("\nRandom words from the English list that pass the checksum:");
for (const n of SP.PHRASE_LENGTHS) {
  const trials = 100000;
  let bip39 = 0;
  let electrum = 0;
  for (let t = 0; t < trials; t++) {
    const phrase = Array.from({ length: n }, () => english[rand() % 2048]).join(" ");
    const r = SP.detectPhrase(phrase);
    bip39 += r?.scheme == "bip39" ? 1 : 0;
    electrum += r?.scheme == "electrum" ? 1 : 0;
  }
  console.log(
    `  N=${n}: BIP39 ${pct(bip39, trials)} (expected ${pct(1, 2 ** (n / 3))})` +
      (n == 12 ? `, Electrum ${pct(electrum, trials)} (expected about 0.46 %)` : "")
  );
}
