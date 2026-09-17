# The Open Spores Marketplace

Spores are published by other people. Everything below follows from that.

## Identity

A spore is `publisher.name` — `acme.adr`, `aneural.comments`. The publisher half is what makes the
marketplace safe to open to strangers:

- A spore may only emit node ids under a prefix it owns: `acme.adr` may emit
  `acme.adr.decision:docs/adr/001.md` and nothing else.
- Core id prefixes (`file`, `dir`, `repo`, `manifest`, `pkg`, `sym`) and core node kinds
  (`File`, `Directory`, `Repo`, `Manifest`, `Package`, `Symbol`) are reserved. A third-party manifest
  declaring one is rejected at validate time.
- Installing a spore that declares a node kind another installed spore already provides is **refused**,
  not warned about. `merge_node_type` is last-writer-wins, so a warning would leave the user with a
  graph whose colours silently changed.
- First-party spores (`publisher: "aneural"`) are grandfathered onto the short prefixes they shipped
  with — `comment:`, `plan:`, `idea:`, `note:`.

Bare names in `config.spores.enabled` keep working forever. `SporesConfig::entry_matches` accepts
either form, which is why migration is cosmetic and a workspace that is never migrated never breaks.

## Tiers

A manifest declares `capabilities`. Its tier is **derived** from them and never declared, so a spore
cannot understate what it will do.

| Tier | Capabilities | Runner |
|---|---|---|
| `declarative` | none | **ships** |
| `http` | `http`, `secret` | **ships** |
| `sandboxed` | `tcp`, `graphRead` | not yet |
| `native` | `fsWrite`, `subprocess` | never from a registry |

A `sandboxed` spore lists and validates normally, then refuses to install: *"needs the `sandboxed`
runtime, which this version of Aneural does not ship."* The capability vocabulary and the consent
sheet exist now precisely so that neither has to be redesigned when the runner lands.

**`native` is never listed.** The WASM sandbox is the ceiling for anything a registry hands out.
`Entry::validate` refuses a native listing, so registry CI cannot merge one and a client will not
install one from any index, official or private. When a native runner exists, the only way in will be
a direct URL or local path the user typed themselves — quarantined, badged and disclaimed as designed.
That one rule is what lets the registry stay a static file with PR review instead of a scanning
pipeline: nothing it distributes can ever run un-sandboxed code.

## Tier 1: reading a web API

A `http` harvester declares a URL, some headers, where the records live in the response, and an emit
block. It is still entirely declarative — no third-party code runs — so what the user is consenting to
is reach, not execution. `spores/github` is the worked example.

**Validation refuses what an allowlist could not police.** A manifest fails if the URL is not
`https://` with a *literal* host (a templated authority cannot be checked against an allowlist), if
the host is outside the declared `http` capability, if a `{secret.*}` appears anywhere in a URL
(URLs get logged and cached — secrets belong in headers), if it uses a secret or a setting it never
declared, or if it asks to poll faster than 60s.

**Listed hosts are public hostnames.** `localhost`, any literal IP address, and anything under
`.local`, `.internal` or `.home.arpa` are refused, as a whole pattern, so `*.internal` is out too. A
user consenting to "make web requests to api.github.com" has not consented to a spore reading their
cloud metadata endpoint or a Redis on the office network. (The address a name *resolves* to is not
re-checked yet; that lands with the TCP runner, which is where it belongs.)

**A secret is scoped to hosts.** The `secret` capability names the secrets a spore reads *and* the
hosts each may be sent to, and every one of those hosts must be inside the `http` allowlist. A
header that would carry a secret anywhere else is refused at validation, and the runner refuses it
again before every request — including each `Link: rel="next"` hop, since the server chooses that
URL. The consent line reads *"read your saved githubToken and send it only to api.github.com"*,
which is the sentence a user can actually evaluate.

**The runner re-checks what validation could only check statically.** The rendered host is verified
against the allowlist before each request *including every `Link: rel="next"` hop*, because the server
chooses that URL. Redirects are refused outright rather than followed: following one would mean
deciding whether to forward a bearer token to wherever the first host points.

**Values are escaped by provenance.** A `{setting.*}` is something the user typed into their own
config, so a `/` in it is a path separator. Everything from a response — `{item.*}` — is escaped in
full, so a crafted title cannot walk the path of the follow-up request.

