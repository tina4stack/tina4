#!/usr/bin/env bash
# Canonical Tina4 skills file list -- the single source of truth for WHICH files
# make up a skills release. Sourced (not executed) by:
#   - gen-skills-sha256.sh   (emits the checksum manifest)
#   - gen-skills-bundle.sh   (stages the tina4.com bundle)
# so the manifest and the tina4.com bundle can NEVER disagree about the file set.
#
# install-skills.sh / .ps1 keep their OWN copies of this list because they ship
# standalone to users (nothing to source from). They are kept in sync by hand;
# the installer stages then runs `sha256 -c` against this manifest, so an installer
# that drops a file the manifest names FAILS LOUD at verification -- the manifest
# is the contract, and drift cannot pass silently.
#
# Each entry is "repo|skill|space-separated references". `skills_entries` prints
# them one per line for a `while IFS='|' read` loop.

# Every file under references/, not most of them. ai-coder-rule-path.svg was once
# omitted, so a SUCCESSFUL install still produced an incomplete skill -- no error.
skills_dev_refs="auth-and-services.md data-and-orm.md deployment.md routes-and-api.md templates-and-frontend.md realtime.md web-push.md ai-coder-rule-path.svg"

# Per-language developer skills come from their own framework repo. The shared
# skills (tina4-js, tina4-maintainer, tina4-architect, tina4-design) have a
# canonical copy in tina4-python and are served from there.
skills_entries() {
  cat <<EOF
tina4-python|tina4-developer-python|$skills_dev_refs
tina4-php|tina4-developer-php|$skills_dev_refs
tina4-ruby|tina4-developer-ruby|$skills_dev_refs
tina4-nodejs|tina4-developer-nodejs|$skills_dev_refs
tina4-python|tina4-js|html-and-components.md signals-and-reactivity.md persistence.md rtc.md
tina4-python|tina4-maintainer|cli-and-deployment.md frond-and-frontend.md routing-and-orm.md subsystems.md
tina4-python|tina4-architect|
tina4-python|tina4-design|
EOF
}
