# @aneural/mcp

MCP server that exposes the **focused** part of an Aneural workspace graph as context for
Claude Code, Codex and any other MCP client.

```sh
npx aneural mcp            # via the CLI (recommended)
npx @aneural/mcp --root .  # standalone
```

Tools: `aneural_focus`, `aneural_search_nodes`, `aneural_get_node`, `aneural_neighborhood`,
`aneural_get_edges`, `aneural_read_file`, `aneural_list_node_types`, `aneural_list_spores`,
`aneural_counts`, `aneural_doctor`, `aneural_index`, `aneural_add_idea`.
Resources: `aneural://focus`, `aneural://focus/context.md`, `aneural://node-types`,
`aneural://spores`, `aneural://node/{id}`, `aneural://file/{path}`. Prompt: `aneural_context`.

The focus is whatever the Aneural GUI last wrote to `.aneural/state/focus.json`
(selected + pinned nodes, filters, neighbourhood depth, notes).
