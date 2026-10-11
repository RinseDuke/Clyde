import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';

// Use the existing compiler so these tests also run on supported Node 18,
// without relying on the newer Node runtime's built-in TypeScript stripping.
async function sourceModule(path) {
  const source = await readFile(new URL(path, import.meta.url), 'utf8');
  const { outputText } = ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022,
  } });
  return import(`data:text/javascript;charset=utf-8,${encodeURIComponent(outputText)}`);
}
const { createDragController } = await sourceModule('../../src/windows/hit/drag-controller.ts');
const { percentRegions } = await sourceModule('../../src/windows/hit/hit-layout.ts');

function deferred() {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return { promise, resolve };
}
const tick = () => new Promise(resolve => setImmediate(resolve));

function harness(overrides = {}) {
  const calls = [];
  const frames = new Map();
  let frameId = 0;
  let gestureId = 0;
  const drag = createDragController({
    start: async () => { calls.push('start'); return ++gestureId; },
    move: async id => { calls.push(['move', id]); },
    end: async (...args) => { calls.push(['end', ...args]); },
    cursor: async () => { calls.push('cursor'); return { x: -125, y: 320 }; },
    frame: callback => { frames.set(++frameId, callback); return frameId; },
    cancelFrame: id => frames.delete(id),
    error: error => { throw error; },
    ...overrides,
  });
  return { drag, calls, frames, flush() {
    for (const [id, callback] of [...frames]) { frames.delete(id); callback(); }
  } };
}

test('rapid release cancels pending frame and uses release sample, without a move', async () => {
  const h = harness();
  h.drag.start();
  h.drag.move();
  await h.drag.end();
  h.flush();
  assert.equal(h.frames.size, 0);
  assert.deepEqual(h.calls.filter(call => Array.isArray(call)), [
    ['end', 1, { x: -125, y: 320 }, false],
  ]);
});

test('release is sampled before waiting for delayed move, end follows that move', async () => {
  const gate = deferred();
  const h = harness({ move: async id => {
    h.calls.push(['move', id]);
    await gate.promise;
    h.calls.push('move-done');
  } });
  h.drag.start();
  await tick();
  h.drag.move();
  h.flush();
  await tick();
  const ended = h.drag.end();
  assert.equal(h.calls.at(-1), 'cursor');
  assert.ok(!h.calls.some(call => Array.isArray(call) && call[0] === 'end'));
  gate.resolve();
  await ended;
  assert.deepEqual(h.calls.slice(-2), ['move-done', ['end', 1, { x: -125, y: 320 }, false]]);
});

test('moves are coalesced even when IPC is slower than animation frames', async () => {
  const gate = deferred();
  let moves = 0;
  const h = harness({ move: async () => { moves++; await gate.promise; } });
  h.drag.start();
  await tick();
  h.drag.move(); h.flush();
  await tick();
  for (let i = 0; i < 50; i++) { h.drag.move(); h.flush(); }
  assert.equal(moves, 1);
  gate.resolve();
  await tick();
  h.flush();
  await tick();
  assert.equal(moves, 2);
  await h.drag.end(true);
});

test('pointer cancellation sends no release/snap position', async () => {
  const h = harness();
  h.drag.start(); h.drag.move();
  await h.drag.end(true);
  // Only the pointerdown sample; cancelling must not sample a snap position.
  assert.equal(h.calls.filter(call => call === 'cursor').length, 1);
  assert.deepEqual(h.calls.at(-1), ['end', 1, null, true]);
});

test('next gesture waits for old release to finish', async () => {
  const gate = deferred();
  const h = harness({ end: async (...args) => {
    h.calls.push(['end', ...args]); await gate.promise;
  } });
  h.drag.start();
  await tick();
  const first = h.drag.end();
  h.drag.start();
  await tick();
  assert.equal(h.calls.filter(call => call === 'start').length, 1);
  gate.resolve();
  await first; await tick();
  assert.equal(h.calls.filter(call => call === 'start').length, 2);
  await h.drag.end(true);
});

test('start receives the pointerdown sample even if its IPC starts later', async () => {
  let sample = 0;
  let start;
  const h = harness({
    cursor: async () => ({ x: ++sample === 1 ? -500 : -125, y: 320 }),
    start: async point => { start = point; return 1; },
  });
  h.drag.start();
  await h.drag.end();
  assert.deepEqual(start, { x: -500, y: 320 });
  assert.deepEqual(h.calls.at(-1), ['end', 1, { x: -125, y: 320 }, false]);
});

test('physical hit regions cover identical CSS proportions at 100/125/150/200% DPI', () => {
  for (const scale of [1, 1.25, 1.5, 2]) {
    assert.deepEqual(percentRegions({ width: 200 * scale, height: 100 * scale,
      regions: [{ x: 20 * scale, y: 10 * scale, width: 120 * scale, height: 70 * scale }] }),
      [{ x: 10, y: 10, width: 60, height: 70 }]);
  }
  assert.deepEqual(percentRegions({ width: 0, height: 100, regions: [] }), []);
});