**An `expand` block may only draw edges.** It is how a pull request finds the files it changes, and
those files are already nodes. Letting a response invent `file:` nodes for paths that are not in the
checkout would quietly fill the graph with things the user does not have, so an edge whose far end is
not in the graph is dropped.

### Settings and secrets

Two different things, deliberately stored in two different places.

**Settings** are *whose* data to read — a repository, a JIRA site. They are declared in the manifest,
recorded in `config.spores.settings`, and shown on the consent sheet before install, because a spore
that installs cleanly and then does nothing is a worse outcome than one that says what it needs.

```
aneural spores set aneural.github repo owner/name
```

**Secrets** are credentials. They are never in the config, never written by Aneural, and never in a
URL. `{secret.githubToken}` resolves from `ANEURAL_SECRET_GITHUB_TOKEN` in the environment, so the
value lives wherever the user already keeps credentials. A missing one is a reported problem, never a
request sent without authentication. An OS keychain can implement the same `SecretStore` trait later
without any manifest changing.

That split is what lets a team commit which repository a spore watches without committing anyone's
token.

## How it is hosted

There is no marketplace server. The official registry is **one static file in this repository**,
`registry/index.json`, served raw from GitHub:

```
https://raw.githubusercontent.com/Parnassix/aneural/main/registry/index.json
```

That URL is `OFFICIAL_REGISTRY_URL` in `aneural-core`, and the entry named `official` in a
workspace's `registries` list is *owned by the client*: `SporesConfig::migrate` re-points it whenever
the constant changes, so the location is a build detail, not a promise. (An entry the user renamed is
theirs and is left alone.)

- **Listing** a spore is a pull request adding one entry to `registry/index.json`.
  `.github/workflows/validate-submissions.yml` runs `aneural registry` on it: structure, every hash
  fetched and verified, the manifest cross-checked against its listing, native tier refused.
- **Spore files** are fetched from the publisher's own repository at the tag the entry pins. Nothing
  is uploaded to us and nothing is executed; the client downloads three files at most and verifies
  each against its hash.
- **First-party spores** are entries like any other, pointing back at this repository at a tag per
  spore per version, `spores/<name>/v<version>`. The index is generated from `spores/*` by
  `crates/aneural-registry/tests/official_index.rs`; cut the tag on the commit that bumps a spore's
  `version`, and CI proves the pinned bytes are the ones the manifest hashes to.

Moving the registry — to its own repository, or behind a domain — is a change to
`OFFICIAL_REGISTRY_URL` and a mirror of the file. A domain must *proxy* the file rather than redirect
to GitHub: the transport refuses a redirect that changes host, on purpose.

## Registries

`config.spores.registries` is an ordered list. The first registry listing an id provides it, so a team
registry can deliberately shadow the official one — the detail pane says "also listed in …" so the
shadow is never invisible. Revocations are the exception: they are unioned across every registry, so a
private index can withdraw an official spore for its own team.

Anything with a scheme is fetched over HTTPS; a bare path is read from disk, which is how private and
fixture registries work without a server.

### Index format

```json
{ "version": 1, "name": "official",
  "spores": [{
    "id": "acme.adr", "version": "1.2.0",
    "displayName": "Architecture Decision Records", "description": "…",
    "repo": "github:acme/adr-spore#v1.2.0", "path": "spore",
    "files": { "spore.json": "<sha256>", "README.md": "<sha256>" },
    "nodeKinds": ["Decision"], "keywords": ["adr"], "capabilities": [] }],
  "revoked": [{ "id": "bad.thing", "versions": ["*"], "reason": "…" }] }
```

A package is at most four files: `spore.json` (required), `README.md`, `icon.svg`. A closed allowlist
rather than a sanitizer — it removes path traversal and file-count bombs in one line.

## What install actually checks

1. The id resolves in some configured registry, and is not revoked.
2. The entry is structurally valid — `publisher.name`, semver, every file pinned by sha256.
3. Every file is downloaded and **verified against its pinned hash**. `sha256` is mandatory; the old
   TypeScript path skipped verification when an entry omitted one, and that did not survive.
