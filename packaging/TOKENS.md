# Getting the publish tokens (one-time)

Four repository secrets drive the automated channel publishing. This is the
click-by-click for creating each one and adding it to
`tina4stack/tina4` → Settings → Secrets. Do them in any order; each channel goes
live the moment its secret exists (until then that channel's job just skips).

| Secret | Type | Where it comes from |
|--------|------|---------------------|
| `SCOOP_BUCKET_TOKEN`  | GitHub fine-grained PAT | write access to `tina4stack/scoop-bucket` |
| `HOMEBREW_TAP_TOKEN`  | GitHub fine-grained PAT | write access to `tina4stack/homebrew-tap` |
| `WINGET_TOKEN`        | GitHub **classic** PAT  | `public_repo` (forks + PRs microsoft/winget-pkgs) |
| `CHOCO_API_KEY`       | chocolatey.org API key  | the info@tina4.com account |

**Which GitHub account?** Use the account that owns (or admins) the `tina4stack`
org. The winget PR will come from whichever account owns `WINGET_TOKEN`, so an
org/bot account tied to info@tina4.com is nicest, but a personal admin account
works fine too.

---

## 0. Create the two target repos first

The Scoop and Homebrew tokens need something to point at:

- `tina4stack/scoop-bucket` — new **public** repo, empty is fine.
- `tina4stack/homebrew-tap` — new **public** repo, empty is fine. The
  `homebrew-` prefix is required (it's what makes `brew tap tina4stack/tap` work).

---

## 1. `SCOOP_BUCKET_TOKEN` and `HOMEBREW_TAP_TOKEN` — fine-grained PATs

These push a single file into an org repo, so a fine-grained token scoped to just
that repo with just Contents write is the least-privilege choice.

1. Go to **https://github.com/settings/personal-access-tokens/new**
   (or: your avatar → Settings → Developer settings → Personal access tokens →
   Fine-grained tokens → **Generate new token**).
2. **Token name:** `tina4 scoop-bucket publish` (and later `tina4 homebrew-tap publish`).
3. **Expiration:** 1 year (set a calendar reminder to rotate).
4. **Resource owner:** `tina4stack`.
   - If `tina4stack` doesn't appear or the token needs approval, an org owner must
     enable fine-grained PATs at
     **Org → Settings → Third-party Access → Personal access tokens** and approve
     the request. This is a common first-time gotcha.
5. **Repository access:** *Only select repositories* → choose `scoop-bucket`.
6. **Permissions:** expand **Repository permissions** → **Contents** → set to
   **Read and write**. (Metadata → Read is added automatically. Nothing else.)
7. **Generate token**, copy the value (`github_pat_…`).
8. Add it as the secret `SCOOP_BUCKET_TOKEN` (see step 4 below).
9. Repeat for Homebrew: same steps, name it for the tap, select `homebrew-tap`,
   Contents = Read and write → secret `HOMEBREW_TAP_TOKEN`.

**Shortcut:** you may instead make **one** fine-grained token that selects *both*
`scoop-bucket` and `homebrew-tap` (Contents: Read and write) and paste the same
value into both secrets. Two tokens is cleaner for independent rotation; one is
less to manage. Your call.

---

## 2. `WINGET_TOKEN` — a classic PAT

winget publishing (`wingetcreate`) forks **microsoft/winget-pkgs** — a repo you
don't own — and opens a PR. That cross-repo fork-and-PR needs a **classic** token
with `public_repo`; a fine-grained token can't do it.

1. Go to **https://github.com/settings/tokens/new**
   (Settings → Developer settings → Personal access tokens → **Tokens (classic)**
   → Generate new token (classic)).
2. **Note:** `tina4 winget publish`.
3. **Expiration:** 1 year.
4. **Scopes:** tick **`public_repo`** (it's the sub-item under `repo`). Nothing else.
5. **Generate token**, copy the value (`ghp_…`).
6. Add it as the secret `WINGET_TOKEN`.

Note: the FIRST winget submission is done by hand once (`wingetcreate new …`, see
`packaging/README.md`); after that this token drives the automated updates.

---

## 3. `CHOCO_API_KEY` — chocolatey.org

1. Create / sign in to an account at **https://community.chocolatey.org** using
   **info@tina4.com** (verify the email).
2. Go to **https://community.chocolatey.org/account** (your username → My Account).
3. Under **API Keys**, copy your key (or click to generate/reset one).
4. Add it as the secret `CHOCO_API_KEY`.

The first `choco push` registers the `tina4` package id and goes through
Chocolatey moderation (a human review). Once the package is trusted, later
versions publish automatically.

---

## 4. Add each secret to the repo

For every value above:

1. Go to **https://github.com/tina4stack/tina4/settings/secrets/actions**
   (repo → Settings → Secrets and variables → **Actions**). You need admin on the repo.
2. **New repository secret**.
3. **Name:** exactly `SCOOP_BUCKET_TOKEN` / `HOMEBREW_TAP_TOKEN` / `WINGET_TOKEN` /
   `CHOCO_API_KEY` (case-sensitive, must match).
4. **Secret:** paste the value → **Add secret**.

---

## 5. Verify without waiting for a release

You don't have to cut a real release to test the wiring. From the repo's
**Actions** tab, run **Publish package manifests** via **workflow_dispatch** and
pass a recent published tag (e.g. `v3.8.77`):

- Channels whose secret you've added run their publish job.
- Channels still missing a secret show as skipped.
- Check the run log: the `render` job prints the version it resolved, and each
  publish job either pushes or is skipped.

That confirms each token is valid and scoped correctly before the next real
release relies on it.

---

## Rotating or revoking

- GitHub PATs: Settings → Developer settings → the token → **Regenerate** or
  **Delete**, then update the repo secret. Nothing else caches the value.
- Chocolatey: reset the key on the account page, then update `CHOCO_API_KEY`.
- Losing/rotating a token only affects that one channel; the others keep working.
