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

## Emulator MCP (agent debugging)

When a task touches the emulator panel, guest UI, e-ink modes, or “what is on the screen,” use the **oseink-emulator** MCP: call `panel_screenshot` before guessing from logs; use `guest_type` to drive the guest. Do not debug the panel from compose logs alone.

Hosts: Cursor (`.cursor/mcp.json`), Claude Code (`.mcp.json`), VS Code (`.vscode/mcp.json`). Shared launch: `tools/emulator-mcp/run.sh`. Full tool list and other hosts: [docs/emulator-mcp.md](docs/emulator-mcp.md). Emulator run path: [docs/emulator.md](docs/emulator.md).

## Releases

Versions follow [Semantic Versioning](https://semver.org/). The published version is the latest `vX.Y.Z` tag.

`.github/workflows/release.yml` runs [semantic-release](https://github.com/semantic-release/semantic-release) after `.github/workflows/semantic.yml` succeeds on a push to `main`. A direct push and a merged pull request both take that path. A `feat` commit publishes the next minor version. A `fix` commit publishes the next patch. A breaking change (`!` before the colon, or a `BREAKING CHANGE:` footer) publishes the next major. The first release is `1.0.0`. Other commit types do not publish a release.

semantic-release writes `CHANGELOG.md`, commits `chore(release): X.Y.Z [skip ci]`, pushes tag `vX.Y.Z`, and creates a GitHub Release. The release body is the changelog for that version. Plugin config is `release.plugins` in `package.json`. Leave `CHANGELOG.md` for that commit to edit. The `version` field in `package.json` and `pyproject.toml` stays `0.0.0`.

A squash merge uses the pull request title as the commit subject, so that title is what semantic-release versions. A merge commit versions from the commits on the branch.

The emulator milestone (issues 1 through 8) is done. `emulator/` boots the Buildroot guest in the browser (`cd emulator && docker compose -p oseink up -d --build --force-recreate`; needs Docker and KVM — see [README.md](README.md)), `distro/` can also boot that image on the host (`./distro/run.sh`), `distro/overlay/` is shared into the guest at `/oseink` (virtiofs when `virtiofsd` is present, otherwise 9p; compose uses qemux's `/shared` 9p tag), guest `oseink-devd` restarts `/oseink/bin/oseink-shell` when that binary, `/oseink/share/`, or `/oseink/run/profile.json` changes while the VM stays up, that shell reads `/oseink/run/profile.json` when it starts, the `:8006` page sets that profile's color (black and white or color) and input (any combination of button, touch, and stylus) by writing the file and `distro/overlay/share/profile.stamp` so the running devd restarts the shell, compose does not set the profile, and `http://127.0.0.1:8006` is the e-ink **display** for that guest (the shell writes a gray8 damage frame, painted through quantize / delay / ghost / flash; click to type on the console). Damage protocol modes live in `emulator/viewer/damage.js`. VNC stays up for console input. The boot-target switch is issue 19, and the desktop repaint hook is phase 2. Details: [docs/emulator.md](docs/emulator.md).

Read [docs/product.md](docs/product.md) and implement the next phase in that file whose "done when" is not yet true. Do not skip ahead, and do not reopen choices that file already locks. The device is dual-session (reader/notes by default, full Linux desktop on demand), like SteamOS Game Mode vs Desktop Mode. See the Session model section in that file.

Hard boundaries:

- Reader/notes is the default session; desktop is a real Linux desktop on the same image. Boot target is a user setting. Do not build an appliance-only OS that blocks desktop, and do not skip the reader/notes session in favor of desktop-only.
- All panel output (both sessions) goes through the damage protocol and a capability profile (`color`, `gray_levels`, `input`, `frontlight`, size). Desktop apps must not bypass e-ink safeguards. No per-device forks of the reader UI.
- Desktop is Weston 14 (pixman, desktop-shell, headless backend) with a repaint hook as the only desktop damage writer. Reader/notes draws damage directly and is not a Weston client. Input is seatd and libinput. Session switch is a BusyBox boot-target file. XWayland and an X server are out. Do not add gamescope, Sway, Cage, Xorg, or systemd to satisfy this.
- Damage stays gray8 until the color-plane phase. GPU offload is its own later phase: Mesa when a GPU is probed, CPU and pixman when it is not. Do not turn on GL for the phase 2 desktop.
- The reader UI is our own damage-buffer toolkit. No GTK and no Qt. EPUB 2 and EPUB 3 go through Readium. PDF goes through MuPDF. Do not write a document parser.
- Progress, catalog, and files go through `shelf`. The device keeps an offline outbox.
- Forced alignment runs on the server. The device plays EPUB 3 Media Overlays. For that playback and for writing overlays into an EPUB, Storyteller is the source reference. See the Storyteller paragraph in [docs/product.md](docs/product.md). Do not embed Storyteller's apps.
- Shelf is one self-hosted container for books. It does not ship books or indexer definitions.
