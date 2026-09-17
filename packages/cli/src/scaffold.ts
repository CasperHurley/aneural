/**
 * Files `aneural spores init` writes.
 *
 * Kept apart from `cli.ts` because they are content, not control flow — and
 * because `AGENTS.md` is the file that teaches a coding agent the rules the
 * validator would otherwise only tell it about after a failure.
 */

export interface Scaffold {
  publisher: string;
  name: string;
}

export const manifest = ({ publisher, name }: Scaffold): string =>
  `${JSON.stringify(
    {
      $schema: 'https://aneural.dev/schema/spore-v2.json',
      publisher,
      name,
      version: '0.1.0',
      displayName: name.replace(/(^|-)(\w)/g, (_, s, c) => (s ? ' ' : '') + c.toUpperCase()),
      description: 'One sentence on what this spore surfaces, and from where.',
      license: 'MIT',
      aneural: '>=0.1',
      keywords: [],
      categories: [],
      nodeTypes: [
        {
          kind: 'Thing',
          label: 'Thing',
          icon: 'LuTag',
          color: '#9fd18f',
          shape: 'pill',
          description: 'One node per thing found',
        },
      ],
      edgeTypes: [{ kind: 'ANNOTATES', label: 'annotates', style: 'dotted' }],
      harvesters: [
        {
          id: 'things',
          kind: 'regex',
          include: ['**/*.md'],
          pattern: '^\\s*@thing\\s+(?<text>.+)$',
          emit: {
            node: {
              kind: 'Thing',
              // Must start with `<publisher>.<name>.` — see AGENTS.md.
              id: `${publisher}.${name}.thing:{file}#{hash(text)}`,
              label: '{text}',
              props: { line: '{line}', file: '{file}' },
            },
            edges: [
              { kind: 'ANNOTATES', src: '$node', dst: 'file:{file}', props: { line: '{line}' } },
            ],
          },
        },
      ],
      panel: { title: 'Things', columns: ['label', 'file', 'line'] },
    },
    null,
    2,
  )}\n`;

export const readme = ({ publisher, name }: Scaffold): string =>
  `# ${publisher}.${name}

One paragraph on what this spore adds to the graph and why someone would want it.
This file is what the marketplace shows in the detail pane, so it is worth writing.

## What it adds

- \`Thing\` nodes, one per \`@thing\` line in a markdown file
- \`ANNOTATES\` edges back to the file each one came from

## Developing

\`\`\`sh
aneural spores validate spore.json   # structural check
aneural spores test .                # run it over fixtures/ and diff the snapshot
aneural spores test . --update       # accept the new output
\`\`\`
`;

export const fixture = (): string =>
  `# Sample

@thing the harvester should find this line

Ordinary prose the harvester should ignore.
`;

export const agents = ({ publisher, name }: Scaffold): string =>
  `# Working on this spore

A spore is a declarative manifest. There is no code to run: \`spore.json\` is the
whole thing, and Aneural's built-in runners execute it.

## The rules that will reject your manifest

- **Namespacing.** Every node id this spore emits must start with
  \`${publisher}.${name}.\` — e.g. \`${publisher}.${name}.thing:{file}#{hash(text)}\`.
  The prefixes \`file\`, \`dir\`, \`repo\`, \`manifest\`, \`pkg\` and \`sym\` belong to the
  engine and are rejected.
- **Reserved kinds.** \`File\`, \`Directory\`, \`Repo\`, \`Manifest\`, \`Package\` and
  \`Symbol\` are the engine's. Pick your own kind names.
- **Icons** must be names from Aneural's curated icondata set (\`LuTag\`,
  \`LuDatabase\`, \`SiTypescript\`, …). An unknown name degrades to a fallback and
  raises a \`doctor\` warning.
- **Harvester ids** must be unique within the manifest and non-empty.
- **\`version\`** is semver; **\`aneural\`** is a semver range.

## Harvester kinds

| kind | input | use it for |
|---|---|---|
| \`regex\` | each line of the file | markers, annotations, anything line-shaped |
| \`tree-sitter\` | a parsed syntax tree | real code structure |
| \`markdown\` | documents or \`##\` headings | notes, plans, docs, wiki-links |
| \`sqlite\` | the file's *path*, opened read-only | local database schemas |
| \`http\` | a JSON web API, on a refresh interval | pull requests, tickets, advisories |

## Templates

\`{file} {line} {basename}\` are always available, plus each harvester kind's own
variables and any named regex captures. Functions: \`hash slug upper lower trim
basename stem\`. \`$node\` in an edge means the node just emitted.

## Capabilities

Leave \`capabilities\` out unless you genuinely need one. A spore that declares
nothing says "needs nothing" on the consent sheet and installs without a prompt,
which is the bar to clear before asking a user for more.

Two capabilities have runners today, and they go together:

\`\`\`json
"capabilities": [
  { "kind": "http", "hosts": ["api.example.com"] },
  { "kind": "secret", "names": ["apiToken"], "hosts": ["api.example.com"] }
]
\`\`\`

\`tcp\` and \`graphRead\` land in the sandboxed tier, whose runner does not ship
yet, and will not install. \`fsWrite\` and \`subprocess\` are the native tier:
a registry will never list one, and CI rejects the entry.

If you write an \`http\` harvester, these will reject it:

- The URL must be \`https://\` with a **literal** host. A templated host cannot be
  checked against the allowlist, so it is refused.
- Every host you call must be in \`capabilities.http.hosts\`. \`*.example.com\`
  covers subdomains; \`*\` is never allowed. Neither is \`localhost\`, an IP
  address, or anything under \`.local\` / \`.internal\`: a listed spore reaches
  public hostnames, never the user's machine or network.
- A \`{secret.*}\` may appear **only in a header value**, never in a URL.
- Declare every setting you interpolate in \`settings\`, and every secret you read
  in the \`secret\` capability — with the \`hosts\` it may be sent to. A header
  that would carry a secret to any other host is refused, at validation and
  again before every request.
- \`refreshSeconds\` may not go below 60; \`maxPages\` is capped at 10.
- An \`expand\` block may emit **edges only**. Use it to join your nodes to files
  that already exist (\`file:{item.filename}\`); an edge to a node that is not in
  the graph is silently dropped, which is the intended behaviour.

## The loop

\`\`\`sh
aneural spores validate spore.json
aneural spores test .
\`\`\`

\`spores test\` runs the manifest over \`fixtures/\` and diffs the emitted nodes and
edges against \`fixtures/expected.json\`. Change the manifest, re-run, and read the
diff. Do not hand-edit the snapshot — regenerate it with \`--update\` once the
output is what you intended.
`;

export const workflow = (): string =>
  `name: validate

on: [push, pull_request]

jobs:
  spore:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
      - run: npx -y aneural spores validate spore.json
      - run: npx -y aneural spores test .
`;
