# Heartbeat Tasks

Tasks checked automatically by `memi heartbeat --watch`.

## Every Cycle (30min default)
- [ ] All specs have `purpose` field
- [ ] No atoms composing other specs
- [ ] All component specs have shadcnBase
- [ ] Token coverage: color, spacing, typography, radius exist
- [ ] No specs modified since last generation (drift check)

## Daily
- [ ] Design system backup to .memoire/backups/
- [ ] Spec count report logged
- [ ] Upstream source check: `python3 scripts/update_company_packs.py`
      → exit 1 means a source repo has new commits; re-install stale packs then re-lock
      → re-install: `paperclip install <company-slug>` for each stale pack
      → re-lock after install: `python3 scripts/check_company_drift.py --update`
- [ ] Local drift check (after installs): `python3 scripts/check_company_drift.py`
      → catches any file-level changes not yet accepted into the lock

## On Figma Connect
- [ ] Pull latest tokens
- [ ] Diff local vs Figma state
- [ ] Flag unbound color fills
