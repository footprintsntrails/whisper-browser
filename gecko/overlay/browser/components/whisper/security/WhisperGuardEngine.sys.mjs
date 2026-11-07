/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The guard engine: Whisper's Rust checks (crates/dl_guard, crates/proto_guard),
 * compiled to WebAssembly (crates/wcore_wasm, built by gecko/scripts/build-wasm.sh)
 * and run in a ChromeWorker (content/guard-worker.js).
 *
 * Why this way in Firefox 159 (and not a Rust library inside libxul): no change to
 * Firefox's C++ or to its Cargo workspace (which the ClickFix work also touches), the
 * module is built with the repository's own Rust and checked by its own tests, and it
 * runs with no imports in a worker: a parsing bug can at worst give a wrong answer,
 * never reach a file or the network. The cost: answers are asynchronous (fine for
 * downloads and external links, which are asynchronous anyway), and the first call
 * compiles the 70 KB module (a few milliseconds, off the main thread). The worker is
 * started on first use and kept for the session.
 *
 * Requests are tag-length-value records (crates/wcore_wasm/src/lib.rs).
 */

import {
  clearTimeout,
  setTimeout,
} from "resource://gre/modules/Timer.sys.mjs";

const WORKER_URL = "chrome://whisper/content/security/guard-worker.js";
const WASM_URL = "chrome://whisper/content/security/wcore.wasm";
const TIMEOUT_MS = 10000;

let gWorker = null;
let gReady = null;
let gNextId = 1;
const gPending = new Map();

function stopWorker() {
  try {
    gWorker?.terminate();
  } catch (e) {}
  gWorker = null;
  gReady = null;
}

function failAll(message) {
  for (const p of gPending.values()) {
    clearTimeout(p.timer);
    p.reject(new Error(message));
  }
  gPending.clear();
}

function post(message, transfer = []) {
  const id = gNextId++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      gPending.delete(id);
      reject(new Error("Whisper guard engine: no answer in time"));
      stopWorker();
    }, TIMEOUT_MS);
    gPending.set(id, { resolve, reject, timer });
    gWorker.postMessage({ id, ...message }, transfer);
  });
}

function startWorker() {
  gWorker = new ChromeWorker(WORKER_URL);
  gWorker.onmessage = event => {
    const { id, json, error } = event.data;
    const p = gPending.get(id);
    if (!p) {
      return;
    }
    gPending.delete(id);
    clearTimeout(p.timer);
    if (error) {
      p.reject(new Error(`Whisper guard engine: ${error}`));
      // Its memory may be in any state after a failure: the next call starts afresh.
      stopWorker();
    } else {
      p.resolve(JSON.parse(json));
    }
  };
  gWorker.onerror = event => {
    event.preventDefault?.();
    failAll(`Whisper guard engine worker failed: ${event.message}`);
    stopWorker();
  };
  gReady = (async () => {
    const response = await fetch(WASM_URL);
    const bytes = await response.arrayBuffer();
    await post({ op: "init", bytes }, [bytes]);
  })();
  gReady.catch(() => stopWorker());
}

async function request(message, transfer) {
  if (!gWorker) {
    startWorker();
  }
  await gReady;
  return post(message, transfer);
}

/** Tag-length-value records. */
function encode(records) {
  const encoder = new TextEncoder();
  const parts = [];
  let total = 0;
  for (const [tag, value] of records) {
    const bytes = typeof value == "string" ? encoder.encode(value) : value;
    const head = new Uint8Array(5);
    head[0] = tag;
    new DataView(head.buffer).setUint32(1, bytes.length, true);
    parts.push(head, bytes);
    total += 5 + bytes.length;
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function u64(n) {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt(n), true);
  return b;
}

export const ORIGIN = Object.freeze({ HTTPS: 0, INSECURE: 1, MADE_BY_PAGE: 2, LOCAL: 3 });

export const WhisperGuardEngine = {
  /** The ranges [offset, length] to read before the first dlInspect. */
  async dlFirstRanges(size) {
    return request({ op: "dlFirstRanges", size });
  },

  /**
   * @param {object} d
   * @param {string} d.fileName
   * @param {string} d.urlName
   * @param {string} d.mime
   * @param {number} d.size
   * @param {number} d.origin one of ORIGIN
   * @param {boolean} d.siteAllowed
   * @param {Array<[number, Uint8Array]>} d.chunks
   * @returns {Promise<object>} {verdict: "allow"|"hold"|"need", kind, format, reasons, ranges}
   */
  async dlInspect(d) {
    const records = [
      [1, d.fileName],
      [2, d.urlName],
      [3, d.mime],
      [4, u64(d.size)],
      [5, Uint8Array.of(d.origin)],
      [6, Uint8Array.of(d.siteAllowed ? 1 : 0)],
    ];
    for (const [offset, data] of d.chunks) {
      const value = new Uint8Array(8 + data.length);
      value.set(u64(offset), 0);
      value.set(data, 8);
      records.push([7, value]);
    }
    const req = encode(records);
    return request({ op: "dl", request: req }, [req.buffer]);
  },

  /**
   * @returns {Promise<{tier: "block"|"ask", scheme: string|null, program: string|null}>}
   */
  async proto(scheme, { allowedMs = "", command = "" } = {}) {
    const req = encode([
      [1, scheme],
      [2, allowedMs],
      [3, command],
    ]);
    return request({ op: "proto", request: req }, [req.buffer]);
  },
};
