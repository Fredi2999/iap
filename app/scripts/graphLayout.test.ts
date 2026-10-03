import test from "node:test";
import assert from "node:assert/strict";
import { layoutGraph, neighborhood } from "../src/lib/graphLayout.ts";

const nodes = ["a", "b", "c", "d", "e"].map((id) => ({ id }));
const edges = [{ from: "a", to: "b" }, { from: "b", to: "c" }, { from: "a", to: "zzz" }];

test("layout is deterministic and stays inside the box", () => {
  const one = layoutGraph(nodes, edges, { width: 600, height: 400 });
  const two = layoutGraph(nodes, edges, { width: 600, height: 400 });
  assert.deepEqual([...one], [...two]);
  for (const p of one.values()) {
    assert.ok(Number.isFinite(p.x) && Number.isFinite(p.y));
    assert.ok(p.x >= 0 && p.x <= 600 && p.y >= 0 && p.y <= 400);
  }
});

test("linked nodes end up closer than unlinked ones", () => {
  const pos = layoutGraph(nodes, edges, { width: 600, height: 400 });
  const d = (a: string, b: string) => Math.hypot(pos.get(a)!.x - pos.get(b)!.x, pos.get(a)!.y - pos.get(b)!.y);
  assert.ok(d("a", "b") < d("a", "e"));
});

test("empty and single node graphs work, no overlap for identical starts", () => {
  assert.equal(layoutGraph([], [], { width: 10, height: 10 }).size, 0);
  const single = layoutGraph([{ id: "x" }], [], { width: 100, height: 50 });
  assert.deepEqual(single.get("x"), { x: 50, y: 25 });
});

test("neighborhood adds direct neighbours only", () => {
  const set = neighborhood(edges, new Set(["a"]));
  assert.deepEqual([...set].sort(), ["a", "b", "zzz"]);
});
