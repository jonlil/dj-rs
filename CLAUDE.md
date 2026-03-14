# dj-rs — Claude Code instructions

## Working on a GitHub issue

1. **Start from master**
   ```bash
   git checkout master
   git pull
   ```

2. **Create a branch named after the issue**
   Use the format `fix/<number>-short-description` for bugs,
   `feature/<number>-short-description` for new features.
   ```bash
   git checkout -b fix/8-playlist-folder-ordering
   ```

3. **Read the code before touching anything**
   Understand the relevant section fully before proposing changes.
   Check the linked issue and any related `docs/` files for context.

4. **Work in small, focused commits**
   Each commit should do one thing. Reference the issue number in the
   commit message where relevant.

5. **Push and open a PR**
   ```bash
   git push -u origin <branch>
   gh pr create
   ```
   - Reference the issue in the PR body with `Closes #<number>` so GitHub
     auto-closes it on merge.
   - Do not include "Generated with Claude" or similar AI mentions in PRs or commits.

## Commit style

- Imperative mood, lowercase: `fix playlist folder ordering by sorting on Seq`
- No co-author lines
- Keep messages factual — describe what changed and why, not how long it took

## Branch hygiene

- Never commit directly to `master`
- Delete branches after merging

## Build

```bash
cargo run        # dev
cargo build --release && ./target/release/dj-rs
```

See `docs/build.md` for Tizen TV app build and deploy steps.
