# Listing a spore

Your spore lives in your own repository. This registry records its location and
the sha256 of each of its files — nothing more. Nobody's code is hosted here.

## Before you open a PR

```sh
aneural spores init my-spore --publisher <you>
# …write the manifest…
aneural spores validate my-spore/spore.json
aneural spores test my-spore
```

Your repo should contain, at the path you are listing:

| file | |
|---|---|
| `spore.json` | required |
| `README.md` | strongly recommended — it is what the marketplace detail pane shows |
| `icon.svg` | optional |

Nothing else is fetched. The package allowlist is closed.

## The entry

Add one object to `spores` in `index.json`:

```json
{
  "id": "acme.adr",
  "version": "1.2.0",
  "displayName": "Architecture Decision Records",
  "description": "ADR documents become Decision nodes.",
  "license": "MIT",
  "repo": "github:acme/adr-spore#v1.2.0",
  "path": "spore",
  "files": { "spore.json": "<sha256>", "README.md": "<sha256>" },
  "nodeKinds": ["Decision"],
  "keywords": ["adr", "docs"],
  "capabilities": []
}
```

**Pin `repo` to a tag or commit**, not a branch. A branch can move under a hash we
have already published, and the client treats that as tampering — correctly.

## What CI checks

- the id is `publisher.name`, kebab-case on both halves, and unclaimed by anyone else
- `version` is semver
- every listed file resolves and its sha256 matches
- the manifest validates: namespaced ids, no reserved kinds, known icons
- `capabilities` in the entry match the manifest exactly

That last one matters most. A listing that advertises fewer capabilities than the
manifest asks for is how a spore would sneak past the consent sheet, so the client
refuses to install on a mismatch and CI refuses to merge one.

## Spores that read a web API

A spore declaring an `http` capability gets read by a human before it merges, and
these are the things that will send it back:

- **A host you do not control, or a broad wildcard.** `hosts` is what the user is
  consenting to. `*` is rejected outright; `*.example.com` needs a reason.
  `localhost`, IP addresses and `.local` / `.internal` names are refused by
  validation: a listed spore reaches public hostnames, never the user's machine.
- **A secret without `hosts`, or with more hosts than it needs.** A secret is
  scoped to the hosts it may be sent to, and the consent sheet says so
  ("send it only to api.github.com"). One host per secret is the normal case.
- **A URL with a `{secret.*}` in it.** Refused by validation, but say it here too:
  URLs are logged and cached, headers are not.
- **An `expand` that fans out further than it needs to.** One follow-up request per
  record, capped at 50, is a real cost on someone else's API with someone's own
  credentials attached.
- **A `refreshSeconds` at the 60s floor** without a reason it needs to be there.
  Most things are fine at 300.
- **Sampling more than the listing implies.** Whatever lands in node props is served
  to whatever coding agent is attached over MCP. If a spore pulls in ticket bodies
  or comment text, its README has to say so plainly.

## What is never listed

A spore whose capabilities put it in the `native` tier — `fsWrite` or `subprocess`
— is refused by CI and by every client, in any registry. The sandbox is the
ceiling for anything a registry hands out; native code only ever arrives by a
URL the user typed themselves.

## Publishing an update

Bump `version`, bump `repo`'s tag, update the hashes, open a PR. Users see the new
version on `aneural spores update`, and are re-prompted for consent **only** if the
new version asks for something the installed one did not.

## Claiming a publisher

The first merged PR using a publisher id claims it. Use something you can defend as
yours — an org name, a domain you own, your handle.

## Revocation

To withdraw a spore, add to `revoked`:

```json
{ "id": "acme.adr", "versions": ["1.2.0"], "reason": "leaks tokens into node props" }
```

`["*"]` covers every version. Revocations are honoured across *all* configured
registries, so a private index can withdraw a spore for its own team without
touching this one. A revoked spore already installed is disabled with a warning —
never deleted, because acting on a remote instruction to remove someone's files is
its own attack.
