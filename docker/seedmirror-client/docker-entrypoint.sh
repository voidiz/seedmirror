#!/usr/bin/env bash

set -euo pipefail

USERNAME="user"
GROUPNAME="$USERNAME"

if ! getent group $GROUPNAME >/dev/null 2>&1; then
    groupadd -g $PGID $GROUPNAME
fi

if ! id -u $USERNAME >/dev/null 2>&1; then
    useradd -m -u $PUID -g $PGID -s /bin/bash -c "" $USERNAME
fi

if [ ! -e "/config/.ssh" ]; then
    echo "Couldn't find /config/.ssh mounted"
    exit 1
fi

ln -s /config/.ssh "/home/$USERNAME/.ssh"
su "$USERNAME" -c "exec seedmirror-client $*"
