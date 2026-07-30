import assert from "node:assert/strict";
import test from "node:test";
// dist/ is CommonJS, so named imports have to come off the default export.
import discord from "./dist/discord.cjs";

const { decodeFrames, encodeFrame } = discord;

const OP_FRAME = 1;

test("round-trips a single frame", () => {
  const { frames, rest } = decodeFrames(encodeFrame(OP_FRAME, { cmd: "HI" }));
  assert.equal(frames.length, 1);
  assert.equal(frames[0].op, OP_FRAME);
  assert.deepEqual(JSON.parse(frames[0].payload), { cmd: "HI" });
  assert.equal(rest.length, 0);
});

test("drains several frames delivered in one read", () => {
  const buffer = Buffer.concat([
    encodeFrame(0, { v: 1 }),
    encodeFrame(OP_FRAME, { cmd: "A" }),
    encodeFrame(OP_FRAME, { cmd: "B" }),
  ]);
  const { frames, rest } = decodeFrames(buffer);
  assert.equal(frames.length, 3);
  assert.equal(rest.length, 0);
});

test("holds back a frame split across reads", () => {
  const whole = encodeFrame(OP_FRAME, { cmd: "SPLIT" });
  const first = decodeFrames(whole.subarray(0, 10));
  assert.equal(first.frames.length, 0, "must not emit a partial frame");

  const second = decodeFrames(Buffer.concat([first.rest, whole.subarray(10)]));
  assert.equal(second.frames.length, 1);
  assert.deepEqual(JSON.parse(second.frames[0].payload), { cmd: "SPLIT" });
});

test("holds back a header split across reads", () => {
  const whole = encodeFrame(OP_FRAME, { cmd: "X" });
  const { frames, rest } = decodeFrames(whole.subarray(0, 5));
  assert.equal(frames.length, 0);
  assert.equal(rest.length, 5, "an incomplete header must be kept, not dropped");
});

test("handles a zero-length payload without stalling", () => {
  const empty = Buffer.alloc(8);
  empty.writeUInt32LE(3, 0);
  empty.writeUInt32LE(0, 4);
  const { frames, rest } = decodeFrames(Buffer.concat([empty, encodeFrame(OP_FRAME, { a: 1 })]));
  assert.equal(frames.length, 2);
  assert.equal(rest.length, 0);
});

test("handles unicode payloads, where byte length differs from string length", () => {
  const title = "Björk — Jóga 日本語";
  const { frames } = decodeFrames(encodeFrame(OP_FRAME, { title }));
  assert.equal(JSON.parse(frames[0].payload).title, title);
});
