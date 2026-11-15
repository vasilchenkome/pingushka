export const MAX_TAGS = 10;

export function normalizeTag(value) {
  return value.trim().toLowerCase();
}

export function filterTagSuggestions(query, selected, suggestions, limit = 6) {
  const needle = normalizeTag(query);
  const chosen = new Set(selected.map(normalizeTag));
  const seen = new Set();
  const results = [];

  for (const candidate of suggestions) {
    const tag = normalizeTag(candidate);
    if (!tag || chosen.has(tag) || seen.has(tag) || !tag.includes(needle)) continue;
    seen.add(tag);
    results.push(tag);
    if (results.length === limit) break;
  }

  return results;
}

export function addTag(tags, raw, limit = MAX_TAGS) {
  const tag = normalizeTag(raw);
  const chosen = new Set(tags.map(normalizeTag));
  if (!tag || chosen.has(tag) || tags.length >= limit) return null;
  return [...tags, tag];
}

export function removeTag(tags, tag) {
  const normalized = normalizeTag(tag);
  return tags.filter((item) => normalizeTag(item) !== normalized);
}

export function resolveTagChoice(query, activeOption, suggestions) {
  const active = activeOption === null ? "" : normalizeTag(activeOption);
  if (active) return active;

  const normalizedQuery = normalizeTag(query);
  if (!normalizedQuery) return null;

  return suggestions.map(normalizeTag).find((tag) => tag === normalizedQuery) ?? normalizedQuery;
}
