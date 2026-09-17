# GitHub Pull Requests

Every open pull request on one repository becomes a `PullRequest` node, joined by
a `TOUCHES` edge to each file in your checkout that it changes. Review pressure
stops being a separate tab: the files under discussion are visibly under
discussion, right where you are already looking.

This is the first spore that reaches off your machine, so it is worth being
precise about what that means.

## What it does

Two calls to `api.github.com`, on a five-minute cycle:

- `GET /repos/{repo}/pulls?state=open` — one node per pull request, carrying its
  number, author, branch, base, labels, draft flag, URL and `updatedAt`.
- `GET /repos/{repo}/pulls/{number}/files` — one `TOUCHES` edge per changed file,
  with `status`, `additions` and `deletions` on the edge.

`readAt` records when the reading was taken. Nothing here is live; it is a
snapshot with a date on it, the same as every other spore.

## Setup

Two things, and it will tell you if either is missing.

**The repository**, in your workspace config:

```
aneural spores set aneural.github repo owner/name
```

**A token**, from the environment — never from a config file, and never written
anywhere by Aneural:

```
export ANEURAL_SECRET_GITHUB_TOKEN=ghp_…
```

A fine-grained token with read-only **Pull requests** and **Contents** access to
that one repository is enough. Do not give it write scopes; nothing here writes.

## What it can and cannot do

The manifest declares two capabilities, and you consent to them at install:

| | |
|---|---|
| `http` | `api.github.com`, and nowhere else |
| `secret` | `githubToken` |

Those are not documentation, they are enforced:

- The **host is re-checked against the allowlist at request time**, not just when
  the manifest was validated — including on every pagination hop, because the
  server chooses that URL.
- **Redirects are refused outright.** A redirect would mean deciding whether to
  forward your token to wherever GitHub points, and the only obviously-correct
  answer is not to go.
- **The token only ever goes into a header.** A manifest that puts `{secret.*}`
  in a URL fails validation, because URLs get logged and cached.
- **Values from the API are escaped before they are used in the next URL**, so a
  crafted pull-request title cannot walk the path.
- **The expansion may only draw edges, never create nodes.** A pull request that
  touches a file you do not have is simply not drawn — it cannot conjure the file
  into your graph.

## What it costs you

Two requests per refresh, plus one per open pull request for the file lists,
capped at 50. On a busy repository that is the bulk of the calls; five minutes
between refreshes keeps it far inside GitHub's rate limit, and the floor Aneural
enforces is 60 seconds.

The MCP server does not make web requests at all. If a coding agent is reading
your graph, it sees whatever the GUI or CLI last fetched, and never triggers a
fetch of its own.
