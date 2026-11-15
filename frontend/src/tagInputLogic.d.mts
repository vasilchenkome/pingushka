export declare const MAX_TAGS: 10;
export declare function normalizeTag(value: string): string;
export declare function filterTagSuggestions(
  query: string,
  selected: string[],
  suggestions: string[],
  limit?: number,
): string[];
export declare function addTag(tags: string[], raw: string, limit?: number): string[] | null;
export declare function removeTag(tags: string[], tag: string): string[];
export declare function resolveTagChoice(
  query: string,
  activeOption: string | null,
  suggestions: string[],
): string | null;
