/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/* eslint-env worker */

/**
 * The guard engine's worker (WhisperGuardEngine.sys.mjs starts it): runs
 * wcore.wasm, built from crates/wcore_wasm (Rust: dl_guard and proto_guard).
 *
 * Why a ChromeWorker: the parent's main thread may not compile WebAssembly (Firefox
 * forbids it for system code there), a worker loaded from a chrome: URL may, and the
 * work then stays off the main thread. The module gets no imports, so all it can do is
 * read the bytes it is given and return JSON; it cannot reach files, the network or
 * the browser.
 *
 * Messages: {id, op: "init", bytes} with the module's bytes, then
 * {id, op: "dl" | "proto", request} (a Uint8Array) or {id, op: "dlFirstRanges", size};
 * answers {id, json} or {id, error}. After an error the caller starts a new worker.
 */

let exports = null;

function read(packed) {
  const outPtr = Number(packed >> 32n);
  const outLen = Number(packed & 0xffffffffn);
  // Copy out before freeing: the memory may move when it grows.
  const bytes = new Uint8Array(exports.memory.buffer, outPtr, outLen).slice();
  exports.wg_free(outPtr, outLen);
  return new TextDecoder().decode(bytes);
}

function call(fn, request) {
  const len = request.length;
  const ptr = len ? exports.wg_alloc(len) : 0;
  if (len && !ptr) {
    throw new Error("the engine could not reserve memory");
  }
  if (len) {
    new Uint8Array(exports.memory.buffer, ptr, len).set(request);
  }
  let packed;
  try {
    packed = fn(ptr, len);
  } finally {
    if (len) {
      exports.wg_free(ptr, len);
    }
  }
  return read(packed);
}

self.onmessage = async event => {
  const { id, op } = event.data;
  try {
    if (op == "init") {
      const { instance } = await WebAssembly.instantiate(event.data.bytes, {});
      exports = instance.exports;
      self.postMessage({ id, json: "true" });
      return;
    }
    if (!exports) {
      throw new Error("the engine is not loaded");
    }
    let json;
    if (op == "dl") {
      json = call(exports.wg_dl_inspect, event.data.request);
    } else if (op == "proto") {
      json = call(exports.wg_proto, event.data.request);
    } else if (op == "dlFirstRanges") {
      json = read(exports.wg_dl_first_ranges(BigInt(event.data.size)));
    } else {
      throw new Error(`unknown operation ${op}`);
    }
    self.postMessage({ id, json });
  } catch (e) {
    self.postMessage({ id, error: String(e) });
  }
};
