# codebase-expedition

Hotspot-first codebase exploration: RankHotspots → TraverseGraph → FetchContent.

## When to use

Use when you need to understand an unfamiliar codebase quickly. Start from the highest-ranked
hotspots (most connected symbols), traverse their call/import graph, then fetch source content
for the key symbols.

## Steps

1. **RankHotspots** — identify the top-N most-connected symbols
2. **TraverseGraph** — walk the dependency graph from each hotspot
3. **FetchContent** — fetch source content for symbols of interest

## Parameters

| Parameter   | Description                          | Required |
|-------------|--------------------------------------|----------|
| `repo_path` | Path to the indexed repository       | yes      |
| `limit`     | Number of hotspots to start from     | no       |
| `depth`     | Graph traversal depth                | no       |
