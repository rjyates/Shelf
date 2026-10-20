# Agent pickup

`AGENTS.md` and the files in `docs/` are living documents. They describe the project as it is now, and they go stale when code, issues, or decisions move. The last step of any implementation or change is to review these files and update whatever that work made untrue: completed tasks, phase status, locked choices, paths, and boundaries. Do not finish while they still describe the old behavior.

## Skills

Skills are installed in `.agents/skills/`. That is the copy every agent uses. `.cursor/skills/` and `.claude/skills/` are links to the same directories, for Cursor and Claude Code. Read the skill's `SKILL.md` and follow it. Do not work from memory of the skill.

- **ponytail**, `.agents/skills/ponytail/SKILL.md`. Use it for every code change in this repo, in every directory, before writing or editing code. The companion skills `ponytail-review`, `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, and `ponytail-help` are for review, repo audit, and the ponytail commands. They do not replace the main skill on ordinary edits.
- **picasso**, `.agents/skills/picasso/SKILL.md`. Use it for every UI or UX change, including the emulator viewer, the shelf admin UI, and any other visual interface. Read `references/anti-patterns.md` before writing that interface.
- **avoid-ai-writing**, `.agents/skills/avoid-ai-writing/SKILL.md`. Use it on every sentence a person reads in a UI. After those sentences exist, and before the change is finished, read `references/patterns.md` and audit the sentences with this skill.

## Commits

Every commit subject, and every pull request title, is a [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/): `type(scope): description`. The scope may be left off. A breaking change puts `!` before the colon.

A commit shows only the user. Do not add yourself, Cursor, or a model as author, committer, or contributor. Do not add a `Co-authored-by` trailer, a `Signed-off-by` trailer, or any other trailer that names an agent. Leave the author and committer as the user already set for the repository, and do not change git config to do that.

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.

`.githooks/commit-msg` rejects a bad commit subject and prints the form to use. It calls `scripts/check_semantic.py`. If `git config --get core.hooksPath` is not `.githooks`, run `git config core.hooksPath .githooks` before the first commit. Do not pass `--no-verify`. If the hook rejects a commit, rewrite the message and commit again.

`.github/workflows/semantic.yml` runs that same script on every pull request and on every push to `main`. A pull request fails when its title or any commit subject is not a Conventional Commit. Rewrite the title or the commits and update the branch. Git-generated `Merge`, `fixup!`, and `squash!` subjects are allowed. Pull request titles are not. A push whose tip is `Merge pull request #N` is checked using that pull request's title. A `chore(release):` commit is the release bot and is skipped.

Pull request descriptions follow [`.github/pull_request_template.md`](.github/pull_request_template.md). GitHub reads that file from `main`. Keep the Summary and Test plan headings. `Fixes #N` closes that issue on merge. A plain `#N` reference does not.

## Tests

The two commands in the Tests section of [README.md](README.md) install Poetry and run the suite.

## Releases

Versions follow [Semantic Versioning](https://semver.org/). The published version is the latest `vX.Y.Z` tag.

`.github/workflows/release.yml` runs [semantic-release](https://github.com/semantic-release/semantic-release) after `.github/workflows/semantic.yml` succeeds on a push to `main`. A direct push and a merged pull request both take that path. A `feat` commit publishes the next minor version. A `fix` commit publishes the next patch. A breaking change (`!` before the colon, or a `BREAKING CHANGE:` footer) publishes the next major. The first release is `1.0.0`. Other commit types do not publish a release.

semantic-release writes `CHANGELOG.md`, commits `chore(release): X.Y.Z [skip ci]`, pushes tag `vX.Y.Z`, and creates a GitHub Release. The release body is the changelog for that version. Plugin config is `release.plugins` in `package.json`. Leave `CHANGELOG.md` for that commit to edit. The `version` field in `package.json` and `pyproject.toml` stays `0.0.0`.

A squash merge uses the pull request title as the commit subject, so that title is what semantic-release versions. A merge commit versions from the commits on the branch.

The current milestone is the emulator: issues 1 through 8 on this repo. Issues 1 and 2 are done: `emulator/` boots a stock ISO in the browser, and `distro/` is the Buildroot x86_64 guest (`./distro/build.sh`, then `./distro/run.sh` on the host). Next is issue 3 (boot that image from the compose stack). Finish 1 through 8 before building the reader service or the device shell features below.

When issues 1 through 8 are done, read [docs/product.md](docs/product.md) and implement the next phase in that file whose "done when" is not yet true. Do not skip ahead, and do not reopen choices that file already locks.

Hard boundaries:

- The shell talks to panels only through the damage protocol and a capability profile (`color`, `gray_levels`, `input`, `frontlight`, size). No per-device UI forks.
- Progress, catalog, and files go through `shelf`. The device keeps an offline outbox.
- Forced alignment runs on the server. The device plays EPUB 3 Media Overlays.
- Shelf is one self-hosted container for books. It does not ship books or indexer definitions.
