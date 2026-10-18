# Agent pickup

`AGENTS.md` and the files in `docs/` are living documents. They describe the project as it is now, and they go stale when code, issues, or decisions move. The last step of any implementation or change is to review these files and update whatever that work made untrue: completed tasks, phase status, locked choices, paths, and boundaries. Do not finish while they still describe the old behavior.

## Skills

Skills are installed in `.agents/skills/`. That is the copy every agent uses. `.cursor/skills/` and `.claude/skills/` are links to the same directories, for Cursor and Claude Code. Read the skill's `SKILL.md` and follow it. Do not work from memory of the skill.

- **ponytail**, `.agents/skills/ponytail/SKILL.md`. Use it for every code change in this repo, in every directory, before writing or editing code. The companion skills `ponytail-review`, `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, and `ponytail-help` are for review, repo audit, and the ponytail commands. They do not replace the main skill on ordinary edits.
- **picasso**, `.agents/skills/picasso/SKILL.md`. Use it for every UI or UX change, including the emulator viewer, the shelf admin UI, and any other visual interface. Read `references/anti-patterns.md` before writing that interface.
- **avoid-ai-writing**, `.agents/skills/avoid-ai-writing/SKILL.md`. Use it on every sentence a person reads in a UI. After those sentences exist, and before the change is finished, read `references/patterns.md` and audit the sentences with this skill.

## Commits

Every commit subject, and every pull request title, is a [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/): `type(scope): description`. The scope may be left off. A breaking change puts `!` before the colon.

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.

`.githooks/commit-msg` rejects a bad commit subject and prints the form to use. It calls `scripts/check-semantic.sh`. If `git config --get core.hooksPath` is not `.githooks`, run `git config core.hooksPath .githooks` before the first commit. Do not pass `--no-verify`. If the hook rejects a commit, rewrite the message and commit again.

`.github/workflows/semantic.yml` runs that same script on every pull request and on every push to `main`. A pull request fails when its title or any commit subject is not a Conventional Commit. Rewrite the title or the commits and update the branch. Git-generated `Merge`, `fixup!`, and `squash!` subjects are allowed. Pull request titles are not.

## Releases

Versions follow [Semantic Versioning](https://semver.org/). `version.txt` is the current version. It is `0.0.0` until the first release.

`.github/workflows/release.yml` runs [release-please](https://github.com/googleapis/release-please) on every push to `main`. A `feat` commit opens a pull request for the next minor version. A `fix`, `perf`, or `revert` commit opens one for the next patch. A breaking change (`!` before the colon, or a `BREAKING CHANGE:` footer) opens a minor release while the version is below `1.0.0`, and a major release from `1.0.0` on. `docs`, `style`, `refactor`, `test`, `build`, `ci`, and `chore` do not open a release.

The pull request title is `chore: release X.Y.Z`. It updates `CHANGELOG.md`, `version.txt`, and `.release-please-manifest.json`. Merge that pull request. The merge publishes tag `vX.Y.Z` and a GitHub Release. The release body is the changelog section for that version.

Leave `CHANGELOG.md`, `version.txt`, and `.release-please-manifest.json` for that workflow to edit. To publish `1.0.0`, add `Release-As: 1.0.0` as the footer of a Conventional Commit on `main`. The next release pull request uses that version.

The release pull request is opened with the Actions token, so `.github/workflows/semantic.yml` does not run on it. The title is a Conventional Commit, and the push to `main` after the merge is checked. An admin of the repo has to allow GitHub Actions to create pull requests, under Settings, Actions, General.

The current milestone is the emulator: issues 1 through 8 on this repo. Finish those before building the reader service or the device shell features below.

When issues 1 through 8 are done, read [docs/product.md](docs/product.md) and implement the next phase in that file whose "done when" is not yet true. Do not skip ahead, and do not reopen choices that file already locks.

Hard boundaries:

- The shell talks to panels only through the damage protocol and a capability profile (`color`, `gray_levels`, `input`, `frontlight`, size). No per-device UI forks.
- Progress, catalog, and files go through `shelf`. The device keeps an offline outbox.
- Forced alignment runs on the server. The device plays EPUB 3 Media Overlays.
- Shelf is one self-hosted container for books. It does not ship books or indexer definitions.