4. The manifest's own id matches the listing's.
5. The manifest validates.
6. **The manifest asks for nothing the listing did not advertise.** An index that understates a spore's
   capabilities is an attack, not a typo, so this aborts the install.
7. The derived tier has a runner in this build.
8. No declared node kind collides with an installed spore's.

Only then is anything written. The GUI shows the consent sheet between step 8 and the write, and
re-resolves and re-verifies on commit, so a stale plan can never be what lands on disk.

## The lockfile

`.aneural/spores.lock` records, per spore: resolved version, the registry name **and the URL as
configured**, repo and path, tier, granted capabilities, a sha256 per file, one `integrity` hash over
the set, and a timestamp. Commit it: a team then gets byte-identical spores, and `aneural spores verify`
can tell an edited file from a tampered one.

A file edited after install is reported as `modified` — a warning, because hand-editing an installed
spore is a legitimate thing to do. A directory in `.aneural/spores` with no lockfile entry is
`notInLock`, which is just what a hand-authored spore looks like.

## Consent

Every install shows what the spore will be allowed to do, in plain English. For a declarative spore
that is *"Nothing. It only reads files you already index, using patterns declared in its manifest."*

The CLI prints the same summary and prompts. **A non-TTY run without `--yes` exits 2** rather than
consenting on the user's behalf. An update only re-prompts when the new version asks for something the
installed one did not.

Every non-first-party listing carries the disclaimer, defined once in `aneural_registry::DISCLAIMER`
so the GUI, the CLI and these docs cannot drift apart.

## Not installable by an agent

`aneural_list_spores` is exposed over MCP. Install, enable, disable and update are **not**, and should
not be added: an agent must not be able to install third-party code into someone's workspace. If a
future version wants this, it needs a human-in-the-loop design first, not a tool definition.

## Authoring

```sh
aneural spores init my-spore --publisher acme
aneural spores validate my-spore/spore.json
aneural spores test my-spore            # run over fixtures/, diff the snapshot
aneural spores test my-spore --update   # accept the new output
```

`init` writes a publishable repo: the manifest, a README (which is what the detail pane shows), a
fixture, a GitHub Action, a seeded snapshot, and an **`AGENTS.md`** stating the rules a manifest will
be rejected for — namespacing, reserved kinds and prefixes, icon names, why `capabilities` should be
left out. That file exists so a coding agent learns the constraints before it trips them, rather than
from a validator message after.

`test` runs the spore over `fixtures/` with no workspace, no cache and no store, and diffs the emitted
nodes and edges against `fixtures/expected.json`. Output is sorted, so a diff means a real change.

## Running a registry

`aneural registry <index.json>` is what registry CI runs on a submission. It checks structure, fetches
every listed file from the repo it names and verifies its sha256, then cross-checks each manifest
against its listing — including that the listing does not advertise fewer capabilities than the spore
asks for.

It resolves `github:` and `https:` entries over the network and relative entries beside the index, so a
registry can be validated from a checkout before it is published anywhere.

The official index is **generated, never hand-edited**: `crates/aneural-registry/tests/official_index.rs`
builds it from `spores/*` and fails if `registry/index.json` drifts. A hash in an index cannot be stale
relative to the file it pins, because nobody types it.

## Telemetry

None. The client sends a GET to each configured registry URL and nothing else. Two honest caveats:
fetching an index reveals your IP to whoever hosts it, and the marketplace fetches a spore's README
from its repo, which reveals which spore you looked at.

## Known limits

- **Kinds are not namespaced internally.** Collisions are refused at install, which is enough while
  every spore is declarative and its kinds are fixed in a manifest we validate. A `sandboxed` spore
  could emit any kind string at runtime, so **full kind namespacing is a prerequisite for tier 2.**
- One version per listing. Pinning an older version needs a `releases` array in the index.
- The detail pane renders a subset of Markdown — headings, lists, code blocks, paragraphs.
- Publisher icons are not fetched; decoding an untrusted image in-process is attack surface that buys
  very little.
- Node props that came from a web API are not yet marked as such, so the MCP server cannot fence
  them before an agent reads them. Do that before any `http` spore is enabled by default.
- The first merged PR claims a publisher id, with no verification. A `verified` flag set by a
  maintainer after a DNS or org check is the next step once there is a second publisher.
