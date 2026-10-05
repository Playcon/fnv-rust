# Contributing

Thanks for taking an interest in nv-rs. It is a personal project maintained by slaterain, and contributions are welcome when they fit the active milestone and have enough evidence to review.

## Proposing work

Start with [docs/MILESTONES.md](docs/MILESTONES.md) and open an issue or discussion before beginning a substantial change. Keep proposals bounded and connected to an active milestone, a reproducible defect, or a focused research question. A maintainer may defer work that does not fit current priorities.

Fork the repository and submit a pull request; contributors do not receive write access to the project. Describe the behavior changed, the evidence behind it, and any unresolved uncertainty. For game behavior, cite the executable build and address, game record or asset, or recording used. Include the relevant branch conditions and a regression case for behavior fixes. Do not present a hypothesis as verified behavior.

Tests should create their inputs from scratch with the existing test fixture tools. Do not commit game assets, executable fragments, decompilation output, recordings, or files copied from a user's installation. The project reads data from a game installation the user owns; it does not redistribute that data. To trace behaviour from your own copy of the executable, see [docs/RESEARCH_TOOLS.md](docs/RESEARCH_TOOLS.md).

## Code and checks

Keep project rules in the appropriate core crate (`world`, `physics`, `preview`, or `cellview`); the viewer handles presentation and input. Menus should use the game's XML through `ui`. Core crates do not use external libraries. The viewer is a separate workspace pinned to Bevy 0.16. Preserve the player/camera, aim/head, action-input and per-view boundaries documented in [docs/VR.md](docs/VR.md). Record VR adaptations as proposals until validated.

For a code change, run the relevant checks before submitting:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
cargo build --release
```

The viewer has its own workspace and must be checked from `viewer/`. Include the commands and results in the pull request. Documentation-only changes do not need game publication.

## AI assistance

Disclose AI assistance in the pull request and describe what it helped with. The human contributor remains responsible for the change: review the resulting code and claims, understand the behavior, and verify the checks and evidence reported.

## Handoffs

Keep handoffs concise and useful to the next person. Update the milestone tracker after a batch with its evidence, blockers and one next action; put detailed investigation in a topic document. If work is shared between Codex and Claude, record the active process, changed files and unfinished checks in the linked topic handoff so both can continue without replaying a large chat.
