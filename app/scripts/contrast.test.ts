import test from "node:test";
import assert from "node:assert/strict";
import { contrastRatio, luminance, MIN_GRAPHIC_CONTRAST } from "../src/lib/contrast.ts";

test("black on white is 21:1 and identical colors are 1:1", () => {
  assert.ok(Math.abs(contrastRatio("#000000", "#ffffff") - 21) < 0.01);
  assert.equal(contrastRatio("#3b93f0", "#3b93f0"), 1);
});

test("order does not matter and invalid input counts as black", () => {
  assert.equal(contrastRatio("#123456", "#abcdef"), contrastRatio("#abcdef", "#123456"));
  assert.equal(luminance("rot"), 0);
});

test("a pale colour on the light paper fails the graphic contrast, the default blue passes", () => {
  assert.ok(contrastRatio("#f1efe9", "#e6eff4") < MIN_GRAPHIC_CONTRAST);
  assert.ok(contrastRatio("#1d6fa5", "#e6eff4") >= MIN_GRAPHIC_CONTRAST);
});
