#!/usr/bin/env bash
# Installs Carl as three user services. Run it from anywhere, no root needed.
#
#   carl-aec      the echo canceller
#   carl-listen   the microphone
#   carl-slack    Slack
#
# User services, not system ones. Carl needs the microphone, the speakers and the screen,
# all of which belong to a logged in person and none of which a root daemon can reach
# without a pile of workarounds that would each be their own bug.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
units="$HOME/.config/systemd/user"

echo "building a release binary, which is what a service should run"
cd "$repo"
cargo build --release

mkdir -p "$HOME/.local/bin" "$units"
install -m755 "$repo/target/release/carl" "$HOME/.local/bin/carl"
# The sandboxed interpreter. Carl only allows this one, so without it here he can compute
# nothing at all and says so rather than falling back to the real python.
install -m755 "$repo/etc/carl-python" "$HOME/.local/bin/carl-python"
echo "installed $HOME/.local/bin/carl and carl-python"

for unit in carl-aec carl-listen carl-slack; do
  install -m644 "$here/$unit.service" "$units/$unit.service"
done

# The unit files name the repo path for the canceller config, so the repo has to stay put.
sed -i "s|%h/Projects/carl|$repo|" "$units/carl-aec.service"

systemctl --user daemon-reload

# Slack is only started if it has been set up. Starting it without tokens gives a service
# that fails every fifteen seconds forever, which buries anything else in the journal.
want=(carl-aec carl-listen)
if [ -f "$HOME/.carl/slack.json" ] || [ -n "${CARL_SLACK_BOT_TOKEN:-}" ]; then
  want+=(carl-slack)
else
  echo
  echo "skipping carl-slack, there is no ~/.carl/slack.json yet."
  echo "set it up as readme.md describes, then: systemctl --user enable --now carl-slack"
fi

systemctl --user enable --now "${want[@]}"

echo
systemctl --user --no-pager --lines=0 status "${want[@]}" || true

cat <<'NOTE'

Two things worth knowing.

The microphone and the canceller stop when you log out, because they are PipeWire clients and
PipeWire is part of your graphical session. That is correct rather than a limitation. There is
no microphone to listen to when nobody is logged in.

Slack does not need a session, but by default no user service survives logout. To keep Carl
answering in Slack while you are logged out, enable lingering. It needs root, so run it
yourself:

    sudo loginctl enable-linger $USER

Watch any of them with:

    journalctl --user -u carl-listen -f
NOTE
