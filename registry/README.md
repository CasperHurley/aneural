# The Open Spores Marketplace

This directory is the official spore registry. `index.json` is what every Aneural
client fetches by default.

```
config.spores.registries = [
  { "name": "official", "url": "https://raw.githubusercontent.com/Parnassix/aneural/main/registry/index.json" }
]
```

**`index.json` is generated, never hand-edited.** It is built from `spores/*` by
`crates/aneural-registry/tests/official_index.rs`, which fails CI if the checked-in
file drifts from the manifests it pins. Regenerate with:

```sh
UPDATE_INDEX=1 cargo test -p aneural-registry official_index
```

That is the point: a sha256 in an index can never be stale relative to the file it
claims to pin, because no human types it.

## Listing a spore

See [CONTRIBUTING.md](CONTRIBUTING.md). Briefly: your spore lives in **your** repo;
this registry only records where it is and what its files hash to.

## Not the only registry

`registries` is an ordered list. Point it at your own index — a company one, a
personal one — and it works exactly the same way, listed alongside this one. The
first registry that lists an id provides it, so a team index can deliberately
shadow an official spore; the marketplace says "also listed in …" so the shadow is
never invisible.

You do not need to be listed anywhere at all: `aneural spores add` accepts a direct
URL.
