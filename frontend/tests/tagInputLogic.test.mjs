import assert from "node:assert/strict";
import test from "node:test";
import {
  addTag,
  filterTagSuggestions,
  normalizeTag,
  removeTag,
  resolveTagChoice,
} from "../src/tagInputLogic.mjs";

test("normalizes tag input", () => {
  assert.equal(normalizeTag("  API-Edge  "), "api-edge");
});

test("filters suggestions, excludes selected tags, removes duplicates, and caps results", () => {
  const suggestions = ["API", "api", "api-edge", "billing", "api-health", "api-v2", "api-prod", "api-test", "api-dev"];
  assert.deepEqual(
    filterTagSuggestions(" API ", ["api"], suggestions),
    ["api-edge", "api-health", "api-v2", "api-prod", "api-test", "api-dev"],
  );
});

test("adds a normalized tag and rejects blank, duplicate, and over-limit additions", () => {
  assert.deepEqual(addTag(["prod"], "  API "), ["prod", "api"]);
  assert.equal(addTag(["prod"], "  "), null);
  assert.equal(addTag(["prod"], " PROD "), null);
  assert.equal(addTag(Array.from({ length: 10 }, (_, index) => `tag-${index}`), "next"), null);
});

test("removes only the requested tag", () => {
  assert.deepEqual(removeTag(["prod", "api", "http"], "api"), ["prod", "http"]);
  assert.deepEqual(removeTag(["prod", "http"], "missing"), ["prod", "http"]);
});

test("resolves active suggestion, exact match, then explicit creation", () => {
  const suggestions = ["api-edge", "api-health"];
  assert.equal(resolveTagChoice("api", "api-edge", suggestions), "api-edge");
  assert.equal(resolveTagChoice(" API-HEALTH ", null, suggestions), "api-health");
  assert.equal(resolveTagChoice(" New-Tag ", null, suggestions), "new-tag");
  assert.equal(resolveTagChoice("   ", null, suggestions), null);
});
